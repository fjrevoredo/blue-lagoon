mod support;

use anyhow::Result;
use chrono::{Duration, Utc};
use contracts::{
    CapabilityScope, ChannelKind, EnvironmentCapabilityScope, ExecutionCapabilityBudget,
    FilesystemCapabilityScope, GovernedActionKind, GovernedActionRiskTier, IngressEventKind,
    NetworkAccessPosture, NormalizedIngress, ScheduledForegroundTaskStatus, WorkspaceArtifactKind,
    WorkspaceScriptRunStatus,
};
use harness::{
    approval::{self, NewApprovalRequestRecord},
    audit,
    background::{
        self, BackgroundJobRunStatus, BackgroundJobStatus, NewBackgroundJob, NewBackgroundJobRun,
        NewWakeSignalRecord, WakeSignalStatus,
    },
    execution::{self, NewExecutionRecord},
    foreground::{self, NewConversationBinding, NewIngressEvent},
    governed_actions, management, recovery,
    workspace::{self, NewWorkspaceArtifact, NewWorkspaceScript, NewWorkspaceScriptRun},
};
use serde_json::json;
use serial_test::serial;
use uuid::Uuid;

#[tokio::test]
#[serial]
async fn runtime_status_reports_supported_schema_and_empty_pending_work() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let report = management::load_runtime_status(&ctx.config).await?;
        assert_eq!(report.schema.compatibility, "supported");
        assert_eq!(report.pending_work.pending_foreground_conversation_count, 0);
        assert_eq!(report.pending_work.pending_background_job_count, 0);
        assert_eq!(report.pending_work.pending_wake_signal_count, 0);
        assert!(!report.telegram.configured);
        assert!(!report.model_gateway.configured);
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn pending_foreground_summary_marks_backlog_recovery_when_thresholds_are_crossed()
-> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let now = Utc::now();
        for (index, seconds_ago) in [180_i64, 90, 0].into_iter().enumerate() {
            let occurred_at = now - Duration::seconds(seconds_ago);
            foreground::insert_ingress_event(
                &ctx.pool,
                &NewIngressEvent {
                    ingress: sample_ingress(
                        format!("event-{index}"),
                        "telegram-primary",
                        occurred_at,
                    ),
                    conversation_binding_id: None,
                    trace_id: Uuid::now_v7(),
                    execution_id: None,
                    status: "accepted".to_string(),
                    rejection_reason: None,
                },
            )
            .await?;
        }

        let summaries = management::list_pending_foreground_conversations(&ctx.config, 10).await?;
        assert_eq!(summaries.len(), 1);
        let summary = &summaries[0];
        assert_eq!(summary.internal_conversation_ref, "telegram-primary");
        assert_eq!(summary.pending_count, 3);
        assert_eq!(summary.suggested_mode, "backlog_recovery");
        assert_eq!(summary.decision_reason, "pending_span_threshold");
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn background_and_wake_signal_lists_surface_recent_operator_state() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let trace_id = Uuid::now_v7();
        let job_id = Uuid::now_v7();
        let run_id = Uuid::now_v7();
        let execution_id = seed_execution(&ctx.pool, trace_id).await?;
        let now = Utc::now();

        background::insert_job(
            &ctx.pool,
            &NewBackgroundJob {
                background_job_id: job_id,
                trace_id,
                job_kind: contracts::UnconsciousJobKind::MemoryConsolidation,
                trigger: sample_trigger(contracts::BackgroundTriggerKind::MaintenanceTrigger),
                deduplication_key: "management-test-job".to_string(),
                scope: contracts::UnconsciousScope {
                    internal_conversation_ref: Some("telegram-primary".to_string()),
                    summary: "management test scope".to_string(),
                    ..contracts::UnconsciousScope::default()
                },
                budget: sample_budget(),
                status: BackgroundJobStatus::Planned,
                available_at: now,
                lease_expires_at: None,
                last_started_at: None,
                last_completed_at: None,
            },
        )
        .await?;

        background::insert_job_run(
            &ctx.pool,
            &NewBackgroundJobRun {
                background_job_run_id: run_id,
                background_job_id: job_id,
                trace_id,
                execution_id: Some(execution_id),
                lease_token: Uuid::now_v7(),
                status: BackgroundJobRunStatus::Completed,
                worker_pid: Some(4242),
                lease_acquired_at: now - Duration::minutes(2),
                lease_expires_at: now + Duration::minutes(3),
                started_at: Some(now - Duration::minutes(2)),
                completed_at: Some(now - Duration::minutes(1)),
                result_payload: Some(json!({ "summary": "completed maintenance" })),
                failure_payload: None,
            },
        )
        .await?;

        background::insert_wake_signal(
            &ctx.pool,
            &NewWakeSignalRecord {
                background_job_id: job_id,
                background_job_run_id: Some(run_id),
                trace_id,
                execution_id: None,
                signal: contracts::WakeSignal {
                    signal_id: Uuid::now_v7(),
                    reason: contracts::WakeSignalReason::MaintenanceInsightReady,
                    priority: contracts::WakeSignalPriority::Normal,
                    reason_code: "maintenance_insight_ready".to_string(),
                    summary: "maintenance summary ready".to_string(),
                    payload_ref: Some("background_job_run:latest".to_string()),
                },
                status: WakeSignalStatus::PendingReview,
                requested_at: now,
                cooldown_until: None,
            },
        )
        .await?;

        let jobs = management::list_background_jobs(&ctx.config, 10).await?;
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].background_job_id, job_id);
        assert_eq!(jobs[0].latest_run_status.as_deref(), Some("completed"));
        assert_eq!(
            jobs[0].internal_conversation_ref.as_deref(),
            Some("telegram-primary")
        );

        let wake_signals = management::list_wake_signals(&ctx.config, 10).await?;
        assert_eq!(wake_signals.len(), 1);
        assert_eq!(wake_signals[0].background_job_id, job_id);
        assert_eq!(wake_signals[0].reason_code, "maintenance_insight_ready");
        assert_eq!(wake_signals[0].status, "pending_review");
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn phase_five_management_surfaces_workspace_approvals_and_actions() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let note_id = Uuid::now_v7();
        workspace::create_workspace_artifact(
            &ctx.config,
            &ctx.pool,
            &NewWorkspaceArtifact {
                workspace_artifact_id: note_id,
                trace_id: Some(Uuid::now_v7()),
                execution_id: None,
                artifact_kind: WorkspaceArtifactKind::Note,
                title: "Operator note".to_string(),
                content_text: Some("Management coverage".to_string()),
                metadata: json!({ "source": "management_component" }),
            },
        )
        .await?;

        let script_artifact_id = Uuid::now_v7();
        let script_id = Uuid::now_v7();
        let script_version_id = Uuid::now_v7();
        workspace::create_workspace_script(
            &ctx.config,
            &ctx.pool,
            &NewWorkspaceScript {
                workspace_script_id: script_id,
                workspace_artifact_id: script_artifact_id,
                workspace_script_version_id: script_version_id,
                trace_id: Some(Uuid::now_v7()),
                execution_id: None,
                title: "Management verification script".to_string(),
                metadata: json!({ "source": "management_component" }),
                language: "python".to_string(),
                entrypoint: Some("main.py".to_string()),
                content_text: "print('ok')\n".to_string(),
                change_summary: Some("initial version".to_string()),
            },
        )
        .await?;

        workspace::record_workspace_script_run(
            &ctx.pool,
            &NewWorkspaceScriptRun {
                workspace_script_run_id: Uuid::now_v7(),
                workspace_script_id: script_id,
                workspace_script_version_id: script_version_id,
                trace_id: Uuid::now_v7(),
                execution_id: None,
                governed_action_execution_id: None,
                approval_request_id: None,
                status: WorkspaceScriptRunStatus::Completed,
                risk_tier: GovernedActionRiskTier::Tier1,
                args: vec!["--check".to_string()],
                output_ref: Some("workspace://runs/check-1".to_string()),
                failure_summary: None,
                started_at: Some(Utc::now() - Duration::seconds(3)),
                completed_at: Some(Utc::now()),
            },
        )
        .await?;

        let proposal = contracts::GovernedActionProposal {
            proposal_id: Uuid::now_v7(),
            title: "Approval-gated subprocess".to_string(),
            rationale: Some("Used to verify management listings.".to_string()),
            action_kind: GovernedActionKind::RunSubprocess,
            requested_risk_tier: None,
            capability_scope: approval_required_scope(),
            payload: contracts::GovernedActionPayload::RunSubprocess(platform_echo_action("ok")),
        };
        let planned = governed_actions::plan_governed_action(
            &ctx.config,
            &ctx.pool,
            &governed_actions::GovernedActionPlanningRequest {
                governed_action_execution_id: Uuid::now_v7(),
                trace_id: Uuid::now_v7(),
                execution_id: None,
                proposal: proposal.clone(),
            },
        )
        .await?;
        let planned = match planned {
            governed_actions::GovernedActionPlanningOutcome::Planned(planned) => planned,
            other => panic!("expected approval-gated action, got {other:?}"),
        };
        let approval_request = approval::create_approval_request(
            &ctx.config,
            &ctx.pool,
            &NewApprovalRequestRecord {
                approval_request_id: Uuid::now_v7(),
                trace_id: planned.record.trace_id,
                execution_id: None,
                action_proposal_id: planned.record.action_proposal_id,
                action_fingerprint: planned.record.action_fingerprint.clone(),
                action_kind: planned.record.action_kind,
                risk_tier: planned.record.risk_tier,
                title: proposal.title,
                consequence_summary: "Used to verify pending approval management surfaces."
                    .to_string(),
                capability_scope: proposal.capability_scope,
                requested_by: "telegram:primary-user".to_string(),
                token: "management-approval".to_string(),
                requested_at: Utc::now(),
                expires_at: Utc::now() + Duration::minutes(15),
            },
        )
        .await?;
        governed_actions::attach_approval_request(
            &ctx.pool,
            planned.record.governed_action_execution_id,
            approval_request.approval_request_id,
        )
        .await?;

        let blocked = governed_actions::plan_governed_action(
            &ctx.config,
            &ctx.pool,
            &governed_actions::GovernedActionPlanningRequest {
                governed_action_execution_id: Uuid::now_v7(),
                trace_id: Uuid::now_v7(),
                execution_id: None,
                proposal: contracts::GovernedActionProposal {
                    proposal_id: Uuid::now_v7(),
                    title: "Blocked subprocess".to_string(),
                    rationale: Some("Used to verify blocked management listings.".to_string()),
                    action_kind: GovernedActionKind::RunSubprocess,
                    requested_risk_tier: None,
                    capability_scope: blocked_scope(),
                    payload: contracts::GovernedActionPayload::RunSubprocess(platform_echo_action(
                        "blocked",
                    )),
                },
            },
        )
        .await?;
        assert!(matches!(
            blocked,
            governed_actions::GovernedActionPlanningOutcome::Blocked(_)
        ));

        let status = management::load_runtime_status(&ctx.config).await?;
        assert_eq!(status.pending_work.pending_approval_request_count, 1);
        assert_eq!(
            status.pending_work.awaiting_approval_governed_action_count,
            1
        );
        assert_eq!(status.pending_work.blocked_governed_action_count, 1);

        let approvals = management::list_approval_requests(&ctx.config, None, 10).await?;
        assert_eq!(approvals.len(), 1);
        assert_eq!(
            approvals[0].approval_request_id,
            approval_request.approval_request_id
        );
        assert_eq!(approvals[0].status, "pending");

        let actions = management::list_governed_actions(&ctx.config, None, 10).await?;
        assert_eq!(actions.len(), 2);
        assert!(
            actions
                .iter()
                .any(|action| action.status == "awaiting_approval")
        );
        assert!(actions.iter().any(|action| action.status == "blocked"));

        let artifacts = management::list_workspace_artifact_summaries(&ctx.config, 10).await?;
        assert_eq!(artifacts.len(), 2);
        assert!(
            artifacts
                .iter()
                .any(|artifact| artifact.artifact_id == note_id)
        );
        assert!(
            artifacts
                .iter()
                .any(|artifact| artifact.artifact_id == script_artifact_id)
        );

        let scripts = management::list_workspace_scripts(&ctx.config, 10).await?;
        assert_eq!(scripts.len(), 1);
        assert_eq!(scripts[0].script_id, script_id);

        let runs = management::list_workspace_script_runs(&ctx.config, None, 10).await?;
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].workspace_script_id, script_id);
        assert_eq!(runs[0].status, "completed");

        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn operational_health_summary_records_recovery_pressure_anomalies() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let trace_id = Uuid::now_v7();
        let execution_id = seed_execution(&ctx.pool, trace_id).await?;

        recovery::create_recovery_checkpoint(
            &ctx.pool,
            &recovery::NewRecoveryCheckpoint {
                recovery_checkpoint_id: Uuid::now_v7(),
                trace_id,
                execution_id: Some(execution_id),
                background_job_id: None,
                background_job_run_id: None,
                governed_action_execution_id: None,
                approval_request_id: None,
                checkpoint_kind: recovery::RecoveryCheckpointKind::Background,
                recovery_reason_code: recovery::RecoveryReasonCode::TimeoutOrStall,
                recovery_budget_remaining: 1,
                checkpoint_payload: json!({
                    "source": "management_component"
                }),
            },
        )
        .await?;

        for offset_minutes in [0_i64, 5, 10] {
            recovery::insert_operational_diagnostic(
                &ctx.pool,
                &recovery::NewOperationalDiagnostic {
                    operational_diagnostic_id: Uuid::now_v7(),
                    trace_id: Some(trace_id),
                    execution_id: Some(execution_id),
                    subsystem: "recovery".to_string(),
                    severity: recovery::OperationalDiagnosticSeverity::Error,
                    reason_code: "worker_lease_timeout_observed".to_string(),
                    summary: "worker lease timeout observed during management health test"
                        .to_string(),
                    diagnostic_payload: json!({
                        "source": "management_component",
                        "offset_minutes": offset_minutes,
                    }),
                },
            )
            .await?;
        }

        let summary = management::load_operational_health_summary(&ctx.config).await?;
        assert_eq!(summary.overall_status, "unhealthy");
        assert_eq!(summary.recovery.open_checkpoint_count, 1);
        assert_eq!(summary.diagnostics.error_count, 3);
        assert!(
            summary
                .anomalies
                .iter()
                .any(|anomaly| anomaly.anomaly_kind == "repeated_reason")
        );
        assert!(
            summary
                .anomalies
                .iter()
                .any(|anomaly| anomaly.anomaly_kind == "failure_pressure")
        );
        assert!(
            summary
                .anomalies
                .iter()
                .any(|anomaly| anomaly.anomaly_kind == "recovery_pressure")
        );

        let diagnostics = management::list_recent_operational_diagnostics(&ctx.config, 20).await?;
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.reason_code == "operational_repeated_condition_detected"
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.reason_code == "operational_failure_pressure_detected"
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.reason_code == "operational_recovery_pressure_detected"
        }));
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn recovery_and_diagnostic_lists_surface_recent_operator_visibility() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let trace_id = Uuid::now_v7();
        let execution_id = seed_execution(&ctx.pool, trace_id).await?;
        let checkpoint = recovery::create_recovery_checkpoint(
            &ctx.pool,
            &recovery::NewRecoveryCheckpoint {
                recovery_checkpoint_id: Uuid::now_v7(),
                trace_id,
                execution_id: Some(execution_id),
                background_job_id: None,
                background_job_run_id: None,
                governed_action_execution_id: None,
                approval_request_id: None,
                checkpoint_kind: recovery::RecoveryCheckpointKind::Foreground,
                recovery_reason_code: recovery::RecoveryReasonCode::Crash,
                recovery_budget_remaining: 1,
                checkpoint_payload: json!({
                    "source": "management_component_visibility"
                }),
            },
        )
        .await?;
        recovery::resolve_recovery_checkpoint(
            &ctx.pool,
            &recovery::RecoveryCheckpointResolution {
                recovery_checkpoint_id: checkpoint.recovery_checkpoint_id,
                status: recovery::RecoveryCheckpointStatus::Resolved,
                recovery_decision: recovery::RecoveryDecision::Continue,
                resolved_summary: Some("fresh worker continuation is safe".to_string()),
                resolved_at: Utc::now(),
            },
        )
        .await?;

        recovery::insert_operational_diagnostic(
            &ctx.pool,
            &recovery::NewOperationalDiagnostic {
                operational_diagnostic_id: Uuid::now_v7(),
                trace_id: Some(trace_id),
                execution_id: Some(execution_id),
                subsystem: "recovery".to_string(),
                severity: recovery::OperationalDiagnosticSeverity::Warn,
                reason_code: "worker_lease_soft_warning".to_string(),
                summary: "worker lease is nearing expiry".to_string(),
                diagnostic_payload: json!({
                    "source": "management_component_visibility"
                }),
            },
        )
        .await?;

        let checkpoints = management::list_recovery_checkpoints(&ctx.config, false, 10).await?;
        assert_eq!(checkpoints.len(), 1);
        assert_eq!(
            checkpoints[0].recovery_checkpoint_id,
            checkpoint.recovery_checkpoint_id
        );
        assert_eq!(checkpoints[0].checkpoint_kind, "foreground");
        assert_eq!(checkpoints[0].recovery_reason_code, "crash");
        assert_eq!(checkpoints[0].status, "resolved");
        assert_eq!(
            checkpoints[0].recovery_decision.as_deref(),
            Some("continue")
        );

        let diagnostics = management::list_recent_operational_diagnostics(&ctx.config, 10).await?;
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].reason_code, "worker_lease_soft_warning");
        assert_eq!(diagnostics[0].severity, "warn");
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn recovery_lease_list_surfaces_stalled_work_inspection() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let trace_id = Uuid::now_v7();
        let execution_id = seed_execution(&ctx.pool, trace_id).await?;
        let now = Utc::now();

        let active_lease = recovery::create_worker_lease(
            &ctx.pool,
            &recovery::NewWorkerLease {
                worker_lease_id: Uuid::now_v7(),
                trace_id,
                execution_id: Some(execution_id),
                background_job_id: None,
                background_job_run_id: None,
                governed_action_execution_id: None,
                worker_kind: recovery::WorkerLeaseKind::Background,
                lease_token: Uuid::now_v7(),
                worker_pid: Some(4242),
                lease_acquired_at: now - Duration::minutes(8),
                lease_expires_at: now + Duration::minutes(1),
                last_heartbeat_at: now - Duration::seconds(30),
                metadata: json!({
                    "source": "management_component_recovery_lease_list"
                }),
            },
        )
        .await?;

        let leases = management::list_active_worker_leases(&ctx.config, 10, 80).await?;
        assert_eq!(leases.len(), 1);
        assert_eq!(leases[0].worker_lease_id, active_lease.worker_lease_id);
        assert_eq!(leases[0].worker_kind, "background");
        assert_eq!(leases[0].lease_status, "active");
        assert_eq!(leases[0].supervision_status, "soft_warning");
        assert_eq!(leases[0].execution_id, Some(execution_id));
        assert_eq!(leases[0].background_job_id, None);
        assert_eq!(leases[0].background_job_run_id, None);
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn scheduled_foreground_management_upsert_list_and_show_are_auditable() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        foreground::upsert_conversation_binding(
            &ctx.pool,
            &NewConversationBinding {
                conversation_binding_id: Uuid::now_v7(),
                channel_kind: ChannelKind::Telegram,
                external_user_id: "42".to_string(),
                external_conversation_id: "24".to_string(),
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
            },
        )
        .await?;

        let next_due_at = Utc::now() + Duration::minutes(10);
        let upserted = management::upsert_scheduled_foreground_task(
            &ctx.config,
            management::UpsertScheduledForegroundTaskRequest {
                task_key: "daily-checkin".to_string(),
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
                message_text: "Daily check-in".to_string(),
                cadence_seconds: 600,
                cooldown_seconds: Some(300),
                next_due_at: Some(next_due_at),
                status: ScheduledForegroundTaskStatus::Active,
                actor_ref: "cli:primary-user".to_string(),
                reason: Some("scheduled foreground management coverage".to_string()),
            },
        )
        .await?;

        assert_eq!(upserted.action, "created");
        assert_eq!(upserted.task.task_key, "daily-checkin");
        assert_eq!(upserted.task.status, "active");
        assert!(upserted.task.conversation_binding_present);
        assert_eq!(upserted.task.cooldown_seconds, 300);

        let listed = management::list_scheduled_foreground_tasks(
            &ctx.config,
            Some(ScheduledForegroundTaskStatus::Active),
            false,
            10,
        )
        .await?;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].task_key, "daily-checkin");
        assert_eq!(listed[0].channel_kind, "telegram");

        let shown = management::get_scheduled_foreground_task(&ctx.config, "daily-checkin")
            .await?
            .expect("scheduled task should exist");
        assert_eq!(
            shown.scheduled_foreground_task_id,
            upserted.task.scheduled_foreground_task_id
        );
        assert_eq!(shown.message_text, "Daily check-in");

        let audit_events = audit::list_for_trace(&ctx.pool, upserted.trace_id).await?;
        assert!(
            audit_events
                .iter()
                .any(|event| event.event_kind == "management_scheduled_foreground_upsert_requested")
        );
        assert!(
            audit_events
                .iter()
                .any(|event| event.event_kind == "management_scheduled_foreground_upsert_completed")
        );
        Ok(())
    })
    .await
}

