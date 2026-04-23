mod support;

use anyhow::Result;
use chrono::{Duration, Utc};
use contracts::{
    ApprovalResolutionDecision, BackgroundExecutionBudget, BackgroundTrigger,
    BackgroundTriggerKind, CapabilityScope, EnvironmentCapabilityScope, ExecutionCapabilityBudget,
    FilesystemCapabilityScope, GovernedActionFingerprint, GovernedActionKind,
    GovernedActionPayload, GovernedActionProposal, GovernedActionRiskTier, NetworkAccessPosture,
    SubprocessAction, UnconsciousJobKind, UnconsciousScope, WakeSignal, WakeSignalPriority,
    WakeSignalReason,
};
use harness::recovery::{
    self, NewWorkerLease, RecoveryApprovalState, RecoveryCheckpointStatus, RecoveryDecision,
    RecoveryReasonCode, WorkerLeaseKind, WorkerLeaseStatus,
};
use harness::{
    approval, background,
    execution::{self, NewExecutionRecord},
    governed_actions,
};
use serde_json::json;
use serial_test::serial;
use sqlx::Row;
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

#[tokio::test]
#[serial]
async fn approval_transition_routes_pending_request_through_recovery() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let requested_at = Utc::now();
        let execution_id = Uuid::now_v7();
        execution::insert(
            &ctx.pool,
            &NewExecutionRecord {
                execution_id,
                trace_id: Uuid::now_v7(),
                trigger_kind: "recovery_test".to_string(),
                synthetic_trigger: None,
                status: "started".to_string(),
                request_payload: json!({ "test": "approval_transition_routes_pending_request_through_recovery" }),
            },
        )
        .await?;
        let approval_request = approval::create_approval_request(
            &ctx.config,
            &ctx.pool,
            &approval::NewApprovalRequestRecord {
                approval_request_id: Uuid::now_v7(),
                trace_id: Uuid::now_v7(),
                execution_id: Some(execution_id),
                action_proposal_id: Uuid::now_v7(),
                action_fingerprint: GovernedActionFingerprint {
                    value: format!("fingerprint:{}", Uuid::now_v7()),
                },
                action_kind: GovernedActionKind::RunSubprocess,
                risk_tier: GovernedActionRiskTier::Tier2,
                title: "Approval transition".to_string(),
                consequence_summary: "Used to verify pending approval recovery.".to_string(),
                capability_scope: sample_capability_scope(),
                requested_by: "telegram:primary-user".to_string(),
                token: Uuid::now_v7().to_string(),
                requested_at,
                expires_at: requested_at + Duration::minutes(5),
            },
        )
        .await?;

        let outcome = recovery::recover_approval_request_transition(
            &ctx.pool,
            &approval_request,
            RecoveryApprovalState::Pending,
            requested_at,
            "approval_transition_pending",
        )
        .await?;

        assert_eq!(outcome.decision.decision, RecoveryDecision::Defer);
        assert_eq!(
            outcome.checkpoint.status,
            RecoveryCheckpointStatus::Resolved
        );
        assert_eq!(
            outcome.checkpoint.recovery_decision,
            Some(RecoveryDecision::Defer)
        );
        assert_eq!(
            outcome.checkpoint.approval_request_id,
            Some(approval_request.approval_request_id)
        );
        assert_eq!(
            outcome.diagnostic.reason_code,
            "approval_transition_pending"
        );
        assert!(outcome.diagnostic.summary.contains("pending approval"));
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
async fn approval_expiry_routes_request_through_reapproval_recovery() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let requested_at = Utc::now() - Duration::minutes(10);
        let execution_id = Uuid::now_v7();
        execution::insert(
            &ctx.pool,
            &NewExecutionRecord {
                execution_id,
                trace_id: Uuid::now_v7(),
                trigger_kind: "recovery_test".to_string(),
                synthetic_trigger: None,
                status: "started".to_string(),
                request_payload: json!({ "test": "approval_expiry_routes_request_through_reapproval_recovery" }),
            },
        )
        .await?;
        let approval_request = approval::create_approval_request(
            &ctx.config,
            &ctx.pool,
            &approval::NewApprovalRequestRecord {
                approval_request_id: Uuid::now_v7(),
                trace_id: Uuid::now_v7(),
                execution_id: Some(execution_id),
                action_proposal_id: Uuid::now_v7(),
                action_fingerprint: GovernedActionFingerprint {
                    value: format!("fingerprint:{}", Uuid::now_v7()),
                },
                action_kind: GovernedActionKind::RunSubprocess,
                risk_tier: GovernedActionRiskTier::Tier2,
                title: "Approval expiry".to_string(),
                consequence_summary: "Used to verify expired approval recovery.".to_string(),
                capability_scope: sample_capability_scope(),
                requested_by: "telegram:primary-user".to_string(),
                token: Uuid::now_v7().to_string(),
                requested_at,
                expires_at: requested_at + Duration::minutes(1),
            },
        )
        .await?;

        let expired = approval::expire_due_approval_requests(&ctx.pool, Utc::now()).await?;
        assert_eq!(expired.len(), 1);
        assert_eq!(
            expired[0].request.approval_request_id,
            approval_request.approval_request_id
        );

        let checkpoint_id: Uuid = sqlx::query_scalar(
            r#"
            SELECT recovery_checkpoint_id
            FROM recovery_checkpoints
            WHERE approval_request_id = $1
            ORDER BY created_at DESC, recovery_checkpoint_id DESC
            LIMIT 1
            "#,
        )
        .bind(approval_request.approval_request_id)
        .fetch_one(&ctx.pool)
        .await?;
        let checkpoint = recovery::get_recovery_checkpoint(&ctx.pool, checkpoint_id).await?;
        assert_eq!(
            checkpoint.recovery_reason_code,
            RecoveryReasonCode::ApprovalTransition
        );
        assert_eq!(
            checkpoint.recovery_decision,
            Some(RecoveryDecision::Reapprove)
        );
        assert_eq!(checkpoint.status, RecoveryCheckpointStatus::Resolved);

        let diagnostics = recovery::list_operational_diagnostics(&ctx.pool, 10).await?;
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.reason_code == "approval_request_expired"
                && diagnostic.diagnostic_payload["approval_request_id"]
                    == json!(approval_request.approval_request_id)
        }));
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
async fn wake_signal_policy_block_routes_through_recovery() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let now = Utc::now();
        let background_job_id = Uuid::now_v7();
        let execution_id = Uuid::now_v7();
        execution::insert(
            &ctx.pool,
            &NewExecutionRecord {
                execution_id,
                trace_id: Uuid::now_v7(),
                trigger_kind: "recovery_test".to_string(),
                synthetic_trigger: None,
                status: "started".to_string(),
                request_payload: json!({ "test": "wake_signal_policy_block_routes_through_recovery" }),
            },
        )
        .await?;
        background::insert_job(
            &ctx.pool,
            &background::NewBackgroundJob {
                background_job_id,
                trace_id: Uuid::now_v7(),
                job_kind: UnconsciousJobKind::RetrievalMaintenance,
                trigger: BackgroundTrigger {
                    trigger_id: Uuid::now_v7(),
                    trigger_kind: BackgroundTriggerKind::TimeSchedule,
                    requested_at: now,
                    reason_summary: "wake-signal recovery test".to_string(),
                    payload_ref: None,
                },
                deduplication_key: format!("recovery-test:{background_job_id}"),
                scope: UnconsciousScope {
                    episode_ids: Vec::new(),
                    memory_artifact_ids: Vec::new(),
                    retrieval_artifact_ids: Vec::new(),
                    self_model_artifact_id: None,
                    internal_principal_ref: None,
                    internal_conversation_ref: Some("telegram-primary".to_string()),
                    summary: "Wake-signal routing recovery".to_string(),
                },
                budget: BackgroundExecutionBudget {
                    iteration_budget: 1,
                    wall_clock_budget_ms: 30_000,
                    token_budget: 1_024,
                },
                status: background::BackgroundJobStatus::Planned,
                available_at: now,
                lease_expires_at: None,
                last_started_at: None,
                last_completed_at: None,
            },
        )
        .await?;

        let signal = WakeSignal {
            signal_id: Uuid::now_v7(),
            reason: WakeSignalReason::MaintenanceInsightReady,
            priority: WakeSignalPriority::Normal,
            reason_code: "foreground_channel_unavailable".to_string(),
            summary: "foreground routing is unavailable".to_string(),
            payload_ref: Some("background_job_run:latest".to_string()),
        };
        background::insert_wake_signal(
            &ctx.pool,
            &background::NewWakeSignalRecord {
                background_job_id,
                background_job_run_id: None,
                trace_id: Uuid::now_v7(),
                execution_id: Some(execution_id),
                signal: signal.clone(),
                status: background::WakeSignalStatus::PendingReview,
                requested_at: now,
                cooldown_until: None,
            },
        )
        .await?;
        let wake_signal = background::get_wake_signal(&ctx.pool, signal.signal_id).await?;

        let outcome = recovery::recover_wake_signal_policy_block(
            &ctx.pool,
            &wake_signal,
            now,
            "wake_signal_routing_rejected",
            "no foreground conversation binding is configured for wake-signal conversion",
        )
        .await?;

        assert_eq!(outcome.decision.decision, RecoveryDecision::Abandon);
        assert_eq!(
            outcome.checkpoint.recovery_decision,
            Some(RecoveryDecision::Abandon)
        );
        assert_eq!(
            outcome.checkpoint.recovery_reason_code,
            RecoveryReasonCode::IntegrityOrPolicyBlock
        );
        assert_eq!(
            outcome.diagnostic.reason_code,
            "wake_signal_routing_rejected"
        );
        assert!(
            outcome
                .diagnostic
                .summary
                .contains("no foreground conversation binding")
        );

        let checkpoint_row = sqlx::query(
            r#"
            SELECT checkpoint_payload_json
            FROM recovery_checkpoints
            WHERE background_job_id = $1
            ORDER BY created_at DESC, recovery_checkpoint_id DESC
            LIMIT 1
            "#,
        )
        .bind(background_job_id)
        .fetch_one(&ctx.pool)
        .await?;
        let payload: serde_json::Value = checkpoint_row.get("checkpoint_payload_json");
        assert_eq!(payload["wake_signal_id"], json!(signal.signal_id));

        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn foreground_stale_processing_routes_through_crash_recovery() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let execution_id = Uuid::now_v7();
        let trace_id = Uuid::now_v7();
        execution::insert(
            &ctx.pool,
            &NewExecutionRecord {
                execution_id,
                trace_id,
                trigger_kind: "recovery_test".to_string(),
                synthetic_trigger: None,
                status: "started".to_string(),
                request_payload: json!({ "test": "foreground_stale_processing_routes_through_crash_recovery" }),
            },
        )
        .await?;

        let outcome = recovery::recover_foreground_restart_trigger(
            &ctx.pool,
            recovery::ForegroundRestartRecoveryRequest {
                trace_id,
                execution_id,
                internal_conversation_ref: "telegram-primary",
                recovery_reason_code: RecoveryReasonCode::Crash,
                trigger_source: "telegram_foreground_processing_loop",
                decision_reason: "stale_processing_resume",
                selected_ingress_ids: &[Uuid::now_v7(), Uuid::now_v7()],
                primary_ingress_id: Uuid::now_v7(),
                recovery_mode: "backlog_recovery",
            },
            Utc::now(),
        )
        .await?;

        assert_eq!(outcome.decision.decision, RecoveryDecision::Continue);
        assert_eq!(outcome.checkpoint.status, RecoveryCheckpointStatus::Resolved);
        assert_eq!(outcome.checkpoint.recovery_reason_code, RecoveryReasonCode::Crash);
        assert_eq!(
            outcome.checkpoint.recovery_decision,
            Some(RecoveryDecision::Continue)
        );
        assert_eq!(
            outcome.diagnostic.reason_code,
            "foreground_processing_crash_recovered"
        );
        assert_eq!(
            outcome.checkpoint.checkpoint_payload["internal_conversation_ref"],
            json!("telegram-primary")
        );
        assert_eq!(
            outcome.checkpoint.checkpoint_payload["decision_reason"],
            json!("stale_processing_resume")
        );

        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn foreground_recovery_scan_routes_through_supervisor_restart_recovery() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let execution_id = Uuid::now_v7();
        let trace_id = Uuid::now_v7();
        execution::insert(
            &ctx.pool,
            &NewExecutionRecord {
                execution_id,
                trace_id,
                trigger_kind: "recovery_test".to_string(),
                synthetic_trigger: None,
                status: "started".to_string(),
                request_payload: json!({ "test": "foreground_recovery_scan_routes_through_supervisor_restart_recovery" }),
            },
        )
        .await?;

        let outcome = recovery::recover_foreground_restart_trigger(
            &ctx.pool,
            recovery::ForegroundRestartRecoveryRequest {
                trace_id,
                execution_id,
                internal_conversation_ref: "telegram-primary",
                recovery_reason_code: RecoveryReasonCode::SupervisorRestart,
                trigger_source: "telegram_foreground_recovery_scan",
                decision_reason: "pending_span_threshold",
                selected_ingress_ids: &[Uuid::now_v7()],
                primary_ingress_id: Uuid::now_v7(),
                recovery_mode: "backlog_recovery",
            },
            Utc::now(),
        )
        .await?;

        assert_eq!(outcome.decision.decision, RecoveryDecision::Continue);
        assert_eq!(
            outcome.checkpoint.recovery_reason_code,
            RecoveryReasonCode::SupervisorRestart
        );
        assert_eq!(
            outcome.diagnostic.reason_code,
            "foreground_supervisor_restart_recovered"
        );
        assert_eq!(
            outcome.checkpoint.checkpoint_payload["source"],
            json!("telegram_foreground_recovery_scan")
        );

        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn governed_action_terminal_failure_evidence_continues_without_rerun() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let planned = plan_governed_action_for_recovery(&ctx.config, &ctx.pool, false).await?;
        let started_at = Utc::now() - Duration::seconds(30);
        let execution_id = Uuid::now_v7();
        execution::insert(
            &ctx.pool,
            &NewExecutionRecord {
                execution_id,
                trace_id: planned.record.trace_id,
                trigger_kind: "governed_action_recovery_test".to_string(),
                synthetic_trigger: None,
                status: "started".to_string(),
                request_payload: json!({ "test": "governed_action_terminal_failure_evidence_continues_without_rerun" }),
            },
        )
        .await?;
        sqlx::query(
            r#"
            UPDATE governed_action_executions
            SET execution_id = $2, started_at = $3, updated_at = NOW()
            WHERE governed_action_execution_id = $1
            "#,
        )
        .bind(planned.record.governed_action_execution_id)
        .bind(execution_id)
        .bind(started_at)
        .execute(&ctx.pool)
        .await?;
        execution::mark_failed(
            &ctx.pool,
            execution_id,
            &json!({
                "status": "timed_out",
                "summary": "bounded subprocess timed out after 50 ms",
            }),
        )
        .await?;
        let lease_id = Uuid::now_v7();
        recovery::create_worker_lease(
            &ctx.pool,
            &NewWorkerLease {
                worker_lease_id: lease_id,
                trace_id: planned.record.trace_id,
                execution_id: Some(execution_id),
                background_job_id: None,
                background_job_run_id: None,
                governed_action_execution_id: Some(planned.record.governed_action_execution_id),
                worker_kind: WorkerLeaseKind::GovernedAction,
                lease_token: Uuid::now_v7(),
                worker_pid: Some(4242),
                lease_acquired_at: started_at,
                lease_expires_at: started_at + Duration::minutes(5),
                last_heartbeat_at: started_at,
                metadata: json!({ "test": "governed_action_terminal_failure_evidence_continues_without_rerun" }),
            },
        )
        .await?;

        let outcome = recovery::recover_observed_worker_timeout(
            &ctx.pool,
            lease_id,
            Utc::now(),
            "governed_action_test_timeout",
            "worker subprocess timed out after 50 ms and was terminated",
        )
        .await?;

        assert_eq!(outcome.decision.decision, RecoveryDecision::Continue);
        assert_eq!(
            outcome.checkpoint.checkpoint_payload["recovery_evidence_state"],
            json!("durable_completed")
        );
        let record = governed_actions::get_governed_action_execution(
            &ctx.pool,
            planned.record.governed_action_execution_id,
        )
        .await?;
        assert_eq!(record.status, contracts::GovernedActionStatus::Failed);
        assert!(record.completed_at.is_some());
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn governed_action_stall_with_approved_side_effect_requires_reapproval() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let planned = plan_governed_action_for_recovery(&ctx.config, &ctx.pool, true).await?;
        let approval_request = approval::create_approval_request(
            &ctx.config,
            &ctx.pool,
            &approval::NewApprovalRequestRecord {
                approval_request_id: Uuid::now_v7(),
                trace_id: planned.record.trace_id,
                execution_id: None,
                action_proposal_id: planned.record.action_proposal_id,
                action_fingerprint: planned.record.action_fingerprint.clone(),
                action_kind: planned.record.action_kind,
                risk_tier: planned.record.risk_tier,
                title: "Approved governed action".to_string(),
                consequence_summary: "Used to verify reapproval recovery.".to_string(),
                capability_scope: approval_required_capability_scope(),
                requested_by: "telegram:primary-user".to_string(),
                token: Uuid::now_v7().to_string(),
                requested_at: Utc::now() - Duration::minutes(5),
                expires_at: Utc::now() + Duration::minutes(10),
            },
        )
        .await?;
        governed_actions::attach_approval_request(
            &ctx.pool,
            planned.record.governed_action_execution_id,
            approval_request.approval_request_id,
        )
        .await?;
        approval::resolve_approval_request(
            &ctx.pool,
            &approval::ApprovalResolutionAttempt {
                token: approval_request.token.clone(),
                actor_ref: "telegram:primary-user".to_string(),
                expected_action_fingerprint: approval_request.action_fingerprint.clone(),
                decision: ApprovalResolutionDecision::Approved,
                reason: Some("recovery integration".to_string()),
                resolved_at: Utc::now() - Duration::minutes(2),
            },
        )
        .await?;
        let approved = governed_actions::sync_status_from_approval_resolution(
            &ctx.pool,
            planned.record.governed_action_execution_id,
            ApprovalResolutionDecision::Approved,
            None,
            Some("recovery integration"),
        )
        .await?;
        let execution_id = Uuid::now_v7();
        let started_at = Utc::now() - Duration::minutes(1);
        execution::insert(
            &ctx.pool,
            &NewExecutionRecord {
                execution_id,
                trace_id: approved.trace_id,
                trigger_kind: "governed_action_recovery_test".to_string(),
                synthetic_trigger: None,
                status: "started".to_string(),
                request_payload: json!({ "test": "governed_action_stall_with_approved_side_effect_requires_reapproval" }),
            },
        )
        .await?;
        sqlx::query(
            r#"
            UPDATE governed_action_executions
            SET execution_id = $2, started_at = $3, updated_at = NOW()
            WHERE governed_action_execution_id = $1
            "#,
        )
        .bind(approved.governed_action_execution_id)
        .bind(execution_id)
        .bind(started_at)
        .execute(&ctx.pool)
        .await?;
        recovery::create_worker_lease(
            &ctx.pool,
            &NewWorkerLease {
                worker_lease_id: Uuid::now_v7(),
                trace_id: approved.trace_id,
                execution_id: Some(execution_id),
                background_job_id: None,
                background_job_run_id: None,
                governed_action_execution_id: Some(approved.governed_action_execution_id),
                worker_kind: WorkerLeaseKind::GovernedAction,
                lease_token: Uuid::now_v7(),
                worker_pid: None,
                lease_acquired_at: started_at,
                lease_expires_at: started_at + Duration::seconds(5),
                last_heartbeat_at: started_at,
                metadata: json!({ "test": "governed_action_stall_with_approved_side_effect_requires_reapproval" }),
            },
        )
        .await?;

        let outcomes = recovery::recover_expired_worker_leases(&ctx.pool, Utc::now()).await?;
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].decision.decision, RecoveryDecision::Reapprove);
        assert_eq!(
            outcomes[0].checkpoint.checkpoint_payload["recovery_approval_state"],
            json!("missing_required")
        );
        let record = governed_actions::get_governed_action_execution(
            &ctx.pool,
            approved.governed_action_execution_id,
        )
        .await?;
        assert_eq!(record.status, contracts::GovernedActionStatus::Invalidated);
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn governed_action_stall_without_approval_routes_to_clarification() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let planned = plan_governed_action_for_recovery(&ctx.config, &ctx.pool, false).await?;
        let execution_id = Uuid::now_v7();
        let started_at = Utc::now() - Duration::minutes(1);
        execution::insert(
            &ctx.pool,
            &NewExecutionRecord {
                execution_id,
                trace_id: planned.record.trace_id,
                trigger_kind: "governed_action_recovery_test".to_string(),
                synthetic_trigger: None,
                status: "started".to_string(),
                request_payload: json!({ "test": "governed_action_stall_without_approval_routes_to_clarification" }),
            },
        )
        .await?;
        sqlx::query(
            r#"
            UPDATE governed_action_executions
            SET execution_id = $2, started_at = $3, updated_at = NOW()
            WHERE governed_action_execution_id = $1
            "#,
        )
        .bind(planned.record.governed_action_execution_id)
        .bind(execution_id)
        .bind(started_at)
        .execute(&ctx.pool)
        .await?;
        recovery::create_worker_lease(
            &ctx.pool,
            &NewWorkerLease {
                worker_lease_id: Uuid::now_v7(),
                trace_id: planned.record.trace_id,
                execution_id: Some(execution_id),
                background_job_id: None,
                background_job_run_id: None,
                governed_action_execution_id: Some(planned.record.governed_action_execution_id),
                worker_kind: WorkerLeaseKind::GovernedAction,
                lease_token: Uuid::now_v7(),
                worker_pid: None,
                lease_acquired_at: started_at,
                lease_expires_at: started_at + Duration::seconds(5),
                last_heartbeat_at: started_at,
                metadata: json!({ "test": "governed_action_stall_without_approval_routes_to_clarification" }),
            },
        )
        .await?;

        let outcomes = recovery::recover_expired_worker_leases(&ctx.pool, Utc::now()).await?;
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].decision.decision, RecoveryDecision::Clarify);
        assert_eq!(
            outcomes[0].checkpoint.checkpoint_payload["recovery_approval_state"],
            json!("not_required")
        );
        assert_eq!(
            outcomes[0].checkpoint.checkpoint_payload["recovery_evidence_state"],
            json!("ambiguous")
        );
        let record = governed_actions::get_governed_action_execution(
            &ctx.pool,
            planned.record.governed_action_execution_id,
        )
        .await?;
        assert_eq!(record.status, contracts::GovernedActionStatus::Failed);
        Ok(())
    })
    .await
}

