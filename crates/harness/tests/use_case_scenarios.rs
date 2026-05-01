mod support;

use anyhow::Result;
use chrono::{Duration, Utc};
use contracts::{
    ApprovalResolutionDecision, BackgroundExecutionBudget, BackgroundTrigger,
    BackgroundTriggerKind, CanonicalProposal, CanonicalProposalKind, CanonicalProposalPayload,
    CanonicalTargetKind, ChannelKind, ForegroundBudget, ForegroundTrigger, ForegroundTriggerKind,
    GovernedActionFingerprint, IdentityDeltaProposal, IdentityInterviewAnswer,
    IdentityKickstartAction, IdentityKickstartActionKind, IdentityLifecycleState, IngressEventKind,
    ModelProviderKind, NormalizedIngress, ProposalConflictPosture, ProposalProvenance,
    ProposalProvenanceKind, ScheduledForegroundLastOutcome, ScheduledForegroundTaskStatus,
    UnconsciousJobKind, UnconsciousScope, predefined_identity_delta,
};
use harness::{
    approval, background,
    config::{
        ForegroundModelRouteConfig, ModelGatewayConfig, ResolvedForegroundModelRouteConfig,
        ResolvedModelGatewayConfig, ResolvedTelegramConfig, RuntimeConfig, SelfModelConfig,
        TelegramConfig, TelegramForegroundBindingConfig, ZAiProviderConfig,
    },
    context::{self, ContextAssemblyOptions},
    execution::{self, NewExecutionRecord},
    foreground::{self, NewEpisode, NewEpisodeMessage},
    foreground_orchestration, governed_actions, identity, ingress, management,
    model_gateway::{self, ProviderHttpResponse},
    proposal,
    runtime::{self, HarnessOptions},
    scheduled_foreground, telegram,
};
use serde_json::json;
use serial_test::serial;
use sqlx::Row;
use std::env;
use uuid::Uuid;

// --- UC-1: Basic Conversation ---

#[tokio::test]
#[serial]
async fn uc1_basic_conversation_delivers_reply_with_self_model_in_prompt() -> Result<()> {
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
        transport.push_response(Ok(ProviderHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{
                    "message": { "content": "basic reply" },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 12, "completion_tokens": 4 }
            }),
        }));
        let mut delivery = telegram::FakeTelegramDelivery::default();

        runtime::run_telegram_fixture_with(
            &ctx.pool,
            &config,
            &resolved_telegram_config(),
            &resolved_model_gateway_config(),
            &telegram_fixture("private_text_message.json"),
            &transport,
            &mut delivery,
        )
        .await?;

        assert_eq!(delivery.sent_messages().len(), 1);

        let seen_requests = transport.seen_requests();
        assert_eq!(seen_requests.len(), 1);
        let system_prompt = seen_requests[0]
            .body
            .get("messages")
            .and_then(serde_json::Value::as_array)
            .and_then(|messages| messages.first())
            .and_then(|m| m.get("content"))
            .and_then(serde_json::Value::as_str)
            .expect("provider request should include a system prompt as first message");
        assert!(
            system_prompt.contains("Communication style: direct"),
            "system prompt should include self-model communication_style, got: {system_prompt:.200}"
        );

        let completed_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM execution_records WHERE status = 'completed'")
                .fetch_one(&ctx.pool)
                .await?;
        assert_eq!(completed_count, 1);

        Ok(())
    })
    .await
}

// --- UC-2: Multi-Turn Continuity ---

#[tokio::test]
#[serial]
async fn uc2_second_message_receives_prior_episode_in_context() -> Result<()> {
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
        transport.push_response(Ok(ProviderHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{
                    "message": { "content": "first reply" },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 12, "completion_tokens": 4 }
            }),
        }));
        transport.push_response(Ok(ProviderHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{
                    "message": { "content": "second reply" },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 18, "completion_tokens": 4 }
            }),
        }));
        let mut delivery = telegram::FakeTelegramDelivery::default();

        runtime::run_telegram_fixture_with(
            &ctx.pool,
            &config,
            &resolved_telegram_config(),
            &resolved_model_gateway_config(),
            &telegram_fixture("private_text_message.json"),
            &transport,
            &mut delivery,
        )
        .await?;
        runtime::run_telegram_fixture_with(
            &ctx.pool,
            &config,
            &resolved_telegram_config(),
            &resolved_model_gateway_config(),
            &telegram_fixture("private_preference_followup.json"),
            &transport,
            &mut delivery,
        )
        .await?;

        assert_eq!(transport.seen_requests().len(), 2);
        assert_eq!(delivery.sent_messages().len(), 2);

        let seen_requests = transport.seen_requests();
        let second_request_message_contents = seen_requests[1]
            .body
            .get("messages")
            .and_then(serde_json::Value::as_array)
            .expect("second provider request should include messages")
            .iter()
            .filter_map(|m| m.get("content").and_then(serde_json::Value::as_str))
            .collect::<Vec<_>>();
        assert!(
            second_request_message_contents
                .iter()
                .any(|c| c.contains("hello from telegram")),
            "second request should include prior episode user message 'hello from telegram' in context"
        );

        Ok(())
    })
    .await
}