fn sample_ingress(
    external_event_id: String,
    internal_conversation_ref: &str,
    occurred_at: chrono::DateTime<chrono::Utc>,
) -> NormalizedIngress {
    NormalizedIngress {
        ingress_id: Uuid::now_v7(),
        channel_kind: ChannelKind::Telegram,
        external_user_id: "telegram-user".to_string(),
        external_conversation_id: "telegram-chat".to_string(),
        external_event_id,
        external_message_id: None,
        internal_principal_ref: "primary-user".to_string(),
        internal_conversation_ref: internal_conversation_ref.to_string(),
        event_kind: IngressEventKind::MessageCreated,
        occurred_at,
        text_body: Some("hello".to_string()),
        reply_to: None,
        attachments: Vec::new(),
        command_hint: None,
        approval_payload: None,
        raw_payload_ref: Some("management-test".to_string()),
    }
}

fn sample_trigger(kind: contracts::BackgroundTriggerKind) -> contracts::BackgroundTrigger {
    contracts::BackgroundTrigger {
        trigger_id: Uuid::now_v7(),
        trigger_kind: kind,
        requested_at: Utc::now(),
        reason_summary: "management test trigger".to_string(),
        payload_ref: None,
    }
}

fn sample_budget() -> contracts::BackgroundExecutionBudget {
    contracts::BackgroundExecutionBudget {
        iteration_budget: 2,
        wall_clock_budget_ms: 120_000,
        token_budget: 6_000,
    }
}

