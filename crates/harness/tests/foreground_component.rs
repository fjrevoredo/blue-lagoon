mod support;

use anyhow::Result;
use chrono::{Duration, Utc};
use contracts::{
    ApprovalRequestStatus, CapabilityScope, ChannelKind, EnvironmentCapabilityScope,
    ExecutionCapabilityBudget, FilesystemCapabilityScope, GovernedActionFingerprint,
    GovernedActionKind, GovernedActionRiskTier, IdentityLifecycleState, LoopKind, ModelCallPurpose,
    ModelCallRequest, ModelInput, ModelInputMessage, ModelMessageRole, ModelOutputMode,
    NetworkAccessPosture, ToolPolicy,
};
use harness::{
    approval::{self, NewApprovalRequestRecord},
    audit,
    config::{
        ForegroundModelRouteConfig, ModelGatewayConfig, ResolvedForegroundModelRouteConfig,
        ResolvedModelGatewayConfig, ResolvedTelegramConfig, SelfModelConfig,
    },
    context, continuity, execution, foreground, foreground_orchestration, identity, ingress,
    model_gateway, runtime, scheduled_foreground, telegram, worker,
};
use serial_test::serial;
use sqlx::{Connection, PgConnection, Row};
use std::ffi::OsString;
use tokio::time::{Duration as TokioDuration, sleep};
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
                    "kind": "foreground_component"
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
        assert_eq!(stored_ingress.foreground_status, "processing");
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
async fn approval_callback_intake_accepts_and_persists_execution() -> Result<()> {
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

        let accepted = match outcome {
            foreground::ForegroundTriggerIntakeOutcome::Accepted(accepted) => accepted,
            other => panic!("expected accepted trigger, got {other:?}"),
        };

        let stored_ingress =
            foreground::get_ingress_event(&ctx.pool, accepted.ingress.ingress_id).await?;
        assert_eq!(stored_ingress.status, "accepted");
        assert_eq!(stored_ingress.foreground_status, "processing");
        assert_eq!(stored_ingress.execution_id, Some(accepted.execution_id));
        assert_eq!(
            stored_ingress.approval_callback_data.as_deref(),
            Some("approve:42")
        );

        let execution = execution::get(&ctx.pool, accepted.execution_id).await?;
        assert_eq!(execution.status, "started");
        assert_eq!(execution.trace_id, accepted.trace_id);

        let audit_events = audit::list_for_trace(&ctx.pool, accepted.trace_id).await?;
        assert_eq!(audit_events.len(), 1);
        assert_eq!(audit_events[0].event_kind, "foreground_trigger_accepted");
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn approval_callback_orchestration_resolves_request_and_replies() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let created = approval::create_approval_request(
            &ctx.config,
            &ctx.pool,
            &NewApprovalRequestRecord {
                approval_request_id: Uuid::now_v7(),
                trace_id: Uuid::now_v7(),
                execution_id: None,
                action_proposal_id: Uuid::now_v7(),
                action_fingerprint: GovernedActionFingerprint {
                    value: "sha256:foreground-callback".to_string(),
                },
                action_kind: GovernedActionKind::RunSubprocess,
                risk_tier: GovernedActionRiskTier::Tier2,
                title: "Run scoped callback action".to_string(),
                consequence_summary: "Used to verify Telegram callback routing.".to_string(),
                capability_scope: sample_capability_scope(),
                requested_by: "telegram:primary-user".to_string(),
                token: "42".to_string(),
                requested_at: Utc::now(),
                expires_at: Utc::now() + Duration::minutes(15),
            },
        )
        .await?;
        assert_eq!(created.status, ApprovalRequestStatus::Pending);

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

        let transport = model_gateway::FakeModelProviderTransport::new();
        let mut delivery = telegram::FakeTelegramDelivery::default();
        let outcome = foreground_orchestration::orchestrate_telegram_foreground_ingress(
            &ctx.pool,
            &ctx.config,
            &sample_telegram_config(),
            &sample_model_gateway_config(),
            ingress,
            &transport,
            &mut delivery,
        )
        .await?;

        let completed = match outcome {
            foreground_orchestration::TelegramForegroundOrchestrationOutcome::ApprovalResolved(
                completed,
            ) => completed,
            other => panic!("expected approval-resolution orchestration, got {other:?}"),
        };

        let resolved_request =
            approval::get_approval_request(&ctx.pool, completed.approval_request_id).await?;
        assert_eq!(resolved_request.status, ApprovalRequestStatus::Approved);

        let execution = execution::get(&ctx.pool, completed.execution_id).await?;
        assert_eq!(execution.status, "completed");

        let stored_ingress = foreground::get_ingress_event(&ctx.pool, completed.ingress_id).await?;
        assert_eq!(stored_ingress.foreground_status, "processed");
        assert_eq!(stored_ingress.execution_id, Some(completed.execution_id));

        assert_eq!(delivery.sent_messages().len(), 1);
        assert_eq!(
            delivery.sent_chat_actions(),
            &[(42, telegram::TelegramChatAction::Typing)]
        );
        assert_eq!(
            delivery.sent_messages()[0].text,
            "Approved: Run scoped callback action"
        );
        assert_eq!(delivery.sent_messages()[0].reply_to_message_id, Some(46));

        let audit_events = audit::list_for_trace(&ctx.pool, completed.trace_id).await?;
        let event_kinds = audit_events
            .into_iter()
            .map(|event| event.event_kind)
            .collect::<Vec<_>>();
        assert!(event_kinds.contains(&"foreground_trigger_accepted".to_string()));
        assert!(event_kinds.contains(&"approval_resolution_resolved".to_string()));
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn approval_command_orchestration_resolves_request_and_replies() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        approval::create_approval_request(
            &ctx.config,
            &ctx.pool,
            &NewApprovalRequestRecord {
                approval_request_id: Uuid::now_v7(),
                trace_id: Uuid::now_v7(),
                execution_id: None,
                action_proposal_id: Uuid::now_v7(),
                action_fingerprint: GovernedActionFingerprint {
                    value: "sha256:foreground-command".to_string(),
                },
                action_kind: GovernedActionKind::RunSubprocess,
                risk_tier: GovernedActionRiskTier::Tier2,
                title: "Run scoped command action".to_string(),
                consequence_summary: "Used to verify Telegram command fallback routing."
                    .to_string(),
                capability_scope: sample_capability_scope(),
                requested_by: "telegram:primary-user".to_string(),
                token: "42".to_string(),
                requested_at: Utc::now(),
                expires_at: Utc::now() + Duration::minutes(15),
            },
        )
        .await?;

        let update =
            telegram::load_fixture_updates(&telegram_fixture("approval_command_approve.json"))?
                .into_iter()
                .next()
                .expect("fixture should contain one update");
        let ingress = match ingress::normalize_telegram_update(
            &sample_telegram_config(),
            &update,
            Some("fixtures/approval_command_approve.json".to_string()),
        )? {
            ingress::TelegramNormalizationOutcome::Accepted(ingress) => *ingress,
            other => {
                panic!("command fixture should normalize into accepted ingress, got {other:?}")
            }
        };

        let transport = model_gateway::FakeModelProviderTransport::new();
        let mut delivery = telegram::FakeTelegramDelivery::default();
        let outcome = foreground_orchestration::orchestrate_telegram_foreground_ingress(
            &ctx.pool,
            &ctx.config,
            &sample_telegram_config(),
            &sample_model_gateway_config(),
            ingress,
            &transport,
            &mut delivery,
        )
        .await?;

        let completed = match outcome {
            foreground_orchestration::TelegramForegroundOrchestrationOutcome::ApprovalResolved(
                completed,
            ) => completed,
            other => panic!("expected approval-resolution orchestration, got {other:?}"),
        };

        let resolved_request =
            approval::get_approval_request(&ctx.pool, completed.approval_request_id).await?;
        assert_eq!(resolved_request.status, ApprovalRequestStatus::Approved);
        assert_eq!(delivery.sent_messages().len(), 1);
        assert_eq!(
            delivery.sent_messages()[0].text,
            "Approved: Run scoped command action"
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn telegram_approval_prompt_delivery_renders_and_sends_prompt() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let approval_request = approval::create_approval_request(
            &ctx.config,
            &ctx.pool,
            &NewApprovalRequestRecord {
                approval_request_id: Uuid::now_v7(),
                trace_id: Uuid::now_v7(),
                execution_id: None,
                action_proposal_id: Uuid::now_v7(),
                action_fingerprint: GovernedActionFingerprint {
                    value: "sha256:foreground-prompt".to_string(),
                },
                action_kind: GovernedActionKind::RunSubprocess,
                risk_tier: GovernedActionRiskTier::Tier2,
                title: "Prompt delivery action".to_string(),
                consequence_summary: "Used to verify Telegram approval prompt delivery."
                    .to_string(),
                capability_scope: sample_capability_scope(),
                requested_by: "telegram:primary-user".to_string(),
                token: "42".to_string(),
                requested_at: Utc::now(),
                expires_at: Utc::now() + Duration::minutes(15),
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
            other => {
                panic!("message fixture should normalize into accepted ingress, got {other:?}")
            }
        };

        let mut delivery = telegram::FakeTelegramDelivery::default();
        let prompt_delivery = foreground_orchestration::deliver_telegram_approval_prompt(
            harness::config::ApprovalPromptMode::InlineKeyboardWithFallback,
            &ingress,
            &approval_request,
            &mut delivery,
        )
        .await?;

        assert_eq!(prompt_delivery.chat_id, 42);
        assert_eq!(
            prompt_delivery.approval_request_id,
            approval_request.approval_request_id
        );
        assert_eq!(delivery.sent_messages().len(), 1);
        assert!(
            delivery.sent_messages()[0]
                .text
                .contains("Approval required")
        );
        assert!(delivery.sent_messages()[0].reply_markup.is_some());
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn telegram_approval_prompt_builder_renders_inline_keyboard() -> Result<()> {
    let prompt = telegram::TelegramApprovalPrompt {
        token: "42".to_string(),
        title: "Run scoped subprocess".to_string(),
        consequence_summary: "Writes a bounded file inside the workspace.".to_string(),
        action_fingerprint: "sha256:abc123".to_string(),
        risk_tier: GovernedActionRiskTier::Tier2,
        expires_at: Utc::now() + Duration::minutes(15),
    };
    let message = telegram::build_approval_prompt_message(
        harness::config::ApprovalPromptMode::InlineKeyboardWithFallback,
        42,
        Some(7),
        &prompt,
    )?;
    assert!(message.text.contains("Approval required"));
    assert!(message.reply_markup.is_some());
    assert_eq!(message.reply_to_message_id, Some(7));
    Ok(())
}

#[tokio::test]
#[serial]
async fn telegram_approval_prompt_builder_falls_back_for_long_tokens() -> Result<()> {
    let prompt = telegram::TelegramApprovalPrompt {
        token: "x".repeat(128),
        title: "Run scoped subprocess".to_string(),
        consequence_summary: "Writes a bounded file inside the workspace.".to_string(),
        action_fingerprint: "sha256:abc123".to_string(),
        risk_tier: GovernedActionRiskTier::Tier2,
        expires_at: Utc::now() + Duration::minutes(15),
    };
    let message = telegram::build_approval_prompt_message(
        harness::config::ApprovalPromptMode::InlineKeyboardWithFallback,
        42,
        None,
        &prompt,
    )?;
    assert_eq!(message.reply_markup, None);
    assert!(message.text.contains("/approve"));
    assert!(message.text.contains("/reject"));
    Ok(())
}

#[tokio::test]
#[serial]
async fn pending_foreground_execution_switches_to_backlog_recovery_and_links_selected_ingress()
-> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let now = Utc::now();
        let ingress_one = insert_pending_ingress(
            &ctx.pool,
            "telegram-primary",
            "primary-user",
            now - Duration::minutes(10),
            "first delayed message",
        )
        .await?;
        let ingress_two = insert_pending_ingress(
            &ctx.pool,
            "telegram-primary",
            "primary-user",
            now - Duration::minutes(6),
            "second delayed message",
        )
        .await?;
        let ingress_three = insert_pending_ingress(
            &ctx.pool,
            "telegram-primary",
            "primary-user",
            now - Duration::minutes(1),
            "latest delayed message",
        )
        .await?;

        let execution_id = Uuid::now_v7();
        let trace_id = Uuid::now_v7();
        execution::insert(
            &ctx.pool,
            &execution::NewExecutionRecord {
                execution_id,
                trace_id,
                trigger_kind: "pending_backlog_test".to_string(),
                synthetic_trigger: None,
                status: "started".to_string(),
                request_payload: serde_json::json!({ "kind": "pending_backlog_test" }),
            },
        )
        .await?;

        let plan = foreground::plan_pending_foreground_execution(
            &ctx.pool,
            &ctx.config,
            trace_id,
            execution_id,
            "telegram-primary",
            foreground::PendingForegroundExecutionOptions::default(),
        )
        .await?
        .expect("pending ingress plan should be created");

        assert_eq!(
            plan.mode,
            contracts::ForegroundExecutionMode::BacklogRecovery
        );
        assert_eq!(
            plan.decision_reason,
            foreground::ForegroundExecutionDecisionReason::PendingSpanThreshold
        );
        assert_eq!(plan.primary_ingress.ingress_id, ingress_three.ingress_id);
        assert_eq!(
            plan.ordered_ingress
                .iter()
                .map(|ingress| ingress.ingress_id)
                .collect::<Vec<_>>(),
            vec![
                ingress_one.ingress_id,
                ingress_two.ingress_id,
                ingress_three.ingress_id,
            ]
        );

        let links = foreground::list_execution_ingress_links(&ctx.pool, execution_id).await?;
        assert_eq!(links.len(), 3);
        assert_eq!(links[0].ingress_id, ingress_one.ingress_id);
        assert_eq!(links[0].link_role, "batch_member");
        assert_eq!(links[1].ingress_id, ingress_two.ingress_id);
        assert_eq!(links[1].link_role, "batch_member");
        assert_eq!(links[2].ingress_id, ingress_three.ingress_id);
        assert_eq!(links[2].link_role, "primary");

        for ingress_id in [
            ingress_one.ingress_id,
            ingress_two.ingress_id,
            ingress_three.ingress_id,
        ] {
            let stored = foreground::get_ingress_event(&ctx.pool, ingress_id).await?;
            assert_eq!(stored.execution_id, Some(execution_id));
            assert_eq!(stored.foreground_status, "processing");
        }

        let audit_events = audit::list_for_execution(&ctx.pool, execution_id).await?;
        assert!(
            audit_events
                .iter()
                .any(|event| event.event_kind == "foreground_recovery_mode_decided")
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn pending_foreground_execution_stays_single_when_backlog_threshold_is_not_met() -> Result<()>
{
    support::with_migrated_database(|ctx| async move {
        let now = Utc::now();
        let oldest = insert_pending_ingress(
            &ctx.pool,
            "telegram-primary",
            "primary-user",
            now - Duration::seconds(30),
            "first pending message",
        )
        .await?;
        let newest = insert_pending_ingress(
            &ctx.pool,
            "telegram-primary",
            "primary-user",
            now - Duration::seconds(5),
            "second pending message",
        )
        .await?;

        let execution_id = Uuid::now_v7();
        let trace_id = Uuid::now_v7();
        execution::insert(
            &ctx.pool,
            &execution::NewExecutionRecord {
                execution_id,
                trace_id,
                trigger_kind: "single_pending_test".to_string(),
                synthetic_trigger: None,
                status: "started".to_string(),
                request_payload: serde_json::json!({ "kind": "single_pending_test" }),
            },
        )
        .await?;

        let plan = foreground::plan_pending_foreground_execution(
            &ctx.pool,
            &ctx.config,
            trace_id,
            execution_id,
            "telegram-primary",
            foreground::PendingForegroundExecutionOptions::default(),
        )
        .await?
        .expect("pending ingress plan should be created");

        assert_eq!(plan.mode, contracts::ForegroundExecutionMode::SingleIngress);
        assert_eq!(
            plan.decision_reason,
            foreground::ForegroundExecutionDecisionReason::SingleIngress
        );
        assert_eq!(plan.primary_ingress.ingress_id, oldest.ingress_id);
        assert_eq!(plan.ordered_ingress.len(), 1);
        assert_eq!(plan.ordered_ingress[0].ingress_id, oldest.ingress_id);

        let links = foreground::list_execution_ingress_links(&ctx.pool, execution_id).await?;
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].ingress_id, oldest.ingress_id);
        assert_eq!(links[0].link_role, "primary");

        let oldest_stored = foreground::get_ingress_event(&ctx.pool, oldest.ingress_id).await?;
        assert_eq!(oldest_stored.execution_id, Some(execution_id));
        assert_eq!(oldest_stored.foreground_status, "processing");

        let newest_stored = foreground::get_ingress_event(&ctx.pool, newest.ingress_id).await?;
        assert_eq!(newest_stored.execution_id, None);
        assert_eq!(newest_stored.foreground_status, "pending");
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn pending_foreground_execution_switches_to_backlog_when_stale_processing_is_resumed()
-> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let now = Utc::now();
        let resumed = insert_pending_ingress(
            &ctx.pool,
            "telegram-primary",
            "primary-user",
            now - Duration::minutes(8),
            "earlier interrupted message",
        )
        .await?;
        let latest = insert_pending_ingress(
            &ctx.pool,
            "telegram-primary",
            "primary-user",
            now - Duration::seconds(20),
            "latest pending message",
        )
        .await?;

        sqlx::query(
            r#"
            UPDATE ingress_events
            SET
                foreground_status = 'processing',
                last_processed_at = $2
            WHERE ingress_id = $1
            "#,
        )
        .bind(resumed.ingress_id)
        .bind(now - Duration::minutes(10))
        .execute(&ctx.pool)
        .await?;

        let execution_id = Uuid::now_v7();
        let trace_id = Uuid::now_v7();
        execution::insert(
            &ctx.pool,
            &execution::NewExecutionRecord {
                execution_id,
                trace_id,
                trigger_kind: "stale_processing_resume_test".to_string(),
                synthetic_trigger: None,
                status: "started".to_string(),
                request_payload: serde_json::json!({ "kind": "stale_processing_resume_test" }),
            },
        )
        .await?;

        let plan = foreground::plan_pending_foreground_execution(
            &ctx.pool,
            &ctx.config,
            trace_id,
            execution_id,
            "telegram-primary",
            foreground::PendingForegroundExecutionOptions::default(),
        )
        .await?
        .expect("pending ingress plan should be created");

        assert_eq!(
            plan.mode,
            contracts::ForegroundExecutionMode::BacklogRecovery
        );
        assert_eq!(
            plan.decision_reason,
            foreground::ForegroundExecutionDecisionReason::StaleProcessingResume
        );
        assert_eq!(plan.primary_ingress.ingress_id, latest.ingress_id);
        assert_eq!(plan.ordered_ingress.len(), 2);
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
async fn concurrent_duplicate_foreground_trigger_is_recovered_from_db_conflict() -> Result<()> {
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

        let _binding = foreground::reconcile_conversation_binding(
            &ctx.pool,
            &foreground::NewConversationBinding {
                conversation_binding_id: Uuid::now_v7(),
                channel_kind: ChannelKind::Telegram,
                external_user_id: ingress.external_user_id.clone(),
                external_conversation_id: ingress.external_conversation_id.clone(),
                internal_principal_ref: ingress.internal_principal_ref.clone(),
                internal_conversation_ref: ingress.internal_conversation_ref.clone(),
            },
        )
        .await?;

        let mut lock_connection = PgConnection::connect(&ctx.config.database.database_url).await?;
        sqlx::query("BEGIN").execute(&mut lock_connection).await?;
        sqlx::query(
            r#"
            SELECT conversation_binding_id
            FROM conversation_bindings
            WHERE internal_conversation_ref = $1
            FOR UPDATE
            "#,
        )
        .bind(&ingress.internal_conversation_ref)
        .fetch_one(&mut lock_connection)
        .await?;

        let pool = ctx.pool.clone();
        let config = ctx.config.clone();
        let duplicate_candidate = ingress.clone();
        let intake_task = tokio::spawn(async move {
            foreground::intake_telegram_foreground_trigger(
                &pool,
                &config,
                &sample_telegram_config(),
                duplicate_candidate,
            )
            .await
        });

        wait_for_binding_reconcile_lock(&ctx.config.database.database_url).await?;

        let canonical_execution_id = Uuid::now_v7();
        let canonical_trace_id = Uuid::now_v7();
        execution::insert(
            &ctx.pool,
            &execution::NewExecutionRecord {
                execution_id: canonical_execution_id,
                trace_id: canonical_trace_id,
                trigger_kind: "telegram_user_ingress".to_string(),
                synthetic_trigger: None,
                status: "started".to_string(),
                request_payload: serde_json::json!({
                    "kind": "duplicate_race_fixture"
                }),
            },
        )
        .await?;
        foreground::insert_ingress_event(
            &ctx.pool,
            &foreground::NewIngressEvent {
                ingress: ingress.clone(),
                conversation_binding_id: None,
                trace_id: canonical_trace_id,
                execution_id: Some(canonical_execution_id),
                status: "accepted".to_string(),
                rejection_reason: None,
            },
        )
        .await?;

        sqlx::query("ROLLBACK")
            .execute(&mut lock_connection)
            .await
            .expect("lock transaction should roll back cleanly");

        let outcome = intake_task
            .await
            .expect("intake task should join cleanly")?;
        let duplicate = match outcome {
            foreground::ForegroundTriggerIntakeOutcome::Duplicate(duplicate) => duplicate,
            other => panic!("expected duplicate outcome after DB conflict, got {other:?}"),
        };

        assert_eq!(duplicate.ingress_id, ingress.ingress_id);
        assert_eq!(duplicate.execution_id, Some(canonical_execution_id));
        assert_eq!(duplicate.trace_id, canonical_trace_id);

        let ingress_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM ingress_events
            WHERE channel_kind = 'telegram' AND external_event_id = $1
            "#,
        )
        .bind(&ingress.external_event_id)
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(ingress_count, 1);

        let execution_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM execution_records
            WHERE trigger_kind = 'telegram_user_ingress'
            "#,
        )
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(execution_count, 1);

        let audit_events = audit::list_for_execution(&ctx.pool, canonical_execution_id).await?;
        assert_eq!(audit_events.len(), 1);
        assert_eq!(audit_events[0].event_kind, "foreground_trigger_duplicate");
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
                episode_id: None,
                recovery_context: contracts::ForegroundRecoveryContext::default(),
            },
        )
        .await?;

        assert_eq!(assembled.context.self_model.stable_identity, "blue-lagoon");
        assert_eq!(
            assembled.context.self_model.identity_lifecycle.state,
            IdentityLifecycleState::BootstrapSeedOnly
        );
        assert!(
            assembled
                .context
                .self_model
                .identity_lifecycle
                .kickstart_available
        );
        assert_eq!(
            assembled
                .context
                .self_model
                .identity_lifecycle
                .kickstart
                .as_ref()
                .expect("seed-only state should expose kickstart context")
                .predefined_templates
                .len(),
            3
        );
        assert!(
            assembled
                .context
                .self_model
                .identity
                .as_ref()
                .is_some_and(|identity| !identity.stable_items.is_empty())
        );
        assert_eq!(
            assembled.metadata.identity_lifecycle_state,
            "bootstrap_seed_only"
        );
        assert!(assembled.metadata.identity_kickstart_available);
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
#[serial]
async fn context_assembly_uses_complete_identity_lifecycle_snapshot() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        config.self_model = Some(SelfModelConfig {
            seed_path: support::workspace_root()
                .join("config")
                .join("self_model_seed.toml"),
        });

        identity::record_lifecycle_transition(
            &ctx.pool,
            &identity::NewIdentityLifecycle {
                identity_lifecycle_id: Uuid::now_v7(),
                status: "current".to_string(),
                lifecycle_state: "complete_identity_active".to_string(),
                active_self_model_artifact_id: None,
                active_interview_id: None,
                transition_reason: "component test complete identity".to_string(),
                transitioned_by: "test".to_string(),
                kickstart_started_at: None,
                kickstart_completed_at: Some(Utc::now()),
                reset_at: None,
                payload: serde_json::json!({}),
            },
        )
        .await?;
        identity::insert_identity_item(
            &ctx.pool,
            &identity::NewIdentityItem {
                identity_item_id: Uuid::now_v7(),
                self_model_artifact_id: None,
                proposal_id: None,
                trace_id: None,
                stability_class: "stable".to_string(),
                category: "name".to_string(),
                item_key: "name".to_string(),
                value_text: "Lagoon Complete".to_string(),
                confidence: 1.0,
                weight: None,
                provenance_kind: "component_test".to_string(),
                source_kind: "custom_interview".to_string(),
                merge_policy: "protected_core".to_string(),
                status: "active".to_string(),
                evidence_refs: serde_json::json!([]),
                valid_from: Some(Utc::now()),
                valid_to: None,
                supersedes_item_id: None,
                payload: serde_json::json!({}),
            },
        )
        .await?;
        identity::insert_identity_item(
            &ctx.pool,
            &identity::NewIdentityItem {
                identity_item_id: Uuid::now_v7(),
                self_model_artifact_id: None,
                proposal_id: None,
                trace_id: None,
                stability_class: "stable".to_string(),
                category: "foundational_value".to_string(),
                item_key: "value:clarity".to_string(),
                value_text: "clarity".to_string(),
                confidence: 0.95,
                weight: Some(0.9),
                provenance_kind: "component_test".to_string(),
                source_kind: "custom_interview".to_string(),
                merge_policy: "protected_core".to_string(),
                status: "active".to_string(),
                evidence_refs: serde_json::json!([]),
                valid_from: Some(Utc::now()),
                valid_to: None,
                supersedes_item_id: None,
                payload: serde_json::json!({}),
            },
        )
        .await?;

        let mut trigger = sample_conscious_context().trigger;
        trigger.received_at = Utc::now();
        trigger.ingress.occurred_at = trigger.received_at;
        execution::insert(
            &ctx.pool,
            &execution::NewExecutionRecord {
                execution_id: trigger.execution_id,
                trace_id: trigger.trace_id,
                trigger_kind: "identity_context_test".to_string(),
                synthetic_trigger: None,
                status: "started".to_string(),
                request_payload: serde_json::json!({ "kind": "identity_context_test" }),
            },
        )
        .await?;

        let assembled = context::assemble_foreground_context(
            &ctx.pool,
            &config,
            trigger,
            context::ContextAssemblyOptions::default(),
        )
        .await?;

        assert_eq!(
            assembled.context.self_model.identity_lifecycle.state,
            IdentityLifecycleState::CompleteIdentityActive
        );
        assert!(
            !assembled
                .context
                .self_model
                .identity_lifecycle
                .kickstart_available
        );
        assert!(
            assembled
                .context
                .self_model
                .identity_lifecycle
                .kickstart
                .is_none()
        );

        let identity = assembled
            .context
            .self_model
            .identity
            .as_ref()
            .expect("complete identity should inject compact identity snapshot");
        assert_eq!(identity.identity_summary, "Lagoon Complete");
        assert_eq!(identity.values, vec!["clarity".to_string()]);
        assert_eq!(
            assembled.metadata.identity_lifecycle_state,
            "complete_identity_active"
        );
        assert!(!assembled.metadata.identity_kickstart_available);
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
#[serial]
async fn context_assembly_injects_retrieved_episode_and_memory_context() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        config.self_model = Some(SelfModelConfig {
            seed_path: support::workspace_root()
                .join("config")
                .join("self_model_seed.toml"),
        });

        let mut trigger = sample_conscious_context().trigger;
        trigger.received_at = Utc::now();
        trigger.ingress.occurred_at = trigger.received_at;
        execution::insert(
            &ctx.pool,
            &execution::NewExecutionRecord {
                execution_id: trigger.execution_id,
                trace_id: trigger.trace_id,
                trigger_kind: "retrieval_context_test".to_string(),
                synthetic_trigger: None,
                status: "started".to_string(),
                request_payload: serde_json::json!({ "kind": "retrieval_context_test" }),
            },
        )
        .await?;
        insert_completed_episode(
            &ctx.pool,
            "episode-retrieval",
            trigger.received_at - Duration::minutes(30),
            "remember the travel preference",
            "noted the travel preference",
        )
        .await?;
        insert_active_memory_artifact(
            &ctx.pool,
            "user:primary",
            "The user prefers direct answers about travel.",
        )
        .await?;

        let assembled = context::assemble_foreground_context(
            &ctx.pool,
            &config,
            trigger,
            context::ContextAssemblyOptions::default(),
        )
        .await?;

        let retrieved_episode = assembled
            .context
            .retrieved_context
            .items
            .iter()
            .find_map(|item| match item {
                contracts::RetrievedContextItem::Episode(episode) => Some(episode),
                _ => None,
            })
            .expect("episode context should be retrieved");
        assert_eq!(
            retrieved_episode.latest_user_message.as_deref(),
            Some("remember the travel preference")
        );
        assert_eq!(
            retrieved_episode.latest_assistant_message.as_deref(),
            Some("noted the travel preference")
        );
        assert!(
            assembled
                .context
                .retrieved_context
                .items
                .iter()
                .any(|item| matches!(item, contracts::RetrievedContextItem::MemoryArtifact(_)))
        );
        assert!(assembled.metadata.selected_retrieved_context_count >= 2);
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn context_assembly_retrieves_semantic_match_from_prior_memory() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        config.self_model = Some(SelfModelConfig {
            seed_path: support::workspace_root()
                .join("config")
                .join("self_model_seed.toml"),
        });

        let mut trigger = sample_conscious_context().trigger;
        trigger.received_at = Utc::now();
        trigger.ingress.occurred_at = trigger.received_at;
        trigger.ingress.text_body = Some("please be brief when you answer".to_string());
        execution::insert(
            &ctx.pool,
            &execution::NewExecutionRecord {
                execution_id: trigger.execution_id,
                trace_id: trigger.trace_id,
                trigger_kind: "semantic_retrieval_context_test".to_string(),
                synthetic_trigger: None,
                status: "started".to_string(),
                request_payload: serde_json::json!({ "kind": "semantic_retrieval_context_test" }),
            },
        )
        .await?;
        insert_active_memory_artifact(
            &ctx.pool,
            "user:primary",
            "The user prefers concise replies.",
        )
        .await?;

        let assembled = context::assemble_foreground_context(
            &ctx.pool,
            &config,
            trigger,
            context::ContextAssemblyOptions::default(),
        )
        .await?;

        let semantic_memory = assembled
            .context
            .retrieved_context
            .items
            .iter()
            .find_map(|item| match item {
                contracts::RetrievedContextItem::MemoryArtifact(artifact)
                    if artifact.relevance_reason.starts_with("semantic_match:") =>
                {
                    Some(artifact)
                }
                _ => None,
            })
            .expect("semantic match should be retrieved");
        assert!(semantic_memory.content_text.contains("concise replies"));
        Ok(())
    })
    .await
}