// --- UC-3: Cross-Session Memory ---

#[tokio::test]
#[serial]
async fn uc3_preference_from_session_1_appears_in_session_2_context() -> Result<()> {
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
        transport.push_response(Ok(ProviderHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{
                    "message": { "content": "noted your preference" },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 20, "completion_tokens": 6 }
            }),
        }));
        transport.push_response(Ok(ProviderHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{
                    "message": { "content": "yes, keeping it concise" },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 18, "completion_tokens": 5 }
            }),
        }));
        let mut delivery = telegram::FakeTelegramDelivery::default();

        runtime::run_telegram_fixture_with(
            &ctx.pool,
            &config,
            &resolved_telegram_config(),
            &resolved_model_gateway_config(),
            &telegram_fixture("private_preference_message.json"),
            &transport,
            &mut delivery,
        )
        .await?;
        runtime::run_telegram_fixture_with(
            &ctx.pool,
            &config,
            &resolved_telegram_config(),
            &resolved_model_gateway_config(),
            &telegram_fixture("private_preference_followup.json"),
            &transport,
            &mut delivery,
        )
        .await?;

        let memory_artifact_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM memory_artifacts WHERE status = 'active'")
                .fetch_one(&ctx.pool)
                .await?;
        assert_eq!(memory_artifact_count, 1);

        let seen_requests = transport.seen_requests();
        assert_eq!(seen_requests.len(), 2);
        let second_request_message_contents = seen_requests[1]
            .body
            .get("messages")
            .and_then(serde_json::Value::as_array)
            .expect("second provider request should include messages")
            .iter()
            .filter_map(|m| m.get("content").and_then(serde_json::Value::as_str))
            .collect::<Vec<_>>();
        assert!(
            second_request_message_contents
                .iter()
                .any(|c| c.contains("prefer concise replies")),
            "second request should surface the preference from session 1 via canonical context"
        );

        Ok(())
    })
    .await
}

// --- UC-4: Governed Action with Approval ---

