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
    background_planning::{self, BackgroundPlanningDecision, BackgroundPlanningRequest},
    continuity,
    execution::{self, NewExecutionRecord},
    foreground::{self, NewEpisode, NewEpisodeMessage},
    migration,
};
use serde_json::json;
use serial_test::serial;
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
#[serial]
async fn migration_application_creates_unconscious_loop_tables() -> Result<()> {
    support::with_clean_database(|ctx| async move {
        let summary =
            migration::apply_pending_migrations(&ctx.pool, env!("CARGO_PKG_VERSION")).await?;

        assert_eq!(summary.discovered_versions, vec![1, 2, 3, 4, 5]);

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
    UnconsciousScope {
        episode_ids: vec![Uuid::now_v7()],
        memory_artifact_ids: vec![Uuid::now_v7()],
        retrieval_artifact_ids: vec![Uuid::now_v7()],
        self_model_artifact_id: Some(Uuid::now_v7()),
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
