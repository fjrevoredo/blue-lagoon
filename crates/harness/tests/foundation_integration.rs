mod support;

use anyhow::Result;
use chrono::Utc;
use contracts::{
    BackgroundExecutionBudget, BackgroundTrigger, BackgroundTriggerKind, ChannelKind,
    ModelProviderKind, UnconsciousJobKind, UnconsciousScope, WorkerRequest,
};
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::Row;
use std::{env, fs, process::Command, time::Duration};
use uuid::Uuid;

use harness::{
    audit,
    background::{self, WakeSignalStatus},
    config::{
        ForegroundModelRouteConfig, ModelGatewayConfig, SelfModelConfig, TelegramConfig,
        TelegramForegroundBindingConfig, ZAiProviderConfig,
    },
    execution::{self, NewExecutionRecord},
    foreground::{self, NewEpisode, NewEpisodeMessage},
    model_gateway::{FakeModelProviderTransport, ProviderHttpResponse},
    runtime::{self, HarnessOptions, HarnessOutcome, SyntheticTrigger},
    worker,
};

#[tokio::test]
#[serial]
async fn synthetic_trigger_runs_end_to_end_and_persists_outputs() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        let worker_binary = support::workers_binary()?;
        config.worker.command = worker_binary.to_string_lossy().into_owned();
        config.worker.args = vec!["smoke-worker".to_string()];

        let outcome = runtime::run_harness_once(
            &config,
            HarnessOptions {
                once: true,
                idle: false,
                background_once: false,
                synthetic_trigger: Some(SyntheticTrigger::Smoke),
            },
        )
        .await?;

        let execution_id = match outcome {
            HarnessOutcome::SyntheticCompleted { execution_id, .. } => execution_id,
            HarnessOutcome::IdleVerified => panic!("synthetic trigger should not return idle"),
            HarnessOutcome::BackgroundNoDueJob | HarnessOutcome::BackgroundCompleted { .. } => {
                panic!("synthetic trigger should not return a background outcome")
            }
        };

        let record = execution::get(&ctx.pool, execution_id).await?;
        assert_eq!(record.status, "completed");
        assert!(record.worker_pid.is_some());
        assert!(record.response_payload.is_some());

        let audit_events = audit::list_for_execution(&ctx.pool, execution_id).await?;
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
    })
    .await
}

#[tokio::test]
#[serial]
async fn idle_boot_verifies_schema_and_returns_idle() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let outcome = runtime::run_harness_once(
            &ctx.config,
            HarnessOptions {
                once: true,
                idle: true,
                background_once: false,
                synthetic_trigger: None,
            },
        )
        .await?;

        assert_eq!(outcome, HarnessOutcome::IdleVerified);
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn timed_out_worker_is_terminated() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        let pid_file =
            std::env::temp_dir().join(format!("blue-lagoon-worker-{}.pid", Uuid::now_v7()));
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
    })
    .await
}

#[tokio::test]
#[serial]
async fn timed_out_foreground_run_is_marked_failed_and_audited() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        let pid_file =
            std::env::temp_dir().join(format!("blue-lagoon-worker-{}.pid", Uuid::now_v7()));
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
                background_once: false,
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
        .fetch_one(&ctx.pool)
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

        let audit_events = audit::list_for_execution(&ctx.pool, execution_id).await?;
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
    })
    .await
}

#[tokio::test]
#[serial]
async fn background_runtime_runs_due_job_end_to_end_and_stages_approved_wake_signal() -> Result<()>
{
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        config.self_model = Some(SelfModelConfig {
            seed_path: support::workspace_root()
                .join("config")
                .join("self_model_seed.toml"),
        });
        config.model_gateway = Some(sample_model_gateway_config(
            "BLUE_LAGOON_TEST_BACKGROUND_RUNTIME_API_KEY",
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
                    "message": {
                        "content": json!({
                            "identity_delta": null,
                            "no_change_rationale": "No durable identity change is warranted from this maintenance review.",
                            "diagnostics": [],
                            "wake_signals": [{
                                "signal_id": "018f0000-0000-7000-8000-000000000601",
                                "reason": "maintenance_insight_ready",
                                "priority": "normal",
                                "reason_code": "maintenance_review_ready",
                                "summary": "Background maintenance found an update worth surfacing.",
                                "payload_ref": "background_job:self_model_reflection"
                            }]
                        }).to_string()
                    },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 16,
                    "completion_tokens": 8
                }
            }),
        }));

        let outcome = with_env_var(
            "BLUE_LAGOON_TEST_BACKGROUND_RUNTIME_API_KEY",
            Some("test-background-runtime-key"),
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

        let (execution_id, trace_id, summary) = match outcome {
            HarnessOutcome::BackgroundCompleted {
                background_job_id: executed_job_id,
                execution_id,
                trace_id,
                summary,
            } => {
                assert_eq!(executed_job_id, background_job_id);
                (execution_id, trace_id, summary)
            }
            other => panic!("expected background completion outcome, got {other:?}"),
        };

        assert!(summary.contains("wake_signals=1"));
        let record = execution::get(&ctx.pool, execution_id).await?;
        assert_eq!(record.status, "completed");

        let stored_job = background::get_job(&ctx.pool, background_job_id).await?;
        assert_eq!(stored_job.status, background::BackgroundJobStatus::Completed);

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
        assert_eq!(stored_signal.status, WakeSignalStatus::Accepted);

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

        let audit_events = audit::list_for_execution(&ctx.pool, execution_id).await?;
        assert!(audit_events.iter().all(|event| event.trace_id == trace_id));
        assert!(
            audit_events
                .iter()
                .any(|event| event.event_kind == "background_job_completed")
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
async fn background_runtime_returns_no_due_job_without_requiring_gateway_config() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let outcome = runtime::run_harness_once_with_transport(
            &ctx.config,
            HarnessOptions {
                once: true,
                idle: false,
                background_once: true,
                synthetic_trigger: None,
            },
            &FakeModelProviderTransport::new(),
        )
        .await?;

        assert_eq!(outcome, HarnessOutcome::BackgroundNoDueJob);

        let row = sqlx::query(
            r#"
            SELECT event_kind
            FROM audit_events
            ORDER BY occurred_at DESC, event_id DESC
            LIMIT 1
            "#,
        )
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(
            row.get::<String, _>("event_kind"),
            "background_maintenance_no_due_job".to_string()
        );
        Ok(())
    })
    .await
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
        openrouter: None,
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
