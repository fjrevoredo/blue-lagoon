use std::{
    future::Future,
    path::PathBuf,
    process::Command,
    sync::OnceLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use futures_util::FutureExt;
use harness::{config::RuntimeConfig, db, migration};
use sqlx::{Connection, Executor, PgConnection, PgPool};
use tokio::sync::Mutex;
use url::Url;
use uuid::Uuid;

const DEFAULT_TEST_POSTGRES_ADMIN_URL: &str =
    "postgres://blue_lagoon:blue_lagoon@localhost:55432/postgres";
const TEST_DATABASE_PREFIX: &str = "blue_lagoon_test_";
const STALE_TEST_DATABASE_MIN_AGE_SECS: u64 = 6 * 60 * 60;
#[allow(dead_code)]
static WORKERS_BINARY: OnceLock<Result<PathBuf, String>> = OnceLock::new();
static POSTGRES_BOOTSTRAP_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct TestDatabaseContext {
    pub config: RuntimeConfig,
    pub pool: PgPool,
    #[allow(dead_code)]
    pub database_name: String,
}

pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("workspace root should exist")
        .to_path_buf()
}

#[allow(dead_code)]
pub fn workers_binary() -> Result<PathBuf> {
    match WORKERS_BINARY.get_or_init(|| resolve_workers_binary().map_err(|error| error.to_string()))
    {
        Ok(path) => Ok(path.clone()),
        Err(message) => Err(anyhow::anyhow!(message.clone())),
    }
}

#[allow(dead_code)]
pub async fn with_clean_database<F, Fut, T>(test_fn: F) -> Result<T>
where
    F: FnOnce(TestDatabaseContext) -> Fut,
    Fut: Future<Output = Result<T>>,
{
    with_database(TestDatabaseProvisioning::Clean, test_fn).await
}

pub async fn with_migrated_database<F, Fut, T>(test_fn: F) -> Result<T>
where
    F: FnOnce(TestDatabaseContext) -> Fut,
    Fut: Future<Output = Result<T>>,
{
    with_database(TestDatabaseProvisioning::Migrated, test_fn).await
}

async fn with_database<F, Fut, T>(provisioning: TestDatabaseProvisioning, test_fn: F) -> Result<T>
where
    F: FnOnce(TestDatabaseContext) -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let admin_database_url = resolved_test_postgres_admin_url();
    if should_bootstrap_local_postgres() {
        ensure_local_postgres_ready(&admin_database_url).await?;
    }
    sweep_stale_test_databases(&admin_database_url).await?;

    // Disposable test databases never reuse names. That keeps concurrent runs isolated
    // and makes any leaked databases safe to identify by prefix and embedded timestamp.
    let database_name = disposable_database_name();
    let database_url = disposable_database_url(&admin_database_url, &database_name)?;
    create_database(&admin_database_url, &database_name).await?;

    let config = build_test_runtime_config(database_url);
    let pool = connect_with_retry(&config).await?;
    if provisioning == TestDatabaseProvisioning::Migrated {
        migration::apply_pending_migrations(&pool, env!("CARGO_PKG_VERSION")).await?;
    }

    let fixture = TestDatabaseFixture {
        admin_database_url,
        database_name: database_name.clone(),
        pool: Some(pool.clone()),
    };

    let result = std::panic::AssertUnwindSafe(test_fn(TestDatabaseContext {
        config: config.clone(),
        pool,
        database_name,
    }))
    .catch_unwind()
    .await;

    let cleanup_result = fixture.cleanup().await;
    match (result, cleanup_result) {
        (Ok(Ok(value)), Ok(())) => Ok(value),
        (Ok(Err(test_error)), Ok(())) => Err(test_error),
        (Ok(Ok(_)), Err(cleanup_error)) => Err(cleanup_error),
        (Ok(Err(test_error)), Err(cleanup_error)) => Err(test_error.context(format!(
            "test database cleanup also failed: {cleanup_error}"
        ))),
        (Err(panic_payload), Ok(())) => std::panic::resume_unwind(panic_payload),
        (Err(panic_payload), Err(cleanup_error)) => {
            eprintln!("test database cleanup failed after panic: {cleanup_error}");
            std::panic::resume_unwind(panic_payload)
        }
    }
}

fn resolved_test_postgres_admin_url() -> String {
    std::env::var("BLUE_LAGOON_TEST_POSTGRES_ADMIN_URL")
        .unwrap_or_else(|_| DEFAULT_TEST_POSTGRES_ADMIN_URL.to_string())
}

fn should_bootstrap_local_postgres() -> bool {
    std::env::var_os("BLUE_LAGOON_TEST_POSTGRES_ADMIN_URL").is_none()
}

