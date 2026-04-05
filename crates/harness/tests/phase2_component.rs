mod support;

use anyhow::Result;
use chrono::Utc;
use contracts::ChannelKind;
use harness::{
    audit, config::ResolvedTelegramConfig, execution, foreground, ingress, migration, telegram,
};
use serial_test::serial;
use sqlx::Row;
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

#[tokio::test]
#[serial]
async fn accepted_foreground_trigger_persists_execution_budget_and_audit() -> Result<()> {
    let (config, pool) = support::prepare_database().await?;
    migration::apply_pending_migrations(&pool, env!("CARGO_PKG_VERSION")).await?;

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

    let outcome = foreground::intake_telegram_foreground_trigger(
        &pool,
        &config,
        &sample_telegram_config(),
        ingress.clone(),
    )
    .await?;

    let trigger = match outcome {
        foreground::ForegroundTriggerIntakeOutcome::Accepted(trigger) => trigger,
        other => panic!("expected accepted trigger, got {other:?}"),
    };

    assert_eq!(trigger.ingress.ingress_id, ingress.ingress_id);
    assert_eq!(trigger.budget.iteration_budget, 1);
    assert_eq!(trigger.budget.wall_clock_budget_ms, 30_000);
    assert_eq!(trigger.budget.token_budget, 4_000);

    let stored_ingress = foreground::get_ingress_event(&pool, ingress.ingress_id).await?;
    assert_eq!(stored_ingress.status, "accepted");
    assert_eq!(stored_ingress.execution_id, Some(trigger.execution_id));
    assert_eq!(stored_ingress.rejection_reason, None);

    let execution = execution::get(&pool, trigger.execution_id).await?;
    assert_eq!(execution.trace_id, trigger.trace_id);
    assert_eq!(execution.status, "started");

    let audit_events = audit::list_for_execution(&pool, trigger.execution_id).await?;
    assert_eq!(audit_events.len(), 1);
    assert_eq!(audit_events[0].event_kind, "foreground_trigger_accepted");
    assert_eq!(audit_events[0].trace_id, trigger.trace_id);
    Ok(())
}

#[tokio::test]
#[serial]
async fn rejected_foreground_trigger_persists_rejection_and_audit() -> Result<()> {
    let (config, pool) = support::prepare_database().await?;
    migration::apply_pending_migrations(&pool, env!("CARGO_PKG_VERSION")).await?;

    let update = telegram::load_fixture_updates(&telegram_fixture("approval_callback.json"))?
        .into_iter()
        .next()
        .expect("fixture should contain one update");
    let ingress = match ingress::normalize_telegram_update(
        &sample_telegram_config(),
        &update,
        Some("fixtures/approval_callback.json".to_string()),
    )? {
        ingress::TelegramNormalizationOutcome::Accepted(ingress) => ingress,
        other => panic!("callback fixture should normalize into accepted ingress, got {other:?}"),
    };

    let outcome = foreground::intake_telegram_foreground_trigger(
        &pool,
        &config,
        &sample_telegram_config(),
        ingress.clone(),
    )
    .await?;

    let rejected = match outcome {
        foreground::ForegroundTriggerIntakeOutcome::Rejected(rejected) => rejected,
        other => panic!("expected rejected trigger, got {other:?}"),
    };

    let stored_ingress = foreground::get_ingress_event(&pool, rejected.ingress_id).await?;
    assert_eq!(stored_ingress.status, "rejected");
    assert!(
        stored_ingress
            .rejection_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("approval callbacks"))
    );
    assert_eq!(stored_ingress.execution_id, None);

    let audit_events = audit::list_for_trace(&pool, rejected.trace_id).await?;
    assert_eq!(audit_events.len(), 1);
    assert_eq!(audit_events[0].event_kind, "foreground_trigger_rejected");
    Ok(())
}

#[tokio::test]
#[serial]
async fn duplicate_foreground_trigger_is_idempotent_and_audited() -> Result<()> {
    let (config, pool) = support::prepare_database().await?;
    migration::apply_pending_migrations(&pool, env!("CARGO_PKG_VERSION")).await?;

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

    let accepted = foreground::intake_telegram_foreground_trigger(
        &pool,
        &config,
        &sample_telegram_config(),
        ingress.clone(),
    )
    .await?;

    let accepted_trigger = match accepted {
        foreground::ForegroundTriggerIntakeOutcome::Accepted(trigger) => trigger,
        other => panic!("expected accepted trigger, got {other:?}"),
    };

    let mut duplicate_ingress = ingress.clone();
    duplicate_ingress.ingress_id = Uuid::now_v7();

    let duplicate = foreground::intake_telegram_foreground_trigger(
        &pool,
        &config,
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
    .fetch_one(&pool)
    .await?
    .get::<i64, _>("count");
    assert_eq!(ingress_count, 1);

    let audit_events = audit::list_for_execution(&pool, accepted_trigger.execution_id).await?;
    assert_eq!(audit_events.len(), 2);
    assert_eq!(audit_events[0].event_kind, "foreground_trigger_accepted");
    assert_eq!(audit_events[1].event_kind, "foreground_trigger_duplicate");
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
