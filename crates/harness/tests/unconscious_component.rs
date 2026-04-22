mod support;

use anyhow::Result;
use chrono::{Duration, Utc};
use contracts::{
    BackgroundExecutionBudget, BackgroundTrigger, BackgroundTriggerKind, UnconsciousJobKind,
    UnconsciousScope, WakeSignal, WakeSignalDecision, WakeSignalDecisionKind, WakeSignalPriority,
    WakeSignalReason,
};
use harness::{
    audit,
    background::{self, BackgroundJobRunStatus, BackgroundJobStatus, WakeSignalStatus},
    background_execution,
    background_planning::{self, BackgroundPlanningDecision, BackgroundPlanningRequest},
    config::{
        ResolvedForegroundModelRouteConfig, ResolvedModelGatewayConfig, SelfModelConfig,
        TelegramConfig, TelegramForegroundBindingConfig,
    },
    continuity,
    execution::{self, NewExecutionRecord},
    foreground::{self, NewEpisode, NewEpisodeMessage},
    migration,
    model_gateway::{FakeModelProviderTransport, ProviderHttpResponse},
};
use serde_json::json;
use serial_test::serial;
use sqlx::Row;
use std::{fs, path::PathBuf, process::Command, time::Duration as StdDuration};
use uuid::Uuid;