fn build_test_runtime_config(database_url: String) -> RuntimeConfig {
    RuntimeConfig {
        app: harness::config::AppConfig {
            name: "blue-lagoon".to_string(),
            log_filter: "info".to_string(),
        },
        database: harness::config::DatabaseConfig {
            database_url,
            minimum_supported_schema_version: 1,
        },
        harness: harness::config::HarnessConfig {
            allow_synthetic_smoke: true,
            default_foreground_iteration_budget: 1,
            default_wall_clock_budget_ms: 30_000,
            default_foreground_token_budget: 4_000,
        },
        background: harness::config::BackgroundConfig {
            scheduler: harness::config::BackgroundSchedulerConfig {
                poll_interval_seconds: 300,
                max_due_jobs_per_iteration: 4,
                lease_timeout_ms: 300_000,
            },
            thresholds: harness::config::BackgroundThresholdsConfig {
                episode_backlog_threshold: 25,
                candidate_memory_threshold: 10,
                contradiction_alert_threshold: 3,
            },
            execution: harness::config::BackgroundExecutionConfig {
                default_iteration_budget: 2,
                default_wall_clock_budget_ms: 120_000,
                default_token_budget: 6_000,
            },
            wake_signals: harness::config::WakeSignalPolicyConfig {
                allow_foreground_conversion: true,
                max_pending_signals: 8,
                cooldown_seconds: 900,
            },
        },
        continuity: harness::config::ContinuityConfig {
            retrieval: harness::config::RetrievalConfig {
                max_recent_episode_candidates: 3,
                max_memory_artifact_candidates: 5,
                max_context_items: 6,
            },
            backlog_recovery: harness::config::BacklogRecoveryConfig {
                pending_message_count_threshold: 3,
                pending_message_span_seconds_threshold: 120,
                stale_pending_ingress_age_seconds_threshold: 300,
                max_recovery_batch_size: 8,
            },
        },
        scheduled_foreground: harness::config::ScheduledForegroundConfig {
            enabled: true,
            max_due_tasks_per_iteration: 2,
            min_cadence_seconds: 300,
            default_cooldown_seconds: 300,
        },
        workspace: harness::config::WorkspaceConfig {
            root_dir: workspace_root(),
            max_artifact_bytes: 1_048_576,
            max_script_bytes: 262_144,
        },
        approvals: harness::config::ApprovalsConfig {
            default_ttl_seconds: 900,
            max_pending_requests: 32,
            allow_cli_resolution: true,
            prompt_mode: harness::config::ApprovalPromptMode::InlineKeyboardWithFallback,
        },
        governed_actions: harness::config::GovernedActionsConfig {
            approval_required_min_risk_tier: contracts::GovernedActionRiskTier::Tier2,
            default_subprocess_timeout_ms: 30_000,
            max_subprocess_timeout_ms: 120_000,
            max_filesystem_roots_per_action: 4,
            default_network_access: contracts::NetworkAccessPosture::Disabled,
            allowlisted_environment_variables: vec!["BLUE_LAGOON_DATABASE_URL".to_string()],
            max_environment_variables_per_action: 8,
            max_captured_output_bytes: 65_536,
            max_web_fetch_timeout_ms: 15_000,
            max_web_fetch_response_bytes: 524_288,
        },
        worker: harness::config::WorkerConfig {
            timeout_ms: 20_000,
            command: String::new(),
            args: Vec::new(),
        },
        telegram: None,
        model_gateway: None,
        self_model: None,
    }
}

fn disposable_database_name() -> String {
    format!(
        "{TEST_DATABASE_PREFIX}{}_{}",
        current_unix_timestamp_secs(),
        Uuid::now_v7().simple()
    )
}

fn disposable_database_url(admin_database_url: &str, database_name: &str) -> Result<String> {
    let mut url = Url::parse(admin_database_url).with_context(|| {
        format!("failed to parse test postgres admin url '{admin_database_url}'")
    })?;
    url.set_path(&format!("/{database_name}"));
    Ok(url.into())
}

async fn connect_with_retry(config: &RuntimeConfig) -> Result<PgPool> {
    let mut last_error = None;
    for _ in 0..30 {
        match db::connect(config).await {
            Ok(pool) => return Ok(pool),
            Err(error) => {
                last_error = Some(error);
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("postgres never became reachable")))
}

async fn sweep_stale_test_databases(admin_database_url: &str) -> Result<()> {
    let mut connection = connect_admin_with_retry(admin_database_url, "stale sweep").await?;
    let database_names: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT datname
        FROM pg_database
        WHERE datname LIKE 'blue_lagoon_test_%'
        "#,
    )
    .fetch_all(&mut connection)
    .await
    .context("failed to enumerate disposable test databases")?;

    for database_name in database_names {
        if !is_stale_test_database_name(&database_name) {
            continue;
        }
        if has_active_sessions(&mut connection, &database_name).await? {
            continue;
        }
        drop_database_with_connection(&mut connection, &database_name).await?;
    }

    Ok(())
}

async fn ensure_local_postgres_ready(admin_database_url: &str) -> Result<()> {
    let _guard = POSTGRES_BOOTSTRAP_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .await;
    ensure_postgres_running()?;
    let connection = connect_admin_with_retry(admin_database_url, "local postgres bootstrap").await;
    drop(_guard);
    connection.map(drop)
}

