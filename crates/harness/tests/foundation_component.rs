mod support;

use anyhow::Result;
use chrono::Utc;
use futures_util::FutureExt;
use serde_json::json;
use serial_test::serial;
use sqlx::{Connection, PgConnection, Row};
use std::sync::{Arc, Mutex};
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
    support::with_clean_database(|ctx| async move {
        let summary =
            migration::apply_pending_migrations(&ctx.pool, env!("CARGO_PKG_VERSION")).await?;

        assert_eq!(summary.discovered_versions, vec![1, 2, 3, 4, 5, 6]);
        let tables = sqlx::query(
            r#"
            SELECT table_name
            FROM information_schema.tables
            WHERE table_schema = 'public'
            ORDER BY table_name
            "#,
        )
        .fetch_all(&ctx.pool)
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
        assert!(names.contains(&"proposals".to_string()));
        assert!(names.contains(&"memory_artifacts".to_string()));
        assert!(names.contains(&"self_model_artifacts".to_string()));
        assert!(names.contains(&"retrieval_artifacts".to_string()));
        assert!(names.contains(&"merge_decisions".to_string()));
        assert!(names.contains(&"execution_ingress_links".to_string()));
        assert!(names.contains(&"workspace_artifacts".to_string()));
        assert!(names.contains(&"workspace_scripts".to_string()));
        assert!(names.contains(&"workspace_script_versions".to_string()));
        assert!(names.contains(&"workspace_script_runs".to_string()));
        assert!(names.contains(&"approval_requests".to_string()));
        assert!(names.contains(&"governed_action_executions".to_string()));
        assert_eq!(ctx.config.database.minimum_supported_schema_version, 1);
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn startup_compatibility_reports_supported_and_unsupported_states() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let supported = schema::verify(
            &ctx.pool,
            SchemaPolicy {
                minimum_supported_version: 1,
                expected_version: 6,
            },
        )
        .await?;
        assert_eq!(supported, 6);
        Ok(())
    })
    .await?;

    support::with_clean_database(|ctx| async move {
        migration::ensure_schema_migrations_table(&ctx.pool).await?;
        sqlx::query(
            r#"
            INSERT INTO schema_migrations
                (version, name, checksum, applied_at, app_version, applied_by, execution_ms)
            VALUES
                (2, 'foreground_only', 'gap', NOW(), 'test', 'test', 1)
            "#,
        )
        .execute(&ctx.pool)
        .await?;

        let incomplete_error = schema::verify(
            &ctx.pool,
            SchemaPolicy {
                minimum_supported_version: 1,
                expected_version: 6,
            },
        )
        .await
        .expect_err("incomplete schema history should fail closed");
        assert!(incomplete_error.to_string().contains("incompatible"));
        Ok(())
    })
    .await?;

    support::with_migrated_database(|ctx| async move {
        sqlx::query(
            r#"
            INSERT INTO schema_migrations
                (version, name, checksum, applied_at, app_version, applied_by, execution_ms)
            VALUES
                (7, 'future_schema', 'future', NOW(), 'test', 'test', 1)
            "#,
        )
        .execute(&ctx.pool)
        .await?;

        let error = schema::verify(
            &ctx.pool,
            SchemaPolicy {
                minimum_supported_version: 1,
                expected_version: 6,
            },
        )
        .await
        .expect_err("future schema should fail closed");
        assert!(error.to_string().contains("incompatible"));
        Ok(())
    })
    .await?;

    support::with_migrated_database(|ctx| async move {
        sqlx::query(
            r#"
            UPDATE schema_migrations
            SET checksum = 'tampered'
            WHERE version = 1
            "#,
        )
        .execute(&ctx.pool)
        .await?;

        let tampered_error = schema::verify(
            &ctx.pool,
            SchemaPolicy {
                minimum_supported_version: 1,
                expected_version: 6,
            },
        )
        .await
        .expect_err("tampered migration history should fail closed");
        assert!(tampered_error.to_string().contains("checksum"));
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn migrate_normalizes_schema_migration_names_to_capability_labels() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        sqlx::query(
            r#"
            UPDATE schema_migrations
            SET name = CASE version
                WHEN 1 THEN 'legacy_foundation'
                WHEN 2 THEN 'legacy_foreground'
                ELSE name
            END
            WHERE version IN (1, 2)
            "#,
        )
        .execute(&ctx.pool)
        .await?;

        let summary =
            migration::apply_pending_migrations(&ctx.pool, env!("CARGO_PKG_VERSION")).await?;
        assert!(summary.applied_versions.is_empty());

        let rows = sqlx::query(
            r#"
            SELECT version, name
            FROM schema_migrations
            ORDER BY version
            "#,
        )
        .fetch_all(&ctx.pool)
        .await?;

        let names = rows
            .into_iter()
            .map(|row| (row.get::<i64, _>("version"), row.get::<String, _>("name")))
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![
                (1, "runtime_foundation".to_string()),
                (2, "foreground_loop".to_string()),
                (3, "migration_metadata_normalization".to_string()),
                (4, "canonical_continuity".to_string()),
                (5, "unconscious_loop".to_string()),
                (6, "workspace_and_governed_actions".to_string()),
            ]
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn audit_event_write_path_persists_rows() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let execution_id = Uuid::now_v7();

        let event_id = audit::insert(
            &ctx.pool,
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

        let events = audit::list_for_execution(&ctx.pool, execution_id).await?;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_id, event_id);
        assert_eq!(events[0].event_kind, "component_test");
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn execution_record_write_path_persists_rows() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let execution_id = Uuid::now_v7();
        let trace_id = Uuid::now_v7();
        execution::insert(
            &ctx.pool,
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
            &ctx.pool,
            execution_id,
            "smoke",
            1234,
            &json!({ "status": "completed" }),
        )
        .await?;

        let record = execution::get(&ctx.pool, execution_id).await?;
        assert_eq!(record.execution_id, execution_id);
        assert_eq!(record.trace_id, trace_id);
        assert_eq!(record.status, "completed");
        assert_eq!(record.worker_pid, Some(1234));
        assert!(record.completed_at.is_some());
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn disposable_database_fixture_cleans_up_after_panic() -> Result<()> {
    let observed_database_name = Arc::new(Mutex::new(None::<String>));
    let observed_database_name_for_task = observed_database_name.clone();

    std::panic::AssertUnwindSafe(support::with_clean_database(move |ctx| {
        let observed_database_name = observed_database_name_for_task.clone();
        async move {
            *observed_database_name
                .lock()
                .expect("observed database mutex should not be poisoned") =
                Some(ctx.database_name.clone());
            panic!("intentional fixture panic");
            #[allow(unreachable_code)]
            Ok::<(), anyhow::Error>(())
        }
    }))
    .catch_unwind()
    .await
    .expect_err("panic should propagate after cleanup");

    let database_name = observed_database_name
        .lock()
        .expect("observed database mutex should not be poisoned")
        .clone()
        .expect("test should record disposable database name before panicking");
    assert!(!database_exists(&resolved_test_postgres_admin_url(), &database_name).await?);
    Ok(())
}

#[tokio::test]
#[serial]
async fn disposable_database_fixture_sweeps_stale_test_databases() -> Result<()> {
    let admin_database_url = resolved_test_postgres_admin_url();
    let stale_database_name = format!("blue_lagoon_test_1_{}", Uuid::now_v7().simple());
    let stale_database_name_for_fixture = stale_database_name.clone();
    create_database(&admin_database_url, &stale_database_name).await?;
    assert!(database_exists(&admin_database_url, &stale_database_name).await?);

    support::with_clean_database(move |ctx| async move {
        assert_ne!(ctx.database_name, stale_database_name_for_fixture);
        Ok(())
    })
    .await?;

    assert!(!database_exists(&admin_database_url, &stale_database_name).await?);
    Ok(())
}

fn resolved_test_postgres_admin_url() -> String {
    std::env::var("BLUE_LAGOON_TEST_POSTGRES_ADMIN_URL").unwrap_or_else(|_| {
        "postgres://blue_lagoon:blue_lagoon@localhost:55432/postgres".to_string()
    })
}

async fn database_exists(admin_database_url: &str, database_name: &str) -> Result<bool> {
    let mut connection = PgConnection::connect(admin_database_url).await?;
    let exists: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM pg_database
            WHERE datname = $1
        )
        "#,
    )
    .bind(database_name)
    .fetch_one(&mut connection)
    .await?;
    Ok(exists)
}

async fn create_database(admin_database_url: &str, database_name: &str) -> Result<()> {
    let mut connection = PgConnection::connect(admin_database_url).await?;
    sqlx::query(format!(r#"CREATE DATABASE "{database_name}""#).as_str())
        .execute(&mut connection)
        .await?;
    Ok(())
}