#[tokio::test]
async fn conscious_worker_path_runs_one_harness_mediated_model_cycle() -> Result<()> {
    support::with_clean_database(|_ctx| async move {
        let mut config = _ctx.config.clone();
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
            contracts::WorkerResult::Unconscious(_) => {
                panic!("expected conscious worker result")
            }
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
async fn conscious_worker_protocol_failure_includes_phase_exit_and_stderr() -> Result<()> {
    support::with_clean_database(|_ctx| async move {
        let mut config = _ctx.config.clone();
        let worker_binary = support::workers_binary()?;
        config.worker.command = worker_binary.to_string_lossy().into_owned();
        config.worker.args = vec!["exit-after-model-request-worker".to_string()];

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

        let error = worker::launch_conscious_worker(&config, &gateway, &request, &transport)
            .await
            .expect_err("early worker exit should fail the protocol boundary");
        let message = error.to_string();
        assert!(message.contains("conscious worker protocol failure"));
        assert!(message.contains("worker_protocol_phase=write_model_response"));
        assert!(message.contains("worker_exit_status="));
        assert!(message.contains("worker_stderr_excerpt="));
        assert!(message.contains("intentionally exiting before final response"));
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
        config.worker.args = Vec::new();

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
        let mut ingress = ingress;
        ingress.text_body =
            Some("remember that I prefer concise replies and be direct".to_string());

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

        let stored_ingress = foreground::get_ingress_event(&ctx.pool, completed.ingress_id).await?;
        assert_eq!(stored_ingress.foreground_status, "processed");
        assert_eq!(stored_ingress.execution_id, Some(completed.execution_id));

        let episode = foreground::get_episode(&ctx.pool, completed.episode_id).await?;
        assert_eq!(episode.status, "completed");
        assert_eq!(episode.execution_id, completed.execution_id);
        assert_eq!(episode.outcome.as_deref(), Some("completed"));
        assert!(
            episode
                .summary
                .as_deref()
                .is_some_and(|summary| summary.contains("proposals evaluated=2"))
        );

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
        assert_eq!(
            delivery.sent_chat_actions(),
            &[(42, telegram::TelegramChatAction::Typing)]
        );
        assert_eq!(delivery.sent_messages()[0].chat_id, 42);
        assert_eq!(delivery.sent_messages()[0].reply_to_message_id, Some(42));
        assert_eq!(
            delivery.sent_messages()[0].text,
            "assistant reply from fake provider"
        );

        let proposals =
            continuity::list_proposals_for_execution(&ctx.pool, completed.execution_id).await?;
        assert_eq!(proposals.len(), 2);
        assert!(
            proposals
                .iter()
                .all(|proposal| proposal.status == "accepted")
        );

        let active_memory = continuity::list_active_memory_artifacts_by_subject(
            &ctx.pool,
            "principal:primary-user",
            10,
        )
        .await?;
        assert_eq!(active_memory.len(), 1);

        let active_self_model = continuity::get_latest_active_self_model_artifact(&ctx.pool)
            .await?
            .expect("active self-model artifact should exist");
        assert!(
            active_self_model
                .preferences
                .iter()
                .any(|value| value.contains("be direct"))
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
                .any(|event| event.event_kind == "proposal_evaluated")
        );
        assert!(
            audit_events
                .iter()
                .any(|event| event.event_kind == "merge_decision_recorded")
        );
        assert!(
            audit_events
                .iter()
                .any(|event| event.event_kind == "canonical_write_applied")
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
async fn foreground_orchestration_infers_custom_identity_start_without_worker_block() -> Result<()>
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
        let mut ingress = ingress;
        ingress.text_body = Some("let's create a custom one".to_string());

        let transport = model_gateway::FakeModelProviderTransport::new();
        transport.push_response(Ok(model_gateway::ProviderHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{
                    "message": { "content": "" },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 13,
                    "completion_tokens": 0
                }
            }),
        }));
        let mut delivery = telegram::FakeTelegramDelivery::default();

        let outcome = foreground_orchestration::orchestrate_telegram_foreground_ingress(
            &ctx.pool,
            &config,
            &sample_telegram_config(),
            &sample_model_gateway_config(),
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

        assert_eq!(delivery.sent_messages().len(), 1);
        assert_eq!(
            delivery.sent_messages()[0].text,
            "Starting the custom identity interview. What name should this assistant identity use?"
        );

        let lifecycle = identity::get_current_lifecycle(&ctx.pool)
            .await?
            .expect("identity lifecycle should be current");
        assert_eq!(lifecycle.lifecycle_state, "identity_kickstart_in_progress");
        let interview_id = lifecycle
            .active_interview_id
            .expect("custom interview should be active");
        let interview = identity::get_identity_interview(&ctx.pool, interview_id)
            .await?
            .expect("identity interview should be stored");
        assert_eq!(interview.current_step, "name");

        let episode = foreground::get_episode(&ctx.pool, completed.episode_id).await?;
        assert!(
            episode
                .summary
                .as_deref()
                .is_some_and(|summary| summary.contains("proposals evaluated=1"))
        );

        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn planned_foreground_orchestration_processes_backlog_batch_with_single_reply() -> Result<()>
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

        let now = Utc::now();
        let ingress_one = insert_pending_ingress(
            &ctx.pool,
            "telegram-primary",
            "primary-user",
            now - Duration::minutes(10),
            "first delayed hello",
        )
        .await?;
        let ingress_two = insert_pending_ingress(
            &ctx.pool,
            "telegram-primary",
            "primary-user",
            now - Duration::minutes(6),
            "second delayed hello",
        )
        .await?;
        let ingress_three = insert_pending_ingress(
            &ctx.pool,
            "telegram-primary",
            "primary-user",
            now - Duration::minutes(1),
            "remember that I prefer concise replies and be direct",
        )
        .await?;

        let execution_id = Uuid::now_v7();
        let trace_id = Uuid::now_v7();
        execution::insert(
            &ctx.pool,
            &execution::NewExecutionRecord {
                execution_id,
                trace_id,
                trigger_kind: "planned_backlog_test".to_string(),
                synthetic_trigger: None,
                status: "started".to_string(),
                request_payload: serde_json::json!({ "kind": "planned_backlog_test" }),
            },
        )
        .await?;

        let plan = foreground::plan_pending_foreground_execution(
            &ctx.pool,
            &config,
            trace_id,
            execution_id,
            "telegram-primary",
            foreground::PendingForegroundExecutionOptions::default(),
        )
        .await?
        .expect("pending ingress plan should be created");

        let gateway = sample_model_gateway_config();
        let transport = model_gateway::FakeModelProviderTransport::new();
        transport.push_response(Ok(model_gateway::ProviderHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{
                    "message": { "content": "assistant reply for delayed backlog" },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 21,
                    "completion_tokens": 7
                }
            }),
        }));
        let mut delivery = telegram::FakeTelegramDelivery::default();

        let outcome = foreground_orchestration::orchestrate_telegram_foreground_plan(
            &ctx.pool,
            &config,
            &gateway,
            foreground_orchestration::TelegramForegroundPlanExecution {
                execution: foreground_orchestration::ForegroundExecutionIds {
                    trace_id,
                    execution_id,
                },
                trigger_kind_override: None,
                plan,
            },
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

        let episode = foreground::get_episode(&ctx.pool, completed.episode_id).await?;
        assert_eq!(episode.status, "completed");

        let messages = foreground::list_episode_messages(&ctx.pool, completed.episode_id).await?;
        assert_eq!(messages.len(), 4);
        assert_eq!(
            messages[0].text_body.as_deref(),
            Some("first delayed hello")
        );
        assert_eq!(
            messages[1].text_body.as_deref(),
            Some("second delayed hello")
        );
        assert_eq!(
            messages[2].text_body.as_deref(),
            Some("remember that I prefer concise replies and be direct")
        );
        assert_eq!(messages[3].message_role, "assistant");
        assert_eq!(
            messages[3].text_body.as_deref(),
            Some("assistant reply for delayed backlog")
        );

        assert_eq!(delivery.sent_messages().len(), 1);
        assert_eq!(delivery.sent_messages()[0].chat_id, 42);
        assert_eq!(
            delivery.sent_messages()[0].reply_to_message_id,
            Some(
                ingress_three
                    .external_message_id
                    .as_deref()
                    .expect("latest ingress should have external message id")
                    .parse::<i64>()?
            )
        );

        for ingress_id in [
            ingress_one.ingress_id,
            ingress_two.ingress_id,
            ingress_three.ingress_id,
        ] {
            let stored = foreground::get_ingress_event(&ctx.pool, ingress_id).await?;
            assert_eq!(stored.execution_id, Some(completed.execution_id));
            assert_eq!(stored.foreground_status, "processed");
        }

        let active_memory = continuity::list_active_memory_artifacts_by_subject(
            &ctx.pool,
            "principal:primary-user",
            10,
        )
        .await?;
        assert_eq!(active_memory.len(), 1);
        assert_eq!(active_memory[0].provenance_kind, "backlog_recovery");

        let seen = transport.seen_requests();
        assert_eq!(seen.len(), 1);
        let message_contents = seen[0]
            .body
            .get("messages")
            .and_then(serde_json::Value::as_array)
            .expect("provider request should include messages")
            .iter()
            .filter_map(|message| message.get("content").and_then(serde_json::Value::as_str))
            .collect::<Vec<_>>();
        assert!(message_contents.iter().any(|content| {
            content.contains("Recovery mode is backlog_recovery")
                && content.contains("first delayed hello")
                && content.contains("second delayed hello")
        }));

        let audit_events = audit::list_for_execution(&ctx.pool, completed.execution_id).await?;
        assert!(
            audit_events
                .iter()
                .any(|event| event.event_kind == "foreground_recovery_mode_decided")
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
        let ingress_id = ingress.ingress_id;

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
                .contains("missing foreground self-model seed configuration")
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

        let stored_ingress = foreground::get_ingress_event(&ctx.pool, ingress_id).await?;
        assert_eq!(stored_ingress.foreground_status, "processed");
        assert_eq!(stored_ingress.execution_id, Some(execution_id));

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
        assert_eq!(delivery.sent_messages().len(), 1);
        let failure_notice = &delivery.sent_messages()[0];
        assert_eq!(failure_notice.chat_id, 42);
        assert_eq!(failure_notice.reply_to_message_id, Some(42));
        assert!(failure_notice.text.contains("internal runtime error"));
        assert!(failure_notice.text.contains("context_assembly_failure"));
        assert!(
            failure_notice
                .text
                .contains(&execution.trace_id.to_string())
        );
        assert!(
            !failure_notice
                .text
                .contains("missing foreground self-model seed configuration")
        );

        let messages = foreground::list_episode_messages(&ctx.pool, episode_id).await?;
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].message_role, "user");
        assert_eq!(messages[1].message_role, "assistant");
        assert_eq!(
            messages[1].text_body.as_deref(),
            Some(failure_notice.text.as_str())
        );
        assert_eq!(messages[1].external_message_id.as_deref(), Some("1"));
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn foreground_orchestration_closes_planned_ingress_batch_on_terminal_failure() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        let worker_binary = support::workers_binary()?;
        config.worker.command = worker_binary.to_string_lossy().into_owned();
        config.worker.args = vec!["conscious-worker".to_string()];

        let now = Utc::now();
        let first = insert_pending_ingress(
            &ctx.pool,
            "telegram-primary",
            "primary-user",
            now - Duration::minutes(2),
            "first planned message",
        )
        .await?;
        let second = insert_pending_ingress(
            &ctx.pool,
            "telegram-primary",
            "primary-user",
            now - Duration::minutes(1),
            "second planned message",
        )
        .await?;
        let trace_id = Uuid::now_v7();
        let execution_id = Uuid::now_v7();
        execution::insert(
            &ctx.pool,
            &execution::NewExecutionRecord {
                execution_id,
                trace_id,
                trigger_kind: "planned_terminal_failure_test".to_string(),
                synthetic_trigger: None,
                status: "started".to_string(),
                request_payload: serde_json::json!({
                    "kind": "planned_terminal_failure_test"
                }),
            },
        )
        .await?;
        let plan = foreground::plan_pending_foreground_execution(
            &ctx.pool,
            &config,
            trace_id,
            execution_id,
            "telegram-primary",
            foreground::PendingForegroundExecutionOptions {
                force_recovery: true,
            },
        )
        .await?
        .expect("pending ingress plan should be created");

        let transport = model_gateway::FakeModelProviderTransport::new();
        let mut delivery = telegram::FakeTelegramDelivery::default();
        let error = foreground_orchestration::orchestrate_telegram_foreground_plan(
            &ctx.pool,
            &config,
            &sample_model_gateway_config(),
            foreground_orchestration::TelegramForegroundPlanExecution {
                execution: foreground_orchestration::ForegroundExecutionIds {
                    trace_id,
                    execution_id,
                },
                trigger_kind_override: None,
                plan,
            },
            &transport,
            &mut delivery,
        )
        .await
        .expect_err("missing self-model config should fail orchestration");
        assert!(
            error
                .to_string()
                .contains("missing foreground self-model seed configuration")
        );

        for ingress_id in [first.ingress_id, second.ingress_id] {
            let stored = foreground::get_ingress_event(&ctx.pool, ingress_id).await?;
            assert_eq!(stored.foreground_status, "processed");
            assert_eq!(stored.execution_id, Some(execution_id));
        }
        assert_eq!(delivery.sent_messages().len(), 1);
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
async fn runtime_fixture_resumes_stale_processing_backlog_without_new_accepted_updates()
-> Result<()> {
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

        let now = Utc::now();
        let resumed = insert_pending_ingress(
            &ctx.pool,
            "telegram-primary",
            "primary-user",
            now - Duration::minutes(12),
            "first interrupted message",
        )
        .await?;
        let latest = insert_pending_ingress(
            &ctx.pool,
            "telegram-primary",
            "primary-user",
            now - Duration::minutes(1),
            "latest backlog message",
        )
        .await?;

        sqlx::query(
            r#"
            UPDATE ingress_events
            SET
                foreground_status = 'processing',
                last_processed_at = $2
            WHERE ingress_id = $1
            "#,
        )
        .bind(resumed.ingress_id)
        .bind(now - Duration::minutes(15))
        .execute(&ctx.pool)
        .await?;

        let transport = model_gateway::FakeModelProviderTransport::new();
        transport.push_response(Ok(model_gateway::ProviderHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{
                    "message": { "content": "assistant reply after resumed backlog" },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 15,
                    "completion_tokens": 6
                }
            }),
        }));
        let mut delivery = telegram::FakeTelegramDelivery::default();

        let summary = runtime::run_telegram_fixture_with(
            &ctx.pool,
            &config,
            &sample_telegram_config(),
            &sample_model_gateway_config(),
            &telegram_fixture("unsupported_update.json"),
            &transport,
            &mut delivery,
        )
        .await?;

        assert_eq!(summary.ignored_count, 1);
        assert_eq!(summary.completed_count, 1);
        assert_eq!(summary.backlog_recovery_count, 1);
        assert_eq!(delivery.sent_messages().len(), 1);
        assert_eq!(
            delivery.sent_messages()[0].text,
            "assistant reply after resumed backlog"
        );

        let resumed_stored = foreground::get_ingress_event(&ctx.pool, resumed.ingress_id).await?;
        let latest_stored = foreground::get_ingress_event(&ctx.pool, latest.ingress_id).await?;
        assert_eq!(resumed_stored.foreground_status, "processed");
        assert_eq!(latest_stored.foreground_status, "processed");
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
                .contains("missing Telegram foreground configuration")
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn scheduled_foreground_runtime_executes_due_task_and_updates_state() -> Result<()> {
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
        foreground::upsert_conversation_binding(
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

        scheduled_foreground::upsert_task(
            &ctx.pool,
            &ctx.config,
            &scheduled_foreground::UpsertScheduledForegroundTask {
                task_key: "daily-checkin".to_string(),
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
                message_text: "Scheduled daily check-in".to_string(),
                cadence_seconds: 600,
                cooldown_seconds: Some(120),
                next_due_at: Some(Utc::now() - Duration::seconds(5)),
                status: contracts::ScheduledForegroundTaskStatus::Active,
                actor_ref: "test-harness".to_string(),
            },
        )
        .await?;
        assert!(config.scheduled_foreground.enabled);
        let preflight = scheduled_foreground::get_task_by_key(&ctx.pool, "daily-checkin")
            .await?
            .expect("scheduled task should exist before runtime iteration");
        assert!(preflight.next_due_at <= Utc::now());
        let due_tasks = scheduled_foreground::list_tasks(
            &ctx.pool,
            scheduled_foreground::ScheduledForegroundTaskListFilter {
                status: Some(contracts::ScheduledForegroundTaskStatus::Active),
                due_only: true,
                limit: 10,
            },
        )
        .await?;
        assert_eq!(due_tasks.len(), 1);

        let transport = model_gateway::FakeModelProviderTransport::new();
        transport.push_response(Ok(model_gateway::ProviderHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{
                    "message": { "content": "scheduled proactive reply" },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 10,
                    "completion_tokens": 5
                }
            }),
        }));
        let mut delivery = telegram::FakeTelegramDelivery::default();

        let handled = runtime::run_scheduled_foreground_iteration_with(
            &ctx.pool,
            &config,
            &sample_model_gateway_config(),
            &transport,
            &mut delivery,
        )
        .await?;

        let task = scheduled_foreground::get_task_by_key(&ctx.pool, "daily-checkin")
            .await?
            .expect("scheduled task should still exist");
        assert_eq!(handled, 1);
        assert_eq!(delivery.sent_messages().len(), 1);
        assert_eq!(delivery.sent_messages()[0].chat_id, 42);
        assert_eq!(
            delivery.sent_messages()[0].text,
            "scheduled proactive reply"
        );
        assert_eq!(delivery.sent_messages()[0].reply_to_message_id, None);
        assert_eq!(task.current_execution_id, None);
        assert_eq!(
            task.last_outcome,
            Some(contracts::ScheduledForegroundLastOutcome::Completed)
        );
        assert!(task.last_execution_id.is_some());
        assert!(task.last_run_started_at.is_some());
        assert!(task.last_run_completed_at.is_some());
        assert!(task.next_due_at > Utc::now());

        let ingress_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM ingress_events
            WHERE raw_payload_ref = $1
            "#,
        )
        .bind(format!(
            "scheduled_foreground_task:{}",
            task.scheduled_foreground_task_id
        ))
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(ingress_count, 1);

        let completed_audit_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM audit_events
            WHERE event_kind = 'scheduled_foreground_task_completed'
            "#,
        )
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(completed_audit_count, 1);
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn scheduled_foreground_runtime_executes_one_shot_task_and_disables_after_success()
-> Result<()> {
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
        foreground::upsert_conversation_binding(
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

        scheduled_foreground::upsert_task(
            &ctx.pool,
            &config,
            &scheduled_foreground::UpsertScheduledForegroundTask {
                task_key: "oneoff_success_20260507".to_string(),
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
                message_text: "One-shot scheduled check-in".to_string(),
                cadence_seconds: 0,
                cooldown_seconds: Some(120),
                next_due_at: Some(Utc::now() - Duration::seconds(5)),
                status: contracts::ScheduledForegroundTaskStatus::Active,
                actor_ref: "test-harness".to_string(),
            },
        )
        .await?;

        let transport = model_gateway::FakeModelProviderTransport::new();
        transport.push_response(Ok(model_gateway::ProviderHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{
                    "message": { "content": "one-shot proactive reply" },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 10,
                    "completion_tokens": 5
                }
            }),
        }));
        let mut delivery = telegram::FakeTelegramDelivery::default();

        let handled = runtime::run_scheduled_foreground_iteration_with(
            &ctx.pool,
            &config,
            &sample_model_gateway_config(),
            &transport,
            &mut delivery,
        )
        .await?;

        let task = scheduled_foreground::get_task_by_key(&ctx.pool, "oneoff_success_20260507")
            .await?
            .expect("one-shot scheduled task should still be traceable");
        assert_eq!(handled, 1);
        assert_eq!(delivery.sent_messages().len(), 1);
        assert_eq!(delivery.sent_messages()[0].text, "one-shot proactive reply");
        assert_eq!(
            task.status,
            contracts::ScheduledForegroundTaskStatus::Disabled
        );
        assert_eq!(task.current_execution_id, None);
        assert_eq!(
            task.last_outcome,
            Some(contracts::ScheduledForegroundLastOutcome::Completed)
        );
        assert!(task.last_execution_id.is_some());

        let due_tasks = scheduled_foreground::list_tasks(
            &ctx.pool,
            scheduled_foreground::ScheduledForegroundTaskListFilter {
                status: Some(contracts::ScheduledForegroundTaskStatus::Active),
                due_only: true,
                limit: 10,
            },
        )
        .await?;
        assert!(due_tasks.is_empty());
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn scheduled_foreground_runtime_suppresses_when_binding_disappears() -> Result<()> {
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

        scheduled_foreground::upsert_task(
            &ctx.pool,
            &ctx.config,
            &scheduled_foreground::UpsertScheduledForegroundTask {
                task_key: "missing-binding".to_string(),
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
                message_text: "Scheduled message without binding".to_string(),
                cadence_seconds: 600,
                cooldown_seconds: Some(180),
                next_due_at: Some(Utc::now() - Duration::seconds(5)),
                status: contracts::ScheduledForegroundTaskStatus::Active,
                actor_ref: "test-harness".to_string(),
            },
        )
        .await?;

        sqlx::query(
            r#"
            DELETE FROM conversation_bindings
            WHERE conversation_binding_id = $1
            "#,
        )
        .bind(binding.conversation_binding_id)
        .execute(&ctx.pool)
        .await?;

        let transport = model_gateway::FakeModelProviderTransport::new();
        let mut delivery = telegram::FakeTelegramDelivery::default();
        let handled = runtime::run_scheduled_foreground_iteration_with(
            &ctx.pool,
            &ctx.config,
            &sample_model_gateway_config(),
            &transport,
            &mut delivery,
        )
        .await?;

        assert_eq!(handled, 1);
        assert!(delivery.sent_messages().is_empty());

        let task = scheduled_foreground::get_task_by_key(&ctx.pool, "missing-binding")
            .await?
            .expect("scheduled task should still exist");
        assert_eq!(task.current_execution_id, None);
        assert_eq!(
            task.last_outcome,
            Some(contracts::ScheduledForegroundLastOutcome::Suppressed)
        );
        assert_eq!(
            task.last_outcome_reason.as_deref(),
            Some("conversation_binding_missing")
        );
        assert!(task.last_execution_id.is_some());
        assert!(task.next_due_at > Utc::now());

        let execution = execution::get(
            &ctx.pool,
            task.last_execution_id
                .expect("suppressed task should record an execution"),
        )
        .await?;
        assert_eq!(execution.status, "completed");

        let ingress_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM ingress_events
            WHERE raw_payload_ref = $1
            "#,
        )
        .bind(format!(
            "scheduled_foreground_task:{}",
            task.scheduled_foreground_task_id
        ))
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(ingress_count, 0);

        let suppressed_audit_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM audit_events
            WHERE event_kind = 'scheduled_foreground_task_suppressed'
            "#,
        )
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(suppressed_audit_count, 1);
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn scheduled_foreground_one_shot_failure_disables_task_and_stops_retry() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        foreground::upsert_conversation_binding(
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

        scheduled_foreground::upsert_task(
            &ctx.pool,
            &ctx.config,
            &scheduled_foreground::UpsertScheduledForegroundTask {
                task_key: "oneoff_test_20260507".to_string(),
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
                message_text: "One-shot scheduled message".to_string(),
                cadence_seconds: 0,
                cooldown_seconds: Some(120),
                next_due_at: Some(Utc::now() - Duration::seconds(5)),
                status: contracts::ScheduledForegroundTaskStatus::Active,
                actor_ref: "test-harness".to_string(),
            },
        )
        .await?;

        let execution_id = Uuid::now_v7();
        let claimed = scheduled_foreground::claim_next_due_task(
            &ctx.pool,
            execution_id,
            Uuid::now_v7(),
            Utc::now(),
        )
        .await?
        .expect("one-shot task should be claimable");
        let completed_at = Utc::now();
        let task = scheduled_foreground::mark_task_failed(
            &ctx.pool,
            &claimed.task,
            execution_id,
            completed_at,
            "execution_failed",
            "worker protocol failure",
        )
        .await?;

        assert_eq!(
            task.status,
            contracts::ScheduledForegroundTaskStatus::Disabled
        );
        assert_eq!(task.current_execution_id, None);
        assert_eq!(
            task.last_outcome,
            Some(contracts::ScheduledForegroundLastOutcome::Failed)
        );

        let due_tasks = scheduled_foreground::list_tasks(
            &ctx.pool,
            scheduled_foreground::ScheduledForegroundTaskListFilter {
                status: Some(contracts::ScheduledForegroundTaskStatus::Active),
                due_only: true,
                limit: 10,
            },
        )
        .await?;
        assert!(due_tasks.is_empty());
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn scheduled_foreground_recovery_clears_stranded_in_progress_task() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        foreground::upsert_conversation_binding(
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

        scheduled_foreground::upsert_task(
            &ctx.pool,
            &ctx.config,
            &scheduled_foreground::UpsertScheduledForegroundTask {
                task_key: "stranded-task".to_string(),
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
                message_text: "Scheduled task that will strand".to_string(),
                cadence_seconds: 600,
                cooldown_seconds: Some(120),
                next_due_at: Some(Utc::now() - Duration::seconds(5)),
                status: contracts::ScheduledForegroundTaskStatus::Active,
                actor_ref: "test-harness".to_string(),
            },
        )
        .await?;

        let execution_id = Uuid::now_v7();
        let trace_id = Uuid::now_v7();
        let claimed_at = Utc::now();
        let claimed = scheduled_foreground::claim_next_due_task(
            &ctx.pool,
            execution_id,
            trace_id,
            claimed_at,
        )
        .await?
        .expect("scheduled task should be claimable");
        assert!(claimed.ingress.is_some());
        let scheduled_causal_link_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM causal_links
            WHERE trace_id = $1
              AND source_kind = 'scheduled_foreground_task'
              AND source_id = $2
              AND target_kind = 'execution_record'
              AND target_id = $3
              AND edge_kind = 'triggered_execution'
            "#,
        )
        .bind(trace_id)
        .bind(claimed.task.scheduled_foreground_task_id)
        .bind(execution_id)
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(scheduled_causal_link_count, 1);
        sqlx::query(
            r#"
            UPDATE scheduled_foreground_tasks
            SET current_run_started_at = $2
            WHERE scheduled_foreground_task_id = $1
            "#,
        )
        .bind(claimed.task.scheduled_foreground_task_id)
        .bind(Utc::now() - Duration::minutes(2))
        .execute(&ctx.pool)
        .await?;

        let recovered =
            runtime::recover_interrupted_scheduled_foreground_tasks(&ctx.pool, &ctx.config, 10)
                .await?;
        assert_eq!(recovered, 1);

        let task = scheduled_foreground::get_task_by_key(&ctx.pool, "stranded-task")
            .await?
            .expect("scheduled task should still exist");
        assert_eq!(task.current_execution_id, None);
        assert_eq!(
            task.last_outcome,
            Some(contracts::ScheduledForegroundLastOutcome::Failed)
        );
        assert_eq!(
            task.last_outcome_reason.as_deref(),
            Some("supervisor_restart_recovery")
        );

        let execution = execution::get(&ctx.pool, execution_id).await?;
        assert_eq!(execution.status, "failed");

        let checkpoint_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM recovery_checkpoints
            WHERE execution_id = $1
            "#,
        )
        .bind(execution_id)
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(checkpoint_count, 1);

        let recovered_audit_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM audit_events
            WHERE event_kind = 'scheduled_foreground_task_recovered_failed'
            "#,
        )
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(recovered_audit_count, 1);
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
            poll_limit: 10,
            foreground_binding: Some(harness::config::TelegramForegroundBindingConfig {
                allowed_user_id: 42,
                allowed_chat_id: 42,
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
            }),
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
            identity: None,
            identity_lifecycle: Default::default(),
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
        retrieved_context: contracts::RetrievedContext::default(),
        governed_action_observations: Vec::new(),
        governed_action_loop_state: None,
        recovery_context: contracts::ForegroundRecoveryContext::default(),
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
            trigger_kind: format!("foreground-context-{suffix}"),
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