async fn seed_execution(pool: &sqlx::PgPool, trace_id: Uuid) -> Result<Uuid> {
    let execution_id = Uuid::now_v7();
    execution::insert(
        pool,
        &NewExecutionRecord {
            execution_id,
            trace_id,
            trigger_kind: "management_test".to_string(),
            synthetic_trigger: None,
            status: "started".to_string(),
            request_payload: json!({
                "request_id": Uuid::now_v7(),
                "sent_at": Utc::now(),
                "kind": "management_test"
            }),
        },
    )
    .await?;
    Ok(execution_id)
}

fn immediate_scope() -> CapabilityScope {
    CapabilityScope {
        filesystem: FilesystemCapabilityScope {
            read_roots: vec![support::workspace_root().display().to_string()],
            write_roots: Vec::new(),
        },
        network: NetworkAccessPosture::Disabled,
        environment: EnvironmentCapabilityScope {
            allow_variables: Vec::new(),
        },
        execution: ExecutionCapabilityBudget {
            timeout_ms: 30_000,
            max_stdout_bytes: 65_536,
            max_stderr_bytes: 32_768,
        },
    }
}

fn approval_required_scope() -> CapabilityScope {
    CapabilityScope {
        filesystem: FilesystemCapabilityScope {
            read_roots: vec![support::workspace_root().display().to_string()],
            write_roots: vec![support::workspace_root().join("docs").display().to_string()],
        },
        ..immediate_scope()
    }
}

fn blocked_scope() -> CapabilityScope {
    CapabilityScope {
        environment: EnvironmentCapabilityScope {
            allow_variables: vec!["HOME".to_string()],
        },
        ..approval_required_scope()
    }
}

fn platform_echo_action(message: &str) -> contracts::SubprocessAction {
    if cfg!(windows) {
        contracts::SubprocessAction {
            command: "powershell".to_string(),
            args: vec![
                "-NoProfile".to_string(),
                "-Command".to_string(),
                format!("Write-Output '{}'", message.replace('\'', "''")),
            ],
            working_directory: Some(support::workspace_root().display().to_string()),
        }
    } else {
        contracts::SubprocessAction {
            command: "sh".to_string(),
            args: vec![
                "-c".to_string(),
                format!("printf '%s\\n' '{}'", message.replace('\'', "'\\''")),
            ],
            working_directory: Some(support::workspace_root().display().to_string()),
        }
    }
}
