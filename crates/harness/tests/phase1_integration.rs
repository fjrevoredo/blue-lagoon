mod support;

use anyhow::Result;
use contracts::WorkerRequest;
use serde_json::Value;
use serial_test::serial;
use sqlx::Row;
use std::{fs, process::Command, time::Duration};
use uuid::Uuid;

use harness::{
    audit, execution, migration,
    runtime::{self, HarnessOptions, HarnessOutcome, SyntheticTrigger},
    worker,
};

#[tokio::test]
#[serial]
async fn synthetic_trigger_runs_end_to_end_and_persists_outputs() -> Result<()> {
    let (config, pool) = support::prepare_database().await?;
    migration::apply_pending_migrations(&pool, env!("CARGO_PKG_VERSION")).await?;

    let outcome = runtime::run_harness_once(
        &config,
        HarnessOptions {
            once: true,
            idle: false,
            synthetic_trigger: Some(SyntheticTrigger::Smoke),
        },
    )
    .await?;

    let execution_id = match outcome {
        HarnessOutcome::SyntheticCompleted { execution_id, .. } => execution_id,
        HarnessOutcome::IdleVerified => panic!("synthetic trigger should not return idle"),
    };

    let record = execution::get(&pool, execution_id).await?;
    assert_eq!(record.status, "completed");
    assert!(record.worker_pid.is_some());
    assert!(record.response_payload.is_some());

    let audit_events = audit::list_for_execution(&pool, execution_id).await?;
    assert_eq!(audit_events.len(), 2);
    assert!(
        audit_events
            .iter()
            .any(|event| event.event_kind == "synthetic_trigger_received")
    );
    assert!(
        audit_events
            .iter()
            .any(|event| event.event_kind == "synthetic_trigger_completed")
    );
    Ok(())
}

#[tokio::test]
#[serial]
async fn idle_boot_verifies_schema_and_returns_idle() -> Result<()> {
    let (config, pool) = support::prepare_database().await?;
    migration::apply_pending_migrations(&pool, env!("CARGO_PKG_VERSION")).await?;

    let outcome = runtime::run_harness_once(
        &config,
        HarnessOptions {
            once: true,
            idle: true,
            synthetic_trigger: None,
        },
    )
    .await?;

    assert_eq!(outcome, HarnessOutcome::IdleVerified);
    Ok(())
}

#[tokio::test]
#[serial]
async fn timed_out_worker_is_terminated() -> Result<()> {
    let (mut config, pool) = support::prepare_database().await?;
    migration::apply_pending_migrations(&pool, env!("CARGO_PKG_VERSION")).await?;

    let pid_file = std::env::temp_dir().join(format!("blue-lagoon-worker-{}.pid", Uuid::now_v7()));
    let worker_binary = support::workers_binary()?;
    config.worker.command = worker_binary.to_string_lossy().into_owned();
    config.worker.args = vec![
        "stall-worker".to_string(),
        "--sleep-ms".to_string(),
        "5000".to_string(),
        "--pid-file".to_string(),
        pid_file.to_string_lossy().into_owned(),
    ];
    config.worker.timeout_ms = 100;

    let error = worker::launch_smoke_worker(
        &config,
        &WorkerRequest::smoke(Uuid::now_v7(), Uuid::now_v7(), "smoke"),
    )
    .await
    .expect_err("worker should time out");
    assert!(error.to_string().contains("timed out"));

    let pid = read_pid_file(&pid_file).await?;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !process_is_running(pid),
        "timed-out worker process {pid} should have been terminated"
    );

    let _ = fs::remove_file(pid_file);
    Ok(())
}

#[tokio::test]
#[serial]
async fn timed_out_foreground_run_is_marked_failed_and_audited() -> Result<()> {
    let (mut config, pool) = support::prepare_database().await?;
    migration::apply_pending_migrations(&pool, env!("CARGO_PKG_VERSION")).await?;

    let pid_file = std::env::temp_dir().join(format!("blue-lagoon-worker-{}.pid", Uuid::now_v7()));
    let worker_binary = support::workers_binary()?;
    config.worker.command = worker_binary.to_string_lossy().into_owned();
    config.worker.args = vec![
        "stall-worker".to_string(),
        "--sleep-ms".to_string(),
        "5000".to_string(),
        "--pid-file".to_string(),
        pid_file.to_string_lossy().into_owned(),
    ];
    config.worker.timeout_ms = 100;

    let error = runtime::run_harness_once(
        &config,
        HarnessOptions {
            once: true,
            idle: false,
            synthetic_trigger: Some(SyntheticTrigger::Smoke),
        },
    )
    .await
    .expect_err("timed-out run should fail");
    assert!(error.to_string().contains("timed out"));

    let row = sqlx::query(
        r#"
        SELECT execution_id, status, response_payload, completed_at
        FROM execution_records
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .fetch_one(&pool)
    .await?;

    let execution_id: Uuid = row.get("execution_id");
    let status: String = row.get("status");
    let response_payload: Option<Value> = row.get("response_payload");
    assert_eq!(status, "failed");
    assert!(
        row.get::<Option<chrono::DateTime<chrono::Utc>>, _>("completed_at")
            .is_some()
    );

    let response_payload =
        response_payload.expect("failed execution should persist an error payload");
    assert_eq!(
        response_payload.get("kind").and_then(Value::as_str),
        Some("worker_failure")
    );
    assert!(
        response_payload
            .get("message")
            .and_then(Value::as_str)
            .is_some_and(|message| message.contains("timed out"))
    );

    let audit_events = audit::list_for_execution(&pool, execution_id).await?;
    assert_eq!(audit_events.len(), 2);
    assert!(
        audit_events
            .iter()
            .any(|event| event.event_kind == "synthetic_trigger_received")
    );
    assert!(
        audit_events
            .iter()
            .any(|event| event.event_kind == "synthetic_trigger_failed")
    );

    let pid = read_pid_file(&pid_file).await?;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !process_is_running(pid),
        "timed-out worker process {pid} should have been terminated"
    );

    let _ = fs::remove_file(pid_file);
    Ok(())
}

async fn read_pid_file(path: &std::path::Path) -> Result<u32> {
    for _ in 0..20 {
        match fs::read_to_string(path) {
            Ok(contents) => return Ok(contents.trim().parse()?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(error) => return Err(error.into()),
        }
    }

    anyhow::bail!("worker pid file was never created")
}

fn process_is_running(pid: u32) -> bool {
    #[cfg(windows)]
    {
        let filter = format!("PID eq {pid}");
        let output = Command::new("tasklist")
            .args(["/FI", &filter])
            .output()
            .expect("tasklist should run");
        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout.contains(&pid.to_string())
    }

    #[cfg(not(windows))]
    {
        Command::new("ps")
            .args(["-p", &pid.to_string()])
            .output()
            .map(|output| {
                output.status.success()
                    && String::from_utf8_lossy(&output.stdout).contains(&pid.to_string())
            })
            .unwrap_or(false)
    }
}
