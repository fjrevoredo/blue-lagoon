mod support;

use anyhow::Result;
use contracts::ModelProviderKind;
use harness::{
    audit,
    config::{
        ResolvedForegroundModelRouteConfig, ResolvedModelGatewayConfig, ResolvedTelegramConfig,
        SelfModelConfig,
    },
    execution, foreground, model_gateway, runtime, telegram,
};
use serial_test::serial;
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
#[serial]
async fn telegram_fixture_runtime_run_persists_response_and_trace_linked_audit() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        config.self_model = Some(SelfModelConfig {
            seed_path: support::workspace_root()
                .join("config")
                .join("self_model_seed.toml"),
        });
        let worker_binary = support::workers_binary()?;
        config.worker.command = worker_binary.to_string_lossy().into_owned();
        config.worker.args = vec!["conscious-worker".to_string()];

        let transport = model_gateway::FakeModelProviderTransport::new();
        transport.push_response(Ok(model_gateway::ProviderHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{
                    "message": { "content": "assistant reply from phase2 integration" },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 17,
                    "completion_tokens": 8
                }
            }),
        }));
        let mut delivery = telegram::FakeTelegramDelivery::default();

        let summary = runtime::run_telegram_fixture_with(
            &ctx.pool,
            &config,
            &sample_telegram_config(),
            &sample_model_gateway_config(),
            &telegram_fixture("private_text_message.json"),
            &transport,
            &mut delivery,
        )
        .await?;

        assert_eq!(summary.fetched_updates, 1);
        assert_eq!(summary.completed_count, 1);
        assert_eq!(delivery.sent_messages().len(), 1);
        assert_eq!(
            delivery.sent_messages()[0].text,
            "assistant reply from phase2 integration"
        );

        let execution_row = sqlx::query(
            r#"
            SELECT execution_id, trace_id
            FROM execution_records
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .fetch_one(&ctx.pool)
        .await?;
        let execution_id: Uuid = execution_row.get("execution_id");
        let trace_id: Uuid = execution_row.get("trace_id");

        let execution = execution::get(&ctx.pool, execution_id).await?;
        assert_eq!(execution.status, "completed");

        let episode_row = sqlx::query(
            r#"
            SELECT episode_id
            FROM episodes
            WHERE execution_id = $1
            "#,
        )
        .bind(execution_id)
        .fetch_one(&ctx.pool)
        .await?;
        let episode_id: Uuid = episode_row.get("episode_id");

        let messages = foreground::list_episode_messages(&ctx.pool, episode_id).await?;
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].message_role, "assistant");
        assert_eq!(
            messages[1].text_body.as_deref(),
            Some("assistant reply from phase2 integration")
        );

        let audit_events = audit::list_for_execution(&ctx.pool, execution_id).await?;
        assert!(audit_events.iter().all(|event| event.trace_id == trace_id));
        assert!(
            audit_events
                .iter()
                .any(|event| event.event_kind == "foreground_trigger_accepted")
        );
        assert!(
            audit_events
                .iter()
                .any(|event| event.event_kind == "foreground_context_assembled")
        );
        assert!(
            audit_events
                .iter()
                .any(|event| event.event_kind == "foreground_execution_completed")
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn telegram_fixture_runtime_duplicate_ingress_is_idempotent_and_audited() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        config.self_model = Some(SelfModelConfig {
            seed_path: support::workspace_root()
                .join("config")
                .join("self_model_seed.toml"),
        });
        let worker_binary = support::workers_binary()?;
        config.worker.command = worker_binary.to_string_lossy().into_owned();
        config.worker.args = vec!["conscious-worker".to_string()];

        let transport = model_gateway::FakeModelProviderTransport::new();
        transport.push_response(Ok(model_gateway::ProviderHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{
                    "message": { "content": "assistant reply for duplicate integration" },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 15,
                    "completion_tokens": 6
                }
            }),
        }));
        let mut delivery = telegram::FakeTelegramDelivery::default();

        let first = runtime::run_telegram_fixture_with(
            &ctx.pool,
            &config,
            &sample_telegram_config(),
            &sample_model_gateway_config(),
            &telegram_fixture("private_text_message.json"),
            &transport,
            &mut delivery,
        )
        .await?;
        let second = runtime::run_telegram_fixture_with(
            &ctx.pool,
            &config,
            &sample_telegram_config(),
            &sample_model_gateway_config(),
            &telegram_fixture("private_text_message.json"),
            &transport,
            &mut delivery,
        )
        .await?;

        assert_eq!(first.completed_count, 1);
        assert_eq!(second.duplicate_count, 1);
        assert_eq!(delivery.sent_messages().len(), 1);
        assert_eq!(transport.seen_requests().len(), 1);

        let execution_row = sqlx::query(
            r#"
            SELECT execution_id, trace_id
            FROM execution_records
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .fetch_one(&ctx.pool)
        .await?;
        let execution_id: Uuid = execution_row.get("execution_id");
        let trace_id: Uuid = execution_row.get("trace_id");

        let trace_events = audit::list_for_trace(&ctx.pool, trace_id).await?;
        assert!(
            trace_events
                .iter()
                .any(|event| event.event_kind == "foreground_trigger_duplicate")
        );

        let execution_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM execution_records
            "#,
        )
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(execution_count, 1);

        let audit_events = audit::list_for_execution(&ctx.pool, execution_id).await?;
        assert!(
            audit_events
                .iter()
                .any(|event| event.event_kind == "foreground_execution_completed")
        );
        Ok(())
    })
    .await
}

fn sample_telegram_config() -> ResolvedTelegramConfig {
    ResolvedTelegramConfig {
        api_base_url: "https://api.telegram.org".to_string(),
        bot_token: "telegram-secret".to_string(),
        allowed_user_id: 42,
        allowed_chat_id: 42,
        internal_principal_ref: "primary-user".to_string(),
        internal_conversation_ref: "telegram-primary".to_string(),
        poll_limit: 10,
    }
}

fn telegram_fixture(name: &str) -> std::path::PathBuf {
    support::workspace_root()
        .join("crates")
        .join("harness")
        .join("tests")
        .join("fixtures")
        .join("telegram")
        .join(name)
}

fn sample_model_gateway_config() -> ResolvedModelGatewayConfig {
    ResolvedModelGatewayConfig {
        foreground: ResolvedForegroundModelRouteConfig {
            provider: ModelProviderKind::ZAi,
            model: "z-ai-foreground".to_string(),
            api_base_url: "https://api.z.ai/api/paas/v4".to_string(),
            api_key: "provider-secret".to_string(),
            timeout_ms: 30_000,
        },
    }
}
