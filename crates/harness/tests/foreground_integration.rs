mod support;

use anyhow::Result;
use contracts::{
    ApprovalRequestStatus, CapabilityScope, ChannelKind, EnvironmentCapabilityScope,
    ExecutionCapabilityBudget, FilesystemCapabilityScope, GovernedActionFingerprint,
    GovernedActionKind, GovernedActionRiskTier, ModelProviderKind, NetworkAccessPosture,
};
use harness::{
    approval::{self, NewApprovalRequestRecord},
    audit,
    config::{
        ResolvedForegroundModelRouteConfig, ResolvedModelGatewayConfig, ResolvedTelegramConfig,
        SelfModelConfig,
    },
    execution, foreground, model_gateway, runtime, scheduled_foreground, telegram,
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
