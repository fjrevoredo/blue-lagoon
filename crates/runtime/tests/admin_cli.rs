use std::process::Command;

use anyhow::{Context, Result};
use assert_cmd::prelude::*;
use contracts::ChannelKind;
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
        .stdout(predicate::str::contains("health"))
        .stdout(predicate::str::contains("diagnostics"))
        .stdout(predicate::str::contains("recovery"))
        .stdout(predicate::str::contains("schema"))
        .stdout(predicate::str::contains("foreground"))
        .stdout(predicate::str::contains("background"))
        .stdout(predicate::str::contains("approvals"))
        .stdout(predicate::str::contains("actions"))
        .stdout(predicate::str::contains("workspace"))
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

#[test]
fn admin_approvals_resolve_help_lists_operator_arguments() -> Result<()> {
    let mut command = Command::cargo_bin("runtime")?;
    command
        .arg("admin")
        .arg("approvals")
        .arg("resolve")
        .arg("--help");
    command
        .assert()
        .success()
        .stdout(predicate::str::contains("--approval-request-id"))
        .stdout(predicate::str::contains("--decision"))
        .stdout(predicate::str::contains("--actor-ref"))
        .stdout(predicate::str::contains("--reason"))
        .stdout(predicate::str::contains("--json"));
    Ok(())
}

#[test]
fn admin_workspace_runs_help_lists_filter_arguments() -> Result<()> {
    let mut command = Command::cargo_bin("runtime")?;
    command
        .arg("admin")
        .arg("workspace")
        .arg("runs")
        .arg("list")
        .arg("--help");
    command
        .assert()
        .success()
        .stdout(predicate::str::contains("--script-id"))
        .stdout(predicate::str::contains("--limit"))
        .stdout(predicate::str::contains("--json"));
    Ok(())
}

#[test]
fn admin_recovery_checkpoints_help_lists_operator_arguments() -> Result<()> {
    let mut command = Command::cargo_bin("runtime")?;
    command
        .arg("admin")
        .arg("recovery")
        .arg("checkpoints")
        .arg("list")
        .arg("--help");
    command
        .assert()
        .success()
        .stdout(predicate::str::contains("--open-only"))
        .stdout(predicate::str::contains("--limit"))
        .stdout(predicate::str::contains("--json"));
    Ok(())
}

#[test]
fn admin_recovery_leases_help_lists_operator_arguments() -> Result<()> {
    let mut command = Command::cargo_bin("runtime")?;
    command
        .arg("admin")
        .arg("recovery")
        .arg("leases")
        .arg("list")
        .arg("--help");
    command
        .assert()
        .success()
        .stdout(predicate::str::contains("--limit"))
        .stdout(predicate::str::contains("--soft-warning-threshold-percent"))
        .stdout(predicate::str::contains("--json"));
    Ok(())
}

#[test]
fn admin_recovery_supervise_help_lists_operator_arguments() -> Result<()> {
    let mut command = Command::cargo_bin("runtime")?;
    command
        .arg("admin")
        .arg("recovery")
        .arg("supervise")
        .arg("--help");
    command
        .assert()
        .success()
        .stdout(predicate::str::contains("--soft-warning-threshold-percent"))
        .stdout(predicate::str::contains("--actor-ref"))
        .stdout(predicate::str::contains("--reason"))
        .stdout(predicate::str::contains("--json"));
    Ok(())
}

#[test]
fn admin_foreground_schedules_list_help_lists_operator_arguments() -> Result<()> {
    let mut command = Command::cargo_bin("runtime")?;
    command
        .arg("admin")
        .arg("foreground")
        .arg("schedules")
        .arg("list")
        .arg("--help");
    command
        .assert()
        .success()
        .stdout(predicate::str::contains("--status"))
        .stdout(predicate::str::contains("--due-only"))
        .stdout(predicate::str::contains("--limit"))
        .stdout(predicate::str::contains("--json"));
    Ok(())
}