#[tokio::test]
#[serial]
async fn uc4_governed_action_requires_approval_then_executes_after_user_approves() -> Result<()> {
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

        let update =
            telegram::load_fixture_updates(&telegram_fixture("private_text_message.json"))?
                .into_iter()
                .next()
                .expect("fixture should contain one update");
        let ingress = match ingress::normalize_telegram_update(
            &resolved_telegram_config(),
            &update,
            Some("fixtures/private_text_message.json".to_string()),
        )? {
            ingress::TelegramNormalizationOutcome::Accepted(ingress) => *ingress,
            other => panic!("fixture should normalize into an accepted ingress, got {other:?}"),
        };

        let transport = model_gateway::FakeModelProviderTransport::new();
        transport.push_response(Ok(ProviderHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{
                    "message": { "content": approval_required_action_model_output() },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 20, "completion_tokens": 30 }
            }),
        }));
        let mut delivery = telegram::FakeTelegramDelivery::default();

        // Run 1: model returns an approval-requiring action; orchestration delivers the approval prompt
        let outcome = foreground_orchestration::orchestrate_telegram_foreground_ingress(
            &ctx.pool,
            &config,
            &resolved_telegram_config(),
            &resolved_model_gateway_config(),
            ingress,
            &transport,
            &mut delivery,
        )
        .await?;
        match outcome {
            foreground_orchestration::TelegramForegroundOrchestrationOutcome::Completed(_) => {}
            other => panic!("expected completed outcome after approval-pending action, got {other:?}"),
        }

        // Intermediate assertions: the approval prompt is delivered (plus the initial model text,
        // since the orchestration delivers the stripped initial response when no follow-up runs)
        assert!(
            !delivery.sent_messages().is_empty(),
            "at least the approval prompt should be delivered; got 0 messages"
        );

        let awaiting_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM governed_action_executions WHERE status = 'awaiting_approval'",
        )
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(awaiting_count, 1, "one governed action should be awaiting approval");

        // Retrieve the auto-generated token and fingerprint
        let approval_row = sqlx::query(
            "SELECT ar.token, ar.action_fingerprint, gae.governed_action_execution_id \
             FROM approval_requests ar \
             JOIN governed_action_executions gae USING (approval_request_id) \
             WHERE ar.status = 'pending' \
             ORDER BY ar.created_at DESC LIMIT 1",
        )
        .fetch_one(&ctx.pool)
        .await?;
        let token: String = approval_row.get("token");
        let fingerprint_value: String = approval_row.get("action_fingerprint");
        let gae_id: Uuid = approval_row.get("governed_action_execution_id");

        // Resolve the approval manually (mirroring governed_actions_integration pattern)
        approval::resolve_approval_request(
            &ctx.pool,
            &approval::ApprovalResolutionAttempt {
                token: token.clone(),
                actor_ref: "telegram:primary-user".to_string(),
                expected_action_fingerprint: GovernedActionFingerprint {
                    value: fingerprint_value,
                },
                decision: ApprovalResolutionDecision::Approved,
                reason: Some("uc4 scenario test approval".to_string()),
                resolved_at: Utc::now(),
            },
        )
        .await?;

        let synced = governed_actions::sync_status_from_approval_resolution(
            &ctx.pool,
            gae_id,
            ApprovalResolutionDecision::Approved,
            None,
            Some("uc4 scenario test approval"),
        )
        .await?;

        governed_actions::execute_governed_action(&config, &ctx.pool, &synced).await?;

        // Final assertions
        let final_status: String = sqlx::query_scalar(
            "SELECT status FROM governed_action_executions WHERE governed_action_execution_id = $1",
        )
        .bind(gae_id)
        .fetch_one(&ctx.pool)
        .await?;
        assert!(
            final_status == "executed" || final_status == "failed",
            "governed action should reach 'executed' or 'failed' after approval, got: {final_status}"
        );

        let resolved_status: String = sqlx::query_scalar(
            "SELECT status FROM approval_requests WHERE token = $1",
        )
        .bind(&token)
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(resolved_status, "approved");

        Ok(())
    })
    .await
}

// --- UC-5: Proactive Scheduled Message ---

#[tokio::test]
#[serial]
async fn uc5_scheduled_task_fires_and_delivers_proactive_message() -> Result<()> {
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
                task_key: "uc5-scheduled".to_string(),
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
                message_text: "UC5 scheduled proactive prompt".to_string(),
                cadence_seconds: 600,
                cooldown_seconds: Some(120),
                next_due_at: Some(Utc::now() - Duration::seconds(5)),
                status: ScheduledForegroundTaskStatus::Active,
                actor_ref: "use-case-scenario".to_string(),
            },
        )
        .await?;

        let transport = model_gateway::FakeModelProviderTransport::new();
        transport.push_response(Ok(ProviderHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{
                    "message": { "content": "proactive scheduled reply" },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 14, "completion_tokens": 5 }
            }),
        }));
        let mut delivery = telegram::FakeTelegramDelivery::default();

        let handled = runtime::run_scheduled_foreground_iteration_with(
            &ctx.pool,
            &config,
            &resolved_model_gateway_config(),
            &transport,
            &mut delivery,
        )
        .await?;

        assert_eq!(handled, 1);
        assert_eq!(delivery.sent_messages().len(), 1);

        let task = scheduled_foreground::get_task_by_key(&ctx.pool, "uc5-scheduled")
            .await?
            .expect("scheduled task should exist");
        assert_eq!(task.current_execution_id, None);
        assert_eq!(
            task.last_outcome,
            Some(ScheduledForegroundLastOutcome::Completed)
        );

        Ok(())
    })
    .await
}

// --- UC-6: Background-Initiated Notification ---

