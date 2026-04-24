mod support;

use anyhow::Result;
use chrono::{Duration as ChronoDuration, Utc};
use contracts::{
    BackgroundExecutionBudget, BackgroundTrigger, BackgroundTriggerKind, ChannelKind,
    ModelProviderKind, UnconsciousJobKind, UnconsciousScope,
};
use serde_json::json;
use serial_test::serial;
use sqlx::Row;
use std::{env, fs, process::Command, time::Duration};
use uuid::Uuid;

use harness::{
    audit, background,
    config::{
        ForegroundModelRouteConfig, ModelGatewayConfig, SelfModelConfig, TelegramConfig,
        TelegramForegroundBindingConfig, ZAiProviderConfig,
    },
    continuity,
    execution::{self, NewExecutionRecord},
    foreground::{self, NewEpisode, NewEpisodeMessage},
    model_gateway::{FakeModelProviderTransport, ProviderHttpResponse},
    recovery,
    runtime::{self, HarnessOptions, HarnessOutcome},
};

#[tokio::test]
#[serial]
async fn background_runtime_flows_due_job_to_memory_merge_end_to_end() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        config.model_gateway = Some(sample_model_gateway_config(
            "BLUE_LAGOON_TEST_UNCONSCIOUS_RUNTIME_API_KEY",
        ));
        let worker_binary = support::workers_binary()?;
        config.worker.command = worker_binary.to_string_lossy().into_owned();
        config.worker.args = vec!["unconscious-worker".to_string()];

        let background_job_id = seed_planned_background_job_with_kind(
            &ctx.pool,
            Utc::now(),
            UnconsciousJobKind::MemoryConsolidation,
        )
        .await?;
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

        let outcome = with_env_var(
            "BLUE_LAGOON_TEST_UNCONSCIOUS_RUNTIME_API_KEY",
            Some("test-unconscious-runtime-key"),
            || async {
                runtime::run_harness_once_with_transport(
                    &config,
                    HarnessOptions {
                        once: true,
                        idle: false,
                        background_once: true,
                        synthetic_trigger: None,
                    },
                    &transport,
                )
                .await
            },
        )
        .await?;

        let execution_id = match outcome {
            HarnessOutcome::BackgroundCompleted {
                background_job_id: executed_job_id,
                execution_id,
                summary,
                ..
            } => {
                assert_eq!(executed_job_id, background_job_id);
                assert!(summary.contains("canonical_writes=1"));
                execution_id
            }
            other => panic!("expected background completion outcome, got {other:?}"),
        };

        let record = execution::get(&ctx.pool, execution_id).await?;
        assert_eq!(record.status, "completed");

        let proposals = continuity::list_proposals_for_execution(&ctx.pool, execution_id).await?;
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].status, "accepted");
        assert_eq!(proposals[0].source_loop_kind, "unconscious");

        let merge_decision =
            continuity::get_merge_decision_by_proposal(&ctx.pool, proposals[0].proposal_id)
                .await?
                .expect("accepted unconscious proposal should have a merge decision");
        assert_eq!(merge_decision.decision_kind, "accepted");

        let memory_artifact_id = merge_decision
            .accepted_memory_artifact_id
            .expect("accepted memory proposal should create a canonical artifact");
        let memory_artifact =
            continuity::get_memory_artifact(&ctx.pool, memory_artifact_id).await?;
        assert_eq!(memory_artifact.content_text, "maintenance lexical summary");
        assert_eq!(memory_artifact.subject_ref, "primary-user");

        let retrieval_artifacts = continuity::list_active_retrieval_artifacts_for_conversation(
            &ctx.pool,
            "telegram-primary",
            10,
        )
        .await?;
        assert_eq!(retrieval_artifacts.len(), 1);
        assert_eq!(
            retrieval_artifacts[0].lexical_document,
            "maintenance lexical summary"
        );

        let audit_events = audit::list_for_execution(&ctx.pool, execution_id).await?;
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
async fn background_runtime_recovers_stranded_run_during_supervisor_restart() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        config.model_gateway = Some(sample_model_gateway_config(
            "BLUE_LAGOON_TEST_UNCONSCIOUS_RUNTIME_API_KEY",
        ));
        let worker_binary = support::workers_binary()?;
        config.worker.command = worker_binary.to_string_lossy().into_owned();
        config.worker.args = vec!["unconscious-worker".to_string()];

        let background_job_id = seed_planned_background_job_with_kind(
            &ctx.pool,
            Utc::now() - ChronoDuration::minutes(1),
            UnconsciousJobKind::MemoryConsolidation,
        )
        .await?;
        let job = background::get_job(&ctx.pool, background_job_id).await?;
        let stranded_execution_id = seed_execution(&ctx.pool, job.trace_id).await?;
        let stranded_run_id = Uuid::now_v7();
        let lease_acquired_at = Utc::now() - ChronoDuration::minutes(5);
        let lease_expires_at = Utc::now() + ChronoDuration::minutes(5);

        background::update_job_status(
            &ctx.pool,
            background_job_id,
            &background::UpdateBackgroundJobStatus {
                status: background::BackgroundJobStatus::Running,
                lease_expires_at: Some(lease_expires_at),
                last_started_at: Some(lease_acquired_at),
                last_completed_at: None,
            },
        )
        .await?;
        background::insert_job_run(
            &ctx.pool,
            &background::NewBackgroundJobRun {
                background_job_run_id: stranded_run_id,
                background_job_id,
                trace_id: job.trace_id,
                execution_id: Some(stranded_execution_id),
                lease_token: Uuid::now_v7(),
                status: background::BackgroundJobRunStatus::Running,
                worker_pid: None,
                lease_acquired_at,
                lease_expires_at,
                started_at: Some(lease_acquired_at),
                completed_at: None,
                result_payload: None,
                failure_payload: None,
            },
        )
        .await?;

        let transport = FakeModelProviderTransport::new();
        transport.push_response(Ok(ProviderHttpResponse {
            status: 200,
            body: json!({
                "choices": [{
                    "message": { "content": "recovered maintenance lexical summary" },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 15,
                    "completion_tokens": 8
                }
            }),
        }));

        let outcome = with_env_var(
            "BLUE_LAGOON_TEST_UNCONSCIOUS_RUNTIME_API_KEY",
            Some("test-unconscious-runtime-key"),
            || async {
                runtime::run_harness_once_with_transport(
                    &config,
                    HarnessOptions {
                        once: true,
                        idle: false,
                        background_once: true,
                        synthetic_trigger: None,
                    },
                    &transport,
                )
                .await
            },
        )
        .await?;

        let recovered_execution_id = match outcome {
            HarnessOutcome::BackgroundCompleted {
                background_job_id: executed_job_id,
                execution_id,
                ..
            } => {
                assert_eq!(executed_job_id, background_job_id);
                execution_id
            }
            other => panic!("expected background completion outcome, got {other:?}"),
        };
        assert_ne!(recovered_execution_id, stranded_execution_id);

        let stranded_execution = execution::get(&ctx.pool, stranded_execution_id).await?;
        assert_eq!(stranded_execution.status, "failed");
        assert_eq!(
            stranded_execution
                .response_payload
                .as_ref()
                .and_then(|payload| payload.get("kind"))
                .and_then(serde_json::Value::as_str),
            Some("supervisor_restart_recovery")
        );

        let completed_runs =
            background::list_completed_job_runs(&ctx.pool, background_job_id, 10).await?;
        assert!(completed_runs.iter().any(|run| {
            run.background_job_run_id == stranded_run_id
                && run.status == background::BackgroundJobRunStatus::Failed
        }));
        assert!(completed_runs.iter().any(|run| {
            run.execution_id == Some(recovered_execution_id)
                && run.status == background::BackgroundJobRunStatus::Completed
        }));

        let checkpoint_row = sqlx::query(
            r#"
            SELECT recovery_reason_code, recovery_decision
            FROM recovery_checkpoints
            WHERE background_job_id = $1
              AND background_job_run_id = $2
            ORDER BY created_at DESC, recovery_checkpoint_id DESC
            LIMIT 1
            "#,
        )
        .bind(background_job_id)
        .bind(stranded_run_id)
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(
            checkpoint_row.get::<String, _>("recovery_reason_code"),
            "supervisor_restart".to_string()
        );
        assert_eq!(
            checkpoint_row.get::<String, _>("recovery_decision"),
            "retry".to_string()
        );

        let diagnostics = recovery::list_operational_diagnostics(&ctx.pool, 20).await?;
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.reason_code == "background_supervisor_restart_recovered"
                && diagnostic.execution_id == Some(stranded_execution_id)
        }));

        let audit_events = audit::list_for_execution(&ctx.pool, stranded_execution_id).await?;
        assert!(
            audit_events
                .iter()
                .any(|event| { event.event_kind == "background_job_supervisor_restart_recovered" })
        );

        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn background_runtime_flows_wake_signal_to_foreground_conversion_end_to_end() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        config.self_model = Some(SelfModelConfig {
            seed_path: support::workspace_root()
                .join("config")
                .join("self_model_seed.toml"),
        });
        config.model_gateway = Some(sample_model_gateway_config(
            "BLUE_LAGOON_TEST_UNCONSCIOUS_RUNTIME_API_KEY",
        ));
        config.telegram = Some(sample_telegram_config());
        let worker_binary = support::workers_binary()?;
        config.worker.command = worker_binary.to_string_lossy().into_owned();
        config.worker.args = vec!["unconscious-worker".to_string()];

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

        let outcome = with_env_var(
            "BLUE_LAGOON_TEST_UNCONSCIOUS_RUNTIME_API_KEY",
            Some("test-unconscious-runtime-key"),
            || async {
                runtime::run_harness_once_with_transport(
                    &config,
                    HarnessOptions {
                        once: true,
                        idle: false,
                        background_once: true,
                        synthetic_trigger: None,
                    },
                    &transport,
                )
                .await
            },
        )
        .await?;

        let execution_id = match outcome {
            HarnessOutcome::BackgroundCompleted {
                background_job_id: executed_job_id,
                execution_id,
                summary,
                ..
            } => {
                assert_eq!(executed_job_id, background_job_id);
                assert!(summary.contains("wake_signals=1"));
                execution_id
            }
            other => panic!("expected background completion outcome, got {other:?}"),
        };

        let signal_row = sqlx::query(
            r#"
            SELECT wake_signal_id
            FROM wake_signals
            WHERE execution_id = $1
            "#,
        )
        .bind(execution_id)
        .fetch_one(&ctx.pool)
        .await?;
        let wake_signal_id: Uuid = signal_row.get("wake_signal_id");
        let stored_signal = background::get_wake_signal(&ctx.pool, wake_signal_id).await?;
        assert_eq!(stored_signal.status, background::WakeSignalStatus::Accepted);

        let ingress_row = sqlx::query(
            r#"
            SELECT internal_conversation_ref, foreground_status
            FROM ingress_events
            WHERE external_event_id = $1
            "#,
        )
        .bind(format!("wake-signal:{wake_signal_id}"))
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(
            ingress_row.get::<String, _>("internal_conversation_ref"),
            "telegram-primary".to_string()
        );
        assert_eq!(
            ingress_row.get::<String, _>("foreground_status"),
            "pending".to_string()
        );

        let active_self_model_artifacts =
            continuity::list_active_self_model_artifacts(&ctx.pool, 10).await?;
        assert_eq!(active_self_model_artifacts.len(), 1);
        assert!(active_self_model_artifacts[0]
            .preferences
            .iter()
            .any(|value| value.contains("concise progress updates")));

        let audit_events = audit::list_for_execution(&ctx.pool, execution_id).await?;
        assert!(
            audit_events
                .iter()
                .any(|event| event.event_kind == "wake_signal_reviewed")
        );
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
async fn background_runtime_timeout_fails_closed_and_records_bounded_termination() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        config.model_gateway = Some(sample_model_gateway_config(
            "BLUE_LAGOON_TEST_UNCONSCIOUS_RUNTIME_API_KEY",
        ));
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

        let background_job_id = seed_planned_background_job_with_kind(
            &ctx.pool,
            Utc::now(),
            UnconsciousJobKind::MemoryConsolidation,
        )
        .await?;

        let error = with_env_var(
            "BLUE_LAGOON_TEST_UNCONSCIOUS_RUNTIME_API_KEY",
            Some("test-unconscious-runtime-key"),
            || async {
                runtime::run_harness_once_with_transport(
                    &config,
                    HarnessOptions {
                        once: true,
                        idle: false,
                        background_once: true,
                        synthetic_trigger: None,
                    },
                    &FakeModelProviderTransport::new(),
                )
                .await
            },
        )
        .await
        .expect_err("timed-out background runtime should fail");
        assert!(error.to_string().contains("timed out"));

        let stored_job = background::get_job(&ctx.pool, background_job_id).await?;
        assert_eq!(stored_job.status, background::BackgroundJobStatus::Failed);

        let completed_runs =
            background::list_completed_job_runs(&ctx.pool, background_job_id, 5).await?;
        assert_eq!(completed_runs.len(), 1);
        assert_eq!(
            completed_runs[0].status,
            background::BackgroundJobRunStatus::TimedOut
        );
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

        let proposals = continuity::list_proposals_for_execution(&ctx.pool, execution_id).await?;
        assert!(proposals.is_empty());

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
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(
            !process_is_running(pid),
            "timed-out unconscious worker process {pid} should have been terminated"
        );
        let _ = fs::remove_file(pid_file);
        Ok(())
    })
    .await
}

