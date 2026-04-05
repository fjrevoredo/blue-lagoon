mod support;

use anyhow::Result;
use chrono::{Duration, Utc};
use contracts::{
    ChannelKind, LoopKind, ModelCallPurpose, ModelCallRequest, ModelInput, ModelInputMessage,
    ModelMessageRole, ModelOutputMode, ToolPolicy,
};
use harness::{
    audit,
    config::{
        ForegroundModelRouteConfig, ModelGatewayConfig, ResolvedForegroundModelRouteConfig,
        ResolvedModelGatewayConfig, ResolvedTelegramConfig, SelfModelConfig,
    },
    context, execution, foreground, foreground_orchestration, ingress, model_gateway, runtime,
    telegram, worker,
};
use serial_test::serial;
use sqlx::Row;
use std::ffi::OsString;
use uuid::Uuid;

#[tokio::test]
#[serial]
async fn foreground_persistence_writes_bindings_and_ingress_events() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let binding = foreground::upsert_conversation_binding(
            &ctx.pool,
            &foreground::NewConversationBinding {
                conversation_binding_id: Uuid::now_v7(),
                channel_kind: ChannelKind::Telegram,
                external_user_id: "telegram-user-42".to_string(),
                external_conversation_id: "telegram-chat-42".to_string(),
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
            },
        )
        .await?;

        let update =
            telegram::load_fixture_updates(&telegram_fixture("private_text_message.json"))?
                .into_iter()
                .next()
                .expect("fixture should contain one update");
        let ingress = match ingress::normalize_telegram_update(
            &sample_telegram_config(),
            &update,
            Some("fixtures/private_text_message.json".to_string()),
        )? {
            ingress::TelegramNormalizationOutcome::Accepted(ingress) => *ingress,
            other => panic!("fixture should normalize into accepted ingress, got {other:?}"),
        };

        foreground::insert_ingress_event(
            &ctx.pool,
            &foreground::NewIngressEvent {
                ingress: ingress.clone(),
                conversation_binding_id: Some(binding.conversation_binding_id),
                trace_id: Uuid::now_v7(),
                execution_id: None,
                status: "accepted".to_string(),
                rejection_reason: None,
            },
        )
        .await?;

        let stored = foreground::get_ingress_event(&ctx.pool, ingress.ingress_id).await?;
        assert_eq!(stored.ingress_id, ingress.ingress_id);
        assert_eq!(stored.channel_kind, "telegram");
        assert_eq!(
            stored.internal_principal_ref.as_deref(),
            Some("primary-user")
        );
        assert_eq!(
            stored.internal_conversation_ref.as_deref(),
            Some("telegram-primary")
        );
        assert_eq!(stored.event_kind, "message_created");
        assert_eq!(stored.external_event_id, ingress.external_event_id);
        assert_eq!(stored.external_message_id.as_deref(), Some("42"));
        assert_eq!(stored.status, "accepted");
        assert_eq!(stored.text_body.as_deref(), Some("hello from telegram"));
        assert_eq!(stored.reply_to_external_message_id.as_deref(), Some("41"));
        assert_eq!(stored.attachment_count, 0);
        assert!(stored.attachments.is_empty());
        assert_eq!(stored.command_name, None);
        assert!(stored.command_args.is_empty());
        assert_eq!(stored.approval_callback_data, None);
        assert_eq!(
            stored.raw_payload_ref.as_deref(),
            Some("fixtures/private_text_message.json")
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn conversation_binding_rebind_updates_canonical_internal_row() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let original = foreground::reconcile_conversation_binding(
            &ctx.pool,
            &foreground::NewConversationBinding {
                conversation_binding_id: Uuid::now_v7(),
                channel_kind: ChannelKind::Telegram,
                external_user_id: "42".to_string(),
                external_conversation_id: "42".to_string(),
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
            },
        )
        .await?;
        assert_eq!(
            original.action,
            foreground::ConversationBindingAction::Created
        );

        let rebound = foreground::reconcile_conversation_binding(
            &ctx.pool,
            &foreground::NewConversationBinding {
                conversation_binding_id: Uuid::now_v7(),
                channel_kind: ChannelKind::Telegram,
                external_user_id: "42".to_string(),
                external_conversation_id: "99".to_string(),
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
            },
        )
        .await?;

        assert_eq!(
            rebound.action,
            foreground::ConversationBindingAction::Rebound
        );
        assert_eq!(
            rebound.record.conversation_binding_id,
            original.record.conversation_binding_id
        );
        assert_eq!(rebound.record.external_conversation_id, "99");

        let binding_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM conversation_bindings")
            .fetch_one(&ctx.pool)
            .await?;
        assert_eq!(binding_count, 1);
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn conversation_binding_merge_keeps_internal_row_identity() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let internal = foreground::reconcile_conversation_binding(
            &ctx.pool,
            &foreground::NewConversationBinding {
                conversation_binding_id: Uuid::now_v7(),
                channel_kind: ChannelKind::Telegram,
                external_user_id: "42".to_string(),
                external_conversation_id: "42".to_string(),
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
            },
        )
        .await?;
        let external = foreground::reconcile_conversation_binding(
            &ctx.pool,
            &foreground::NewConversationBinding {
                conversation_binding_id: Uuid::now_v7(),
                channel_kind: ChannelKind::Telegram,
                external_user_id: "77".to_string(),
                external_conversation_id: "77".to_string(),
                internal_principal_ref: "other-user".to_string(),
                internal_conversation_ref: "telegram-secondary".to_string(),
            },
        )
        .await?;
        foreground::insert_ingress_event(
            &ctx.pool,
            &foreground::NewIngressEvent {
                ingress: contracts::NormalizedIngress {
                    ingress_id: Uuid::now_v7(),
                    channel_kind: ChannelKind::Telegram,
                    external_user_id: external.record.external_user_id.clone(),
                    external_conversation_id: external.record.external_conversation_id.clone(),
                    external_event_id: "external-binding-event".to_string(),
                    external_message_id: Some("9001".to_string()),
                    internal_principal_ref: external.record.internal_principal_ref.clone(),
                    internal_conversation_ref: external.record.internal_conversation_ref.clone(),
                    event_kind: contracts::IngressEventKind::MessageCreated,
                    occurred_at: Utc::now(),
                    text_body: Some("hello from superseded binding".to_string()),
                    reply_to: None,
                    attachments: Vec::new(),
                    command_hint: None,
                    approval_payload: None,
                    raw_payload_ref: Some("merge-test".to_string()),
                },
                conversation_binding_id: Some(external.record.conversation_binding_id),
                trace_id: Uuid::now_v7(),
                execution_id: None,
                status: "accepted".to_string(),
                rejection_reason: None,
            },
        )
        .await?;

        let merged = foreground::reconcile_conversation_binding(
            &ctx.pool,
            &foreground::NewConversationBinding {
                conversation_binding_id: Uuid::now_v7(),
                channel_kind: ChannelKind::Telegram,
                external_user_id: external.record.external_user_id.clone(),
                external_conversation_id: external.record.external_conversation_id.clone(),
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
            },
        )
        .await?;

        assert_eq!(merged.action, foreground::ConversationBindingAction::Merged);
        assert_eq!(
            merged.record.conversation_binding_id,
            internal.record.conversation_binding_id
        );
        assert_eq!(merged.record.external_user_id, "77");
        assert_eq!(merged.record.internal_conversation_ref, "telegram-primary");

        let reassigned_ingress_binding_id: Option<Uuid> = sqlx::query_scalar(
            r#"
            SELECT conversation_binding_id
            FROM ingress_events
            WHERE external_event_id = 'external-binding-event'
            "#,
        )
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(
            reassigned_ingress_binding_id,
            Some(internal.record.conversation_binding_id)
        );

        let binding_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM conversation_bindings")
            .fetch_one(&ctx.pool)
            .await?;
        assert_eq!(binding_count, 1);
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn foreground_persistence_reads_recent_episode_history() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let execution_id = Uuid::now_v7();
        let trace_id = Uuid::now_v7();
        execution::insert(
            &ctx.pool,
            &execution::NewExecutionRecord {
                execution_id,
                trace_id,
                trigger_kind: "telegram".to_string(),
                synthetic_trigger: None,
                status: "started".to_string(),
                request_payload: serde_json::json!({
                    "kind": "phase2_component"
                }),
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
                ingress_id: None,
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
                trigger_kind: "user_ingress".to_string(),
                trigger_source: "telegram".to_string(),
                status: "started".to_string(),
                started_at: Utc::now(),
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
                text_body: Some("hello".to_string()),
                external_message_id: Some("message-42".to_string()),
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
                message_order: 1,
                message_role: "assistant".to_string(),
                channel_kind: ChannelKind::Telegram,
                text_body: Some("hi".to_string()),
                external_message_id: None,
            },
        )
        .await?;

        foreground::mark_episode_completed(&ctx.pool, episode_id, "completed", "replied to user")
            .await?;

        let stored = foreground::get_episode(&ctx.pool, episode_id).await?;
        assert_eq!(stored.execution_id, execution_id);
        assert_eq!(stored.status, "completed");
        assert_eq!(stored.outcome.as_deref(), Some("completed"));

        let messages = foreground::list_episode_messages(&ctx.pool, episode_id).await?;
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].message_role, "user");
        assert_eq!(messages[1].message_role, "assistant");

        let excerpts =
            foreground::list_recent_episode_excerpts(&ctx.pool, "telegram-primary", 5).await?;
        assert_eq!(excerpts.len(), 1);
        assert_eq!(excerpts[0].episode_id, episode_id);
        assert_eq!(excerpts[0].user_message.as_deref(), Some("hello"));
        assert_eq!(excerpts[0].assistant_message.as_deref(), Some("hi"));
        assert_eq!(excerpts[0].outcome, "completed");
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn foreground_persistence_retains_attachment_command_and_callback_fields() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let binding = foreground::upsert_conversation_binding(
            &ctx.pool,
            &foreground::NewConversationBinding {
                conversation_binding_id: Uuid::now_v7(),
                channel_kind: ChannelKind::Telegram,
                external_user_id: "42".to_string(),
                external_conversation_id: "42".to_string(),
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
            },
        )
        .await?;

        let command_update = telegram::load_fixture_updates(&telegram_fixture(
            "private_command_with_document.json",
        ))?
        .into_iter()
        .next()
        .expect("fixture should contain one update");
        let command_ingress = match ingress::normalize_telegram_update(
            &sample_telegram_config(),
            &command_update,
            Some("fixtures/private_command_with_document.json".to_string()),
        )? {
            ingress::TelegramNormalizationOutcome::Accepted(ingress) => *ingress,
            other => panic!("command fixture should be accepted, got {other:?}"),
        };
        foreground::insert_ingress_event(
            &ctx.pool,
            &foreground::NewIngressEvent {
                ingress: command_ingress.clone(),
                conversation_binding_id: Some(binding.conversation_binding_id),
                trace_id: Uuid::now_v7(),
                execution_id: None,
                status: "accepted".to_string(),
                rejection_reason: None,
            },
        )
        .await?;
        let stored_command =
            foreground::get_ingress_event(&ctx.pool, command_ingress.ingress_id).await?;
        assert_eq!(stored_command.command_name.as_deref(), Some("start"));
        assert_eq!(stored_command.command_args, vec!["now".to_string()]);
        assert_eq!(stored_command.attachment_count, 1);
        assert_eq!(stored_command.attachments.len(), 1);
        assert_eq!(stored_command.attachments[0].attachment_id, "doc-1");
        assert_eq!(
            stored_command.attachments[0].file_name.as_deref(),
            Some("note.txt")
        );

        let callback_update =
            telegram::load_fixture_updates(&telegram_fixture("approval_callback.json"))?
                .into_iter()
                .next()
                .expect("fixture should contain one update");
        let callback_ingress = match ingress::normalize_telegram_update(
            &sample_telegram_config(),
            &callback_update,
            Some("fixtures/approval_callback.json".to_string()),
        )? {
            ingress::TelegramNormalizationOutcome::Accepted(ingress) => *ingress,
            other => panic!("callback fixture should be accepted, got {other:?}"),
        };
        foreground::insert_ingress_event(
            &ctx.pool,
            &foreground::NewIngressEvent {
                ingress: callback_ingress.clone(),
                conversation_binding_id: Some(binding.conversation_binding_id),
                trace_id: Uuid::now_v7(),
                execution_id: None,
                status: "accepted".to_string(),
                rejection_reason: None,
            },
        )
        .await?;
        let stored_callback =
            foreground::get_ingress_event(&ctx.pool, callback_ingress.ingress_id).await?;
        assert_eq!(stored_callback.event_kind, "approval_callback");
        assert_eq!(
            stored_callback.approval_token.as_deref(),
            Some("callback-123")
        );
        assert_eq!(
            stored_callback.approval_callback_data.as_deref(),
            Some("approve:42")
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn accepted_foreground_trigger_persists_execution_budget_and_audit() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let update =
            telegram::load_fixture_updates(&telegram_fixture("private_text_message.json"))?
                .into_iter()
                .next()
                .expect("fixture should contain one update");
        let ingress = match ingress::normalize_telegram_update(
            &sample_telegram_config(),
            &update,
            Some("fixtures/private_text_message.json".to_string()),
        )? {
            ingress::TelegramNormalizationOutcome::Accepted(ingress) => *ingress,
            other => panic!("fixture should normalize into accepted ingress, got {other:?}"),
        };

        let outcome = foreground::intake_telegram_foreground_trigger(
            &ctx.pool,
            &ctx.config,
            &sample_telegram_config(),
            ingress.clone(),
        )
        .await?;

        let trigger = match outcome {
            foreground::ForegroundTriggerIntakeOutcome::Accepted(trigger) => *trigger,
            other => panic!("expected accepted trigger, got {other:?}"),
        };

        assert_eq!(trigger.ingress.ingress_id, ingress.ingress_id);
        assert_eq!(trigger.budget.iteration_budget, 1);
        assert_eq!(trigger.budget.wall_clock_budget_ms, 30_000);
        assert_eq!(trigger.budget.token_budget, 4_000);

        let stored_ingress = foreground::get_ingress_event(&ctx.pool, ingress.ingress_id).await?;
        assert_eq!(stored_ingress.status, "accepted");
        assert_eq!(stored_ingress.execution_id, Some(trigger.execution_id));
        assert_eq!(stored_ingress.rejection_reason, None);

        let execution = execution::get(&ctx.pool, trigger.execution_id).await?;
        assert_eq!(execution.trace_id, trigger.trace_id);
        assert_eq!(execution.status, "started");

        let audit_events = audit::list_for_execution(&ctx.pool, trigger.execution_id).await?;
        assert_eq!(audit_events.len(), 1);
        assert_eq!(audit_events[0].event_kind, "foreground_trigger_accepted");
        assert_eq!(audit_events[0].trace_id, trigger.trace_id);
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn rejected_foreground_trigger_persists_rejection_and_audit() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let update = telegram::load_fixture_updates(&telegram_fixture("approval_callback.json"))?
            .into_iter()
            .next()
            .expect("fixture should contain one update");
        let ingress = match ingress::normalize_telegram_update(
            &sample_telegram_config(),
            &update,
            Some("fixtures/approval_callback.json".to_string()),
        )? {
            ingress::TelegramNormalizationOutcome::Accepted(ingress) => *ingress,
            other => {
                panic!("callback fixture should normalize into accepted ingress, got {other:?}")
            }
        };

        let outcome = foreground::intake_telegram_foreground_trigger(
            &ctx.pool,
            &ctx.config,
            &sample_telegram_config(),
            ingress.clone(),
        )
        .await?;

        let rejected = match outcome {
            foreground::ForegroundTriggerIntakeOutcome::Rejected(rejected) => rejected,
            other => panic!("expected rejected trigger, got {other:?}"),
        };

        let stored_ingress = foreground::get_ingress_event(&ctx.pool, rejected.ingress_id).await?;
        assert_eq!(stored_ingress.status, "rejected");
        assert!(
            stored_ingress
                .rejection_reason
                .as_deref()
                .is_some_and(|reason| reason.contains("approval callbacks"))
        );
        assert_eq!(stored_ingress.execution_id, None);

        let audit_events = audit::list_for_trace(&ctx.pool, rejected.trace_id).await?;
        assert_eq!(audit_events.len(), 1);
        assert_eq!(audit_events[0].event_kind, "foreground_trigger_rejected");
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn duplicate_foreground_trigger_is_idempotent_and_audited() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let update =
            telegram::load_fixture_updates(&telegram_fixture("private_text_message.json"))?
                .into_iter()
                .next()
                .expect("fixture should contain one update");
        let ingress = match ingress::normalize_telegram_update(
            &sample_telegram_config(),
            &update,
            Some("fixtures/private_text_message.json".to_string()),
        )? {
            ingress::TelegramNormalizationOutcome::Accepted(ingress) => *ingress,
            other => panic!("fixture should normalize into accepted ingress, got {other:?}"),
        };

        let accepted = foreground::intake_telegram_foreground_trigger(
            &ctx.pool,
            &ctx.config,
            &sample_telegram_config(),
            ingress.clone(),
        )
        .await?;

        let accepted_trigger = match accepted {
            foreground::ForegroundTriggerIntakeOutcome::Accepted(trigger) => *trigger,
            other => panic!("expected accepted trigger, got {other:?}"),
        };

        let mut duplicate_ingress = ingress.clone();
        duplicate_ingress.ingress_id = Uuid::now_v7();

        let duplicate = foreground::intake_telegram_foreground_trigger(
            &ctx.pool,
            &ctx.config,
            &sample_telegram_config(),
            duplicate_ingress,
        )
        .await?;

        let duplicate = match duplicate {
            foreground::ForegroundTriggerIntakeOutcome::Duplicate(duplicate) => duplicate,
            other => panic!("expected duplicate trigger, got {other:?}"),
        };

        assert_eq!(duplicate.ingress_id, accepted_trigger.ingress.ingress_id);
        assert_eq!(duplicate.execution_id, Some(accepted_trigger.execution_id));
        assert_eq!(duplicate.trace_id, accepted_trigger.trace_id);

        let ingress_count = sqlx::query(
            r#"
            SELECT COUNT(*) AS count
            FROM ingress_events
            WHERE channel_kind = 'telegram' AND external_event_id = $1
            "#,
        )
        .bind(&ingress.external_event_id)
        .fetch_one(&ctx.pool)
        .await?
        .get::<i64, _>("count");
        assert_eq!(ingress_count, 1);

        let audit_events =
            audit::list_for_execution(&ctx.pool, accepted_trigger.execution_id).await?;
        assert_eq!(audit_events.len(), 2);
        assert_eq!(audit_events[0].event_kind, "foreground_trigger_accepted");
        assert_eq!(audit_events[1].event_kind, "foreground_trigger_duplicate");
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn context_assembly_v0_loads_seed_and_bounded_recent_history() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        config.self_model = Some(SelfModelConfig {
            seed_path: support::workspace_root()
                .join("config")
                .join("self_model_seed.toml"),
        });

        let update =
            telegram::load_fixture_updates(&telegram_fixture("private_text_message.json"))?
                .into_iter()
                .next()
                .expect("fixture should contain one update");
        let mut ingress = match ingress::normalize_telegram_update(
            &sample_telegram_config(),
            &update,
            Some("fixtures/private_text_message.json".to_string()),
        )? {
            ingress::TelegramNormalizationOutcome::Accepted(ingress) => *ingress,
            other => panic!("fixture should normalize into accepted ingress, got {other:?}"),
        };
        ingress.text_body = Some("trigger text that should be truncated".to_string());

        let trigger = match foreground::intake_telegram_foreground_trigger(
            &ctx.pool,
            &config,
            &sample_telegram_config(),
            ingress,
        )
        .await?
        {
            foreground::ForegroundTriggerIntakeOutcome::Accepted(trigger) => *trigger,
            other => panic!("expected accepted trigger, got {other:?}"),
        };

        insert_completed_episode(
            &ctx.pool,
            "episode-older-1",
            trigger.received_at - Duration::minutes(3),
            "older user message that is intentionally long",
            "older assistant message that is intentionally long",
        )
        .await?;
        insert_completed_episode(
            &ctx.pool,
            "episode-older-2",
            trigger.received_at - Duration::minutes(2),
            "second older user message that is intentionally long",
            "second older assistant message that is intentionally long",
        )
        .await?;
        insert_completed_episode(
            &ctx.pool,
            "episode-future",
            trigger.received_at + Duration::minutes(1),
            "future user message",
            "future assistant message",
        )
        .await?;

        let assembled = context::assemble_foreground_context(
            &ctx.pool,
            &config,
            trigger,
            context::ContextAssemblyOptions {
                limits: context::ContextAssemblyLimits {
                    recent_history_limit: 2,
                    trigger_text_char_limit: 12,
                    history_message_char_limit: 10,
                },
                internal_state_seed: harness::self_model::InternalStateSeed {
                    load_pct: 22,
                    health_pct: 97,
                    reliability_pct: 95,
                    resource_pressure_pct: 15,
                    confidence_pct: 78,
                    connection_quality_pct: 92,
                },
                active_conditions: vec!["postgres_ready".to_string()],
            },
        )
        .await?;

        assert_eq!(assembled.context.self_model.stable_identity, "blue-lagoon");
        assert_eq!(
            assembled.context.trigger.ingress.text_body.as_deref(),
            Some("trigger text")
        );
        assert_eq!(assembled.context.internal_state.load_pct, 22);
        assert_eq!(
            assembled.context.internal_state.active_conditions,
            vec!["postgres_ready".to_string()]
        );
        assert_eq!(assembled.context.recent_history.len(), 2);
        assert_eq!(
            assembled.context.recent_history[0].user_message.as_deref(),
            Some("second old")
        );
        assert_eq!(
            assembled.context.recent_history[1].user_message.as_deref(),
            Some("older user")
        );
        assert!(
            assembled
                .metadata
                .self_model_seed_path
                .contains("self_model_seed.toml")
        );
        assert!(assembled.metadata.trigger_text_truncated);
        assert_eq!(assembled.metadata.selected_recent_history_count, 2);
        assert_eq!(assembled.metadata.truncated_history_message_count, 4);
        Ok(())
    })
    .await
}

