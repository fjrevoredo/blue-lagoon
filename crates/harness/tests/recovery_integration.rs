mod support;

use anyhow::Result;
use chrono::{Duration, Utc};
use harness::recovery::{
    self, NewWorkerLease, RecoveryCheckpointStatus, RecoveryDecision, WorkerLeaseKind,
    WorkerLeaseStatus,
};
use serde_json::json;
use serial_test::serial;
use uuid::Uuid;

#[tokio::test]
#[serial]
async fn expired_worker_lease_routes_through_recovery_decision_and_diagnostic() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let _ = &ctx.config;
        let now = Utc::now();
        let lease_id = Uuid::now_v7();
        recovery::create_worker_lease(
            &ctx.pool,
            &NewWorkerLease {
                worker_lease_id: lease_id,
                trace_id: Uuid::now_v7(),
                execution_id: None,
                background_job_id: None,
                background_job_run_id: None,
                governed_action_execution_id: None,
                worker_kind: WorkerLeaseKind::Background,
                lease_token: Uuid::now_v7(),
                worker_pid: None,
                lease_acquired_at: now,
                lease_expires_at: now + Duration::seconds(5),
                last_heartbeat_at: now,
                metadata: json!({ "test": "expired-background-lease" }),
            },
        )
        .await?;

        let outcomes =
            recovery::recover_expired_worker_leases(&ctx.pool, now + Duration::seconds(6)).await?;
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].lease.worker_lease_id, lease_id);
        assert_eq!(outcomes[0].lease.status, WorkerLeaseStatus::Expired);
        assert_eq!(outcomes[0].decision.decision, RecoveryDecision::Retry);
        assert_eq!(
            outcomes[0].checkpoint.status,
            RecoveryCheckpointStatus::Resolved
        );
        assert_eq!(
            outcomes[0].checkpoint.recovery_decision,
            Some(RecoveryDecision::Retry)
        );
        assert_eq!(
            outcomes[0].diagnostic.reason_code,
            "worker_lease_expired".to_string()
        );

        let open = recovery::list_open_recovery_checkpoints(&ctx.pool, 10).await?;
        assert!(open.is_empty());
        let diagnostics = recovery::list_operational_diagnostics(&ctx.pool, 10).await?;
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].diagnostic_payload["worker_lease_id"],
            json!(lease_id)
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn worker_lease_supervision_records_soft_warning_once() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let _ = &ctx.config;
        let now = Utc::now();
        let lease_id = Uuid::now_v7();
        recovery::create_worker_lease(
            &ctx.pool,
            &NewWorkerLease {
                worker_lease_id: lease_id,
                trace_id: Uuid::now_v7(),
                execution_id: None,
                background_job_id: None,
                background_job_run_id: None,
                governed_action_execution_id: None,
                worker_kind: WorkerLeaseKind::Foreground,
                lease_token: Uuid::now_v7(),
                worker_pid: None,
                lease_acquired_at: now,
                lease_expires_at: now + Duration::seconds(100),
                last_heartbeat_at: now,
                metadata: json!({ "test": "soft-warning" }),
            },
        )
        .await?;

        let first =
            recovery::supervise_worker_leases(&ctx.pool, now + Duration::seconds(85), 80).await?;
        assert_eq!(first.recovered_expired_leases.len(), 0);
        assert_eq!(first.soft_warning_diagnostics.len(), 1);
        assert_eq!(
            first.soft_warning_diagnostics[0].reason_code,
            "worker_lease_soft_warning"
        );

        let second =
            recovery::supervise_worker_leases(&ctx.pool, now + Duration::seconds(86), 80).await?;
        assert_eq!(second.recovered_expired_leases.len(), 0);
        assert_eq!(second.soft_warning_diagnostics.len(), 0);
        assert_eq!(
            recovery::list_operational_diagnostics(&ctx.pool, 10)
                .await?
                .len(),
            1
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn observed_worker_timeout_routes_active_lease_through_recovery() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let _ = &ctx.config;
        let now = Utc::now();
        let lease_id = Uuid::now_v7();
        recovery::create_worker_lease(
            &ctx.pool,
            &NewWorkerLease {
                worker_lease_id: lease_id,
                trace_id: Uuid::now_v7(),
                execution_id: None,
                background_job_id: None,
                background_job_run_id: None,
                governed_action_execution_id: None,
                worker_kind: WorkerLeaseKind::Background,
                lease_token: Uuid::now_v7(),
                worker_pid: Some(4242),
                lease_acquired_at: now,
                lease_expires_at: now + Duration::seconds(100),
                last_heartbeat_at: now,
                metadata: json!({ "test": "observed-timeout" }),
            },
        )
        .await?;

        let outcome = recovery::recover_observed_worker_timeout(
            &ctx.pool,
            lease_id,
            now + Duration::seconds(10),
            "integration_test_timeout",
            "worker subprocess timed out after 100 ms and was terminated",
        )
        .await?;

        assert_eq!(outcome.lease.worker_lease_id, lease_id);
        assert_eq!(outcome.lease.status, WorkerLeaseStatus::Terminated);
        assert_eq!(outcome.decision.decision, RecoveryDecision::Retry);
        assert_eq!(
            outcome.checkpoint.recovery_decision,
            Some(RecoveryDecision::Retry)
        );
        assert_eq!(
            outcome.diagnostic.reason_code,
            "worker_lease_timeout_observed"
        );
        assert_eq!(
            outcome.diagnostic.diagnostic_payload["source"],
            json!("integration_test_timeout")
        );
        assert!(
            recovery::list_open_recovery_checkpoints(&ctx.pool, 10)
                .await?
                .is_empty()
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn progress_refresh_extends_active_worker_lease_from_original_duration() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let _ = &ctx.config;
        let now = Utc::now();
        let lease_id = Uuid::now_v7();
        recovery::create_worker_lease(
            &ctx.pool,
            &NewWorkerLease {
                worker_lease_id: lease_id,
                trace_id: Uuid::now_v7(),
                execution_id: None,
                background_job_id: None,
                background_job_run_id: None,
                governed_action_execution_id: None,
                worker_kind: WorkerLeaseKind::Foreground,
                lease_token: Uuid::now_v7(),
                worker_pid: None,
                lease_acquired_at: now,
                lease_expires_at: now + Duration::seconds(30),
                last_heartbeat_at: now,
                metadata: json!({ "test": "progress-refresh" }),
            },
        )
        .await?;

        let refreshed = recovery::refresh_worker_lease_progress(
            &ctx.pool,
            lease_id,
            now + Duration::seconds(12),
        )
        .await?;

        assert_eq!(refreshed.worker_lease_id, lease_id);
        assert_eq!(
            refreshed.last_heartbeat_at.timestamp_millis(),
            (now + Duration::seconds(12)).timestamp_millis()
        );
        assert_eq!(
            refreshed.lease_expires_at.timestamp_millis(),
            (now + Duration::seconds(42)).timestamp_millis()
        );
        Ok(())
    })
    .await
}
