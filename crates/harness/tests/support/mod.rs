use std::{path::PathBuf, process::Command, time::Duration};

use anyhow::{Context, Result};
use harness::{config::RuntimeConfig, db};
use sqlx::{Executor, PgPool};

const LOCAL_TEST_DATABASE_URL: &str =
    "postgres://blue_lagoon:blue_lagoon@localhost:55432/blue_lagoon";

pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("workspace root should exist")
        .to_path_buf()
}

pub async fn prepare_database() -> Result<(RuntimeConfig, PgPool)> {
    let database_url =
        configured_database_url().unwrap_or_else(|| LOCAL_TEST_DATABASE_URL.to_string());

    if should_bootstrap_local_postgres() {
        ensure_postgres_running()?;
    }

    let config = RuntimeConfig {
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
    };

    let pool = connect_with_retry(&config).await?;
    reset_database(&pool).await?;
    Ok((config, pool))
}

fn configured_database_url() -> Option<String> {
    std::env::var("BLUE_LAGOON_DATABASE_URL")
        .ok()
        .or_else(|| std::env::var("BLUE_LAGOON_TEST_DATABASE_URL").ok())
}

fn should_bootstrap_local_postgres() -> bool {
    configured_database_url().is_none()
}

pub async fn connect_with_retry(config: &RuntimeConfig) -> Result<PgPool> {
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

pub async fn reset_database(pool: &PgPool) -> Result<()> {
    pool.execute("DROP TABLE IF EXISTS episode_messages CASCADE")
        .await
        .context("failed to drop episode_messages")?;
    pool.execute("DROP TABLE IF EXISTS episodes CASCADE")
        .await
        .context("failed to drop episodes")?;
    pool.execute("DROP TABLE IF EXISTS ingress_events CASCADE")
        .await
        .context("failed to drop ingress_events")?;
    pool.execute("DROP TABLE IF EXISTS conversation_bindings CASCADE")
        .await
        .context("failed to drop conversation_bindings")?;
    pool.execute("DROP TABLE IF EXISTS audit_events CASCADE")
        .await
        .context("failed to drop audit_events")?;
    pool.execute("DROP TABLE IF EXISTS execution_records CASCADE")
        .await
        .context("failed to drop execution_records")?;
    pool.execute("DROP TABLE IF EXISTS schema_migrations CASCADE")
        .await
        .context("failed to drop schema_migrations")?;
    Ok(())
}