#[tokio::test]
async fn model_gateway_executes_foreground_request_with_fake_provider() -> Result<()> {
    let gateway = sample_model_gateway_config();
    let request = sample_model_call_request();
    let transport = model_gateway::FakeModelProviderTransport::new();
    transport.push_response(Ok(model_gateway::ProviderHttpResponse {
        status: 200,
        body: serde_json::json!({
            "choices": [{
                "message": { "content": "hello from fake provider" },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 11,
                "completion_tokens": 4
            }
        }),
    }));

    let response = model_gateway::execute_foreground_model_call(&gateway, &request, &transport)
        .await
        .expect("gateway call should succeed");

    assert_eq!(response.request_id, request.request_id);
    assert_eq!(response.provider, contracts::ModelProviderKind::ZAi);
    assert_eq!(response.model, "z-ai-foreground");
    assert_eq!(response.output.text, "hello from fake provider");

    let seen = transport.seen_requests();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].url, "https://api.z.ai/api/paas/v4/chat/completions");
    assert_eq!(
        seen[0]
            .body
            .get("messages")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(3)
    );
    Ok(())
}

#[tokio::test]
async fn conscious_worker_path_runs_one_harness_mediated_model_cycle() -> Result<()> {
    support::with_clean_database(|ctx| async move {
        let mut config = ctx.config.clone();
        let worker_binary = support::workers_binary()?;
        config.worker.command = worker_binary.to_string_lossy().into_owned();
        config.worker.args = vec!["conscious-worker".to_string()];

        let gateway = sample_model_gateway_config();
        let transport = model_gateway::FakeModelProviderTransport::new();
        transport.push_response(Ok(model_gateway::ProviderHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{
                    "message": { "content": "assistant reply from fake provider" },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 13,
                    "completion_tokens": 6
                }
            }),
        }));

        let request = contracts::WorkerRequest::conscious(
            Uuid::now_v7(),
            Uuid::now_v7(),
            sample_conscious_context(),
        );

        let response =
            worker::launch_conscious_worker(&config, &gateway, &request, &transport).await?;
        match response.result {
            contracts::WorkerResult::Conscious(result) => {
                assert_eq!(result.status, contracts::ConsciousWorkerStatus::Completed);
                assert_eq!(
                    result.assistant_output.text,
                    "assistant reply from fake provider"
                );
                assert_eq!(
                    result.assistant_output.internal_conversation_ref,
                    "telegram-primary"
                );
                assert_eq!(result.episode_summary.outcome, "completed");
            }
            contracts::WorkerResult::Smoke(_) => panic!("expected conscious worker result"),
            contracts::WorkerResult::Error(error) => {
                panic!("unexpected worker error: {}", error.message)
            }
        }

        let seen = transport.seen_requests();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].url, "https://api.z.ai/api/paas/v4/chat/completions");
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn foreground_orchestration_runs_from_ingress_to_delivery() -> Result<()> {
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

        let update =
            telegram::load_fixture_updates(&telegram_fixture("private_text_message.json"))?
                .into_iter()
                .next()
                .expect("fixture should contain one update");
        let ingress = match ingress::normalize_telegram_update(
            &sample_telegram_config(),
            &update,
            Some("fixtures/private_text_message.json".to_string()),
        )? {
            ingress::TelegramNormalizationOutcome::Accepted(ingress) => *ingress,
            other => panic!("fixture should normalize into accepted ingress, got {other:?}"),
        };

        let gateway = sample_model_gateway_config();
        let transport = model_gateway::FakeModelProviderTransport::new();
        transport.push_response(Ok(model_gateway::ProviderHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{
                    "message": { "content": "assistant reply from fake provider" },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 13,
                    "completion_tokens": 6
                }
            }),
        }));
        let mut delivery = telegram::FakeTelegramDelivery::default();

        let outcome = foreground_orchestration::orchestrate_telegram_foreground_ingress(
            &ctx.pool,
            &config,
            &sample_telegram_config(),
            &gateway,
            ingress,
            &transport,
            &mut delivery,
        )
        .await?;

        let completed = match outcome {
            foreground_orchestration::TelegramForegroundOrchestrationOutcome::Completed(
                completed,
            ) => completed,
            other => panic!("expected completed orchestration, got {other:?}"),
        };

        let execution = execution::get(&ctx.pool, completed.execution_id).await?;
        assert_eq!(execution.status, "completed");
        assert_eq!(execution.trace_id, completed.trace_id);

        let episode = foreground::get_episode(&ctx.pool, completed.episode_id).await?;
        assert_eq!(episode.status, "completed");
        assert_eq!(episode.execution_id, completed.execution_id);
        assert_eq!(episode.outcome.as_deref(), Some("completed"));

        let messages = foreground::list_episode_messages(&ctx.pool, completed.episode_id).await?;
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].message_role, "user");
        assert_eq!(messages[1].message_role, "assistant");
        assert_eq!(
            messages[1].text_body.as_deref(),
            Some("assistant reply from fake provider")
        );
        assert_eq!(
            messages[1].external_message_id.as_deref(),
            Some(completed.outbound_message_id.to_string().as_str())
        );

        assert_eq!(delivery.sent_messages().len(), 1);
        assert_eq!(delivery.sent_messages()[0].chat_id, 42);
        assert_eq!(delivery.sent_messages()[0].reply_to_message_id, Some(42));
        assert_eq!(
            delivery.sent_messages()[0].text,
            "assistant reply from fake provider"
        );

        let audit_events = audit::list_for_execution(&ctx.pool, completed.execution_id).await?;
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
async fn foreground_orchestration_marks_execution_failed_when_context_assembly_fails() -> Result<()>
{
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        let worker_binary = support::workers_binary()?;
        config.worker.command = worker_binary.to_string_lossy().into_owned();
        config.worker.args = vec!["conscious-worker".to_string()];

        let update =
            telegram::load_fixture_updates(&telegram_fixture("private_text_message.json"))?
                .into_iter()
                .next()
                .expect("fixture should contain one update");
        let ingress = match ingress::normalize_telegram_update(
            &sample_telegram_config(),
            &update,
            Some("fixtures/private_text_message.json".to_string()),
        )? {
            ingress::TelegramNormalizationOutcome::Accepted(ingress) => *ingress,
            other => panic!("fixture should normalize into accepted ingress, got {other:?}"),
        };

        let transport = model_gateway::FakeModelProviderTransport::new();
        let mut delivery = telegram::FakeTelegramDelivery::default();
        let error = foreground_orchestration::orchestrate_telegram_foreground_ingress(
            &ctx.pool,
            &config,
            &sample_telegram_config(),
            &sample_model_gateway_config(),
            ingress,
            &transport,
            &mut delivery,
        )
        .await
        .expect_err("missing self-model config should fail orchestration");
        assert!(
            error
                .to_string()
                .contains("missing Phase 2 self-model seed configuration")
        );

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
        let execution = execution::get(&ctx.pool, execution_id).await?;
        assert_eq!(execution.status, "failed");

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
        let episode = foreground::get_episode(&ctx.pool, episode_id).await?;
        assert_eq!(episode.status, "failed");
        assert!(
            episode
                .summary
                .as_deref()
                .is_some_and(|summary| summary.contains("self-model"))
        );

        let audit_events = audit::list_for_execution(&ctx.pool, execution_id).await?;
        assert!(
            audit_events
                .iter()
                .any(|event| event.event_kind == "foreground_execution_failed")
        );
        assert!(transport.seen_requests().is_empty());
        assert!(delivery.sent_messages().is_empty());
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn runtime_fixture_entrypoint_processes_telegram_fixture_once() -> Result<()> {
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
                    "message": { "content": "assistant reply from fixture runtime path" },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 11,
                    "completion_tokens": 7
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
        assert_eq!(summary.duplicate_count, 0);
        assert_eq!(summary.trigger_rejected_count, 0);
        assert_eq!(summary.normalization_rejected_count, 0);
        assert_eq!(summary.ignored_count, 0);

        assert_eq!(delivery.sent_messages().len(), 1);
        assert_eq!(
            delivery.sent_messages()[0].text,
            "assistant reply from fixture runtime path"
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn runtime_fixture_rejected_normalization_is_audited() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let transport = model_gateway::FakeModelProviderTransport::new();
        let mut delivery = telegram::FakeTelegramDelivery::default();
        let summary = runtime::run_telegram_fixture_with(
            &ctx.pool,
            &ctx.config,
            &sample_telegram_config(),
            &sample_model_gateway_config(),
            &telegram_fixture("rejected_group_message.json"),
            &transport,
            &mut delivery,
        )
        .await?;

        assert_eq!(summary.fetched_updates, 1);
        assert_eq!(summary.normalization_rejected_count, 1);
        assert_eq!(summary.completed_count, 0);
        assert_eq!(delivery.sent_messages().len(), 0);
        assert!(transport.seen_requests().is_empty());

        let audit_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM audit_events
            WHERE event_kind = 'telegram_ingress_normalization_rejected'
            "#,
        )
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(audit_count, 1);
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn runtime_fixture_ignored_update_is_audited() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let transport = model_gateway::FakeModelProviderTransport::new();
        let mut delivery = telegram::FakeTelegramDelivery::default();
        let summary = runtime::run_telegram_fixture_with(
            &ctx.pool,
            &ctx.config,
            &sample_telegram_config(),
            &sample_model_gateway_config(),
            &telegram_fixture("unsupported_update.json"),
            &transport,
            &mut delivery,
        )
        .await?;

        assert_eq!(summary.fetched_updates, 1);
        assert_eq!(summary.ignored_count, 1);
        assert_eq!(summary.completed_count, 0);
        assert_eq!(delivery.sent_messages().len(), 0);
        assert!(transport.seen_requests().is_empty());

        let audit_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM audit_events
            WHERE event_kind = 'telegram_ingress_ignored'
            "#,
        )
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(audit_count, 1);
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn runtime_poll_once_fails_closed_when_telegram_config_is_absent() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let error = runtime::run_telegram_once(
            &ctx.config,
            runtime::TelegramOptions {
                fixture_path: None,
                poll_once: true,
            },
        )
        .await
        .expect_err("missing telegram config should fail closed");

        assert!(
            error
                .to_string()
                .contains("missing Phase 2 Telegram configuration")
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn runtime_poll_once_audits_live_telegram_fetch_failures() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        let bot_token_env = format!("BLUE_LAGOON_TEST_TELEGRAM_TOKEN_{}", Uuid::now_v7());
        let _bot_token_guard = ScopedEnvVar::set(&bot_token_env, "test-token");
        let _foreground_api_key_guard =
            ScopedEnvVar::set("BLUE_LAGOON_FOREGROUND_API_KEY", "test-api-key");

        config.telegram = Some(harness::config::TelegramConfig {
            api_base_url: "http://127.0.0.1:1".to_string(),
            bot_token_env: bot_token_env.clone(),
            allowed_user_id: 42,
            allowed_chat_id: 42,
            internal_principal_ref: "primary-user".to_string(),
            internal_conversation_ref: "telegram-primary".to_string(),
            poll_limit: 10,
        });
        config.model_gateway = Some(ModelGatewayConfig {
            foreground: ForegroundModelRouteConfig {
                provider: contracts::ModelProviderKind::ZAi,
                model: "glm-test".to_string(),
                api_base_url: None,
                api_key_env: "BLUE_LAGOON_FOREGROUND_API_KEY".to_string(),
                timeout_ms: 30_000,
            },
            z_ai: Some(harness::config::ZAiProviderConfig {
                api_surface: Some(harness::config::ZAiApiSurface::Coding),
                api_base_url: None,
            }),
        });

        let error = runtime::run_telegram_once(
            &config,
            runtime::TelegramOptions {
                fixture_path: None,
                poll_once: true,
            },
        )
        .await
        .expect_err("live telegram fetch failure should be returned");
        assert!(
            error
                .to_string()
                .contains("failed to call Telegram getUpdates")
        );

        let audit_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM audit_events
            WHERE event_kind = 'telegram_fetch_failed'
            "#,
        )
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(audit_count, 1);
        Ok(())
    })
    .await
}

