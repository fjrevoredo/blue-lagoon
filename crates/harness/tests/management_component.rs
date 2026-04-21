mod support;

use anyhow::Result;
use chrono::{Duration, Utc};
use contracts::{ChannelKind, IngressEventKind, NormalizedIngress};
use harness::{
    background::{
        self, BackgroundJobRunStatus, BackgroundJobStatus, NewBackgroundJob, NewBackgroundJobRun,
        NewWakeSignalRecord, WakeSignalStatus,
    },
    foreground::{self, NewIngressEvent},
    management,
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
                execution_id: Some(Uuid::now_v7()),
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