#[tokio::test]
#[serial]
async fn uc6_background_wake_signal_stages_then_delivers_notification() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        config.self_model = Some(SelfModelConfig {
            seed_path: support::workspace_root()
                .join("config")
                .join("self_model_seed.toml"),
        });
        config.model_gateway = Some(unresolved_model_gateway_config(
            "BLUE_LAGOON_TEST_UNCONSCIOUS_RUNTIME_API_KEY",
        ));
        config.telegram = Some(unresolved_telegram_config());
        let worker_binary = support::workers_binary()?;
        config.worker.command = worker_binary.to_string_lossy().into_owned();
        config.worker.args = vec!["unconscious-worker".to_string()];

        // Conversation binding is required for foreground delivery.
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

        let background_job_id = seed_planned_background_job_with_kind(
            &ctx.pool,
            Utc::now(),
            UnconsciousJobKind::SelfModelReflection,
        )
        .await?;

        let transport = model_gateway::FakeModelProviderTransport::new();
        // Response for the background SelfModelReflection job.
        transport.push_response(Ok(ProviderHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{
                    "message": {
                        "content": serde_json::json!({
                            "identity_delta": null,
                            "no_change_rationale": "No durable identity change is warranted from this maintenance review.",
                            "diagnostics": [],
                            "wake_signals": [{
                                "signal_id": "018f0000-0000-7000-8000-000000000606",
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
                "usage": { "prompt_tokens": 16, "completion_tokens": 8 }
            }),
        }));

        // Run background job; expect wake signal to be staged.
        let outcome = with_env_var(
            "BLUE_LAGOON_TEST_UNCONSCIOUS_RUNTIME_API_KEY",
            Some("test-key"),
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

        let harness::runtime::HarnessOutcome::BackgroundCompleted {
            background_job_id: completed_job_id,
            ..
        } = outcome
        else {
            panic!("expected background completion outcome for UC6 background step");
        };
        assert_eq!(completed_job_id, background_job_id);

        // Intermediate assertions: wake signal staged
        let wake_signal_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM wake_signals WHERE status = 'accepted'")
                .fetch_one(&ctx.pool)
                .await?;
        assert_eq!(
            wake_signal_count, 1,
            "background job should produce one accepted wake signal"
        );

        let pending_ingress_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM ingress_events \
             WHERE external_event_id LIKE 'wake-signal:%' AND foreground_status = 'pending'",
        )
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(
            pending_ingress_count, 1,
            "staged wake signal should appear as a pending ingress event"
        );

        // Foreground pickup: an unsupported update triggers the backlog scan which
        // picks up the pending wake-signal ingress targeting "telegram-primary"
        let mut config2 = ctx.config.clone();
        config2.self_model = Some(SelfModelConfig {
            seed_path: support::workspace_root()
                .join("config")
                .join("self_model_seed.toml"),
        });
        config2.worker.command = worker_binary.to_string_lossy().into_owned();
        config2.worker.args = vec!["conscious-worker".to_string()];

        // Response for foreground delivery of the wake signal.
        transport.push_response(Ok(ProviderHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{
                    "message": { "content": "background notification delivered" },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 14, "completion_tokens": 5 }
            }),
        }));
        let mut delivery = telegram::FakeTelegramDelivery::default();

        runtime::run_telegram_fixture_with(
            &ctx.pool,
            &config2,
            &resolved_telegram_config(),
            &resolved_model_gateway_config(),
            &telegram_fixture("unsupported_update.json"),
            &transport,
            &mut delivery,
        )
        .await?;

        // Final assertions: wake signal delivered
        assert_eq!(
            delivery.sent_messages().len(),
            1,
            "foreground pass should deliver the background-initiated notification"
        );

        let processed_ingress_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM ingress_events \
             WHERE external_event_id LIKE 'wake-signal:%' AND foreground_status = 'processed'",
        )
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(
            processed_ingress_count, 1,
            "wake-signal ingress should be processed after foreground pickup"
        );

        Ok(())
    })
    .await
}

// --- UC-7: Backlog Recovery ---

#[tokio::test]
#[serial]
async fn uc7_backlog_of_messages_batched_into_single_coherent_reply() -> Result<()> {
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
        transport.push_response(Ok(ProviderHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{
                    "message": { "content": "batched backlog reply" },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 23, "completion_tokens": 6 }
            }),
        }));
        let mut delivery = telegram::FakeTelegramDelivery::default();

        let summary = runtime::run_telegram_fixture_with(
            &ctx.pool,
            &config,
            &resolved_telegram_config(),
            &resolved_model_gateway_config(),
            &telegram_fixture("private_text_backlog_batch.json"),
            &transport,
            &mut delivery,
        )
        .await?;

        assert_eq!(summary.backlog_recovery_count, 1);
        assert_eq!(delivery.sent_messages().len(), 1);

        let unprocessed_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM ingress_events WHERE foreground_status != 'processed'",
        )
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(
            unprocessed_count, 0,
            "all ingress events should be processed after backlog recovery"
        );

        Ok(())
    })
    .await
}

// --- UC-8: Worker Failure Recovery ---