async fn insert_active_memory_artifact(
    pool: &sqlx::PgPool,
    subject_ref: &str,
    content_text: &str,
) -> Result<()> {
    let execution_id = Uuid::now_v7();
    let trace_id = Uuid::now_v7();
    execution::insert(
        pool,
        &execution::NewExecutionRecord {
            execution_id,
            trace_id,
            trigger_kind: "memory-retrieval-test".to_string(),
            synthetic_trigger: None,
            status: "started".to_string(),
            request_payload: serde_json::json!({ "kind": "memory_retrieval_test" }),
        },
    )
    .await?;

    let ingress = contracts::NormalizedIngress {
        ingress_id: Uuid::now_v7(),
        channel_kind: ChannelKind::Telegram,
        external_user_id: "42".to_string(),
        external_conversation_id: "42".to_string(),
        external_event_id: format!("memory-{}", Uuid::now_v7()),
        external_message_id: Some("memory-message".to_string()),
        internal_principal_ref: "primary-user".to_string(),
        internal_conversation_ref: "telegram-primary".to_string(),
        event_kind: contracts::IngressEventKind::MessageCreated,
        occurred_at: Utc::now() - Duration::minutes(20),
        text_body: Some("remember travel preference".to_string()),
        reply_to: None,
        attachments: Vec::new(),
        command_hint: None,
        approval_payload: None,
        raw_payload_ref: Some("memory-retrieval-test".to_string()),
    };
    foreground::insert_ingress_event(
        pool,
        &foreground::NewIngressEvent {
            ingress: ingress.clone(),
            conversation_binding_id: None,
            trace_id,
            execution_id: Some(execution_id),
            status: "accepted".to_string(),
            rejection_reason: None,
        },
    )
    .await?;

    let proposal_id = Uuid::now_v7();
    continuity::insert_proposal(
        pool,
        &continuity::NewProposalRecord {
            proposal_id,
            trace_id,
            execution_id,
            episode_id: None,
            source_ingress_id: Some(ingress.ingress_id),
            source_loop_kind: "conscious".to_string(),
            proposal_kind: "memory_artifact".to_string(),
            canonical_target: "memory_artifacts".to_string(),
            status: "accepted".to_string(),
            confidence: 0.9,
            conflict_posture: "independent".to_string(),
            subject_ref: subject_ref.to_string(),
            content_text: content_text.to_string(),
            rationale: Some("retrieval context test".to_string()),
            valid_from: Some(ingress.occurred_at),
            valid_to: None,
            supersedes_artifact_id: None,
            supersedes_artifact_kind: None,
            payload: serde_json::json!({ "artifact_kind": "preference" }),
        },
    )
    .await?;

    continuity::insert_memory_artifact(
        pool,
        &continuity::NewMemoryArtifact {
            memory_artifact_id: Uuid::now_v7(),
            proposal_id,
            trace_id,
            execution_id,
            episode_id: None,
            source_ingress_id: Some(ingress.ingress_id),
            artifact_kind: "preference".to_string(),
            subject_ref: subject_ref.to_string(),
            content_text: content_text.to_string(),
            confidence: 0.9,
            provenance_kind: "episode_observation".to_string(),
            status: "active".to_string(),
            valid_from: Some(ingress.occurred_at),
            valid_to: None,
            superseded_at: None,
            superseded_by_artifact_id: None,
            supersedes_artifact_id: None,
            payload: serde_json::json!({}),
        },
    )
    .await?;

    Ok(())
}