fn sample_capability_scope() -> CapabilityScope {
    CapabilityScope {
        filesystem: FilesystemCapabilityScope {
            read_roots: vec![".".to_string()],
            write_roots: vec![],
        },
        network: NetworkAccessPosture::Disabled,
        environment: EnvironmentCapabilityScope {
            allow_variables: vec![],
        },
        execution: ExecutionCapabilityBudget {
            timeout_ms: 30_000,
            max_stdout_bytes: 16_384,
            max_stderr_bytes: 16_384,
        },
    }
}

fn approval_required_capability_scope() -> CapabilityScope {
    CapabilityScope {
        filesystem: FilesystemCapabilityScope {
            read_roots: vec![".".to_string()],
            write_roots: vec!["./docs".to_string()],
        },
        ..sample_capability_scope()
    }
}

async fn plan_governed_action_for_recovery(
    config: &harness::config::RuntimeConfig,
    pool: &sqlx::PgPool,
    approval_required: bool,
) -> Result<governed_actions::PlannedGovernedAction> {
    let proposal = GovernedActionProposal {
        proposal_id: Uuid::now_v7(),
        title: if approval_required {
            "Approval-scoped governed action".to_string()
        } else {
            "Immediate governed action".to_string()
        },
        rationale: Some("Used to verify governed-action recovery.".to_string()),
        action_kind: GovernedActionKind::RunSubprocess,
        requested_risk_tier: None,
        capability_scope: if approval_required {
            approval_required_capability_scope()
        } else {
            sample_capability_scope()
        },
        payload: GovernedActionPayload::RunSubprocess(SubprocessAction {
            command: if cfg!(windows) {
                "powershell".to_string()
            } else {
                "sh".to_string()
            },
            args: if cfg!(windows) {
                vec![
                    "-NoProfile".to_string(),
                    "-Command".to_string(),
                    "Write-Output 'recovery integration'".to_string(),
                ]
            } else {
                vec![
                    "-c".to_string(),
                    "printf 'recovery integration\\n'".to_string(),
                ]
            },
            working_directory: Some(".".to_string()),
        }),
    };
    let planned = governed_actions::plan_governed_action(
        config,
        pool,
        &governed_actions::GovernedActionPlanningRequest {
            governed_action_execution_id: Uuid::now_v7(),
            trace_id: Uuid::now_v7(),
            execution_id: None,
            proposal,
        },
    )
    .await?;
    match planned {
        governed_actions::GovernedActionPlanningOutcome::Planned(planned) => Ok(planned),
        other => panic!("expected planned governed action, got {other:?}"),
    }
}