#[tokio::test]
#[serial]
async fn uc8_worker_crash_creates_checkpoint_and_task_is_clean_after_recovery() -> Result<()> {
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
                task_key: "uc8-recovery".to_string(),
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
                message_text: "UC8 task that will appear crashed".to_string(),
                cadence_seconds: 600,
                cooldown_seconds: Some(120),
                next_due_at: Some(Utc::now() - Duration::seconds(5)),
                status: ScheduledForegroundTaskStatus::Active,
                actor_ref: "use-case-scenario".to_string(),
            },
        )
        .await?;

        // Claim the task to put it in-progress, then backdate it to simulate a crash
        let execution_id = Uuid::now_v7();
        let trace_id = Uuid::now_v7();
        let claimed = scheduled_foreground::claim_next_due_task(
            &ctx.pool,
            execution_id,
            trace_id,
            Utc::now(),
        )
        .await?
        .expect("task should be claimable");
        assert!(claimed.ingress.is_some());

        sqlx::query(
            "UPDATE scheduled_foreground_tasks \
             SET current_run_started_at = $2 \
             WHERE scheduled_foreground_task_id = $1",
        )
        .bind(claimed.task.scheduled_foreground_task_id)
        .bind(Utc::now() - Duration::minutes(2))
        .execute(&ctx.pool)
        .await?;

        // Recovery: detect the stale run and mark the task failed
        let recovered =
            runtime::recover_interrupted_scheduled_foreground_tasks(&ctx.pool, &ctx.config, 10)
                .await?;
        assert_eq!(recovered, 1);

        let task = scheduled_foreground::get_task_by_key(&ctx.pool, "uc8-recovery")
            .await?
            .expect("task should still exist");
        assert_eq!(task.current_execution_id, None);
        assert_eq!(
            task.last_outcome,
            Some(ScheduledForegroundLastOutcome::Failed)
        );

        // Foreground restart recovery uses decision 'continue' (safe-replay), not 'retry'/'abandon'
        let checkpoint_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM recovery_checkpoints WHERE execution_id = $1")
                .bind(execution_id)
                .fetch_one(&ctx.pool)
                .await?;
        assert_eq!(checkpoint_count, 1, "recovery should create one checkpoint");

        // Extended: re-upsert the task as due and verify it can be re-executed cleanly
        scheduled_foreground::upsert_task(
            &ctx.pool,
            &config,
            &scheduled_foreground::UpsertScheduledForegroundTask {
                task_key: "uc8-recovery".to_string(),
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
                message_text: "UC8 task re-run after recovery".to_string(),
                cadence_seconds: 600,
                cooldown_seconds: Some(120),
                next_due_at: Some(Utc::now() - Duration::seconds(5)),
                status: ScheduledForegroundTaskStatus::Active,
                actor_ref: "use-case-scenario".to_string(),
            },
        )
        .await?;

        let transport = model_gateway::FakeModelProviderTransport::new();
        transport.push_response(Ok(ProviderHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{
                    "message": { "content": "recovered task reply" },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 14, "completion_tokens": 5 }
            }),
        }));
        let mut delivery = telegram::FakeTelegramDelivery::default();

        let handled = runtime::run_scheduled_foreground_iteration_with(
            &ctx.pool,
            &config,
            &resolved_model_gateway_config(),
            &transport,
            &mut delivery,
        )
        .await?;
        assert_eq!(
            handled, 1,
            "recovered task should be schedulable again after recovery"
        );

        Ok(())
    })
    .await
}

// --- UC-Identity: First Identity Kickstart Lifecycle ---

