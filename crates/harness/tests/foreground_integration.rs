mod support;

use anyhow::Result;
use chrono::{Duration, Utc};
use contracts::{
    ApprovalRequestStatus, CanonicalProposal, CanonicalProposalKind, CanonicalProposalPayload,
    CanonicalTargetKind, CapabilityScope, ChannelKind, EnvironmentCapabilityScope,
    ExecutionCapabilityBudget, FilesystemCapabilityScope, GovernedActionFingerprint,
    GovernedActionKind, GovernedActionPayload, GovernedActionProposal, GovernedActionRiskTier,
    IdentityDeltaProposal, IdentityInterviewAnswer, IdentityKickstartAction,
    IdentityLifecycleState, ModelProviderKind, NetworkAccessPosture, ProposalConflictPosture,
    ProposalProvenance, ProposalProvenanceKind, SubprocessAction,
};
use harness::{
    approval::{self, NewApprovalRequestRecord},
    audit,
    config::{
        ResolvedForegroundModelRouteConfig, ResolvedModelGatewayConfig, ResolvedTelegramConfig,
        SelfModelConfig,
    },
    execution, foreground, governed_actions, identity, ingress, model_gateway, proposal, runtime,
    scheduled_foreground, telegram,
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
                    "message": { "content": "assistant reply from foreground integration" },
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
            "assistant reply from foreground integration"
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
            Some("assistant reply from foreground integration")
        );

        let audit_events = audit::list_for_execution(&ctx.pool, execution_id).await?;
        assert!(audit_events.iter().all(|event| event.trace_id == trace_id));
        assert!(
            audit_events
                .iter()
                .any(|event| event.event_kind == "foreground_recovery_mode_decided")
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
async fn telegram_fixture_runtime_run_applies_predefined_identity_selection() -> Result<()> {
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

        let identity_block = serde_json::json!({
            "action": "select_predefined_identity",
            "template_key": "continuity_operator",
            "answer": serde_json::Value::Null,
            "cancel_reason": serde_json::Value::Null,
        });
        let model_text = format!(
            "Continuity Operator selected.\n```blue-lagoon-identity-kickstart\n{}\n```",
            identity_block
        );
        let transport = model_gateway::FakeModelProviderTransport::new();
        transport.push_response(Ok(model_gateway::ProviderHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{
                    "message": { "content": model_text },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 21,
                    "completion_tokens": 9
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

        assert_eq!(summary.completed_count, 1);
        assert_eq!(delivery.sent_messages().len(), 1);
        assert_eq!(delivery.sent_messages()[0].text, "Continuity Operator selected.");

        let lifecycle = identity::get_current_lifecycle(&ctx.pool)
            .await?
            .expect("identity lifecycle should be current");
        assert_eq!(lifecycle.lifecycle_state, "complete_identity_active");

        let active_item_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM identity_items
            WHERE status = 'active'
            "#,
        )
        .fetch_one(&ctx.pool)
        .await?;
        assert!(active_item_count >= 21);

        let proposal_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM proposals
            WHERE proposal_kind = 'identity_delta'
              AND canonical_target = 'identity_items'
              AND status = 'accepted'
            "#,
        )
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(proposal_count, 1);

        let audit_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM audit_events
            WHERE event_kind = 'canonical_write_applied'
            "#,
        )
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(audit_count, 1);

        let compact_identity = identity::reconstruct_compact_identity_snapshot(&ctx.pool, 32).await?;
        assert_eq!(compact_identity.identity_summary, "Blue Lagoon");
        assert_eq!(
            compact_identity.self_description.as_deref(),
            Some("I am a continuity-oriented assistant that keeps context organized, follows through on commitments, and stays direct about state, limits, and next actions.")
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn telegram_fixture_runtime_run_completes_custom_identity_interview() -> Result<()> {
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

        let start_block = serde_json::json!({
            "action": "start_custom_identity_interview",
            "template_key": serde_json::Value::Null,
            "answer": serde_json::Value::Null,
            "cancel_reason": serde_json::Value::Null,
        });
        let transport = model_gateway::FakeModelProviderTransport::new();
        transport.push_response(Ok(model_gateway::ProviderHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{
                    "message": {
                        "content": format!(
                            "Let's build a custom identity.\n```blue-lagoon-identity-kickstart\n{}\n```",
                            start_block
                        )
                    },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 21,
                    "completion_tokens": 9
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
        assert_eq!(summary.completed_count, 1);
        assert_eq!(delivery.sent_messages()[0].text, "Let's build a custom identity.");

        let lifecycle = identity::get_current_lifecycle(&ctx.pool)
            .await?
            .expect("identity lifecycle should be current");
        assert_eq!(lifecycle.lifecycle_state, "identity_kickstart_in_progress");
        let interview_id = lifecycle
            .active_interview_id
            .expect("custom interview should be active");
        let interview = identity::get_identity_interview(&ctx.pool, interview_id)
            .await?
            .expect("interview should persist");
        assert_eq!(interview.current_step, "name");

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
        let processing_context = proposal::ProposalProcessingContext {
            trace_id: execution_row.get("trace_id"),
            execution_id: execution_row.get("execution_id"),
            episode_id: None,
            source_ingress_id: None,
            source_loop_kind: "conscious".to_string(),
        };

        for (step_key, answer_text) in [
            ("name", "Lagoon Forge"),
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
        ] {
            let candidate = custom_identity_answer_proposal(step_key, answer_text);
            let summary = proposal::apply_candidate_proposals(
                &ctx.pool,
                &config,
                &processing_context,
                "foreground_identity_interview",
                None,
                &[candidate],
            )
            .await?;
            assert_eq!(summary.rejected_count, 0);
            assert_eq!(summary.accepted_count, 1);
        }

        let lifecycle = identity::get_current_lifecycle(&ctx.pool)
            .await?
            .expect("identity lifecycle should remain current");
        assert_eq!(lifecycle.lifecycle_state, "complete_identity_active");
        assert_eq!(lifecycle.active_interview_id, None);

        let completed_interview = identity::get_identity_interview(&ctx.pool, interview_id)
            .await?
            .expect("interview should remain queryable");
        assert_eq!(completed_interview.status, "completed");
        assert_eq!(completed_interview.current_step, "completed");

        let compact_identity = identity::reconstruct_compact_identity_snapshot(&ctx.pool, 32).await?;
        assert_eq!(compact_identity.identity_summary, "Lagoon Forge");
        assert!(
            compact_identity
                .self_description
                .as_deref()
                .is_some_and(|description| description.contains("Lagoon Forge"))
        );
        assert!(compact_identity.stable_items.len() >= 10);
        assert!(compact_identity.evolving_items.len() >= 6);
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn telegram_callback_fixture_runtime_run_resolves_pending_approval() -> Result<()> {
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
                    value: "sha256:foreground-integration-callback".to_string(),
                },
                action_kind: GovernedActionKind::RunSubprocess,
                risk_tier: GovernedActionRiskTier::Tier2,
                title: "Runtime callback approval".to_string(),
                consequence_summary: "Used to verify runtime callback routing.".to_string(),
                capability_scope: sample_capability_scope(),
                requested_by: "telegram:primary-user".to_string(),
                token: "42".to_string(),
                requested_at: chrono::Utc::now(),
                expires_at: chrono::Utc::now() + chrono::Duration::minutes(15),
            },
        )
        .await?;

        let transport = model_gateway::FakeModelProviderTransport::new();
        let mut delivery = telegram::FakeTelegramDelivery::default();

        let summary = runtime::run_telegram_fixture_with(
            &ctx.pool,
            &ctx.config,
            &sample_telegram_config(),
            &sample_model_gateway_config(),
            &telegram_fixture("approval_callback.json"),
            &transport,
            &mut delivery,
        )
        .await?;

        assert_eq!(summary.fetched_updates, 1);
        assert_eq!(summary.completed_count, 1);
        assert_eq!(delivery.sent_messages().len(), 1);
        assert_eq!(
            delivery.sent_messages()[0].text,
            "Approved: Runtime callback approval"
        );

        let resolved = approval::get_approval_request_by_token(&ctx.pool, "42")
            .await?
            .expect("approval request should still be queryable");
        assert_eq!(resolved.status, ApprovalRequestStatus::Approved);
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn telegram_command_fixture_runtime_run_resolves_pending_approval() -> Result<()> {
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
                    value: "sha256:foreground-integration-command".to_string(),
                },
                action_kind: GovernedActionKind::RunSubprocess,
                risk_tier: GovernedActionRiskTier::Tier2,
                title: "Runtime command approval".to_string(),
                consequence_summary: "Used to verify runtime command approval routing.".to_string(),
                capability_scope: sample_capability_scope(),
                requested_by: "telegram:primary-user".to_string(),
                token: "42".to_string(),
                requested_at: chrono::Utc::now(),
                expires_at: chrono::Utc::now() + chrono::Duration::minutes(15),
            },
        )
        .await?;

        let transport = model_gateway::FakeModelProviderTransport::new();
        let mut delivery = telegram::FakeTelegramDelivery::default();

        let summary = runtime::run_telegram_fixture_with(
            &ctx.pool,
            &ctx.config,
            &sample_telegram_config(),
            &sample_model_gateway_config(),
            &telegram_fixture("approval_command_approve.json"),
            &transport,
            &mut delivery,
        )
        .await?;

        assert_eq!(summary.fetched_updates, 1);
        assert_eq!(summary.completed_count, 1);
        assert_eq!(delivery.sent_messages().len(), 1);
        assert_eq!(
            delivery.sent_messages()[0].text,
            "Approved: Runtime command approval"
        );

        let resolved = approval::get_approval_request_by_token(&ctx.pool, "42")
            .await?
            .expect("approval request should still be queryable");
        assert_eq!(resolved.status, ApprovalRequestStatus::Approved);
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn telegram_fixture_runtime_batch_activates_backlog_recovery() -> Result<()> {
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
                    "message": { "content": "assistant reply from backlog runtime integration" },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 23,
                    "completion_tokens": 9
                }
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

        assert_eq!(summary.fetched_updates, 3);
        assert_eq!(summary.completed_count, 1);
        assert_eq!(summary.backlog_recovery_count, 1);
        assert_eq!(delivery.sent_messages().len(), 1);
        assert_eq!(
            delivery.sent_messages()[0].text,
            "assistant reply from backlog runtime integration"
        );
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

        let seen_requests = transport.seen_requests();
        assert_eq!(seen_requests.len(), 1);
        let message_contents = seen_requests[0]
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
async fn telegram_fixture_runtime_retrieves_prior_canonical_memory_on_later_run() -> Result<()> {
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
                "usage": {
                    "prompt_tokens": 20,
                    "completion_tokens": 7
                }
            }),
        }));
        transport.push_response(Ok(model_gateway::ProviderHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{
                    "message": { "content": "assistant reply after retrieval" },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 18,
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
            &telegram_fixture("private_preference_message.json"),
            &transport,
            &mut delivery,
        )
        .await?;
        let second = runtime::run_telegram_fixture_with(
            &ctx.pool,
            &config,
            &sample_telegram_config(),
            &sample_model_gateway_config(),
            &telegram_fixture("private_preference_followup.json"),
            &transport,
            &mut delivery,
        )
        .await?;

        assert_eq!(first.completed_count, 1);
        assert_eq!(second.completed_count, 1);
        assert_eq!(transport.seen_requests().len(), 2);

        let seen_requests = transport.seen_requests();
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

        let memory_artifact_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM memory_artifacts
            WHERE status = 'active'
            "#,
        )
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(memory_artifact_count, 1);
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

        let duplicate_audit_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM audit_events
            WHERE event_kind = 'foreground_trigger_duplicate'
            "#,
        )
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(duplicate_audit_count, 1);

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

