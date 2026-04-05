mod support;

use anyhow::Result;
use chrono::Utc;
use contracts::ChannelKind;
use harness::{
    config::ResolvedTelegramConfig, execution, foreground, ingress, migration, telegram,
};
use serial_test::serial;
use uuid::Uuid;

#[tokio::test]
#[serial]
async fn foreground_persistence_writes_bindings_and_ingress_events() -> Result<()> {
    let (_config, pool) = support::prepare_database().await?;
    migration::apply_pending_migrations(&pool, env!("CARGO_PKG_VERSION")).await?;

    let binding = foreground::upsert_conversation_binding(
        &pool,
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

    let update = telegram::load_fixture_updates(&telegram_fixture("private_text_message.json"))?
        .into_iter()
        .next()
        .expect("fixture should contain one update");
    let ingress = match ingress::normalize_telegram_update(
        &sample_telegram_config(),
        &update,
        Some("fixtures/private_text_message.json".to_string()),
    )? {
        ingress::TelegramNormalizationOutcome::Accepted(ingress) => ingress,
        other => panic!("fixture should normalize into accepted ingress, got {other:?}"),
    };

    foreground::insert_ingress_event(
        &pool,
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

    let stored = foreground::get_ingress_event(&pool, ingress.ingress_id).await?;
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
}

#[tokio::test]
#[serial]
async fn foreground_persistence_reads_recent_episode_history() -> Result<()> {
    let (_config, pool) = support::prepare_database().await?;
    migration::apply_pending_migrations(&pool, env!("CARGO_PKG_VERSION")).await?;

    let execution_id = Uuid::now_v7();
    let trace_id = Uuid::now_v7();
    execution::insert(
        &pool,
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
        &pool,
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
        &pool,
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
        &pool,
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

    foreground::mark_episode_completed(&pool, episode_id, "completed", "replied to user").await?;

    let stored = foreground::get_episode(&pool, episode_id).await?;
    assert_eq!(stored.execution_id, execution_id);
    assert_eq!(stored.status, "completed");
    assert_eq!(stored.outcome.as_deref(), Some("completed"));

    let messages = foreground::list_episode_messages(&pool, episode_id).await?;
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].message_role, "user");
    assert_eq!(messages[1].message_role, "assistant");

    let excerpts = foreground::list_recent_episode_excerpts(&pool, "telegram-primary", 5).await?;
    assert_eq!(excerpts.len(), 1);
    assert_eq!(excerpts[0].episode_id, episode_id);
    assert_eq!(excerpts[0].user_message.as_deref(), Some("hello"));
    assert_eq!(excerpts[0].assistant_message.as_deref(), Some("hi"));
    assert_eq!(excerpts[0].outcome, "completed");
    Ok(())
}

#[tokio::test]
#[serial]
async fn foreground_persistence_retains_attachment_command_and_callback_fields() -> Result<()> {
    let (_config, pool) = support::prepare_database().await?;
    migration::apply_pending_migrations(&pool, env!("CARGO_PKG_VERSION")).await?;

    let binding = foreground::upsert_conversation_binding(
        &pool,
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

    let command_update =
        telegram::load_fixture_updates(&telegram_fixture("private_command_with_document.json"))?
            .into_iter()
            .next()
            .expect("fixture should contain one update");
    let command_ingress = match ingress::normalize_telegram_update(
        &sample_telegram_config(),
        &command_update,
        Some("fixtures/private_command_with_document.json".to_string()),
    )? {
        ingress::TelegramNormalizationOutcome::Accepted(ingress) => ingress,
        other => panic!("command fixture should be accepted, got {other:?}"),
    };
    foreground::insert_ingress_event(
        &pool,
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
    let stored_command = foreground::get_ingress_event(&pool, command_ingress.ingress_id).await?;
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
        ingress::TelegramNormalizationOutcome::Accepted(ingress) => ingress,
        other => panic!("callback fixture should be accepted, got {other:?}"),
    };
    foreground::insert_ingress_event(
        &pool,
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
    let stored_callback = foreground::get_ingress_event(&pool, callback_ingress.ingress_id).await?;
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