struct ScopedEnvVar {
    key: String,
    original_value: Option<OsString>,
}

impl ScopedEnvVar {
    fn set(key: impl Into<String>, value: &str) -> Self {
        let key = key.into();
        let original_value = std::env::var_os(&key);
        unsafe {
            std::env::set_var(&key, value);
        }
        Self {
            key,
            original_value,
        }
    }
}

impl Drop for ScopedEnvVar {
    fn drop(&mut self) {
        match &self.original_value {
            Some(value) => unsafe { std::env::set_var(&self.key, value) },
            None => unsafe { std::env::remove_var(&self.key) },
        }
    }
}

fn sample_telegram_config() -> ResolvedTelegramConfig {
    ResolvedTelegramConfig {
        api_base_url: "https://api.telegram.org".to_string(),
        bot_token: "secret".to_string(),
        allowed_user_id: 42,
        allowed_chat_id: 42,
        internal_principal_ref: "primary-user".to_string(),
        internal_conversation_ref: "telegram-primary".to_string(),
        poll_limit: 10,
    }
}

fn telegram_fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("telegram")
        .join(name)
}

fn sample_model_gateway_config() -> ResolvedModelGatewayConfig {
    ResolvedModelGatewayConfig {
        foreground: ResolvedForegroundModelRouteConfig {
            provider: contracts::ModelProviderKind::ZAi,
            model: "z-ai-foreground".to_string(),
            api_base_url: "https://api.z.ai/api/paas/v4".to_string(),
            api_key: "secret".to_string(),
            timeout_ms: 20_000,
        },
    }
}

