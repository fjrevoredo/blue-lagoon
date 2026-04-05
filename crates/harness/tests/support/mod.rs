use std::{future::Future, path::PathBuf, process::Command, sync::OnceLock, time::Duration};

use anyhow::{Context, Result};
use harness::{config::RuntimeConfig, db, migration};
use sqlx::{Connection, Executor, PgConnection, PgPool};
use url::Url;
use uuid::Uuid;

const DEFAULT_TEST_POSTGRES_ADMIN_URL: &str =
    "postgres://blue_lagoon:blue_lagoon@localhost:55432/postgres";
#[allow(dead_code)]
static WORKERS_BINARY: OnceLock<Result<PathBuf, String>> = OnceLock::new();

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
        ensure_postgres_running()?;
    }

    let database_name = disposable_database_name();
    let database_url = disposable_database_url(&admin_database_url, &database_name)?;
    create_database(&admin_database_url, &database_name).await?;

    let config = build_test_runtime_config(database_url);
    let pool = connect_with_retry(&config).await?;
    if provisioning == TestDatabaseProvisioning::Migrated {
        migration::apply_pending_migrations(&pool, env!("CARGO_PKG_VERSION")).await?;
    }

    let result = test_fn(TestDatabaseContext {
        config: config.clone(),
        pool: pool.clone(),
        database_name: database_name.clone(),
    })
    .await;

    let cleanup_result = drop_database(&admin_database_url, pool, &database_name).await;
    match (result, cleanup_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(test_error), Ok(())) => Err(test_error),
        (Ok(_), Err(cleanup_error)) => Err(cleanup_error),
        (Err(test_error), Err(cleanup_error)) => Err(test_error.context(format!(
            "test database cleanup also failed: {cleanup_error}"
        ))),
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
    format!("blue_lagoon_test_{}", Uuid::now_v7().simple())
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
    let mut connection = PgConnection::connect(admin_database_url)
        .await
        .context("failed to connect to test postgres admin database")?;
    connection
        .execute(format!(r#"CREATE DATABASE "{database_name}""#).as_str())
        .await
        .with_context(|| format!("failed to create disposable test database '{database_name}'"))?;
    Ok(())
}

async fn drop_database(admin_database_url: &str, pool: PgPool, database_name: &str) -> Result<()> {
    pool.close().await;

    let mut connection = PgConnection::connect(admin_database_url)
        .await
        .context("failed to reconnect to test postgres admin database for cleanup")?;
    sqlx::query(
        r#"
        SELECT pg_terminate_backend(pid)
        FROM pg_stat_activity
        WHERE datname = $1 AND pid <> pg_backend_pid()
        "#,
    )
    .bind(database_name)
    .execute(&mut connection)
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
    if binary_path.exists() {
        return Ok(binary_path);
    }

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
