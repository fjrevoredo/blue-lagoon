mod support;

use anyhow::Result;
use contracts::ModelProviderKind;
use harness::{
    audit,
    config::{
        ResolvedForegroundModelRouteConfig, ResolvedModelGatewayConfig, ResolvedTelegramConfig,
        SelfModelConfig,
    },
    model_gateway, runtime, telegram,
};
use serial_test::serial;
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
#[serial]
async fn continuity_runtime_retrieves_prior_canonical_memory_on_later_run() -> Result<()> {
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
                    "message": { "content": "assistant reply after preference capture" },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 20, "completion_tokens": 7 }
            }),
        }));
        transport.push_response(Ok(model_gateway::ProviderHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{
                    "message": { "content": "assistant reply after retrieval" },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 18, "completion_tokens": 6 }
            }),
        }));
        let mut delivery = telegram::FakeTelegramDelivery::default();

        runtime::run_telegram_fixture_with(
            &ctx.pool,
            &config,
            &sample_telegram_config(),
            &sample_model_gateway_config(),
            &telegram_fixture("private_preference_message.json"),
            &transport,
            &mut delivery,
        )
        .await?;
        runtime::run_telegram_fixture_with(
            &ctx.pool,
            &config,
            &sample_telegram_config(),
            &sample_model_gateway_config(),
            &telegram_fixture("private_preference_followup.json"),
            &transport,
            &mut delivery,
        )
        .await?;

        let seen_requests = transport.seen_requests();
        assert_eq!(seen_requests.len(), 2);
        let second_request_messages = seen_requests[1]
            .body
            .get("messages")
            .and_then(serde_json::Value::as_array)
            .expect("provider request should include messages")
            .iter()
            .filter_map(|message| message.get("content").and_then(serde_json::Value::as_str))
            .collect::<Vec<_>>();
        assert!(second_request_messages.iter().any(|content| {
            content.contains("Retrieved canonical context:")
                && content.contains("remember that I prefer concise replies and be direct")
        }));
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn continuity_runtime_persists_self_model_into_later_context() -> Result<()> {
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
                    "message": { "content": "assistant reply after preference capture" },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 20, "completion_tokens": 7 }
            }),
        }));
        transport.push_response(Ok(model_gateway::ProviderHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{
                    "message": { "content": "assistant reply after self-model carry-forward" },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 18, "completion_tokens": 6 }
            }),
        }));
        let mut delivery = telegram::FakeTelegramDelivery::default();

        runtime::run_telegram_fixture_with(
            &ctx.pool,
            &config,
            &sample_telegram_config(),
            &sample_model_gateway_config(),
            &telegram_fixture("private_preference_message.json"),
            &transport,
            &mut delivery,
        )
        .await?;
        runtime::run_telegram_fixture_with(
            &ctx.pool,
            &config,
            &sample_telegram_config(),
            &sample_model_gateway_config(),
            &telegram_fixture("private_preference_followup.json"),
            &transport,
            &mut delivery,
        )
        .await?;

        let seen_requests = transport.seen_requests();
        assert_eq!(seen_requests.len(), 2);
        let system_prompt = seen_requests[1]
            .body
            .get("messages")
            .and_then(serde_json::Value::as_array)
            .and_then(|messages| messages.first())
            .and_then(|message| message.get("content"))
            .and_then(serde_json::Value::as_str)
            .expect("provider request should include a system prompt message");
        assert!(system_prompt.contains("be direct"));
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn continuity_runtime_preserves_backlog_durability_under_single_reply_recovery() -> Result<()>
{
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
                    "message": { "content": "assistant reply from continuity backlog integration" },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 23, "completion_tokens": 9 }
            }),
        }));
        let mut delivery = telegram::FakeTelegramDelivery::default();

        let summary = runtime::run_telegram_fixture_with(
            &ctx.pool,
            &config,
            &sample_telegram_config(),
            &sample_model_gateway_config(),
            &telegram_fixture("private_text_backlog_batch.json"),
            &transport,
            &mut delivery,
        )
        .await?;

        assert_eq!(summary.backlog_recovery_count, 1);
        assert_eq!(delivery.sent_messages().len(), 1);
        assert_eq!(delivery.sent_messages()[0].reply_to_message_id, Some(53));

        let execution_row = sqlx::query(
            r#"
            SELECT execution_id
            FROM execution_records
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .fetch_one(&ctx.pool)
        .await?;
        let execution_id: Uuid = execution_row.get("execution_id");

        let audit_events = audit::list_for_execution(&ctx.pool, execution_id).await?;
        assert!(
            audit_events
                .iter()
                .any(|event| event.event_kind == "foreground_recovery_mode_decided")
        );
        let messages = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM ingress_events
            WHERE execution_id = $1
            "#,
        )
        .bind(execution_id)
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(messages, 3);
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
            provider_headers: Vec::new(),
            timeout_ms: 30_000,
        },
    }
}