fn sample_model_call_request() -> ModelCallRequest {
    ModelCallRequest {
        request_id: Uuid::now_v7(),
        trace_id: Uuid::now_v7(),
        execution_id: Uuid::now_v7(),
        loop_kind: LoopKind::Conscious,
        purpose: ModelCallPurpose::ForegroundResponse,
        task_class: "telegram_foreground_reply".to_string(),
        budget: contracts::ModelBudget {
            max_input_tokens: 2_000,
            max_output_tokens: 500,
            timeout_ms: 30_000,
        },
        input: ModelInput {
            system_prompt: "You are Blue Lagoon.".to_string(),
            messages: vec![
                ModelInputMessage {
                    role: ModelMessageRole::Developer,
                    content: "Stay concise.".to_string(),
                },
                ModelInputMessage {
                    role: ModelMessageRole::User,
                    content: "hello".to_string(),
                },
            ],
        },
        output_mode: ModelOutputMode::PlainText,
        schema_name: None,
        schema_json: None,
        tool_policy: ToolPolicy::NoTools,
        provider_hint: None,
    }
}

fn sample_conscious_context() -> contracts::ConsciousContext {
    contracts::ConsciousContext {
        context_id: Uuid::now_v7(),
        assembled_at: Utc::now(),
        trigger: contracts::ForegroundTrigger {
            trigger_id: Uuid::now_v7(),
            trace_id: Uuid::now_v7(),
            execution_id: Uuid::now_v7(),
            trigger_kind: contracts::ForegroundTriggerKind::UserIngress,
            ingress: contracts::NormalizedIngress {
                ingress_id: Uuid::now_v7(),
                channel_kind: ChannelKind::Telegram,
                external_user_id: "42".to_string(),
                external_conversation_id: "42".to_string(),
                external_event_id: "update-42".to_string(),
                external_message_id: Some("message-42".to_string()),
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
                event_kind: contracts::IngressEventKind::MessageCreated,
                occurred_at: Utc::now(),
                text_body: Some("hello from trigger".to_string()),
                reply_to: None,
                attachments: Vec::new(),
                command_hint: None,
                approval_payload: None,
                raw_payload_ref: None,
            },
            received_at: Utc::now(),
            deduplication_key: "telegram:update-42".to_string(),
            budget: contracts::ForegroundBudget {
                iteration_budget: 1,
                wall_clock_budget_ms: 30_000,
                token_budget: 4_000,
            },
        },
        self_model: contracts::SelfModelSnapshot {
            stable_identity: "blue-lagoon".to_string(),
            role: "personal_assistant".to_string(),
            communication_style: "direct".to_string(),
            capabilities: vec!["conversation".to_string()],
            constraints: vec!["respect_harness_policy".to_string()],
            preferences: vec!["concise".to_string()],
            current_goals: vec!["support_the_user".to_string()],
            current_subgoals: vec!["reply_to_current_message".to_string()],
        },
        internal_state: contracts::InternalStateSnapshot {
            load_pct: 15,
            health_pct: 100,
            reliability_pct: 100,
            resource_pressure_pct: 10,
            confidence_pct: 80,
            connection_quality_pct: 95,
            active_conditions: Vec::new(),
        },
        recent_history: vec![contracts::EpisodeExcerpt {
            episode_id: Uuid::now_v7(),
            trace_id: Uuid::now_v7(),
            started_at: Utc::now(),
            user_message: Some("older user".to_string()),
            assistant_message: Some("older assistant".to_string()),
            outcome: "completed".to_string(),
        }],
    }
}