fn ensure_postgres_running() -> Result<()> {
    let status = Command::new("docker")
        .arg("compose")
        .arg("up")
        .arg("-d")
        .arg("postgres")
        .current_dir(workspace_root())
        .status()
        .context("failed to run docker compose up -d postgres")?;
    if !status.success() {
        anyhow::bail!("docker compose up -d postgres failed");
    }
    Ok(())
}

async fn create_database(admin_database_url: &str, database_name: &str) -> Result<()> {
    let mut connection =
        connect_admin_with_retry(admin_database_url, "test database creation").await?;
    connection
        .execute(format!(r#"CREATE DATABASE "{database_name}""#).as_str())
        .await
        .with_context(|| format!("failed to create disposable test database '{database_name}'"))?;
    Ok(())
}

async fn drop_database(admin_database_url: &str, pool: PgPool, database_name: &str) -> Result<()> {
    pool.close().await;

    let mut connection =
        connect_admin_with_retry(admin_database_url, "test database cleanup").await?;
    drop_database_with_connection(&mut connection, database_name).await?;
    Ok(())
}

async fn connect_admin_with_retry(
    admin_database_url: &str,
    operation: &str,
) -> Result<PgConnection> {
    let mut last_error = None;
    for _ in 0..30 {
        match PgConnection::connect(admin_database_url).await {
            Ok(connection) => return Ok(connection),
            Err(error) => {
                last_error = Some(error);
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }

    let error = last_error
        .map(anyhow::Error::from)
        .unwrap_or_else(|| anyhow::anyhow!("postgres admin database never became reachable"));
    Err(error).with_context(|| {
        format!("failed to connect to test postgres admin database for {operation}")
    })
}

async fn has_active_sessions(connection: &mut PgConnection, database_name: &str) -> Result<bool> {
    let session_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM pg_stat_activity
        WHERE datname = $1 AND pid <> pg_backend_pid()
        "#,
    )
    .bind(database_name)
    .fetch_one(&mut *connection)
    .await
    .with_context(|| {
        format!("failed to inspect active sessions for disposable test database '{database_name}'")
    })?;
    Ok(session_count > 0)
}

async fn drop_database_with_connection(
    connection: &mut PgConnection,
    database_name: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        SELECT pg_terminate_backend(pid)
        FROM pg_stat_activity
        WHERE datname = $1 AND pid <> pg_backend_pid()
        "#,
    )
    .bind(database_name)
    .execute(&mut *connection)
    .await
    .with_context(|| {
        format!(
            "failed to terminate active sessions for disposable test database '{database_name}'"
        )
    })?;
    connection
        .execute(format!(r#"DROP DATABASE "{database_name}""#).as_str())
        .await
        .with_context(|| format!("failed to drop disposable test database '{database_name}'"))?;
    Ok(())
}

fn is_stale_test_database_name(database_name: &str) -> bool {
    let Some((timestamp, _suffix)) = parse_test_database_name(database_name) else {
        return false;
    };
    current_unix_timestamp_secs().saturating_sub(timestamp) >= STALE_TEST_DATABASE_MIN_AGE_SECS
}

fn parse_test_database_name(database_name: &str) -> Option<(u64, &str)> {
    let suffix = database_name.strip_prefix(TEST_DATABASE_PREFIX)?;
    let (timestamp, uuid_suffix) = suffix.split_once('_')?;
    let timestamp = timestamp.parse::<u64>().ok()?;
    if uuid_suffix.is_empty() {
        return None;
    }
    Some((timestamp, uuid_suffix))
}

fn current_unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_secs()
}

struct TestDatabaseFixture {
    admin_database_url: String,
    database_name: String,
    pool: Option<PgPool>,
}

impl TestDatabaseFixture {
    async fn cleanup(mut self) -> Result<()> {
        let pool = self
            .pool
            .take()
            .expect("test database fixture pool should exist during cleanup");
        drop_database(&self.admin_database_url, pool, &self.database_name).await
    }
}

#[allow(dead_code)]
fn resolve_workers_binary() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("CARGO_BIN_EXE_workers") {
        return Ok(PathBuf::from(path));
    }

    let binary_name = if cfg!(windows) {
        "workers.exe"
    } else {
        "workers"
    };
    let binary_path = workspace_root()
        .join("target")
        .join("debug")
        .join(binary_name);

    let status = Command::new("cargo")
        .arg("build")
        .arg("-p")
        .arg("workers")
        .current_dir(workspace_root())
        .status()
        .context("failed to run cargo build -p workers")?;
    if !status.success() {
        anyhow::bail!("cargo build -p workers failed");
    }
    if !binary_path.exists() {
        anyhow::bail!(
            "workers binary was not produced at expected path {}",
            binary_path.display()
        );
    }
    Ok(binary_path)
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestDatabaseProvisioning {
    Clean,
    Migrated,
}