fn sample_capability_scope() -> CapabilityScope {
    CapabilityScope {
        filesystem: FilesystemCapabilityScope {
            read_roots: vec![support::workspace_root().display().to_string()],
            write_roots: Vec::new(),
        },
        network: NetworkAccessPosture::Disabled,
        environment: EnvironmentCapabilityScope {
            allow_variables: Vec::new(),
        },
        execution: ExecutionCapabilityBudget {
            timeout_ms: 30_000,
            max_stdout_bytes: 65_536,
            max_stderr_bytes: 32_768,
        },
    }
}

async fn insert_pending_ingress(
    pool: &sqlx::PgPool,
    internal_conversation_ref: &str,
    internal_principal_ref: &str,
    occurred_at: chrono::DateTime<Utc>,
    text_body: &str,
) -> Result<contracts::NormalizedIngress> {
    let ingress = contracts::NormalizedIngress {
        ingress_id: Uuid::now_v7(),
        channel_kind: ChannelKind::Telegram,
        external_user_id: "42".to_string(),
        external_conversation_id: "42".to_string(),
        external_event_id: format!("pending-{}", Uuid::now_v7()),
        external_message_id: Some(occurred_at.timestamp_millis().to_string()),
        internal_principal_ref: internal_principal_ref.to_string(),
        internal_conversation_ref: internal_conversation_ref.to_string(),
        event_kind: contracts::IngressEventKind::MessageCreated,
        occurred_at,
        text_body: Some(text_body.to_string()),
        reply_to: None,
        attachments: Vec::new(),
        command_hint: None,
        approval_payload: None,
        raw_payload_ref: Some("pending-ingress-test".to_string()),
    };
    foreground::insert_ingress_event(
        pool,
        &foreground::NewIngressEvent {
            ingress: ingress.clone(),
            conversation_binding_id: None,
            trace_id: Uuid::now_v7(),
            execution_id: None,
            status: "accepted".to_string(),
            rejection_reason: None,
        },
    )
    .await?;

    Ok(ingress)
}

async fn wait_for_binding_reconcile_lock(database_url: &str) -> Result<()> {
    for _ in 0..50 {
        let mut connection = PgConnection::connect(database_url).await?;
        let waiting_sessions: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM pg_stat_activity
            WHERE state = 'active'
              AND wait_event_type = 'Lock'
              AND query LIKE '%FROM conversation_bindings%'
              AND query LIKE '%FOR UPDATE%'
            "#,
        )
        .fetch_one(&mut connection)
        .await?;
        if waiting_sessions > 0 {
            return Ok(());
        }
        sleep(TokioDuration::from_millis(20)).await;
    }

    anyhow::bail!("foreground intake never blocked on the expected binding lock")
}