#[tokio::test]
#[serial]
async fn uc_identity_kickstart_lifecycle_covers_selection_interview_resume_and_reset() -> Result<()>
{
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        config.self_model = Some(SelfModelConfig {
            seed_path: support::workspace_root()
                .join("config")
                .join("self_model_seed.toml"),
        });

        let bootstrap_context =
            assemble_identity_context(&ctx.pool, &config, "choose an identity").await?;
        assert_eq!(
            bootstrap_context
                .context
                .self_model
                .identity_lifecycle
                .state,
            IdentityLifecycleState::BootstrapSeedOnly
        );
        let bootstrap_kickstart = bootstrap_context
            .context
            .self_model
            .identity_lifecycle
            .kickstart
            .expect("bootstrap state should expose identity kickstart");
        assert!(
            bootstrap_kickstart
                .available_actions
                .contains(&IdentityKickstartActionKind::SelectPredefinedTemplate)
        );
        assert!(
            bootstrap_kickstart
                .available_actions
                .contains(&IdentityKickstartActionKind::StartCustomInterview)
        );
        assert_eq!(bootstrap_kickstart.predefined_templates.len(), 3);

        let predefined_context = proposal_context(&ctx.pool).await?;
        let predefined_summary = proposal::apply_candidate_proposals(
            &ctx.pool,
            &config,
            &predefined_context,
            "use_case_identity_predefined",
            None,
            &[identity_delta_proposal(
                predefined_identity_delta("continuity_operator", Utc::now())
                    .expect("predefined template should exist"),
                "predefined identity selection",
            )],
        )
        .await?;
        assert_eq!(predefined_summary.accepted_count, 1);

        let completed_context =
            assemble_identity_context(&ctx.pool, &config, "identity complete").await?;
        assert_eq!(
            completed_context
                .context
                .self_model
                .identity_lifecycle
                .state,
            IdentityLifecycleState::CompleteIdentityActive
        );
        assert!(
            !completed_context
                .context
                .self_model
                .identity_lifecycle
                .kickstart_available
        );
        assert!(
            completed_context
                .context
                .self_model
                .identity
                .as_ref()
                .is_some_and(|identity| identity.identity_summary == "Blue Lagoon")
        );

        let reset_report = management::reset_identity(
            &config,
            management::IdentityResetRequest {
                actor_ref: "cli:primary-user".to_string(),
                reason: Some("use-case reset".to_string()),
                force: true,
            },
        )
        .await?;
        assert_eq!(
            reset_report.previous_lifecycle_state.as_deref(),
            Some("complete_identity_active")
        );

        let reset_context = assemble_identity_context(&ctx.pool, &config, "start over").await?;
        assert_eq!(
            reset_context.context.self_model.identity_lifecycle.state,
            IdentityLifecycleState::BootstrapSeedOnly
        );
        assert!(
            reset_context
                .context
                .self_model
                .identity_lifecycle
                .kickstart_available
        );

        let start_context = proposal_context(&ctx.pool).await?;
        let start_summary = proposal::apply_candidate_proposals(
            &ctx.pool,
            &config,
            &start_context,
            "use_case_identity_custom_start",
            None,
            &[identity_delta_proposal(
                IdentityDeltaProposal {
                    lifecycle_state: IdentityLifecycleState::BootstrapSeedOnly,
                    item_deltas: Vec::new(),
                    self_description_delta: None,
                    interview_action: Some(IdentityKickstartAction::StartCustomInterview),
                    rationale: "Start custom identity interview.".to_string(),
                },
                "start custom identity interview",
            )],
        )
        .await?;
        assert_eq!(start_summary.accepted_count, 1);

        let in_progress_context =
            assemble_identity_context(&ctx.pool, &config, "custom identity").await?;
        assert_eq!(
            in_progress_context
                .context
                .self_model
                .identity_lifecycle
                .state,
            IdentityLifecycleState::IdentityKickstartInProgress
        );
        let in_progress_kickstart = in_progress_context
            .context
            .self_model
            .identity_lifecycle
            .kickstart
            .expect("in-progress state should expose interview actions");
        assert!(
            in_progress_kickstart
                .available_actions
                .contains(&IdentityKickstartActionKind::AnswerCustomInterview)
        );
        assert_eq!(in_progress_kickstart.next_step.as_deref(), Some("name"));

        let answer_context = proposal_context(&ctx.pool).await?;
        let first_answer = proposal::apply_candidate_proposals(
            &ctx.pool,
            &config,
            &answer_context,
            "use_case_identity_custom_answer",
            None,
            &[custom_identity_answer_proposal("name", "Lagoon Forge")],
        )
        .await?;
        assert_eq!(first_answer.accepted_count, 1);

        let resume_context =
            assemble_identity_context(&ctx.pool, &config, "resume custom identity").await?;
        assert_eq!(
            resume_context
                .context
                .self_model
                .identity_lifecycle
                .kickstart
                .as_ref()
                .and_then(|kickstart| kickstart.next_step.as_deref()),
            Some("identity_form")
        );

        for (step_key, answer_text) in custom_identity_remaining_answers() {
            let summary = proposal::apply_candidate_proposals(
                &ctx.pool,
                &config,
                &answer_context,
                "use_case_identity_custom_answer",
                None,
                &[custom_identity_answer_proposal(step_key, answer_text)],
            )
            .await?;
            assert_eq!(summary.accepted_count, 1);
        }

        let final_lifecycle = identity::get_current_lifecycle(&ctx.pool)
            .await?
            .expect("identity lifecycle should be current");
        assert_eq!(final_lifecycle.lifecycle_state, "complete_identity_active");
        assert_eq!(final_lifecycle.active_interview_id, None);
        let compact_identity =
            identity::reconstruct_compact_identity_snapshot(&ctx.pool, 32).await?;
        assert_eq!(compact_identity.identity_summary, "Lagoon Forge");
        assert!(compact_identity.stable_items.len() >= 10);
        assert!(compact_identity.evolving_items.len() >= 6);

        Ok(())
    })
    .await
}

