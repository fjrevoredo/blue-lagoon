mod support;

use anyhow::Result;
use chrono::{Duration, Utc};
use harness::{
    execution::{self, NewExecutionRecord},
    recovery::{
        self, NewOperationalDiagnostic, NewRecoveryCheckpoint, NewWorkerLease,
        OperationalDiagnosticSeverity, RecoveryCheckpointKind, RecoveryCheckpointResolution,
        RecoveryCheckpointStatus, RecoveryDecision, RecoveryReasonCode, WorkerLeaseKind,
        WorkerLeaseStatus,
    },
};
use serde_json::json;
use serial_test::serial;
use uuid::Uuid;

#[tokio::test]
#[serial]
async fn recovery_checkpoint_persists_rehydrates_and_resolves() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let _ = &ctx.config;
        let trace_id = Uuid::now_v7();
        let execution_id = Uuid::now_v7();
        execution::insert(
            &ctx.pool,
            &NewExecutionRecord {
                execution_id,
                trace_id,
                trigger_kind: "supervisor_recovery".to_string(),
                synthetic_trigger: None,
                status: "started".to_string(),
                request_payload: json!({ "reason": "timeout_or_stall" }),
            },
        )
        .await?;

        let checkpoint_id = Uuid::now_v7();
        let checkpoint = recovery::create_recovery_checkpoint(
            &ctx.pool,
            &NewRecoveryCheckpoint {
                recovery_checkpoint_id: checkpoint_id,
                trace_id,
                execution_id: Some(execution_id),
                background_job_id: None,
                background_job_run_id: None,
                governed_action_execution_id: None,
                approval_request_id: None,
                checkpoint_kind: RecoveryCheckpointKind::Foreground,
                recovery_reason_code: RecoveryReasonCode::TimeoutOrStall,
                recovery_budget_remaining: 2,
                checkpoint_payload: json!({
                    "active_goal": "resume foreground conversation safely",
                    "selected_ingress_count": 3
                }),
            },
        )
        .await?;
        assert_eq!(checkpoint.recovery_checkpoint_id, checkpoint_id);
        assert_eq!(checkpoint.status, RecoveryCheckpointStatus::Open);
        assert_eq!(checkpoint.recovery_budget_remaining, 2);
        assert_eq!(
            checkpoint.checkpoint_payload["selected_ingress_count"],
            json!(3)
        );

        let open = recovery::list_open_recovery_checkpoints(&ctx.pool, 10).await?;
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].recovery_checkpoint_id, checkpoint_id);

        let resolved_at = Utc::now();
        let resolved = recovery::resolve_recovery_checkpoint(
            &ctx.pool,
            &RecoveryCheckpointResolution {
                recovery_checkpoint_id: checkpoint_id,
                status: RecoveryCheckpointStatus::Resolved,
                recovery_decision: RecoveryDecision::Continue,
                resolved_summary: Some("safe foreground continuation selected".to_string()),
                resolved_at,
            },
        )
        .await?;
        assert_eq!(resolved.status, RecoveryCheckpointStatus::Resolved);
        assert_eq!(resolved.recovery_decision, Some(RecoveryDecision::Continue));
        assert!(resolved.resolved_at.is_some());

        let open = recovery::list_open_recovery_checkpoints(&ctx.pool, 10).await?;
        assert!(open.is_empty());
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn worker_lease_persists_refreshes_releases_and_expires_due_leases() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let _ = &ctx.config;
        let trace_id = Uuid::now_v7();
        let now = Utc::now();
        let lease_id = Uuid::now_v7();
        let created = recovery::create_worker_lease(
            &ctx.pool,
            &NewWorkerLease {
                worker_lease_id: lease_id,
                trace_id,
                execution_id: None,
                background_job_id: None,
                background_job_run_id: None,
                governed_action_execution_id: None,
                worker_kind: WorkerLeaseKind::Background,
                lease_token: Uuid::now_v7(),
                worker_pid: Some(1234),
                lease_acquired_at: now,
                lease_expires_at: now + Duration::seconds(30),
                last_heartbeat_at: now,
                metadata: json!({ "class": "normal_background" }),
            },
        )
        .await?;
        assert_eq!(created.status, WorkerLeaseStatus::Active);
        assert_eq!(created.worker_kind, WorkerLeaseKind::Background);

        let refreshed = recovery::refresh_worker_lease(
            &ctx.pool,
            lease_id,
            now + Duration::seconds(10),
            now + Duration::seconds(60),
        )
        .await?;
        assert_eq!(
            refreshed.last_heartbeat_at.timestamp_millis(),
            (now + Duration::seconds(10)).timestamp_millis()
        );
        assert_eq!(
            refreshed.lease_expires_at.timestamp_millis(),
            (now + Duration::seconds(60)).timestamp_millis()
        );

        let released =
            recovery::release_worker_lease(&ctx.pool, lease_id, now + Duration::seconds(11))
                .await?;
        assert_eq!(released.status, WorkerLeaseStatus::Released);
        assert!(released.released_at.is_some());

        let expired_id = Uuid::now_v7();
        recovery::create_worker_lease(
            &ctx.pool,
            &NewWorkerLease {
                worker_lease_id: expired_id,
                trace_id,
                execution_id: None,
                background_job_id: None,
                background_job_run_id: None,
                governed_action_execution_id: None,
                worker_kind: WorkerLeaseKind::Foreground,
                lease_token: Uuid::now_v7(),
                worker_pid: None,
                lease_acquired_at: now,
                lease_expires_at: now + Duration::seconds(1),
                last_heartbeat_at: now,
                metadata: json!({}),
            },
        )
        .await?;
        let expired =
            recovery::expire_due_worker_leases(&ctx.pool, now + Duration::seconds(2)).await?;
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].worker_lease_id, expired_id);
        assert_eq!(expired[0].status, WorkerLeaseStatus::Expired);
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn operational_diagnostics_persist_and_list_recent_records() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let _ = &ctx.config;
        let diagnostic_id = Uuid::now_v7();
        let diagnostic = recovery::insert_operational_diagnostic(
            &ctx.pool,
            &NewOperationalDiagnostic {
                operational_diagnostic_id: diagnostic_id,
                trace_id: Some(Uuid::now_v7()),
                execution_id: None,
                subsystem: "recovery".to_string(),
                severity: OperationalDiagnosticSeverity::Warn,
                reason_code: "lease_expired".to_string(),
                summary: "background worker lease expired".to_string(),
                diagnostic_payload: json!({
                    "worker_kind": "background",
                    "recovery_reason_code": "timeout_or_stall"
                }),
            },
        )
        .await?;
        assert_eq!(diagnostic.operational_diagnostic_id, diagnostic_id);
        assert_eq!(diagnostic.severity, OperationalDiagnosticSeverity::Warn);

        let diagnostics = recovery::list_operational_diagnostics(&ctx.pool, 10).await?;
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].reason_code, "lease_expired");
        assert_eq!(
            diagnostics[0].diagnostic_payload["worker_kind"],
            "background"
        );
        Ok(())
    })
    .await
}