#[tokio::test]
#[serial]
async fn migration_application_creates_unconscious_loop_tables() -> Result<()> {
    support::with_clean_database(|ctx| async move {
        let summary =
            migration::apply_pending_migrations(&ctx.pool, env!("CARGO_PKG_VERSION")).await?;

        assert_eq!(summary.discovered_versions, vec![1, 2, 3, 4, 5, 6]);

        let tables = sqlx::query(
            r#"
            SELECT table_name
            FROM information_schema.tables
            WHERE table_schema = 'public'
              AND table_name IN ('background_jobs', 'background_job_runs', 'wake_signals')
            ORDER BY table_name
            "#,
        )
        .fetch_all(&ctx.pool)
        .await?;

        let names = tables
            .into_iter()
            .map(|row| row.get::<String, _>("table_name"))
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![
                "background_job_runs".to_string(),
                "background_jobs".to_string(),
                "wake_signals".to_string()
            ]
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn background_job_storage_lists_due_jobs_and_run_history() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let trace_id = Uuid::now_v7();
        let older_job_id = Uuid::now_v7();
        let newer_job_id = Uuid::now_v7();
        let now = Utc::now();

        background::insert_job(
            &ctx.pool,
            &background::NewBackgroundJob {
                background_job_id: older_job_id,
                trace_id,
                job_kind: UnconsciousJobKind::MemoryConsolidation,
                trigger: sample_trigger(BackgroundTriggerKind::TimeSchedule, "scheduled sweep"),
                deduplication_key: "job:memory:scheduled".to_string(),
                scope: sample_scope("older due job"),
                budget: sample_budget(),
                status: BackgroundJobStatus::Planned,
                available_at: now - Duration::minutes(10),
                lease_expires_at: None,
                last_started_at: None,
                last_completed_at: None,
            },
        )
        .await?;

        background::insert_job(
            &ctx.pool,
            &background::NewBackgroundJob {
                background_job_id: newer_job_id,
                trace_id,
                job_kind: UnconsciousJobKind::RetrievalMaintenance,
                trigger: sample_trigger(
                    BackgroundTriggerKind::VolumeThreshold,
                    "retrieval backlog crossed threshold",
                ),
                deduplication_key: "job:retrieval:threshold".to_string(),
                scope: sample_scope("newer due job"),
                budget: sample_budget(),
                status: BackgroundJobStatus::Planned,
                available_at: now - Duration::minutes(5),
                lease_expires_at: None,
                last_started_at: None,
                last_completed_at: None,
            },
        )
        .await?;

        let due_jobs = background::list_due_jobs(&ctx.pool, now, 10).await?;
        assert_eq!(due_jobs.len(), 2);
        assert_eq!(due_jobs[0].background_job_id, older_job_id);
        assert_eq!(due_jobs[1].background_job_id, newer_job_id);

        background::update_job_status(
            &ctx.pool,
            older_job_id,
            &background::UpdateBackgroundJobStatus {
                status: BackgroundJobStatus::Leased,
                lease_expires_at: Some(now + Duration::minutes(5)),
                last_started_at: None,
                last_completed_at: None,
            },
        )
        .await?;

        let execution_id = seed_execution(&ctx.pool, trace_id).await?;
        let active_run_id = Uuid::now_v7();
        background::insert_job_run(
            &ctx.pool,
            &background::NewBackgroundJobRun {
                background_job_run_id: active_run_id,
                background_job_id: older_job_id,
                trace_id,
                execution_id: None,
                lease_token: Uuid::now_v7(),
                status: BackgroundJobRunStatus::Leased,
                worker_pid: None,
                lease_acquired_at: now,
                lease_expires_at: now + Duration::minutes(5),
                started_at: None,
                completed_at: None,
                result_payload: None,
                failure_payload: None,
            },
        )
        .await?;

        let completed_run_id = Uuid::now_v7();
        background::insert_job_run(
            &ctx.pool,
            &background::NewBackgroundJobRun {
                background_job_run_id: completed_run_id,
                background_job_id: older_job_id,
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

        let active_runs = background::list_active_job_runs(&ctx.pool, older_job_id).await?;
        assert_eq!(active_runs.len(), 1);
        assert_eq!(active_runs[0].background_job_run_id, active_run_id);

        let completed_runs =
            background::list_completed_job_runs(&ctx.pool, older_job_id, 10).await?;
        assert_eq!(completed_runs.len(), 1);
        assert_eq!(completed_runs[0].background_job_run_id, completed_run_id);
        assert_eq!(completed_runs[0].execution_id, Some(execution_id));

        let stored_job = background::get_job(&ctx.pool, older_job_id).await?;
        assert_eq!(stored_job.status, BackgroundJobStatus::Leased);
        assert_eq!(stored_job.scope.summary, "older due job");

        let stored_run = background::get_job_run(&ctx.pool, completed_run_id).await?;
        assert_eq!(stored_run.status, BackgroundJobRunStatus::Completed);
        assert_eq!(stored_run.worker_pid, Some(4242));
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn wake_signal_storage_persists_pending_and_reviewed_state() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let trace_id = Uuid::now_v7();
        let background_job_id = Uuid::now_v7();
        let now = Utc::now();

        background::insert_job(
            &ctx.pool,
            &background::NewBackgroundJob {
                background_job_id,
                trace_id,
                job_kind: UnconsciousJobKind::SelfModelReflection,
                trigger: sample_trigger(
                    BackgroundTriggerKind::MaintenanceTrigger,
                    "reflection maintenance requested",
                ),
                deduplication_key: "job:self-model:maintenance".to_string(),
                scope: sample_scope("self-model focus"),
                budget: sample_budget(),
                status: BackgroundJobStatus::Planned,
                available_at: now,
                lease_expires_at: None,
                last_started_at: None,
                last_completed_at: None,
            },
        )
        .await?;

        let run_id = Uuid::now_v7();
        background::insert_job_run(
            &ctx.pool,
            &background::NewBackgroundJobRun {
                background_job_run_id: run_id,
                background_job_id,
                trace_id,
                execution_id: None,
                lease_token: Uuid::now_v7(),
                status: BackgroundJobRunStatus::Leased,
                worker_pid: None,
                lease_acquired_at: now,
                lease_expires_at: now + Duration::minutes(2),
                started_at: None,
                completed_at: None,
                result_payload: None,
                failure_payload: None,
            },
        )
        .await?;

        let signal = WakeSignal {
            signal_id: Uuid::now_v7(),
            reason: WakeSignalReason::MaintenanceInsightReady,
            priority: WakeSignalPriority::Normal,
            reason_code: "maintenance_insight_ready".to_string(),
            summary: "Background reflection produced a useful summary.".to_string(),
            payload_ref: Some("background_job_run:latest".to_string()),
        };
        background::insert_wake_signal(
            &ctx.pool,
            &background::NewWakeSignalRecord {
                background_job_id,
                background_job_run_id: Some(run_id),
                trace_id,
                execution_id: None,
                signal: signal.clone(),
                status: WakeSignalStatus::PendingReview,
                requested_at: now,
                cooldown_until: None,
            },
        )
        .await?;

        let pending = background::list_pending_wake_signals(&ctx.pool, 10).await?;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].signal.signal_id, signal.signal_id);
        assert_eq!(pending[0].status, WakeSignalStatus::PendingReview);

        background::record_wake_signal_decision(
            &ctx.pool,
            signal.signal_id,
            &WakeSignalDecision {
                signal_id: signal.signal_id,
                decision: WakeSignalDecisionKind::Deferred,
                reason: "foreground channel is cooling down".to_string(),
            },
            now + Duration::seconds(30),
            Some(now + Duration::minutes(15)),
        )
        .await?;

        let reviewed = background::get_wake_signal(&ctx.pool, signal.signal_id).await?;
        assert_eq!(reviewed.status, WakeSignalStatus::Deferred);
        assert_eq!(
            reviewed
                .decision
                .expect("wake signal should have a review decision")
                .decision,
            WakeSignalDecisionKind::Deferred
        );
        assert!(reviewed.reviewed_at.is_some());
        assert_eq!(
            reviewed
                .cooldown_until
                .expect("wake signal should preserve cooldown")
                .timestamp_micros(),
            (now + Duration::minutes(15)).timestamp_micros()
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn background_planning_persists_due_job_scope_and_audit_event() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let trace_id = Uuid::now_v7();
        let conversation = "telegram-primary";
        let episode_id = seed_episode_for_conversation(&ctx.pool, trace_id, conversation).await?;
        let retrieval_artifact_id =
            seed_retrieval_artifact(&ctx.pool, episode_id, conversation).await?;
        let self_model_artifact_id = seed_self_model_artifact(&ctx.pool, trace_id).await?;

        let outcome = background_planning::plan_background_job(
            &ctx.pool,
            &ctx.config,
            BackgroundPlanningRequest {
                trace_id,
                job_kind: UnconsciousJobKind::SelfModelReflection,
                trigger: BackgroundTrigger {
                    trigger_id: Uuid::now_v7(),
                    trigger_kind: BackgroundTriggerKind::TimeSchedule,
                    requested_at: Utc::now(),
                    reason_summary: "nightly reflection".to_string(),
                    payload_ref: None,
                },
                internal_conversation_ref: Some(conversation.to_string()),
                available_at: Utc::now(),
            },
        )
        .await?;

        let planned = match outcome {
            BackgroundPlanningDecision::Planned(planned) => planned,
            other => panic!("expected planned job, got {other:?}"),
        };

        let stored = background::get_job(&ctx.pool, planned.background_job_id).await?;
        assert_eq!(stored.status, BackgroundJobStatus::Planned);
        assert!(stored.scope.episode_ids.contains(&episode_id));
        assert!(
            stored
                .scope
                .retrieval_artifact_ids
                .contains(&retrieval_artifact_id)
        );
        assert_eq!(
            stored.scope.self_model_artifact_id,
            Some(self_model_artifact_id)
        );
        assert_eq!(stored.budget.iteration_budget, 2);

        let audit_events = audit::list_for_trace(&ctx.pool, trace_id).await?;
        assert!(
            audit_events
                .iter()
                .any(|event| event.event_kind == "background_job_planned")
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn background_planning_suppresses_duplicate_active_jobs() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let trace_id = Uuid::now_v7();

        let first = background_planning::plan_background_job(
            &ctx.pool,
            &ctx.config,
            BackgroundPlanningRequest {
                trace_id,
                job_kind: UnconsciousJobKind::MemoryConsolidation,
                trigger: BackgroundTrigger {
                    trigger_id: Uuid::now_v7(),
                    trigger_kind: BackgroundTriggerKind::TimeSchedule,
                    requested_at: Utc::now(),
                    reason_summary: "scheduled consolidation".to_string(),
                    payload_ref: None,
                },
                internal_conversation_ref: None,
                available_at: Utc::now(),
            },
        )
        .await?;
        assert!(matches!(first, BackgroundPlanningDecision::Planned(_)));

        let second = background_planning::plan_background_job(
            &ctx.pool,
            &ctx.config,
            BackgroundPlanningRequest {
                trace_id,
                job_kind: UnconsciousJobKind::MemoryConsolidation,
                trigger: BackgroundTrigger {
                    trigger_id: Uuid::now_v7(),
                    trigger_kind: BackgroundTriggerKind::TimeSchedule,
                    requested_at: Utc::now(),
                    reason_summary: "same schedule".to_string(),
                    payload_ref: None,
                },
                internal_conversation_ref: None,
                available_at: Utc::now(),
            },
        )
        .await?;

        match second {
            BackgroundPlanningDecision::SuppressedDuplicate {
                existing_job_id,
                deduplication_key,
                ..
            } => {
                assert!(!deduplication_key.is_empty());
                assert!(
                    background::get_job(&ctx.pool, existing_job_id)
                        .await
                        .is_ok()
                );
            }
            other => panic!("expected duplicate suppression, got {other:?}"),
        }

        assert_eq!(count_background_jobs(&ctx.pool).await?, 1);
        let audit_events = audit::list_for_trace(&ctx.pool, trace_id).await?;
        assert!(
            audit_events
                .iter()
                .any(|event| event.event_kind == "background_job_suppressed")
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn background_planning_rejects_recognized_but_unsupported_trigger_kinds() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let trace_id = Uuid::now_v7();
        let outcome = background_planning::plan_background_job(
            &ctx.pool,
            &ctx.config,
            BackgroundPlanningRequest {
                trace_id,
                job_kind: UnconsciousJobKind::ContradictionAndDriftScan,
                trigger: BackgroundTrigger {
                    trigger_id: Uuid::now_v7(),
                    trigger_kind: BackgroundTriggerKind::DriftOrAnomalySignal,
                    requested_at: Utc::now(),
                    reason_summary: "diagnostic signal".to_string(),
                    payload_ref: Some("diagnostic://event".to_string()),
                },
                internal_conversation_ref: None,
                available_at: Utc::now(),
            },
        )
        .await?;

        match outcome {
            BackgroundPlanningDecision::Rejected { reason } => {
                assert!(reason.contains("recognized"));
            }
            other => panic!("expected rejected planning outcome, got {other:?}"),
        }

        assert_eq!(count_background_jobs(&ctx.pool).await?, 0);
        let audit_events = audit::list_for_trace(&ctx.pool, trace_id).await?;
        assert!(
            audit_events
                .iter()
                .any(|event| event.event_kind == "background_job_rejected")
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn background_execution_leases_due_job_and_creates_execution_state() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let background_job_id = seed_planned_background_job(&ctx.pool, Utc::now()).await?;

        let leased = background_execution::lease_next_due_job(&ctx.pool, &ctx.config, Utc::now())
            .await?
            .expect("expected a due background job to be leased");

        assert_eq!(leased.job.background_job_id, background_job_id);
        assert_eq!(leased.job.status, BackgroundJobStatus::Leased);

        let stored_job = background::get_job(&ctx.pool, background_job_id).await?;
        assert_eq!(stored_job.status, BackgroundJobStatus::Leased);
        assert!(stored_job.lease_expires_at.is_some());

        let stored_run = background::get_job_run(&ctx.pool, leased.background_job_run_id).await?;
        assert_eq!(stored_run.status, BackgroundJobRunStatus::Leased);
        assert_eq!(stored_run.execution_id, Some(leased.execution_id));

        let execution = execution::get(&ctx.pool, leased.execution_id).await?;
        assert_eq!(execution.status, "started");
        assert!(execution.completed_at.is_none());
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn background_execution_completes_due_job_and_records_audit_history() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        let worker_binary = support::workers_binary()?;
        config.worker.command = worker_binary.to_string_lossy().into_owned();
        config.worker.args = vec!["unconscious-worker".to_string()];

        let background_job_id = seed_planned_background_job(&ctx.pool, Utc::now()).await?;
        let transport = FakeModelProviderTransport::new();
        transport.push_response(Ok(ProviderHttpResponse {
            status: 200,
            body: json!({
                "choices": [{
                    "message": { "content": "maintenance lexical summary" },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 14,
                    "completion_tokens": 8
                }
            }),
        }));

        let outcome = background_execution::execute_next_due_job(
            &ctx.pool,
            &config,
            &sample_model_gateway_config(),
            &transport,
            Utc::now(),
        )
        .await?
        .expect("expected a due job to execute");

        assert_eq!(outcome.background_job_id, background_job_id);
        assert!(outcome.summary.contains("memory_consolidation"));

        let stored_job = background::get_job(&ctx.pool, background_job_id).await?;
        assert_eq!(stored_job.status, BackgroundJobStatus::Completed);
        assert!(stored_job.last_started_at.is_some());
        assert!(stored_job.last_completed_at.is_some());
        assert!(stored_job.lease_expires_at.is_none());

        let stored_run = background::get_job_run(&ctx.pool, outcome.background_job_run_id).await?;
        assert_eq!(stored_run.status, BackgroundJobRunStatus::Completed);
        assert_eq!(stored_run.execution_id, Some(outcome.execution_id));
        assert!(stored_run.result_payload.is_some());
        assert!(stored_run.failure_payload.is_none());

        let execution = execution::get(&ctx.pool, outcome.execution_id).await?;
        assert_eq!(execution.status, "completed");
        assert_eq!(execution.worker_pid, Some(outcome.worker_pid as i32));

        let proposals =
            continuity::list_proposals_for_execution(&ctx.pool, outcome.execution_id).await?;
        assert_eq!(proposals.len(), 1);
        let proposal = &proposals[0];
        assert_eq!(proposal.status, "accepted");
        assert_eq!(proposal.source_loop_kind, "unconscious");
        assert_eq!(proposal.subject_ref, "primary-user");

        let merge_decision =
            continuity::get_merge_decision_by_proposal(&ctx.pool, proposal.proposal_id)
                .await?
                .expect("accepted background proposal should have a merge decision");
        assert_eq!(merge_decision.decision_kind, "accepted");
        let memory_artifact_id = merge_decision
            .accepted_memory_artifact_id
            .expect("accepted background memory proposal should create a canonical artifact");
        let memory_artifact =
            continuity::get_memory_artifact(&ctx.pool, memory_artifact_id).await?;
        assert_eq!(memory_artifact.proposal_id, proposal.proposal_id);
        assert_eq!(memory_artifact.subject_ref, "primary-user");
        assert_eq!(memory_artifact.content_text, "maintenance lexical summary");
        let retrieval_artifacts = continuity::list_active_retrieval_artifacts_for_conversation(
            &ctx.pool,
            "telegram-primary",
            10,
        )
        .await?;
        assert_eq!(retrieval_artifacts.len(), 1);
        assert_eq!(retrieval_artifacts[0].source_kind, "episode");
        assert!(retrieval_artifacts[0].source_episode_id.is_some());
        assert!(retrieval_artifacts[0].source_memory_artifact_id.is_none());
        assert_eq!(
            retrieval_artifacts[0].lexical_document,
            "maintenance lexical summary"
        );

        let audit_events = audit::list_for_execution(&ctx.pool, outcome.execution_id).await?;
        assert!(
            audit_events
                .iter()
                .any(|event| event.event_kind == "background_job_started")
        );
        assert!(
            audit_events
                .iter()
                .any(|event| event.event_kind == "background_job_completed")
        );
        assert!(
            audit_events
                .iter()
                .any(|event| event.event_kind == "canonical_write_applied")
        );
        assert!(
            audit_events
                .iter()
                .any(|event| event.event_kind == "retrieval_update_applied")
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn background_execution_persists_retrieval_maintenance_outputs() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        let worker_binary = support::workers_binary()?;
        config.worker.command = worker_binary.to_string_lossy().into_owned();
        config.worker.args = vec!["unconscious-worker".to_string()];

        let background_job_id = seed_planned_background_job_with_kind(
            &ctx.pool,
            Utc::now(),
            UnconsciousJobKind::RetrievalMaintenance,
        )
        .await?;
        let transport = FakeModelProviderTransport::new();
        transport.push_response(Ok(ProviderHttpResponse {
            status: 200,
            body: json!({
                "choices": [{
                    "message": { "content": "retrieval lexical summary" },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 12,
                    "completion_tokens": 7
                }
            }),
        }));

        let outcome = background_execution::execute_next_due_job(
            &ctx.pool,
            &config,
            &sample_model_gateway_config(),
            &transport,
            Utc::now(),
        )
        .await?
        .expect("expected a due retrieval-maintenance job to execute");

        assert_eq!(outcome.background_job_id, background_job_id);
        let proposals =
            continuity::list_proposals_for_execution(&ctx.pool, outcome.execution_id).await?;
        assert!(proposals.is_empty());

        let retrieval_artifacts = continuity::list_active_retrieval_artifacts_for_conversation(
            &ctx.pool,
            "telegram-primary",
            10,
        )
        .await?;
        assert_eq!(retrieval_artifacts.len(), 1);
        assert_eq!(retrieval_artifacts[0].source_kind, "episode");
        assert!(retrieval_artifacts[0].source_episode_id.is_some());
        assert!(retrieval_artifacts[0].source_memory_artifact_id.is_none());
        assert_eq!(
            retrieval_artifacts[0].lexical_document,
            "retrieval lexical summary"
        );
        assert!(
            retrieval_artifacts[0]
                .payload
                .get("source_ref")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|value| value.starts_with("background_job:"))
        );

        let audit_events = audit::list_for_execution(&ctx.pool, outcome.execution_id).await?;
        assert!(
            audit_events
                .iter()
                .any(|event| event.event_kind == "retrieval_update_applied")
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn background_execution_records_contradiction_diagnostics_without_mutating_state()
-> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        let worker_binary = support::workers_binary()?;
        config.worker.command = worker_binary.to_string_lossy().into_owned();
        config.worker.args = vec!["unconscious-worker".to_string()];

        let background_job_id = seed_planned_background_job_with_kind(
            &ctx.pool,
            Utc::now(),
            UnconsciousJobKind::ContradictionAndDriftScan,
        )
        .await?;
        let transport = FakeModelProviderTransport::new();
        transport.push_response(Ok(ProviderHttpResponse {
            status: 200,
            body: json!({
                "choices": [{
                    "message": { "content": "Potential contradiction detected between recent memory snapshots." },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 14,
                    "completion_tokens": 8
                }
            }),
        }));

        let outcome = background_execution::execute_next_due_job(
            &ctx.pool,
            &config,
            &sample_model_gateway_config(),
            &transport,
            Utc::now(),
        )
        .await?
        .expect("expected a due contradiction-and-drift job to execute");

        assert_eq!(outcome.background_job_id, background_job_id);
        let proposals =
            continuity::list_proposals_for_execution(&ctx.pool, outcome.execution_id).await?;
        assert!(proposals.is_empty());

        let retrieval_artifacts = continuity::list_active_retrieval_artifacts_for_conversation(
            &ctx.pool,
            "telegram-primary",
            10,
        )
        .await?;
        assert!(retrieval_artifacts.is_empty());

        let audit_events = audit::list_for_execution(&ctx.pool, outcome.execution_id).await?;
        assert!(
            audit_events
                .iter()
                .any(|event| event.event_kind == "background_diagnostic_recorded")
        );
        let diagnostic_row = sqlx::query(
            r#"
            SELECT severity, payload
            FROM audit_events
            WHERE execution_id = $1
              AND event_kind = 'background_diagnostic_recorded'
            ORDER BY occurred_at DESC, event_id DESC
            LIMIT 1
            "#,
        )
        .bind(outcome.execution_id)
        .fetch_one(&ctx.pool)
        .await?;
        let severity: String = sqlx::Row::get(&diagnostic_row, "severity");
        let payload: serde_json::Value = sqlx::Row::get(&diagnostic_row, "payload");
        assert_eq!(severity, "critical");
        assert_eq!(
            payload.get("code").and_then(serde_json::Value::as_str),
            Some("contradiction_detected")
        );

        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn background_execution_applies_self_model_reflection_through_canonical_merge_path()
-> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        let worker_binary = support::workers_binary()?;
        config.worker.command = worker_binary.to_string_lossy().into_owned();
        config.worker.args = vec!["unconscious-worker".to_string()];
        let prior_self_model_artifact_id = seed_self_model_artifact(&ctx.pool, Uuid::now_v7()).await?;

        let background_job_id = seed_planned_background_job_with_kind(
            &ctx.pool,
            Utc::now(),
            UnconsciousJobKind::SelfModelReflection,
        )
        .await?;
        let transport = FakeModelProviderTransport::new();
        transport.push_response(Ok(ProviderHttpResponse {
            status: 200,
            body: json!({
                "choices": [{
                    "message": { "content": "Prefer concise progress updates during long maintenance runs." },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 16,
                    "completion_tokens": 8
                }
            }),
        }));

        let outcome = background_execution::execute_next_due_job(
            &ctx.pool,
            &config,
            &sample_model_gateway_config(),
            &transport,
            Utc::now(),
        )
        .await?
        .expect("expected a due self-model-reflection job to execute");

        assert_eq!(outcome.background_job_id, background_job_id);
        let proposals =
            continuity::list_proposals_for_execution(&ctx.pool, outcome.execution_id).await?;
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].proposal_kind, "self_model_observation");
        assert_eq!(proposals[0].status, "accepted");

        let merge_decision =
            continuity::get_merge_decision_by_proposal(&ctx.pool, proposals[0].proposal_id)
                .await?
                .expect("accepted self-model reflection should have a merge decision");
        let accepted_artifact_id = merge_decision
            .accepted_self_model_artifact_id
            .expect("accepted self-model reflection should create a canonical artifact");
        let active_artifacts = continuity::list_active_self_model_artifacts(&ctx.pool, 10).await?;
        assert_eq!(active_artifacts.len(), 1);
        assert_eq!(active_artifacts[0].self_model_artifact_id, accepted_artifact_id);
        assert!(
            active_artifacts[0].preferences.iter().any(|value| {
                value == "Prefer concise progress updates during long maintenance runs."
            })
        );

        let superseded_artifacts =
            continuity::list_superseded_self_model_artifacts(&ctx.pool, 10).await?;
        assert_eq!(superseded_artifacts.len(), 1);
        assert_eq!(
            superseded_artifacts[0].self_model_artifact_id,
            prior_self_model_artifact_id
        );

        let retrieval_artifacts = continuity::list_active_retrieval_artifacts_for_conversation(
            &ctx.pool,
            "telegram-primary",
            10,
        )
        .await?;
        assert!(retrieval_artifacts.is_empty());

        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn background_execution_converts_accepted_wake_signal_into_staged_foreground_work()
-> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        let worker_binary = support::workers_binary()?;
        config.worker.command = worker_binary.to_string_lossy().into_owned();
        config.worker.args = vec!["unconscious-worker".to_string()];
        config.self_model = Some(SelfModelConfig {
            seed_path: PathBuf::from("config/self_model_seed.toml"),
        });
        config.telegram = Some(TelegramConfig {
            api_base_url: "https://api.telegram.org".to_string(),
            bot_token_env: "BLUE_LAGOON_TEST_TELEGRAM_TOKEN".to_string(),
            poll_limit: 10,
            foreground_binding: Some(TelegramForegroundBindingConfig {
                allowed_user_id: 42,
                allowed_chat_id: 24,
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
            }),
        });

        let background_job_id = seed_planned_background_job_with_kind(
            &ctx.pool,
            Utc::now(),
            UnconsciousJobKind::SelfModelReflection,
        )
        .await?;
        let transport = FakeModelProviderTransport::new();
        transport.push_response(Ok(ProviderHttpResponse {
            status: 200,
            body: json!({
                "choices": [{
                    "message": { "content": "Prefer concise progress updates during long maintenance runs." },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 16,
                    "completion_tokens": 8
                }
            }),
        }));

        let outcome = background_execution::execute_next_due_job(
            &ctx.pool,
            &config,
            &sample_model_gateway_config(),
            &transport,
            Utc::now(),
        )
        .await?
        .expect("expected a due self-model-reflection job to execute");

        assert_eq!(outcome.background_job_id, background_job_id);
        assert!(background::list_pending_wake_signals(&ctx.pool, 10).await?.is_empty());

        let signal_row = sqlx::query(
            r#"
            SELECT wake_signal_id
            FROM wake_signals
            WHERE execution_id = $1
            "#,
        )
        .bind(outcome.execution_id)
        .fetch_one(&ctx.pool)
        .await?;
        let wake_signal_id: Uuid = signal_row.get("wake_signal_id");
        let stored_signal = background::get_wake_signal(&ctx.pool, wake_signal_id).await?;
        assert_eq!(stored_signal.status, WakeSignalStatus::Accepted);
        assert_eq!(
            stored_signal
                .decision
                .as_ref()
                .expect("accepted wake signal should record a decision")
                .decision,
            WakeSignalDecisionKind::Accepted
        );

        let ingress_row = sqlx::query(
            r#"
            SELECT ingress_id, internal_conversation_ref, foreground_status, text_body
            FROM ingress_events
            WHERE external_event_id = $1
            "#,
        )
        .bind(format!("wake-signal:{wake_signal_id}"))
        .fetch_one(&ctx.pool)
        .await?;
        let staged_ingress_id: Uuid = ingress_row.get("ingress_id");
        assert_eq!(
            ingress_row.get::<String, _>("internal_conversation_ref"),
            "telegram-primary".to_string()
        );
        assert_eq!(
            ingress_row.get::<String, _>("foreground_status"),
            "pending".to_string()
        );
        assert!(
            ingress_row
                .get::<String, _>("text_body")
                .contains("policy-approved maintenance wake signal")
        );

        let normalized_ingress =
            foreground::load_normalized_ingress(&ctx.pool, staged_ingress_id).await?;
        let trigger = foreground::build_foreground_trigger(
            &config,
            outcome.trace_id,
            Uuid::now_v7(),
            normalized_ingress,
        )?;
        assert_eq!(
            trigger.trigger_kind,
            contracts::ForegroundTriggerKind::ApprovedWakeSignal
        );

        let audit_events = audit::list_for_execution(&ctx.pool, outcome.execution_id).await?;
        assert!(audit_events.iter().any(|event| event.event_kind == "wake_signal_recorded"));
        assert!(audit_events.iter().any(|event| event.event_kind == "wake_signal_reviewed"));
        assert!(
            audit_events
                .iter()
                .any(|event| event.event_kind == "wake_signal_foreground_conversion_staged")
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn background_execution_rejects_wake_signal_when_no_foreground_binding_is_available()
-> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        let worker_binary = support::workers_binary()?;
        config.worker.command = worker_binary.to_string_lossy().into_owned();
        config.worker.args = vec!["unconscious-worker".to_string()];
        config.self_model = Some(SelfModelConfig {
            seed_path: PathBuf::from("config/self_model_seed.toml"),
        });
        let background_job_id = seed_planned_background_job_with_kind(
            &ctx.pool,
            Utc::now(),
            UnconsciousJobKind::SelfModelReflection,
        )
        .await?;
        let transport = FakeModelProviderTransport::new();
        transport.push_response(Ok(ProviderHttpResponse {
            status: 200,
            body: json!({
                "choices": [{
                    "message": { "content": "Prefer concise progress updates during long maintenance runs." },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 16,
                    "completion_tokens": 8
                }
            }),
        }));

        let outcome = background_execution::execute_next_due_job(
            &ctx.pool,
            &config,
            &sample_model_gateway_config(),
            &transport,
            Utc::now(),
        )
        .await?
        .expect("expected a due self-model-reflection job to execute");

        assert_eq!(outcome.background_job_id, background_job_id);
        let signal_row = sqlx::query(
            r#"
            SELECT wake_signal_id
            FROM wake_signals
            WHERE execution_id = $1
            "#,
        )
        .bind(outcome.execution_id)
        .fetch_one(&ctx.pool)
        .await?;
        let wake_signal_id: Uuid = signal_row.get("wake_signal_id");
        let stored_signal = background::get_wake_signal(&ctx.pool, wake_signal_id).await?;
        assert_eq!(stored_signal.status, WakeSignalStatus::Rejected);
        assert_eq!(
            stored_signal
                .decision
                .as_ref()
                .expect("rejected wake signal should record a decision")
                .decision,
            WakeSignalDecisionKind::Rejected
        );

        let staged_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM ingress_events
            WHERE external_event_id = $1
            "#,
        )
        .bind(format!("wake-signal:{wake_signal_id}"))
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(staged_count, 0);
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn background_execution_times_out_and_marks_run_timed_out() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        let pid_file =
            std::env::temp_dir().join(format!("blue-lagoon-unconscious-{}.pid", Uuid::now_v7()));
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

        let background_job_id = seed_planned_background_job(&ctx.pool, Utc::now()).await?;
        let error = background_execution::execute_next_due_job(
            &ctx.pool,
            &config,
            &sample_model_gateway_config(),
            &FakeModelProviderTransport::new(),
            Utc::now(),
        )
        .await
        .expect_err("timed-out background execution should fail");
        assert!(error.to_string().contains("timed out"));

        let stored_job = background::get_job(&ctx.pool, background_job_id).await?;
        assert_eq!(stored_job.status, BackgroundJobStatus::Failed);

        let completed_runs =
            background::list_completed_job_runs(&ctx.pool, background_job_id, 5).await?;
        assert_eq!(completed_runs.len(), 1);
        assert_eq!(completed_runs[0].status, BackgroundJobRunStatus::TimedOut);
        let execution_id = completed_runs[0]
            .execution_id
            .expect("timed-out run should retain execution linkage");

        let execution = execution::get(&ctx.pool, execution_id).await?;
        assert_eq!(execution.status, "failed");
        assert!(
            execution
                .response_payload
                .as_ref()
                .and_then(|payload| payload.get("kind"))
                .and_then(serde_json::Value::as_str)
                .is_some_and(|kind| kind == "worker_timeout")
        );

        let audit_events = audit::list_for_execution(&ctx.pool, execution_id).await?;
        assert!(
            audit_events
                .iter()
                .any(|event| event.event_kind == "background_job_started")
        );
        assert!(
            audit_events
                .iter()
                .any(|event| event.event_kind == "background_job_timed_out")
        );

        let pid = read_pid_file(&pid_file).await?;
        tokio::time::sleep(StdDuration::from_millis(200)).await;
        assert!(
            !process_is_running(pid),
            "timed-out unconscious worker process {pid} should have been terminated"
        );
        let _ = fs::remove_file(pid_file);
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn background_execution_rejects_mismatched_worker_result_payloads() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        let worker_binary = support::workers_binary()?;
        config.worker.command = worker_binary.to_string_lossy().into_owned();
        config.worker.args = vec!["wrong-result-worker".to_string()];

        let background_job_id = seed_planned_background_job(&ctx.pool, Utc::now()).await?;
        let error = background_execution::execute_next_due_job(
            &ctx.pool,
            &config,
            &sample_model_gateway_config(),
            &FakeModelProviderTransport::new(),
            Utc::now(),
        )
        .await
        .expect_err("mismatched worker result should fail closed");
        assert!(error.to_string().contains("non-unconscious result payload"));

        let stored_job = background::get_job(&ctx.pool, background_job_id).await?;
        assert_eq!(stored_job.status, BackgroundJobStatus::Failed);

        let completed_runs =
            background::list_completed_job_runs(&ctx.pool, background_job_id, 5).await?;
        assert_eq!(completed_runs.len(), 1);
        assert_eq!(completed_runs[0].status, BackgroundJobRunStatus::Failed);
        let execution_id = completed_runs[0]
            .execution_id
            .expect("failed run should retain execution linkage");

        let execution = execution::get(&ctx.pool, execution_id).await?;
        assert_eq!(execution.status, "failed");
        assert!(
            execution
                .response_payload
                .as_ref()
                .and_then(|payload| payload.get("kind"))
                .and_then(serde_json::Value::as_str)
                .is_some_and(|kind| kind == "worker_failure")
        );

        let audit_events = audit::list_for_execution(&ctx.pool, execution_id).await?;
        assert!(
            audit_events
                .iter()
                .any(|event| event.event_kind == "background_job_failed")
        );
        Ok(())
    })
    .await
}

fn sample_trigger(kind: BackgroundTriggerKind, reason_summary: &str) -> BackgroundTrigger {
    BackgroundTrigger {
        trigger_id: Uuid::now_v7(),
        trigger_kind: kind,
        requested_at: Utc::now(),
        reason_summary: reason_summary.to_string(),
        payload_ref: Some("test://trigger".to_string()),
    }
}

fn sample_scope(summary: &str) -> UnconsciousScope {
    sample_scope_with_episode(summary, Uuid::now_v7())
}

fn sample_scope_with_episode(summary: &str, episode_id: Uuid) -> UnconsciousScope {
    UnconsciousScope {
        episode_ids: vec![episode_id],
        memory_artifact_ids: vec![Uuid::now_v7()],
        retrieval_artifact_ids: vec![Uuid::now_v7()],
        self_model_artifact_id: Some(Uuid::now_v7()),
        internal_principal_ref: Some("primary-user".to_string()),
        internal_conversation_ref: Some("telegram-primary".to_string()),
        summary: summary.to_string(),
    }
}

fn sample_budget() -> BackgroundExecutionBudget {
    BackgroundExecutionBudget {
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
            trigger_kind: "maintenance_trigger".to_string(),
            synthetic_trigger: None,
            status: "started".to_string(),
            request_payload: json!({
                "request_id": Uuid::now_v7(),
                "sent_at": Utc::now(),
                "worker_kind": "unconscious"
            }),
        },
    )
    .await?;
    Ok(execution_id)
}

async fn seed_episode_for_conversation(
    pool: &sqlx::PgPool,
    trace_id: Uuid,
    internal_conversation_ref: &str,
) -> Result<Uuid> {
    let execution_id = seed_execution(pool, trace_id).await?;
    let episode_id = Uuid::now_v7();
    foreground::insert_episode(
        pool,
        &NewEpisode {
            episode_id,
            trace_id,
            execution_id,
            ingress_id: None,
            internal_principal_ref: "principal:primary".to_string(),
            internal_conversation_ref: internal_conversation_ref.to_string(),
            trigger_kind: "background_seed".to_string(),
            trigger_source: "test".to_string(),
            status: "completed".to_string(),
            started_at: Utc::now(),
        },
    )
    .await?;
    foreground::insert_episode_message(
        pool,
        &NewEpisodeMessage {
            episode_message_id: Uuid::now_v7(),
            episode_id,
            trace_id,
            execution_id,
            message_order: 1,
            message_role: "user".to_string(),
            channel_kind: contracts::ChannelKind::Telegram,
            text_body: Some("Remember that I prefer concise summaries.".to_string()),
            external_message_id: None,
        },
    )
    .await?;
    foreground::insert_episode_message(
        pool,
        &NewEpisodeMessage {
            episode_message_id: Uuid::now_v7(),
            episode_id,
            trace_id,
            execution_id,
            message_order: 2,
            message_role: "assistant".to_string(),
            channel_kind: contracts::ChannelKind::Telegram,
            text_body: Some("Acknowledged and summarized briefly.".to_string()),
            external_message_id: None,
        },
    )
    .await?;
    Ok(episode_id)
}

async fn seed_retrieval_artifact(
    pool: &sqlx::PgPool,
    episode_id: Uuid,
    internal_conversation_ref: &str,
) -> Result<Uuid> {
    let retrieval_artifact_id = Uuid::now_v7();
    continuity::insert_retrieval_artifact(
        pool,
        &continuity::NewRetrievalArtifact {
            retrieval_artifact_id,
            source_kind: "episode".to_string(),
            source_episode_id: Some(episode_id),
            source_memory_artifact_id: None,
            internal_conversation_ref: Some(internal_conversation_ref.to_string()),
            lexical_document: "Concise summaries are preferred.".to_string(),
            relevance_timestamp: Utc::now(),
            status: "active".to_string(),
            payload: json!({ "projection": "test" }),
        },
    )
    .await?;
    Ok(retrieval_artifact_id)
}

async fn seed_self_model_artifact(pool: &sqlx::PgPool, trace_id: Uuid) -> Result<Uuid> {
    let execution_id = seed_execution(pool, trace_id).await?;
    let self_model_artifact_id = Uuid::now_v7();
    continuity::insert_self_model_artifact(
        pool,
        &continuity::NewSelfModelArtifact {
            self_model_artifact_id,
            proposal_id: None,
            trace_id: Some(trace_id),
            execution_id: Some(execution_id),
            episode_id: None,
            artifact_origin: "test_seed".to_string(),
            status: "active".to_string(),
            stable_identity: "blue-lagoon".to_string(),
            role: "assistant".to_string(),
            communication_style: "concise".to_string(),
            capabilities: vec!["memory".to_string()],
            constraints: vec!["single-user".to_string()],
            preferences: vec!["clarity".to_string()],
            current_goals: vec!["maintain continuity".to_string()],
            current_subgoals: vec!["review background planning".to_string()],
            superseded_at: None,
            superseded_by_artifact_id: None,
            supersedes_artifact_id: None,
            payload: json!({ "seed": true }),
        },
    )
    .await?;
    Ok(self_model_artifact_id)
}

async fn count_background_jobs(pool: &sqlx::PgPool) -> Result<i64> {
    let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM background_jobs")
        .fetch_one(pool)
        .await?;
    Ok(count)
}

async fn seed_planned_background_job(
    pool: &sqlx::PgPool,
    available_at: chrono::DateTime<chrono::Utc>,
) -> Result<Uuid> {
    seed_planned_background_job_with_kind(
        pool,
        available_at,
        UnconsciousJobKind::MemoryConsolidation,
    )
    .await
}

async fn seed_planned_background_job_with_kind(
    pool: &sqlx::PgPool,
    available_at: chrono::DateTime<chrono::Utc>,
    job_kind: UnconsciousJobKind,
) -> Result<Uuid> {
    let trace_id = Uuid::now_v7();
    let scoped_episode_id =
        seed_episode_for_conversation(pool, trace_id, "telegram-primary").await?;
    let background_job_id = Uuid::now_v7();
    background::insert_job(
        pool,
        &background::NewBackgroundJob {
            background_job_id,
            trace_id,
            job_kind,
            trigger: sample_trigger(BackgroundTriggerKind::TimeSchedule, "scheduled maintenance"),
            deduplication_key: format!("job:{job_kind:?}:scheduled:{background_job_id}"),
            scope: sample_scope_with_episode("maintenance scope", scoped_episode_id),
            budget: sample_budget(),
            status: BackgroundJobStatus::Planned,
            available_at,
            lease_expires_at: None,
            last_started_at: None,
            last_completed_at: None,
        },
    )
    .await?;
    Ok(background_job_id)
}

fn sample_model_gateway_config() -> ResolvedModelGatewayConfig {
    ResolvedModelGatewayConfig {
        foreground: ResolvedForegroundModelRouteConfig {
            provider: contracts::ModelProviderKind::ZAi,
            model: "z-ai-background".to_string(),
            api_base_url: "https://api.z.ai/api/paas/v4".to_string(),
            api_key: "test-key".to_string(),
            timeout_ms: 20_000,
        },
    }
}

async fn read_pid_file(path: &std::path::Path) -> Result<u32> {
    for _ in 0..20 {
        match fs::read_to_string(path) {
            Ok(contents) => return Ok(contents.trim().parse()?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                tokio::time::sleep(StdDuration::from_millis(50)).await;
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