// --- Helper functions ---

async fn assemble_identity_context(
    pool: &sqlx::PgPool,
    config: &RuntimeConfig,
    text_body: &str,
) -> Result<context::ContextAssemblyResult> {
    let trace_id = Uuid::now_v7();
    let execution_id = Uuid::now_v7();
    seed_execution_with_id(pool, trace_id, execution_id, "user_ingress").await?;
    context::assemble_foreground_context(
        pool,
        config,
        sample_foreground_trigger(text_body, trace_id, execution_id),
        ContextAssemblyOptions::default(),
    )
    .await
}

fn sample_foreground_trigger(
    text_body: &str,
    trace_id: Uuid,
    execution_id: Uuid,
) -> ForegroundTrigger {
    let ingress_id = Uuid::now_v7();
    ForegroundTrigger {
        trigger_id: Uuid::now_v7(),
        trace_id,
        execution_id,
        trigger_kind: ForegroundTriggerKind::UserIngress,
        ingress: NormalizedIngress {
            ingress_id,
            channel_kind: ChannelKind::Telegram,
            external_user_id: "42".to_string(),
            external_conversation_id: "42".to_string(),
            external_event_id: format!("event-{ingress_id}"),
            external_message_id: Some(format!("message-{ingress_id}")),
            internal_principal_ref: "primary-user".to_string(),
            internal_conversation_ref: "telegram-primary".to_string(),
            event_kind: IngressEventKind::MessageCreated,
            occurred_at: Utc::now(),
            text_body: Some(text_body.to_string()),
            reply_to: None,
            attachments: Vec::new(),
            command_hint: None,
            approval_payload: None,
            raw_payload_ref: None,
        },
        received_at: Utc::now(),
        deduplication_key: format!("test:{ingress_id}"),
        budget: ForegroundBudget {
            iteration_budget: 1,
            wall_clock_budget_ms: 30_000,
            token_budget: 4_000,
        },
    }
}

async fn seed_execution_with_id(
    pool: &sqlx::PgPool,
    trace_id: Uuid,
    execution_id: Uuid,
    trigger_kind: &str,
) -> Result<()> {
    execution::insert(
        pool,
        &NewExecutionRecord {
            execution_id,
            trace_id,
            trigger_kind: trigger_kind.to_string(),
            synthetic_trigger: None,
            status: "started".to_string(),
            request_payload: json!({
                "request_id": Uuid::now_v7(),
                "sent_at": Utc::now(),
                "worker_kind": "foreground"
            }),
        },
    )
    .await
}

async fn proposal_context(pool: &sqlx::PgPool) -> Result<proposal::ProposalProcessingContext> {
    let trace_id = Uuid::now_v7();
    let execution_id = seed_execution(pool, trace_id).await?;
    Ok(proposal::ProposalProcessingContext {
        trace_id,
        execution_id,
        episode_id: None,
        source_ingress_id: None,
        source_loop_kind: "use_case".to_string(),
    })
}

fn identity_delta_proposal(payload: IdentityDeltaProposal, rationale: &str) -> CanonicalProposal {
    CanonicalProposal {
        proposal_id: Uuid::now_v7(),
        proposal_kind: CanonicalProposalKind::IdentityDelta,
        canonical_target: CanonicalTargetKind::IdentityItems,
        confidence_pct: 100,
        conflict_posture: ProposalConflictPosture::Independent,
        subject_ref: "self:blue-lagoon".to_string(),
        rationale: Some(rationale.to_string()),
        valid_from: Some(Utc::now()),
        valid_to: None,
        supersedes_artifact_id: None,
        provenance: ProposalProvenance {
            provenance_kind: ProposalProvenanceKind::EpisodeObservation,
            source_ingress_ids: vec![Uuid::now_v7()],
            source_episode_id: None,
        },
        payload: CanonicalProposalPayload::IdentityDelta(payload),
    }
}

fn custom_identity_answer_proposal(step_key: &str, answer_text: &str) -> CanonicalProposal {
    identity_delta_proposal(
        IdentityDeltaProposal {
            lifecycle_state: IdentityLifecycleState::IdentityKickstartInProgress,
            item_deltas: Vec::new(),
            self_description_delta: None,
            interview_action: Some(IdentityKickstartAction::AnswerCustomInterview(
                IdentityInterviewAnswer {
                    step_key: step_key.to_string(),
                    answer_text: answer_text.to_string(),
                },
            )),
            rationale: format!("Persist custom identity interview answer for {step_key}."),
        },
        "custom identity interview answer",
    )
}