async fn with_env_var<F, Fut, T>(name: &str, value: Option<&str>, action: F) -> Result<T>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let original = env::var_os(name);
    unsafe {
        match value {
            Some(value) => env::set_var(name, value),
            None => env::remove_var(name),
        }
    }

    let result = action().await;

    unsafe {
        match original {
            Some(value) => env::set_var(name, value),
            None => env::remove_var(name),
        }
    }

    result
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
            message_order: 0,
            message_role: "user".to_string(),
            channel_kind: ChannelKind::Telegram,
            text_body: Some("background seed context".to_string()),
            external_message_id: None,
        },
    )
    .await?;
    Ok(episode_id)
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
            status: background::BackgroundJobStatus::Planned,
            available_at,
            lease_expires_at: None,
            last_started_at: None,
            last_completed_at: None,
        },
    )
    .await?;
    Ok(background_job_id)
}

fn sample_model_gateway_config(api_key_env: &str) -> ModelGatewayConfig {
    ModelGatewayConfig {
        foreground: ForegroundModelRouteConfig {
            provider: ModelProviderKind::ZAi,
            model: "z-ai-background".to_string(),
            api_base_url: Some("https://api.z.ai/api/coding/paas/v4".to_string()),
            api_key_env: api_key_env.to_string(),
            timeout_ms: 20_000,
        },
        z_ai: Some(ZAiProviderConfig {
            api_surface: None,
            api_base_url: Some("https://api.z.ai/api/coding/paas/v4".to_string()),
        }),
    }
}

fn sample_telegram_config() -> TelegramConfig {
    TelegramConfig {
        api_base_url: "https://api.telegram.org".to_string(),
        bot_token_env: "BLUE_LAGOON_TEST_TELEGRAM_TOKEN".to_string(),
        poll_limit: 10,
        foreground_binding: Some(TelegramForegroundBindingConfig {
            allowed_user_id: 42,
            allowed_chat_id: 24,
            internal_principal_ref: "primary-user".to_string(),
            internal_conversation_ref: "telegram-primary".to_string(),
        }),
    }
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
