mod support;

use anyhow::Result;
use chrono::Utc;
use serde_json::json;
use serial_test::serial;
use sqlx::Row;
use uuid::Uuid;

use harness::{
    audit::{self, NewAuditEvent},
    execution::{self, NewExecutionRecord},
    migration,
    schema::{self, SchemaPolicy},
};

#[tokio::test]
#[serial]
async fn migration_application_creates_foundation_and_foreground_tables() -> Result<()> {
    let (config, pool) = support::prepare_database().await?;
    let summary = migration::apply_pending_migrations(&pool, env!("CARGO_PKG_VERSION")).await?;

    assert_eq!(summary.discovered_versions, vec![1, 2]);
    let tables = sqlx::query(
        r#"
        SELECT table_name
        FROM information_schema.tables
        WHERE table_schema = 'public'
        ORDER BY table_name
        "#,
    )
    .fetch_all(&pool)
    .await?;

    let names = tables
        .into_iter()
        .map(|row| row.get::<String, _>("table_name"))
        .collect::<Vec<_>>();
    assert!(names.contains(&"schema_migrations".to_string()));
    assert!(names.contains(&"audit_events".to_string()));
    assert!(names.contains(&"execution_records".to_string()));
    assert!(names.contains(&"conversation_bindings".to_string()));
    assert!(names.contains(&"ingress_events".to_string()));
    assert!(names.contains(&"episodes".to_string()));
    assert!(names.contains(&"episode_messages".to_string()));
    assert_eq!(config.database.minimum_supported_schema_version, 1);
    Ok(())
}

#[tokio::test]
#[serial]
async fn startup_compatibility_reports_supported_and_unsupported_states() -> Result<()> {
    let (_config, pool) = support::prepare_database().await?;
    migration::apply_pending_migrations(&pool, env!("CARGO_PKG_VERSION")).await?;

    let supported = schema::verify(
        &pool,
        SchemaPolicy {
            minimum_supported_version: 1,
            expected_version: 2,
        },
    )
    .await?;
    assert_eq!(supported, 2);

    sqlx::query(
        r#"
        INSERT INTO schema_migrations
            (version, name, checksum, applied_at, app_version, applied_by, execution_ms)
        VALUES
            (3, 'future_schema', 'future', NOW(), 'test', 'test', 1)
        "#,
    )
    .execute(&pool)
    .await?;

    let error = schema::verify(
        &pool,
        SchemaPolicy {
            minimum_supported_version: 1,
            expected_version: 2,
        },
    )
    .await
    .expect_err("future schema should fail closed");
    assert!(error.to_string().contains("incompatible"));

    support::reset_database(&pool).await?;
    migration::ensure_schema_migrations_table(&pool).await?;
    sqlx::query(
        r#"
        INSERT INTO schema_migrations
            (version, name, checksum, applied_at, app_version, applied_by, execution_ms)
        VALUES
            (2, 'phase_2_only', 'gap', NOW(), 'test', 'test', 1)
        "#,
    )
    .execute(&pool)
    .await?;

    let incomplete_error = schema::verify(
        &pool,
        SchemaPolicy {
            minimum_supported_version: 1,
            expected_version: 2,
        },
    )
    .await
    .expect_err("incomplete schema history should fail closed");
    assert!(incomplete_error.to_string().contains("incompatible"));

    support::reset_database(&pool).await?;
    migration::apply_pending_migrations(&pool, env!("CARGO_PKG_VERSION")).await?;
    sqlx::query(
        r#"
        UPDATE schema_migrations
        SET checksum = 'tampered'
        WHERE version = 1
        "#,
    )
    .execute(&pool)
    .await?;

    let tampered_error = schema::verify(
        &pool,
        SchemaPolicy {
            minimum_supported_version: 1,
            expected_version: 2,
        },
    )
    .await
    .expect_err("tampered migration history should fail closed");
    assert!(tampered_error.to_string().contains("checksum"));
    Ok(())
}

#[tokio::test]
#[serial]
async fn audit_event_write_path_persists_rows() -> Result<()> {
    let (_config, pool) = support::prepare_database().await?;
    migration::apply_pending_migrations(&pool, env!("CARGO_PKG_VERSION")).await?;
    let execution_id = Uuid::now_v7();

    let event_id = audit::insert(
        &pool,
        &NewAuditEvent {
            loop_kind: "conscious".to_string(),
            subsystem: "harness".to_string(),
            event_kind: "component_test".to_string(),
            severity: "info".to_string(),
            trace_id: Uuid::now_v7(),
            execution_id: Some(execution_id),
            worker_pid: None,
            payload: json!({ "ok": true }),
        },
    )
    .await?;

    let events = audit::list_for_execution(&pool, execution_id).await?;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_id, event_id);
    assert_eq!(events[0].event_kind, "component_test");
    Ok(())
}

#[tokio::test]
#[serial]
async fn execution_record_write_path_persists_rows() -> Result<()> {
    let (_config, pool) = support::prepare_database().await?;
    migration::apply_pending_migrations(&pool, env!("CARGO_PKG_VERSION")).await?;

    let execution_id = Uuid::now_v7();
    let trace_id = Uuid::now_v7();
    execution::insert(
        &pool,
        &NewExecutionRecord {
            execution_id,
            trace_id,
            trigger_kind: "synthetic".to_string(),
            synthetic_trigger: Some("smoke".to_string()),
            status: "started".to_string(),
            request_payload: json!({
                "request_id": Uuid::now_v7(),
                "sent_at": Utc::now(),
            }),
        },
    )
    .await?;

    execution::mark_succeeded(
        &pool,
        execution_id,
        "smoke",
        1234,
        &json!({ "status": "completed" }),
    )
    .await?;

    let record = execution::get(&pool, execution_id).await?;
    assert_eq!(record.execution_id, execution_id);
    assert_eq!(record.trace_id, trace_id);
    assert_eq!(record.status, "completed");
    assert_eq!(record.worker_pid, Some(1234));
    assert!(record.completed_at.is_some());
    Ok(())
}