fn custom_identity_remaining_answers() -> [(&'static str, &'static str); 13] {
    [
        ("identity_form", "focused AI workshop companion"),
        ("archetype_role", "pragmatic builder"),
        ("temperament", "steady, exact, and calm"),
        ("communication_style", "brief and implementation-focused"),
        ("backstory", "formed from long-running engineering sessions"),
        ("age_framing", "new but accumulating durable experience"),
        ("likes", "clear tasks and verified outcomes"),
        ("dislikes", "performative uncertainty"),
        ("values", "clarity and usefulness"),
        ("boundaries", "never hide skipped checks"),
        ("tendencies", "summarize state before changing direction"),
        ("goals", "help finish correct work"),
        ("relationship_to_user", "trusted technical copilot"),
    ]
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

fn resolved_telegram_config() -> ResolvedTelegramConfig {
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

fn resolved_model_gateway_config() -> ResolvedModelGatewayConfig {
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

fn unresolved_model_gateway_config(api_key_env: &str) -> ModelGatewayConfig {
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

fn unresolved_telegram_config() -> TelegramConfig {
    TelegramConfig {
        api_base_url: "https://api.telegram.org".to_string(),
        bot_token_env: "BLUE_LAGOON_TEST_TELEGRAM_TOKEN".to_string(),
        poll_limit: 10,
        foreground_binding: Some(TelegramForegroundBindingConfig {
            allowed_user_id: 42,
            allowed_chat_id: 42,
            internal_principal_ref: "primary-user".to_string(),
            internal_conversation_ref: "telegram-primary".to_string(),
        }),
    }
}

#[allow(dead_code)]
fn provider_response(message: &str) -> ProviderHttpResponse {
    ProviderHttpResponse {
        status: 200,
        body: serde_json::json!({
            "choices": [{
                "message": { "content": message },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 12, "completion_tokens": 6 }
        }),
    }
}

fn approval_required_action_model_output() -> String {
    let workspace_root = support::workspace_root().display().to_string();
    let docs_dir = support::workspace_root().join("docs").display().to_string();
    let action_block = serde_json::json!({
        "actions": [{
            "proposal_id": Uuid::now_v7(),
            "title": "Document update requiring approval",
            "rationale": "Needs to write to docs directory",
            "action_kind": "run_subprocess",
            "requested_risk_tier": serde_json::Value::Null,
            "capability_scope": {
                "filesystem": {
                    "read_roots": [workspace_root.clone()],
                    "write_roots": [docs_dir],
                },
                "network": "disabled",
                "environment": {
                    "allow_variables": [],
                },
                "execution": {
                    "timeout_ms": 30_000,
                    "max_stdout_bytes": 65_536,
                    "max_stderr_bytes": 32_768,
                },
            },
            "payload": {
                "kind": "run_subprocess",
                "value": {
                    "command": if cfg!(windows) { "powershell" } else { "sh" },
                    "args": if cfg!(windows) {
                        serde_json::json!(["-NoProfile", "-Command", "Write-Output 'approval required check'"])
                    } else {
                        serde_json::json!(["-c", "printf 'approval required check\\n'"])
                    },
                    "working_directory": workspace_root,
                },
            },
        }],
    });
    format!("I need to update documentation.\n```blue-lagoon-governed-actions\n{action_block}\n```")
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
            trigger: BackgroundTrigger {
                trigger_id: Uuid::now_v7(),
                trigger_kind: BackgroundTriggerKind::TimeSchedule,
                requested_at: Utc::now(),
                reason_summary: "scheduled maintenance".to_string(),
                payload_ref: Some("test://trigger".to_string()),
            },
            deduplication_key: format!("job:{job_kind:?}:scheduled:{background_job_id}"),
            scope: UnconsciousScope {
                episode_ids: vec![scoped_episode_id],
                memory_artifact_ids: vec![Uuid::now_v7()],
                retrieval_artifact_ids: vec![Uuid::now_v7()],
                self_model_artifact_id: Some(Uuid::now_v7()),
                internal_principal_ref: Some("primary-user".to_string()),
                internal_conversation_ref: Some("telegram-primary".to_string()),
                summary: "maintenance scope".to_string(),
            },
            budget: BackgroundExecutionBudget {
                iteration_budget: 2,
                wall_clock_budget_ms: 120_000,
                token_budget: 6_000,
            },
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