#[tokio::test]
#[serial]
async fn runtime_fixture_blocks_stale_processing_replay_when_prior_governed_action_exists()
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

        let interrupted_execution_id = Uuid::now_v7();
        let interrupted_trace_id = Uuid::now_v7();
        execution::insert(
            &ctx.pool,
            &execution::NewExecutionRecord {
                execution_id: interrupted_execution_id,
                trace_id: interrupted_trace_id,
                trigger_kind: "foreground_recovery_test".to_string(),
                synthetic_trigger: None,
                status: "started".to_string(),
                request_payload: serde_json::json!({
                    "test": "runtime_fixture_blocks_stale_processing_replay_when_prior_governed_action_exists"
                }),
            },
        )
        .await?;

        let update = telegram::load_fixture_updates(&telegram_fixture("private_text_message.json"))?
            .into_iter()
            .next()
            .expect("fixture should contain one update");
        let normalized = match ingress::normalize_telegram_update(
            &sample_telegram_config(),
            &update,
            Some("fixtures/private_text_message.json".to_string()),
        )? {
            ingress::TelegramNormalizationOutcome::Accepted(ingress) => *ingress,
            other => panic!("fixture should normalize into accepted ingress, got {other:?}"),
        };
        let staged = match foreground::stage_telegram_foreground_ingress(
            &ctx.pool,
            &sample_telegram_config(),
            normalized,
        )
        .await? {
            foreground::StagedForegroundIngressOutcome::Accepted(staged) => staged,
            other => panic!("fixture should stage foreground ingress, got {other:?}"),
        };
        let latest_update =
            telegram::load_fixture_updates(&telegram_fixture("private_preference_followup.json"))?
                .into_iter()
                .next()
                .expect("fixture should contain one update");
        let latest_normalized = match ingress::normalize_telegram_update(
            &sample_telegram_config(),
            &latest_update,
            Some("fixtures/private_preference_followup.json".to_string()),
        )? {
            ingress::TelegramNormalizationOutcome::Accepted(ingress) => *ingress,
            other => panic!("fixture should normalize into accepted ingress, got {other:?}"),
        };
        let latest_staged = match foreground::stage_telegram_foreground_ingress(
            &ctx.pool,
            &sample_telegram_config(),
            latest_normalized,
        )
        .await? {
            foreground::StagedForegroundIngressOutcome::Accepted(staged) => staged,
            other => panic!("fixture should stage foreground ingress, got {other:?}"),
        };

        sqlx::query(
            r#"
            UPDATE ingress_events
            SET
                execution_id = $2,
                foreground_status = 'processing',
                last_processed_at = $3
            WHERE ingress_id = $1
            "#,
        )
        .bind(staged.ingress_id)
        .bind(interrupted_execution_id)
        .bind(Utc::now() - Duration::minutes(15))
        .execute(&ctx.pool)
        .await?;

        plan_nonrepeatable_recovery_governed_action(
            &ctx.config,
            &ctx.pool,
            interrupted_trace_id,
            interrupted_execution_id,
        )
        .await?;

        let transport = model_gateway::FakeModelProviderTransport::new();
        transport.push_response(Ok(model_gateway::ProviderHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{
                    "message": { "content": "unexpected resumed reply" },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 10,
                    "completion_tokens": 5
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
        assert_eq!(summary.completed_count, 0);
        assert_eq!(summary.backlog_recovery_count, 1);
        assert_eq!(summary.trigger_rejected_count, 1);
        assert!(transport.seen_requests().is_empty());

        assert_eq!(delivery.sent_messages().len(), 1);
        assert_eq!(delivery.sent_messages()[0].reply_to_message_id, Some(62));
        assert!(delivery.sent_messages()[0]
            .text
            .contains("I could not automatically resume that interrupted request."));

        let execution_row = sqlx::query(
            r#"
            SELECT execution_id
            FROM execution_records
            ORDER BY created_at DESC, execution_id DESC
            LIMIT 1
            "#,
        )
        .fetch_one(&ctx.pool)
        .await?;
        let recovery_execution_id: Uuid = execution_row.get("execution_id");
        let recovery_execution = execution::get(&ctx.pool, recovery_execution_id).await?;
        assert_eq!(recovery_execution.status, "failed");
        let response_payload = recovery_execution
            .response_payload
            .expect("blocked recovery execution should persist failure payload");
        assert_eq!(
            response_payload["kind"],
            serde_json::json!("foreground_recovery_blocked")
        );
        assert_eq!(
            response_payload["diagnostic_reason_code"],
            serde_json::json!("foreground_processing_crash_replay_blocked")
        );
        assert!(delivery.sent_messages()[0]
            .text
            .contains(&recovery_execution.trace_id.to_string()));

        let stored_ingress = foreground::get_ingress_event(&ctx.pool, staged.ingress_id).await?;
        let latest_stored = foreground::get_ingress_event(&ctx.pool, latest_staged.ingress_id).await?;
        assert_eq!(stored_ingress.foreground_status, "processed");
        assert_eq!(latest_stored.foreground_status, "processed");

        let diagnostic_reason_code: String = sqlx::query_scalar(
            r#"
            SELECT reason_code
            FROM operational_diagnostics
            ORDER BY created_at DESC, operational_diagnostic_id DESC
            LIMIT 1
            "#,
        )
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(
            diagnostic_reason_code,
            "foreground_processing_crash_replay_blocked"
        );

        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn scheduled_foreground_runtime_run_executes_due_task_through_worker_binary() -> Result<()> {
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
                task_key: "integration-scheduled".to_string(),
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
                message_text: "Scheduled foreground integration prompt".to_string(),
                cadence_seconds: 600,
                cooldown_seconds: Some(120),
                next_due_at: Some(chrono::Utc::now() - chrono::Duration::seconds(5)),
                status: contracts::ScheduledForegroundTaskStatus::Active,
                actor_ref: "integration-test".to_string(),
            },
        )
        .await?;

        let transport = model_gateway::FakeModelProviderTransport::new();
        transport.push_response(Ok(model_gateway::ProviderHttpResponse {
            status: 200,
            body: serde_json::json!({
                "choices": [{
                    "message": { "content": "scheduled integration reply" },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 14,
                    "completion_tokens": 7
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

        assert_eq!(handled, 1);
        assert_eq!(delivery.sent_messages().len(), 1);
        assert_eq!(
            delivery.sent_messages()[0].text,
            "scheduled integration reply"
        );

        let task = scheduled_foreground::get_task_by_key(&ctx.pool, "integration-scheduled")
            .await?
            .expect("scheduled task should exist");
        assert_eq!(task.current_execution_id, None);
        assert_eq!(
            task.last_outcome,
            Some(contracts::ScheduledForegroundLastOutcome::Completed)
        );

        let audit_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM audit_events
            WHERE event_kind = 'scheduled_foreground_task_completed'
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
async fn scheduled_foreground_runtime_recovery_finalizes_stranded_execution() -> Result<()> {
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
                task_key: "integration-recovery".to_string(),
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
                message_text: "Task that will be recovered".to_string(),
                cadence_seconds: 600,
                cooldown_seconds: Some(120),
                next_due_at: Some(chrono::Utc::now() - chrono::Duration::seconds(5)),
                status: contracts::ScheduledForegroundTaskStatus::Active,
                actor_ref: "integration-test".to_string(),
            },
        )
        .await?;

        let execution_id = Uuid::now_v7();
        let trace_id = Uuid::now_v7();
        let claimed = scheduled_foreground::claim_next_due_task(
            &ctx.pool,
            execution_id,
            trace_id,
            chrono::Utc::now(),
        )
        .await?
        .expect("scheduled task should be claimable");
        assert!(claimed.ingress.is_some());
        sqlx::query(
            r#"
            UPDATE scheduled_foreground_tasks
            SET current_run_started_at = $2
            WHERE scheduled_foreground_task_id = $1
            "#,
        )
        .bind(claimed.task.scheduled_foreground_task_id)
        .bind(chrono::Utc::now() - chrono::Duration::minutes(2))
        .execute(&ctx.pool)
        .await?;

        let recovered =
            runtime::recover_interrupted_scheduled_foreground_tasks(&ctx.pool, &ctx.config, 10)
                .await?;
        assert_eq!(recovered, 1);

        let task = scheduled_foreground::get_task_by_key(&ctx.pool, "integration-recovery")
            .await?
            .expect("scheduled task should exist");
        assert_eq!(task.current_execution_id, None);
        assert_eq!(
            task.last_outcome,
            Some(contracts::ScheduledForegroundLastOutcome::Failed)
        );
        assert_eq!(
            task.last_outcome_reason.as_deref(),
            Some("supervisor_restart_recovery")
        );

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
        Ok(())
    })
    .await
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

async fn plan_nonrepeatable_recovery_governed_action(
    config: &harness::config::RuntimeConfig,
    pool: &sqlx::PgPool,
    trace_id: Uuid,
    execution_id: Uuid,
) -> Result<governed_actions::PlannedGovernedAction> {
    let planned = governed_actions::plan_governed_action(
        config,
        pool,
        &governed_actions::GovernedActionPlanningRequest {
            governed_action_execution_id: Uuid::now_v7(),
            trace_id,
            execution_id: Some(execution_id),
            proposal: GovernedActionProposal {
                proposal_id: Uuid::now_v7(),
                title: "Recovery replay blocker".to_string(),
                rationale: Some(
                    "Used to verify stale foreground replay is blocked once a non-repeatable governed action is linked."
                        .to_string(),
                ),
                action_kind: GovernedActionKind::RunSubprocess,
                requested_risk_tier: None,
                capability_scope: sample_capability_scope(),
                payload: GovernedActionPayload::RunSubprocess(SubprocessAction {
                    command: if cfg!(windows) {
                        "powershell".to_string()
                    } else {
                        "sh".to_string()
                    },
                    args: if cfg!(windows) {
                        vec![
                            "-NoProfile".to_string(),
                            "-Command".to_string(),
                            "Write-Output 'recovery replay blocker'".to_string(),
                        ]
                    } else {
                        vec![
                            "-c".to_string(),
                            "printf 'recovery replay blocker\\n'".to_string(),
                        ]
                    },
                    working_directory: Some(".".to_string()),
                }),
            },
        },
    )
    .await?;

    match planned {
        governed_actions::GovernedActionPlanningOutcome::Planned(planned) => Ok(planned),
        other => panic!("expected planned governed action, got {other:?}"),
    }
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

fn custom_identity_answer_proposal(step_key: &str, answer_text: &str) -> CanonicalProposal {
    CanonicalProposal {
        proposal_id: Uuid::now_v7(),
        proposal_kind: CanonicalProposalKind::IdentityDelta,
        canonical_target: CanonicalTargetKind::IdentityItems,
        confidence_pct: 100,
        conflict_posture: ProposalConflictPosture::Independent,
        subject_ref: "self:blue-lagoon".to_string(),
        rationale: Some(format!("Custom identity interview answer for {step_key}.")),
        valid_from: Some(chrono::Utc::now()),
        valid_to: None,
        supersedes_artifact_id: None,
        provenance: ProposalProvenance {
            provenance_kind: ProposalProvenanceKind::EpisodeObservation,
            source_ingress_ids: vec![Uuid::now_v7()],
            source_episode_id: None,
        },
        payload: CanonicalProposalPayload::IdentityDelta(IdentityDeltaProposal {
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
        }),
    }
}
