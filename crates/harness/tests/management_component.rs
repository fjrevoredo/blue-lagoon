mod support;

use anyhow::Result;
use chrono::{Duration, Utc};
use contracts::{
    CapabilityScope, ChannelKind, EnvironmentCapabilityScope, ExecutionCapabilityBudget,
    FilesystemCapabilityScope, GovernedActionKind, GovernedActionRiskTier, IngressEventKind,
    LoopKind, ModelBudget, ModelCallPurpose, ModelCallRequest, ModelCallResponse, ModelInput,
    ModelInputMessage, ModelMessageRole, ModelOutput, ModelOutputMode, ModelProviderKind,
    ModelUsage, NetworkAccessPosture, NormalizedIngress, ScheduledForegroundTaskStatus, ToolPolicy,
    WorkspaceArtifactKind, WorkspaceScriptRunStatus,
};
use harness::{
    approval::{self, NewApprovalRequestRecord},
    audit,
    background::{
        self, BackgroundJobRunStatus, BackgroundJobStatus, NewBackgroundJob, NewBackgroundJobRun,
        NewWakeSignalRecord, WakeSignalStatus,
    },
    causal_links::{self, NewCausalLink},
    config::{ResolvedForegroundModelRouteConfig, ResolvedModelGatewayConfig},
    continuity,
    execution::{self, NewExecutionRecord},
    foreground::{self, NewConversationBinding, NewIngressEvent},
    governed_actions, identity, management, model_calls, recovery, scheduled_foreground,
    workspace::{self, NewWorkspaceArtifact, NewWorkspaceScript, NewWorkspaceScriptRun},
};
use serde_json::json;
use serial_test::serial;
use sqlx::{PgPool, Row};
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
async fn identity_reset_requires_force_and_archives_active_state() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let rejected = management::reset_identity(
            &ctx.config,
            management::IdentityResetRequest {
                actor_ref: "cli:primary-user".to_string(),
                reason: Some("missing confirmation".to_string()),
                force: false,
            },
        )
        .await;
        assert!(
            rejected
                .expect_err("identity reset without force should fail")
                .to_string()
                .contains("--force")
        );

        let interview_id = Uuid::now_v7();
        identity::insert_identity_interview(
            &ctx.pool,
            &identity::NewIdentityInterview {
                identity_interview_id: interview_id,
                status: "in_progress".to_string(),
                current_step: "name".to_string(),
                answered_fields: json!({}),
                required_fields: json!(["name"]),
                last_prompt_text: Some("What name should I use?".to_string()),
                selected_template_id: None,
                payload: json!({ "test": "identity_reset" }),
            },
        )
        .await?;
        identity::insert_identity_item(
            &ctx.pool,
            &identity::NewIdentityItem {
                identity_item_id: Uuid::now_v7(),
                self_model_artifact_id: None,
                proposal_id: None,
                trace_id: None,
                stability_class: "stable".to_string(),
                category: "trait".to_string(),
                item_key: "name".to_string(),
                value_text: "Lagoon".to_string(),
                confidence: 1.0,
                weight: Some(1.0),
                provenance_kind: "operator_seed".to_string(),
                source_kind: "identity_kickstart".to_string(),
                merge_policy: "replace_key".to_string(),
                status: "active".to_string(),
                evidence_refs: json!([]),
                valid_from: Some(Utc::now()),
                valid_to: None,
                supersedes_item_id: None,
                payload: json!({ "test": "identity_reset" }),
            },
        )
        .await?;
        identity::record_lifecycle_transition(
            &ctx.pool,
            &identity::NewIdentityLifecycle {
                identity_lifecycle_id: Uuid::now_v7(),
                status: "current".to_string(),
                lifecycle_state: "identity_kickstart_in_progress".to_string(),
                active_self_model_artifact_id: None,
                active_interview_id: Some(interview_id),
                transition_reason: "component test seed".to_string(),
                transitioned_by: "test".to_string(),
                kickstart_started_at: Some(Utc::now()),
                kickstart_completed_at: None,
                reset_at: None,
                payload: json!({ "test": "identity_reset" }),
            },
        )
        .await?;

        let report = management::reset_identity(
            &ctx.config,
            management::IdentityResetRequest {
                actor_ref: "cli:primary-user".to_string(),
                reason: Some("restart identity formation".to_string()),
                force: true,
            },
        )
        .await?;

        assert_eq!(report.actor_ref, "cli:primary-user");
        assert_eq!(
            report.previous_lifecycle_state.as_deref(),
            Some("identity_kickstart_in_progress")
        );
        assert_eq!(report.lifecycle_state, "bootstrap_seed_only");
        assert_eq!(report.superseded_identity_item_count, 1);
        assert_eq!(report.cancelled_interview_count, 1);

        let active_item_count: i64 = sqlx::query(
            "SELECT COUNT(*)::BIGINT AS count FROM identity_items WHERE status = 'active'",
        )
        .fetch_one(&ctx.pool)
        .await?
        .get("count");
        assert_eq!(active_item_count, 0);

        let lifecycle = identity::get_current_lifecycle(&ctx.pool)
            .await?
            .expect("reset should write current lifecycle");
        assert_eq!(lifecycle.lifecycle_state, "bootstrap_seed_only");
        assert_eq!(lifecycle.transitioned_by, "cli:primary-user");

        let interview = identity::get_identity_interview(&ctx.pool, interview_id)
            .await?
            .expect("seeded interview should remain available");
        assert_eq!(interview.status, "cancelled");

        let audit_events = audit::list_for_trace(&ctx.pool, report.trace_id).await?;
        let event_kinds: Vec<_> = audit_events
            .iter()
            .map(|event| event.event_kind.as_str())
            .collect();
        assert!(event_kinds.contains(&"management_identity_reset_requested"));
        assert!(event_kinds.contains(&"management_identity_reset_completed"));
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn identity_status_and_show_surface_active_identity_summary() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        seed_identity_item(&ctx.pool, "stable", "name", "name", "Blue Lagoon").await?;
        seed_identity_item(
            &ctx.pool,
            "stable",
            "foundational_value",
            "clarity",
            "clarity",
        )
        .await?;
        seed_identity_item(
            &ctx.pool,
            "stable",
            "enduring_boundary",
            "no_hidden_autonomy",
            "Do not claim hidden autonomy.",
        )
        .await?;
        seed_identity_item(
            &ctx.pool,
            "evolving",
            "recurring_self_description",
            "self_description",
            "Blue Lagoon is a policy-bound assistant with continuity.",
        )
        .await?;
        identity::record_lifecycle_transition(
            &ctx.pool,
            &identity::NewIdentityLifecycle {
                identity_lifecycle_id: Uuid::now_v7(),
                status: "current".to_string(),
                lifecycle_state: "complete_identity_active".to_string(),
                active_self_model_artifact_id: None,
                active_interview_id: None,
                transition_reason: "component test complete identity".to_string(),
                transitioned_by: "test".to_string(),
                kickstart_started_at: Some(Utc::now()),
                kickstart_completed_at: Some(Utc::now()),
                reset_at: None,
                payload: json!({ "test": "identity_status" }),
            },
        )
        .await?;

        let status = management::load_identity_status(&ctx.config).await?;
        assert_eq!(status.lifecycle_state, "complete_identity_active");
        assert!(!status.kickstart_available);
        assert_eq!(status.active_item_count, 4);
        assert_eq!(status.stable_item_count, 3);
        assert_eq!(status.evolving_item_count, 1);
        assert_eq!(status.value_count, 1);
        assert_eq!(status.boundary_count, 1);
        assert!(status.self_description_present);
        assert_eq!(status.compact_summary, "Blue Lagoon");

        let show = management::load_identity_show(&ctx.config).await?;
        assert_eq!(show.status.active_item_count, 4);
        assert_eq!(
            show.compact_identity.self_description.as_deref(),
            Some("Blue Lagoon is a policy-bound assistant with continuity.")
        );
        assert_eq!(show.compact_identity.values, vec!["clarity".to_string()]);
        assert_eq!(
            show.compact_identity.boundaries,
            vec!["Do not claim hidden autonomy.".to_string()]
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn identity_history_and_diagnostics_surface_operator_follow_up_refs() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let trace_id = Uuid::now_v7();
        let proposal_id = Uuid::now_v7();
        let execution_id = seed_execution(&ctx.pool, trace_id).await?;
        continuity::insert_proposal(
            &ctx.pool,
            &continuity::NewProposalRecord {
                proposal_id,
                trace_id,
                execution_id,
                episode_id: None,
                source_ingress_id: None,
                source_loop_kind: "background".to_string(),
                proposal_kind: "identity_delta".to_string(),
                canonical_target: "identity_items".to_string(),
                status: "accepted".to_string(),
                confidence: 0.9,
                conflict_posture: "independent".to_string(),
                subject_ref: "self:blue-lagoon".to_string(),
                content_text: "Do not bypass approval.".to_string(),
                rationale: Some("component test proposal".to_string()),
                valid_from: Some(Utc::now()),
                valid_to: None,
                supersedes_artifact_id: None,
                supersedes_artifact_kind: None,
                payload: json!({ "test": "identity_history" }),
            },
        )
        .await?;
        let identity_item_id = seed_identity_item(
            &ctx.pool,
            "stable",
            "enduring_boundary",
            "approval_boundary",
            "Do not bypass approval.",
        )
        .await?;
        sqlx::query(
            r#"
            UPDATE identity_items
            SET proposal_id = $1, trace_id = $2
            WHERE identity_item_id = $3
            "#,
        )
        .bind(proposal_id)
        .bind(trace_id)
        .bind(identity_item_id)
        .execute(&ctx.pool)
        .await?;
        identity::insert_identity_diagnostic(
            &ctx.pool,
            &identity::NewIdentityDiagnostic {
                identity_diagnostic_id: Uuid::now_v7(),
                diagnostic_kind: "drift".to_string(),
                severity: "warning".to_string(),
                status: "open".to_string(),
                identity_item_id: Some(identity_item_id),
                proposal_id: Some(proposal_id),
                trace_id: Some(trace_id),
                message: "Potential identity drift needs operator review.".to_string(),
                evidence_refs: json!([{ "source": "component_test" }]),
                payload: json!({ "test": "identity_diagnostics" }),
            },
        )
        .await?;

        let history = management::list_identity_history(&ctx.config, 10).await?;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].identity_item_id, identity_item_id);
        assert_eq!(history[0].proposal_id, Some(proposal_id));
        assert_eq!(history[0].trace_id, Some(trace_id));
        assert_eq!(history[0].category, "enduring_boundary");
        assert_eq!(history[0].status, "active");

        let diagnostics = management::list_identity_diagnostics(&ctx.config, 10).await?;
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].diagnostic_kind, "drift");
        assert_eq!(diagnostics[0].identity_item_id, Some(identity_item_id));
        assert_eq!(diagnostics[0].proposal_id, Some(proposal_id));
        assert_eq!(diagnostics[0].trace_id, Some(trace_id));
        assert!(diagnostics[0].message.contains("operator review"));
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn identity_edit_proposal_workflow_requires_confirmation_and_merges_on_approval() -> Result<()>
{
    support::with_migrated_database(|ctx| async move {
        let rejected_stable = management::propose_identity_edit(
            &ctx.config,
            management::IdentityEditProposalRequest {
                actor_ref: "cli:primary-user".to_string(),
                reason: "stable change must be explicit".to_string(),
                operation: "add".to_string(),
                stability_class: "stable".to_string(),
                category: "foundational_value".to_string(),
                item_key: "clarity".to_string(),
                value: "clarity".to_string(),
                confidence_pct: 100,
                weight_pct: Some(100),
                target_identity_item_id: None,
                confirm_stable: false,
            },
        )
        .await;
        assert!(
            rejected_stable
                .expect_err("stable identity edits require confirmation")
                .to_string()
                .contains("--confirm-stable")
        );

        let proposal = management::propose_identity_edit(
            &ctx.config,
            management::IdentityEditProposalRequest {
                actor_ref: "cli:primary-user".to_string(),
                reason: "operator reviewed evolving preference".to_string(),
                operation: "add".to_string(),
                stability_class: "evolving".to_string(),
                category: "preference".to_string(),
                item_key: "operator_reviewed_preference".to_string(),
                value: "Prefer concise operator summaries.".to_string(),
                confidence_pct: 90,
                weight_pct: Some(80),
                target_identity_item_id: None,
                confirm_stable: false,
            },
        )
        .await?;
        assert_eq!(proposal.status, "pending_operator_review");
        assert!(!proposal.stable_identity_change);

        let proposals = management::list_identity_edit_proposals(&ctx.config, 10).await?;
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].proposal_id, proposal.proposal_id);
        assert_eq!(proposals[0].status, "pending_operator_review");
        assert_eq!(proposals[0].category.as_deref(), Some("preference"));

        let resolution = management::resolve_identity_edit_proposal(
            &ctx.config,
            management::IdentityEditResolutionRequest {
                proposal_id: proposal.proposal_id,
                actor_ref: "cli:primary-user".to_string(),
                decision: "approve".to_string(),
                reason: Some("approved in component test".to_string()),
            },
        )
        .await?;
        assert_eq!(resolution.status, "merged");

        let active_items = identity::list_active_identity_items(&ctx.pool, 10).await?;
        assert!(active_items.iter().any(|item| {
            item.proposal_id == Some(proposal.proposal_id)
                && item.category == "preference"
                && item.value_text == "Prefer concise operator summaries."
        }));
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
async fn trace_report_connects_existing_foreground_records() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let trace_id = Uuid::now_v7();
        let execution_id = seed_execution(&ctx.pool, trace_id).await?;
        let now = Utc::now();
        let ingress = sample_ingress("trace-event-1".to_string(), "telegram-primary", now);
        let ingress_id = ingress.ingress_id;
        foreground::insert_ingress_event(
            &ctx.pool,
            &NewIngressEvent {
                ingress,
                conversation_binding_id: None,
                trace_id,
                execution_id: Some(execution_id),
                status: "accepted".to_string(),
                rejection_reason: None,
            },
        )
        .await?;
        foreground::insert_execution_ingress_link(
            &ctx.pool,
            &foreground::NewExecutionIngressLink {
                execution_ingress_link_id: Uuid::now_v7(),
                execution_id,
                ingress_id,
                link_role: "primary_trigger".to_string(),
                sequence_index: 0,
            },
        )
        .await?;

        let episode_id = Uuid::now_v7();
        foreground::insert_episode(
            &ctx.pool,
            &foreground::NewEpisode {
                episode_id,
                trace_id,
                execution_id,
                ingress_id: Some(ingress_id),
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
                trigger_kind: "telegram_message".to_string(),
                trigger_source: "telegram".to_string(),
                status: "running".to_string(),
                started_at: now,
            },
        )
        .await?;
        foreground::insert_episode_message(
            &ctx.pool,
            &foreground::NewEpisodeMessage {
                episode_message_id: Uuid::now_v7(),
                episode_id,
                trace_id,
                execution_id,
                message_order: 0,
                message_role: "user".to_string(),
                channel_kind: ChannelKind::Telegram,
                text_body: Some("schedule a check-in".to_string()),
                external_message_id: Some("telegram-message-1".to_string()),
            },
        )
        .await?;
        audit::insert(
            &ctx.pool,
            &audit::NewAuditEvent {
                loop_kind: "foreground".to_string(),
                subsystem: "foreground".to_string(),
                event_kind: "foreground_context_assembled".to_string(),
                severity: "info".to_string(),
                trace_id,
                execution_id: Some(execution_id),
                worker_pid: None,
                payload: json!({ "summary": "context assembled" }),
            },
        )
        .await?;

        let scheduled_task = scheduled_foreground::upsert_task(
            &ctx.pool,
            &ctx.config,
            &scheduled_foreground::UpsertScheduledForegroundTask {
                task_key: "trace-check-in".to_string(),
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
                message_text: "schedule a check-in".to_string(),
                cadence_seconds: 600,
                cooldown_seconds: Some(120),
                next_due_at: Some(now + Duration::minutes(10)),
                status: ScheduledForegroundTaskStatus::Active,
                actor_ref: "management-component".to_string(),
            },
        )
        .await?
        .record;
        sqlx::query(
            r#"
            UPDATE scheduled_foreground_tasks
            SET current_execution_id = $2,
                current_run_started_at = $3,
                last_execution_id = $2,
                last_run_started_at = $3,
                last_run_completed_at = $4,
                last_outcome = 'completed',
                last_outcome_reason = 'trace_projection_test',
                last_outcome_summary = 'scheduled trace projection coverage'
            WHERE scheduled_foreground_task_id = $1
            "#,
        )
        .bind(scheduled_task.scheduled_foreground_task_id)
        .bind(execution_id)
        .bind(now)
        .bind(now + Duration::seconds(5))
        .execute(&ctx.pool)
        .await?;

        let report = management::load_trace_report(
            &ctx.config,
            management::TraceLookupRequest {
                trace_id: Some(trace_id),
                execution_id: None,
            },
        )
        .await?;

        assert_eq!(report.trace_id, trace_id);
        assert!(
            report
                .nodes
                .iter()
                .any(|node| node.node_kind == "execution")
        );
        assert!(report.nodes.iter().any(|node| node.node_kind == "ingress"));
        assert!(report.nodes.iter().any(|node| node.node_kind == "episode"));
        assert!(
            report
                .nodes
                .iter()
                .any(|node| node.node_kind == "episode_message")
        );
        assert!(
            report
                .nodes
                .iter()
                .any(|node| node.node_kind == "audit_event")
        );
        assert!(
            report
                .edges
                .iter()
                .any(|edge| edge.edge_kind == "triggered_execution")
        );
        assert!(
            report
                .edges
                .iter()
                .any(|edge| edge.edge_kind == "opened_episode")
        );
        assert_eq!(report.scheduling.len(), 1);
        assert_eq!(report.scheduling[0].task_key, "trace-check-in");
        assert_eq!(
            report.scheduling[0].last_outcome.as_deref(),
            Some("completed")
        );
        let json_report = serde_json::to_value(&report)?;
        assert!(json_report.get("trace_id").is_some());
        assert!(json_report.get("nodes").is_some());
        assert!(json_report.get("edges").is_some());
        assert!(json_report.get("scheduling").is_some());
        assert!(json_report.get("notes").is_some());

        let summaries = management::list_recent_traces(&ctx.config, 10).await?;
        assert!(summaries.iter().any(|summary| summary.trace_id == trace_id));
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn model_call_records_are_persisted_and_visible_in_trace_report() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let trace_id = Uuid::now_v7();
        let execution_id = seed_execution(&ctx.pool, trace_id).await?;
        let background_job_id = Uuid::now_v7();
        let background_job_run_id = Uuid::now_v7();
        let now = Utc::now();
        background::insert_job(
            &ctx.pool,
            &NewBackgroundJob {
                background_job_id,
                trace_id,
                job_kind: contracts::UnconsciousJobKind::MemoryConsolidation,
                trigger: sample_trigger(contracts::BackgroundTriggerKind::MaintenanceTrigger),
                deduplication_key: "model-call-trace-background-job".to_string(),
                scope: contracts::UnconsciousScope {
                    summary: "model-call trace coverage".to_string(),
                    ..contracts::UnconsciousScope::default()
                },
                budget: sample_budget(),
                status: BackgroundJobStatus::Running,
                available_at: now,
                lease_expires_at: Some(now + Duration::minutes(5)),
                last_started_at: Some(now),
                last_completed_at: None,
            },
        )
        .await?;
        background::insert_job_run(
            &ctx.pool,
            &NewBackgroundJobRun {
                background_job_run_id,
                background_job_id,
                trace_id,
                execution_id: Some(execution_id),
                lease_token: Uuid::now_v7(),
                status: BackgroundJobRunStatus::Running,
                worker_pid: Some(4242),
                lease_acquired_at: now,
                lease_expires_at: now + Duration::minutes(5),
                started_at: Some(now),
                completed_at: None,
                result_payload: None,
                failure_payload: None,
            },
        )
        .await?;
        let gateway = ResolvedModelGatewayConfig {
            foreground: ResolvedForegroundModelRouteConfig {
                provider: ModelProviderKind::ZAi,
                model: "test-model".to_string(),
                api_base_url: "https://example.invalid".to_string(),
                api_key: "redacted-test-key".to_string(),
                timeout_ms: 30_000,
            },
        };
        let request = ModelCallRequest {
            request_id: Uuid::now_v7(),
            trace_id,
            execution_id,
            loop_kind: LoopKind::Conscious,
            purpose: ModelCallPurpose::ForegroundResponse,
            task_class: "telegram_foreground_reply".to_string(),
            budget: ModelBudget {
                max_input_tokens: 100,
                max_output_tokens: 50,
                timeout_ms: 30_000,
            },
            input: ModelInput {
                system_prompt: "system prompt for trace test".to_string(),
                messages: vec![ModelInputMessage {
                    role: ModelMessageRole::User,
                    content: "hello model".to_string(),
                }],
            },
            output_mode: ModelOutputMode::PlainText,
            schema_name: None,
            schema_json: None,
            tool_policy: ToolPolicy::ProposalOnly,
            provider_hint: None,
        };

        let started_at = Utc::now();
        let model_call_id = model_calls::insert_pending_model_call_record(
            &ctx.pool,
            &gateway,
            &request,
            started_at,
            ctx.config.observability.model_call_payload_retention_days,
        )
        .await?;
        let response = ModelCallResponse {
            request_id: request.request_id,
            trace_id,
            execution_id,
            provider: ModelProviderKind::ZAi,
            model: "test-model".to_string(),
            received_at: Utc::now(),
            output: ModelOutput {
                text: "hello user".to_string(),
                json: None,
                finish_reason: "stop".to_string(),
            },
            usage: ModelUsage {
                input_tokens: 7,
                output_tokens: 3,
            },
        };
        model_calls::mark_model_call_succeeded(&ctx.pool, model_call_id, &response, Utc::now())
            .await?;
        let failed_request = ModelCallRequest {
            request_id: Uuid::now_v7(),
            ..request.clone()
        };
        let failed_model_call_id = model_calls::insert_pending_model_call_record(
            &ctx.pool,
            &gateway,
            &failed_request,
            Utc::now(),
            ctx.config.observability.model_call_payload_retention_days,
        )
        .await?;
        model_calls::mark_model_call_failed(
            &ctx.pool,
            failed_model_call_id,
            "provider returned status 500",
            Utc::now(),
        )
        .await?;

        let report = management::load_trace_report(
            &ctx.config,
            management::TraceLookupRequest {
                trace_id: Some(trace_id),
                execution_id: None,
            },
        )
        .await?;

        let model_node = report
            .nodes
            .iter()
            .find(|node| node.node_kind == "model_call")
            .expect("trace should contain a model_call node");
        assert_eq!(model_node.source_id, model_call_id);
        assert_eq!(model_node.status.as_deref(), Some("succeeded"));
        assert_eq!(
            model_node.payload["system_prompt_text"].as_str(),
            Some(request.input.system_prompt.as_str())
        );
        assert!(
            report
                .nodes
                .iter()
                .any(|node| node.node_kind == "model_call"
                    && node.status.as_deref() == Some("failed")
                    && node.payload["error_summary"].as_str()
                        == Some("provider returned status 500"))
        );
        assert!(report.edges.iter().any(|edge| edge.source_node_id
            == format!("execution:{execution_id}")
            && edge.target_node_id == format!("model_call:{model_call_id}")
            && edge.edge_kind == "invoked_model"
            && edge.inference == "explicit"));
        assert!(report.edges.iter().any(|edge| edge.source_node_id
            == format!("background_job_run:{background_job_run_id}")
            && edge.target_node_id == format!("model_call:{model_call_id}")
            && edge.edge_kind == "invoked_model"
            && edge.inference == "explicit"));
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn model_call_payload_retention_clears_bulky_fields_but_keeps_metadata() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let trace_id = Uuid::now_v7();
        let execution_id = seed_execution(&ctx.pool, trace_id).await?;
        let gateway = ResolvedModelGatewayConfig {
            foreground: ResolvedForegroundModelRouteConfig {
                provider: ModelProviderKind::ZAi,
                model: "test-model".to_string(),
                api_base_url: "https://example.invalid".to_string(),
                api_key: "redacted-test-key".to_string(),
                timeout_ms: 30_000,
            },
        };
        let request = ModelCallRequest {
            request_id: Uuid::now_v7(),
            trace_id,
            execution_id,
            loop_kind: LoopKind::Conscious,
            purpose: ModelCallPurpose::ForegroundResponse,
            task_class: "telegram_foreground_reply".to_string(),
            budget: ModelBudget {
                max_input_tokens: 100,
                max_output_tokens: 50,
                timeout_ms: 30_000,
            },
            input: ModelInput {
                system_prompt: "short-lived system prompt".to_string(),
                messages: vec![ModelInputMessage {
                    role: ModelMessageRole::User,
                    content: "short-lived message".to_string(),
                }],
            },
            output_mode: ModelOutputMode::PlainText,
            schema_name: None,
            schema_json: None,
            tool_policy: ToolPolicy::ProposalOnly,
            provider_hint: None,
        };
        let started_at = Utc::now() - Duration::days(8);
        let model_call_id = model_calls::insert_pending_model_call_record(
            &ctx.pool, &gateway, &request, started_at, 7,
        )
        .await?;

        let records = model_calls::list_model_call_records_for_trace(&ctx.pool, trace_id).await?;
        let expiry_delta = records[0]
            .payload_retention_expires_at
            .expect("retention expiry should be present")
            - (started_at + Duration::days(7));
        assert!(expiry_delta.num_milliseconds().abs() <= 1);

        let cleared = model_calls::clear_expired_model_call_payloads(&ctx.pool, Utc::now()).await?;
        assert_eq!(cleared, 1);

        let records = model_calls::list_model_call_records_for_trace(&ctx.pool, trace_id).await?;
        let record = records
            .iter()
            .find(|record| record.model_call_id == model_call_id)
            .expect("model call record should remain after payload cleanup");
        assert_eq!(record.status, "pending");
        assert_eq!(record.provider, "z_ai");
        assert!(record.request_payload_json.is_none());
        assert!(record.response_payload_json.is_none());
        assert!(record.system_prompt_text.is_none());
        assert!(record.messages_json.is_none());
        assert_eq!(
            record.payload_retention_reason.as_deref(),
            Some("retention_expired")
        );

        let report = management::load_trace_report(
            &ctx.config,
            management::TraceLookupRequest {
                trace_id: Some(trace_id),
                execution_id: None,
            },
        )
        .await?;
        let model_node = report
            .nodes
            .iter()
            .find(|node| node.source_id == model_call_id)
            .expect("trace should still include cleaned model call metadata");
        assert_eq!(
            model_node.payload["payload_retention_reason"].as_str(),
            Some("retention_expired")
        );

        let explanation = management::explain_trace_report(
            &report,
            Some(management::TraceFocusSelector::FailingNode),
        );
        let focus = explanation
            .focus
            .expect("focused failing node inspection should be present");
        assert_eq!(
            focus.payload_availability,
            management::TraceFocusPayloadAvailability::Unavailable
        );
        assert_eq!(
            focus.payload_availability_reason.as_deref(),
            Some("No node was resolved for the requested focus target.")
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn trace_diagnosis_marks_pending_approval_as_operator_gated() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let proposal = contracts::GovernedActionProposal {
            proposal_id: Uuid::now_v7(),
            title: "Approval-gated subprocess".to_string(),
            rationale: Some("Used to verify trace diagnosis.".to_string()),
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
            other => panic!("expected planned approval-gated action, got {other:?}"),
        };
        assert!(planned.requires_approval);
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
                consequence_summary: "Used to verify pending approval trace diagnosis.".to_string(),
                capability_scope: proposal.capability_scope,
                requested_by: "telegram:primary-user".to_string(),
                token: "diagnosis-approval".to_string(),
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

        let report = management::load_trace_report(
            &ctx.config,
            management::TraceLookupRequest {
                trace_id: Some(planned.record.trace_id),
                execution_id: None,
            },
        )
        .await?;
        let diagnosis = management::diagnose_trace_report(&report);
        assert_eq!(
            diagnosis.verdict,
            management::TraceDiagnosisVerdict::AwaitingApproval
        );
        assert_eq!(
            diagnosis.failure_class,
            Some(management::TraceFailureClass::ApprovalPending)
        );
        assert_eq!(
            diagnosis.retry_safety,
            management::TraceRetrySafety::RequiresOperator
        );
        assert_eq!(
            diagnosis.side_effect_status,
            management::TraceSideEffectStatus::NoneExecuted
        );
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
        let linked_task = scheduled_foreground::upsert_task(
            &ctx.pool,
            &ctx.config,
            &scheduled_foreground::UpsertScheduledForegroundTask {
                task_key: "approval-linked-check-in".to_string(),
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
                message_text: "Approval-linked schedule projection".to_string(),
                cadence_seconds: ctx.config.scheduled_foreground.min_cadence_seconds,
                cooldown_seconds: Some(ctx.config.scheduled_foreground.default_cooldown_seconds),
                next_due_at: Some(Utc::now() + Duration::minutes(20)),
                status: ScheduledForegroundTaskStatus::Active,
                actor_ref: "management-component".to_string(),
            },
        )
        .await?
        .record;
        causal_links::insert(
            &ctx.pool,
            &NewCausalLink {
                trace_id: planned.record.trace_id,
                source_kind: "governed_action_execution".to_string(),
                source_id: planned.record.governed_action_execution_id,
                target_kind: "scheduled_foreground_task".to_string(),
                target_id: linked_task.scheduled_foreground_task_id,
                edge_kind: "mutated_scheduled_task".to_string(),
                payload: json!({ "task_key": linked_task.task_key }),
            },
        )
        .await?;
        let approval_trace = management::load_trace_report(
            &ctx.config,
            management::TraceLookupRequest {
                trace_id: Some(planned.record.trace_id),
                execution_id: None,
            },
        )
        .await?;
        assert!(approval_trace.edges.iter().any(|edge| {
            edge.source_node_id
                == format!(
                    "governed_action:{}",
                    planned.record.governed_action_execution_id
                )
                && edge.target_node_id
                    == format!("approval_request:{}", approval_request.approval_request_id)
                && edge.edge_kind == "required_approval"
                && edge.inference == "explicit"
        }));
        assert!(approval_trace.nodes.iter().any(|node| {
            node.node_kind == "scheduled_foreground_task"
                && node.source_id == linked_task.scheduled_foreground_task_id
        }));
        assert!(
            approval_trace
                .scheduling
                .iter()
                .any(|summary| summary.task_key == "approval-linked-check-in")
        );

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

async fn seed_identity_item(
    pool: &PgPool,
    stability_class: &str,
    category: &str,
    item_key: &str,
    value_text: &str,
) -> Result<Uuid> {
    let identity_item_id = Uuid::now_v7();
    identity::insert_identity_item(
        pool,
        &identity::NewIdentityItem {
            identity_item_id,
            self_model_artifact_id: None,
            proposal_id: None,
            trace_id: None,
            stability_class: stability_class.to_string(),
            category: category.to_string(),
            item_key: item_key.to_string(),
            value_text: value_text.to_string(),
            confidence: 1.0,
            weight: Some(1.0),
            provenance_kind: "component_test".to_string(),
            source_kind: "identity_kickstart".to_string(),
            merge_policy: "replace_key".to_string(),
            status: "active".to_string(),
            evidence_refs: json!([]),
            valid_from: Some(Utc::now()),
            valid_to: None,
            supersedes_item_id: None,
            payload: json!({ "test": "management_identity_inspection" }),
        },
    )
    .await?;
    Ok(identity_item_id)
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
