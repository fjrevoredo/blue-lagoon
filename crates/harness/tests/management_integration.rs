mod support;

use anyhow::Result;
use harness::{
    config::{ForegroundModelRouteConfig, ModelGatewayConfig},
    management::{
        self, BackgroundEnqueueOutcome, BackgroundRunNextOutcome, EnqueueBackgroundJobRequest,
    },
    model_gateway::{FakeModelProviderTransport, ProviderHttpResponse},
};
use serde_json::json;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn enqueue_background_job_plans_once_and_then_suppresses_duplicates() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let _ = &ctx.pool;
        let request = EnqueueBackgroundJobRequest {
            job_kind: contracts::UnconsciousJobKind::MemoryConsolidation,
            trigger_kind: contracts::BackgroundTriggerKind::MaintenanceTrigger,
            internal_conversation_ref: Some("telegram-primary".to_string()),
            reason: Some("manual verification".to_string()),
        };

        let first = management::enqueue_background_job(&ctx.config, request.clone()).await?;
        let second = management::enqueue_background_job(&ctx.config, request).await?;

        match first {
            BackgroundEnqueueOutcome::Planned { .. } => {}
            other => panic!("expected planned background job, got {other:?}"),
        }
        match second {
            BackgroundEnqueueOutcome::SuppressedDuplicate { .. } => {}
            other => panic!("expected duplicate suppression, got {other:?}"),
        }
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn run_next_background_job_returns_no_due_job_without_work() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        configure_management_execution(&mut config);

        let outcome = management::run_next_background_job_with_transport(
            &config,
            &FakeModelProviderTransport::new(),
        )
        .await?;
        assert!(matches!(outcome, BackgroundRunNextOutcome::NoDueJob));
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn run_next_background_job_executes_due_job_from_management_surface() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        configure_management_execution(&mut config);

        let enqueue = management::enqueue_background_job(
            &config,
            EnqueueBackgroundJobRequest {
                job_kind: contracts::UnconsciousJobKind::MemoryConsolidation,
                trigger_kind: contracts::BackgroundTriggerKind::MaintenanceTrigger,
                internal_conversation_ref: Some("telegram-primary".to_string()),
                reason: Some("manual verification".to_string()),
            },
        )
        .await?;
        assert!(matches!(enqueue, BackgroundEnqueueOutcome::Planned { .. }));

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

        let outcome =
            management::run_next_background_job_with_transport(&config, &transport).await?;
        match outcome {
            BackgroundRunNextOutcome::Completed { summary, .. } => {
                assert!(summary.contains("memory_consolidation"));
            }
            other => panic!("expected completed background job, got {other:?}"),
        }
        Ok(())
    })
    .await
}

fn configure_management_execution(config: &mut harness::config::RuntimeConfig) {
    let worker_binary = support::workers_binary().expect("workers binary should resolve");
    config.worker.command = worker_binary.to_string_lossy().into_owned();
    config.worker.args = vec!["unconscious-worker".to_string()];

    let api_key_env = format!(
        "BLUE_LAGOON_TEST_FOREGROUND_API_KEY_{}",
        uuid::Uuid::now_v7()
    );
    // SAFETY: these serial integration tests set a unique env var name and restore process
    // state only through process exit, so there is no concurrent aliasing across tests.
    unsafe {
        std::env::set_var(&api_key_env, "test-key");
    }

    config.model_gateway = Some(ModelGatewayConfig {
        foreground: ForegroundModelRouteConfig {
            provider: contracts::ModelProviderKind::ZAi,
            model: "z-ai-background".to_string(),
            api_base_url: Some("https://api.z.ai/api/paas/v4".to_string()),
            api_key_env,
            timeout_ms: 20_000,
        },
        z_ai: None,
    });
}
