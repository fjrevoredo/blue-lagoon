use std::process::Command;

use anyhow::{Context, Result};
use assert_cmd::prelude::*;
use predicates::prelude::*;
use sqlx::{Connection, Executor, PgConnection, PgPool};
use url::Url;
use uuid::Uuid;

const DEFAULT_TEST_POSTGRES_ADMIN_URL: &str =
    "postgres://blue_lagoon:blue_lagoon@localhost:55432/postgres";

#[test]
fn admin_help_lists_management_subcommands() -> Result<()> {
    let mut command = Command::cargo_bin("runtime")?;
    command.arg("admin").arg("--help");
    command
        .assert()
        .success()
        .stdout(predicate::str::contains("status"))
        .stdout(predicate::str::contains("foreground"))
        .stdout(predicate::str::contains("background"))
        .stdout(predicate::str::contains("wake-signals"));
    Ok(())
}

#[test]
fn admin_background_enqueue_help_lists_operator_arguments() -> Result<()> {
    let mut command = Command::cargo_bin("runtime")?;
    command
        .arg("admin")
        .arg("background")
        .arg("enqueue")
        .arg("--help");
    command
        .assert()
        .success()
        .stdout(predicate::str::contains("--job-kind"))
        .stdout(predicate::str::contains("--trigger-kind"))
        .stdout(predicate::str::contains("--conversation-ref"))
        .stdout(predicate::str::contains("--json"));
    Ok(())
}

#[tokio::test]
async fn admin_status_json_runs_against_a_real_database() -> Result<()> {
    let admin_database_url = std::env::var("BLUE_LAGOON_TEST_POSTGRES_ADMIN_URL")
        .unwrap_or_else(|_| DEFAULT_TEST_POSTGRES_ADMIN_URL.to_string());
    let database_name = format!("blue_lagoon_runtime_test_{}", Uuid::now_v7().simple());
    let database_url = disposable_database_url(&admin_database_url, &database_name)?;
    create_database(&admin_database_url, &database_name).await?;

    let pool = PgPool::connect(&database_url)
        .await
        .context("failed to connect to disposable runtime test database")?;
    harness::migration::apply_pending_migrations(&pool, env!("CARGO_PKG_VERSION")).await?;
    pool.close().await;

    let mut command = Command::cargo_bin("runtime")?;
    command
        .current_dir(harness::migration::workspace_root())
        .arg("admin")
        .arg("status")
        .arg("--json")
        .env("BLUE_LAGOON_DATABASE_URL", &database_url);

    command
        .assert()
        .success()
        .stdout(predicate::str::contains("\"schema\""))
        .stdout(predicate::str::contains("\"compatibility\": \"supported\""))
        .stdout(predicate::str::contains("\"pending_work\""));

    drop_database(&admin_database_url, &database_name).await?;
    Ok(())
}

fn disposable_database_url(admin_database_url: &str, database_name: &str) -> Result<String> {
    let mut url = Url::parse(admin_database_url)
        .with_context(|| format!("failed to parse admin database url '{admin_database_url}'"))?;
    url.set_path(&format!("/{database_name}"));
    Ok(url.into())
}

async fn create_database(admin_database_url: &str, database_name: &str) -> Result<()> {
    let mut connection = PgConnection::connect(admin_database_url)
        .await
        .context("failed to connect to postgres admin database")?;
    connection
        .execute(format!(r#"CREATE DATABASE "{database_name}""#).as_str())
        .await
        .with_context(|| format!("failed to create runtime test database '{database_name}'"))?;
    Ok(())
}

async fn drop_database(admin_database_url: &str, database_name: &str) -> Result<()> {
    let mut connection = PgConnection::connect(admin_database_url)
        .await
        .context("failed to reconnect to postgres admin database for cleanup")?;
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
    .ok();
    connection
        .execute(format!(r#"DROP DATABASE "{database_name}""#).as_str())
        .await
        .with_context(|| format!("failed to drop runtime test database '{database_name}'"))?;
    Ok(())
}