async fn insert_completed_episode(
    pool: &sqlx::PgPool,
    suffix: &str,
    started_at: chrono::DateTime<Utc>,
    user_message: &str,
    assistant_message: &str,
) -> Result<()> {
    let execution_id = Uuid::now_v7();
    let trace_id = Uuid::now_v7();
    execution::insert(
        pool,
        &execution::NewExecutionRecord {
            execution_id,
            trace_id,
            trigger_kind: format!("phase2-context-{suffix}"),
            synthetic_trigger: None,
            status: "started".to_string(),
            request_payload: serde_json::json!({ "kind": "context_assembly_test", "suffix": suffix }),
        },
    )
    .await?;

    let episode_id = Uuid::now_v7();
    foreground::insert_episode(
        pool,
        &foreground::NewEpisode {
            episode_id,
            trace_id,
            execution_id,
            ingress_id: None,
            internal_principal_ref: "primary-user".to_string(),
            internal_conversation_ref: "telegram-primary".to_string(),
            trigger_kind: "user_ingress".to_string(),
            trigger_source: "telegram".to_string(),
            status: "started".to_string(),
            started_at,
        },
    )
    .await?;

    foreground::insert_episode_message(
        pool,
        &foreground::NewEpisodeMessage {
            episode_message_id: Uuid::now_v7(),
            episode_id,
            trace_id,
            execution_id,
            message_order: 0,
            message_role: "user".to_string(),
            channel_kind: ChannelKind::Telegram,
            text_body: Some(user_message.to_string()),
            external_message_id: None,
        },
    )
    .await?;

    foreground::insert_episode_message(
        pool,
        &foreground::NewEpisodeMessage {
            episode_message_id: Uuid::now_v7(),
            episode_id,
            trace_id,
            execution_id,
            message_order: 1,
            message_role: "assistant".to_string(),
            channel_kind: ChannelKind::Telegram,
            text_body: Some(assistant_message.to_string()),
            external_message_id: None,
        },
    )
    .await?;

    foreground::mark_episode_completed(pool, episode_id, "completed", "context test").await?;
    Ok(())
}
