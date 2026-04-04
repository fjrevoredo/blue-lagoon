use std::{path::PathBuf, process::Command, time::Duration};

use anyhow::{Context, Result};
use harness::{config::RuntimeConfig, db};
use sqlx::{Executor, PgPool};

pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("workspace root should exist")
        .to_path_buf()
}

pub async fn prepare_database() -> Result<(RuntimeConfig, PgPool)> {
    ensure_postgres_running()?;

    let config = RuntimeConfig {
        app: harness::config::AppConfig {
            name: "blue-lagoon".to_string(),
            log_filter: "info".to_string(),
        },
        database: harness::config::DatabaseConfig {
            database_url: std::env::var("BLUE_LAGOON_TEST_DATABASE_URL").unwrap_or_else(|_| {
                "postgres://blue_lagoon:blue_lagoon@localhost:55432/blue_lagoon".to_string()
            }),
            minimum_supported_schema_version: 1,
        },
        harness: harness::config::HarnessConfig {
            allow_synthetic_smoke: true,
            default_wall_clock_budget_ms: 30_000,
        },
        worker: harness::config::WorkerConfig {
            timeout_ms: 20_000,
            command: String::new(),
            args: Vec::new(),
        },
    };

    let pool = connect_with_retry(&config).await?;
    reset_database(&pool).await?;
    Ok((config, pool))
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