#[test]
fn admin_foreground_schedules_upsert_help_lists_operator_arguments() -> Result<()> {
    let mut command = Command::cargo_bin("runtime")?;
    command
        .arg("admin")
        .arg("foreground")
        .arg("schedules")
        .arg("upsert")
        .arg("--help");
    command
        .assert()
        .success()
        .stdout(predicate::str::contains("--task-key"))
        .stdout(predicate::str::contains("--internal-principal-ref"))
        .stdout(predicate::str::contains("--internal-conversation-ref"))
        .stdout(predicate::str::contains("--message-text"))
        .stdout(predicate::str::contains("--cadence-seconds"))
        .stdout(predicate::str::contains("--next-due-at"))
        .stdout(predicate::str::contains("--actor-ref"))
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

#[tokio::test]
async fn phase_six_admin_json_commands_run_against_a_real_database() -> Result<()> {
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

    assert_admin_json_command(
        &database_url,
        &["admin", "health", "summary", "--json"],
        "\"overall_status\": \"healthy\"",
    )?;
    assert_admin_json_command(
        &database_url,
        &["admin", "diagnostics", "list", "--json"],
        "[]",
    )?;
    assert_admin_json_command(
        &database_url,
        &["admin", "recovery", "checkpoints", "list", "--json"],
        "[]",
    )?;
    assert_admin_json_command(
        &database_url,
        &["admin", "recovery", "leases", "list", "--json"],
        "[]",
    )?;
    assert_admin_json_command(
        &database_url,
        &[
            "admin",
            "recovery",
            "supervise",
            "--actor-ref",
            "cli:test-operator",
            "--reason",
            "runtime-cli-verification",
            "--json",
        ],
        "\"actor_ref\": \"cli:test-operator\"",
    )?;
    assert_admin_json_command(
        &database_url,
        &["admin", "schema", "upgrade-path", "--json"],
        "\"compatibility\": \"supported\"",
    )?;

    drop_database(&admin_database_url, &database_name).await?;
    Ok(())
}

#[tokio::test]
async fn phase_seven_admin_scheduled_foreground_commands_run_against_a_real_database() -> Result<()>
{
    let admin_database_url = std::env::var("BLUE_LAGOON_TEST_POSTGRES_ADMIN_URL")
        .unwrap_or_else(|_| DEFAULT_TEST_POSTGRES_ADMIN_URL.to_string());
    let database_name = format!("blue_lagoon_runtime_test_{}", Uuid::now_v7().simple());
    let database_url = disposable_database_url(&admin_database_url, &database_name)?;
    create_database(&admin_database_url, &database_name).await?;

    let pool = PgPool::connect(&database_url)
        .await
        .context("failed to connect to disposable runtime test database")?;
    harness::migration::apply_pending_migrations(&pool, env!("CARGO_PKG_VERSION")).await?;
    harness::foreground::upsert_conversation_binding(
        &pool,
        &harness::foreground::NewConversationBinding {
            conversation_binding_id: Uuid::now_v7(),
            channel_kind: ChannelKind::Telegram,
            external_user_id: "42".to_string(),
            external_conversation_id: "24".to_string(),
            internal_principal_ref: "primary-user".to_string(),
            internal_conversation_ref: "telegram-primary".to_string(),
        },
    )
    .await?;
    pool.close().await;

    assert_admin_json_command(
        &database_url,
        &["admin", "foreground", "schedules", "list", "--json"],
        "[]",
    )?;
    assert_admin_json_command(
        &database_url,
        &[
            "admin",
            "foreground",
            "schedules",
            "upsert",
            "--task-key",
            "daily-checkin",
            "--internal-principal-ref",
            "primary-user",
            "--internal-conversation-ref",
            "telegram-primary",
            "--message-text",
            "Daily check-in",
            "--cadence-seconds",
            "600",
            "--cooldown-seconds",
            "300",
            "--actor-ref",
            "cli:test-operator",
            "--reason",
            "runtime-cli-verification",
            "--json",
        ],
        "\"action\": \"created\"",
    )?;
    assert_admin_json_command(
        &database_url,
        &[
            "admin",
            "foreground",
            "schedules",
            "show",
            "--task-key",
            "daily-checkin",
            "--json",
        ],
        "\"task_key\": \"daily-checkin\"",
    )?;
    assert_admin_json_command(
        &database_url,
        &[
            "admin",
            "foreground",
            "schedules",
            "list",
            "--status",
            "active",
            "--json",
        ],
        "\"conversation_binding_present\": true",
    )?;

    drop_database(&admin_database_url, &database_name).await?;
    Ok(())
}

fn assert_admin_json_command(
    database_url: &str,
    args: &[&str],
    expected_fragment: &str,
) -> Result<()> {
    let mut command = Command::cargo_bin("runtime")?;
    command
        .current_dir(harness::migration::workspace_root())
        .args(args)
        .env("BLUE_LAGOON_DATABASE_URL", database_url);

    command
        .assert()
        .success()
        .stdout(predicate::str::contains(expected_fragment));
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
