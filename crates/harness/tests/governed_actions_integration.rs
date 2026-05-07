mod support;

use anyhow::Result;
use chrono::{Duration, Utc};
use contracts::{
    ApprovalRequestStatus, CapabilityScope, ChannelKind, EnvironmentCapabilityScope,
    ExecutionCapabilityBudget, FilesystemCapabilityScope, GovernedActionKind,
    GovernedActionPayload, GovernedActionProposal, NetworkAccessPosture, SubprocessAction,
};
use harness::{
    approval::{self, NewApprovalRequestRecord},
    config::SelfModelConfig,
    foreground, foreground_orchestration, governed_actions, ingress,
    model_gateway::{self, ProviderHttpResponse},
    telegram,
};
use serial_test::serial;
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
#[serial]
async fn foreground_orchestration_executes_immediate_governed_action_and_runs_follow_up_turn()
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
        transport.push_response(Ok(provider_response(&immediate_action_model_output())));
        transport.push_response(Ok(provider_response(
            "The bounded workspace check completed and no approval was needed.",
        )));
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

        match outcome {
            foreground_orchestration::TelegramForegroundOrchestrationOutcome::Completed(_) => {}
            other => panic!("expected completed foreground outcome, got {other:?}"),
        }
        assert_eq!(delivery.sent_messages().len(), 1);
        assert_eq!(
            delivery.sent_messages()[0].text,
            "The bounded workspace check completed and no approval was needed."
        );

        let seen_requests = transport.seen_requests();
        assert_eq!(seen_requests.len(), 2);
        assert!(
            seen_requests[1]
                .body
                .to_string()
                .contains("Harness governed-action observations")
        );

        let executed_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM governed_action_executions
            WHERE status = 'executed'
            "#,
        )
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(executed_count, 1);
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn foreground_orchestration_auto_approves_cap_exceeded_continuation_when_configured()
-> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        config.self_model = Some(SelfModelConfig {
            seed_path: support::workspace_root()
                .join("config")
                .join("self_model_seed.toml"),
        });
        config.governed_actions.max_actions_per_foreground_turn = 1;
        config.governed_actions.cap_exceeded_behavior =
            contracts::GovernedActionCapExceededBehavior::AlwaysApprove;
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
            &sample_telegram_config(),
            &update,
            Some("fixtures/private_text_message.json".to_string()),
        )? {
            ingress::TelegramNormalizationOutcome::Accepted(ingress) => *ingress,
            other => panic!("fixture should normalize into accepted ingress, got {other:?}"),
        };

        let transport = model_gateway::FakeModelProviderTransport::new();
        transport.push_response(Ok(provider_response(
            &harness_native_artifact_list_model_output(
                "First artifact inspection",
                "Need one bounded harness-native inspection before replying.",
            ),
        )));
        transport.push_response(Ok(provider_response(
            &harness_native_artifact_list_model_output(
                "Second artifact inspection",
                "Need one more bounded harness-native inspection before replying.",
            ),
        )));
        transport.push_response(Ok(provider_response(
            "I completed both bounded artifact inspections in this turn.",
        )));
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

        match outcome {
            foreground_orchestration::TelegramForegroundOrchestrationOutcome::Completed(_) => {}
            other => panic!("expected completed foreground outcome, got {other:?}"),
        }
        assert_eq!(delivery.sent_messages().len(), 1);
        assert_eq!(
            delivery.sent_messages()[0].text,
            "I completed both bounded artifact inspections in this turn."
        );

        let seen_requests = transport.seen_requests();
        assert_eq!(seen_requests.len(), 3);
        assert!(
            seen_requests[1]
                .body
                .to_string()
                .contains("Continue the foreground turn using these outcomes.")
        );
        assert!(
            seen_requests[2]
                .body
                .to_string()
                .contains("Foreground action loop state:")
        );

        let executed_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM governed_action_executions
            WHERE action_kind = 'list_workspace_artifacts'
              AND status = 'executed'
            "#,
        )
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(executed_count, 2);

        let cap_auto_approved_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM audit_events
            WHERE event_kind = 'governed_action_cap_auto_approved'
            "#,
        )
        .fetch_one(&ctx.pool)
        .await?;
        assert_eq!(cap_auto_approved_count, 1);
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn foreground_orchestration_blocks_cap_exceeded_continuation_when_configured() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        config.self_model = Some(SelfModelConfig {
            seed_path: support::workspace_root()
                .join("config")
                .join("self_model_seed.toml"),
        });
        config.governed_actions.max_actions_per_foreground_turn = 1;
        config.governed_actions.cap_exceeded_behavior =
            contracts::GovernedActionCapExceededBehavior::AlwaysDeny;
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
            &sample_telegram_config(),
            &update,
            Some("fixtures/private_text_message.json".to_string()),
        )? {
            ingress::TelegramNormalizationOutcome::Accepted(ingress) => *ingress,
            other => panic!("fixture should normalize into accepted ingress, got {other:?}"),
        };

        let transport = model_gateway::FakeModelProviderTransport::new();
        transport.push_response(Ok(provider_response(
            &harness_native_artifact_list_model_output(
                "First artifact inspection",
                "Need one bounded harness-native inspection before replying.",
            ),
        )));
        transport.push_response(Ok(provider_response(
            &harness_native_artifact_list_model_output(
                "Second artifact inspection",
                "Need one more bounded harness-native inspection before replying.",
            ),
        )));
        transport.push_response(Ok(provider_response(
            "I hit the configured per-turn action limit, so I stopped after the first inspection.",
        )));
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

        match outcome {
            foreground_orchestration::TelegramForegroundOrchestrationOutcome::Completed(_) => {}
            other => panic!("expected completed foreground outcome, got {other:?}"),
        }
        assert_eq!(delivery.sent_messages().len(), 1);
        assert_eq!(
            delivery.sent_messages()[0].text,
            "I hit the configured per-turn action limit, so I stopped after the first inspection."
        );

        let seen_requests = transport.seen_requests();
        assert_eq!(seen_requests.len(), 3);
        assert!(
            seen_requests[2]
                .body
                .to_string()
                .contains("foreground governed-action limit reached")
        );

        let action_rows: Vec<(String, Option<String>)> = sqlx::query_as(
            r#"
            SELECT status, blocked_reason
            FROM governed_action_executions
            WHERE action_kind = 'list_workspace_artifacts'
            ORDER BY created_at
            "#,
        )
        .fetch_all(&ctx.pool)
        .await?;
        assert_eq!(action_rows.len(), 2);
        assert_eq!(action_rows[0].0, "executed");
        assert_eq!(action_rows[1].0, "blocked");
        assert!(
            action_rows[1]
                .1
                .as_deref()
                .is_some_and(|reason| reason.contains("foreground governed-action limit reached"))
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn foreground_orchestration_escalates_cap_exceeded_continuation_into_approval() -> Result<()>
{
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        config.self_model = Some(SelfModelConfig {
            seed_path: support::workspace_root()
                .join("config")
                .join("self_model_seed.toml"),
        });
        config.governed_actions.max_actions_per_foreground_turn = 1;
        config.governed_actions.cap_exceeded_behavior =
            contracts::GovernedActionCapExceededBehavior::Escalate;
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
            &sample_telegram_config(),
            &update,
            Some("fixtures/private_text_message.json".to_string()),
        )? {
            ingress::TelegramNormalizationOutcome::Accepted(ingress) => *ingress,
            other => panic!("fixture should normalize into accepted ingress, got {other:?}"),
        };

        let transport = model_gateway::FakeModelProviderTransport::new();
        transport.push_response(Ok(provider_response(
            &harness_native_artifact_list_model_output(
                "First artifact inspection",
                "Need one bounded harness-native inspection before replying.",
            ),
        )));
        transport.push_response(Ok(provider_response(
            &harness_native_artifact_list_model_output(
                "Second artifact inspection",
                "Need one more bounded harness-native inspection before replying.",
            ),
        )));
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

        match outcome {
            foreground_orchestration::TelegramForegroundOrchestrationOutcome::Completed(_) => {}
            other => panic!("expected completed foreground outcome, got {other:?}"),
        }

        let seen_requests = transport.seen_requests();
        assert_eq!(seen_requests.len(), 2);

        assert_eq!(delivery.sent_messages().len(), 2);
        assert!(delivery.sent_messages().iter().any(|message| {
            message
                .text
                .contains("Action: Continue after action limit: Second artifact inspection")
        }));
        assert!(
            delivery
                .sent_messages()
                .iter()
                .any(|message| { message.text == "I will inspect current workspace artifacts." })
        );

        let approval_requests = approval::list_approval_requests(&ctx.pool, None, 10).await?;
        assert_eq!(approval_requests.len(), 1);
        assert_eq!(approval_requests[0].status, ApprovalRequestStatus::Pending);
        assert!(
            approval_requests[0]
                .title
                .contains("Continue after action limit: Second artifact inspection")
        );
        assert!(
            approval_requests[0]
                .consequence_summary
                .contains("foreground governed-action limit reached")
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn approval_resolution_executes_linked_governed_action_after_approval() -> Result<()> {
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

        let proposal = contracts::GovernedActionProposal {
            proposal_id: Uuid::now_v7(),
            title: "Approval-gated subprocess".to_string(),
            rationale: Some("Used to verify approval to execution flow.".to_string()),
            action_kind: GovernedActionKind::RunSubprocess,
            requested_risk_tier: None,
            capability_scope: approval_required_scope(),
            payload: contracts::GovernedActionPayload::RunSubprocess(platform_echo_action("ok")),
        };
        let planned = governed_actions::plan_governed_action(
            &config,
            &ctx.pool,
            &governed_actions::GovernedActionPlanningRequest {
                governed_action_execution_id: Uuid::now_v7(),
                trace_id: Uuid::now_v7(),
                execution_id: None,
                proposal: proposal.clone(),
            },
        )
        .await?;
        let planned = match planned {
            governed_actions::GovernedActionPlanningOutcome::Planned(planned) => planned,
            other => panic!("expected approval-gated governed action, got {other:?}"),
        };
        assert!(planned.requires_approval);

        let approval_request = approval::create_approval_request(
            &config,
            &ctx.pool,
            &NewApprovalRequestRecord {
                approval_request_id: Uuid::now_v7(),
                trace_id: planned.record.trace_id,
                execution_id: None,
                action_proposal_id: planned.record.action_proposal_id,
                action_fingerprint: planned.record.action_fingerprint.clone(),
                action_kind: planned.record.action_kind,
                risk_tier: planned.record.risk_tier,
                title: proposal.title,
                consequence_summary: "Used to verify approval-linked execution.".to_string(),
                capability_scope: proposal.capability_scope,
                requested_by: "telegram:primary-user".to_string(),
                token: "42".to_string(),
                requested_at: Utc::now(),
                expires_at: Utc::now() + Duration::minutes(15),
            },
        )
        .await?;
        governed_actions::attach_approval_request(
            &ctx.pool,
            planned.record.governed_action_execution_id,
            approval_request.approval_request_id,
        )
        .await?;

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
            other => panic!("fixture should normalize into accepted ingress, got {other:?}"),
        };

        let transport = model_gateway::FakeModelProviderTransport::new();
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

        match outcome {
            foreground_orchestration::TelegramForegroundOrchestrationOutcome::ApprovalResolved(
                _,
            ) => {}
            other => panic!("expected approval-resolution outcome, got {other:?}"),
        }

        let resolved_request =
            approval::get_approval_request(&ctx.pool, approval_request.approval_request_id).await?;
        assert_eq!(resolved_request.status, ApprovalRequestStatus::Approved);
        let action_record = governed_actions::get_governed_action_execution_by_approval_request_id(
            &ctx.pool,
            approval_request.approval_request_id,
        )
        .await?
        .expect("governed action should be linked to the approval");
        assert_eq!(
            action_record.status,
            contracts::GovernedActionStatus::Executed
        );
        assert_eq!(delivery.sent_messages().len(), 1);
        assert_eq!(
            delivery.sent_messages()[0].text,
            "Approved: Approval-gated subprocess"
        );
        assert!(
            delivery
                .sent_chat_actions()
                .contains(&(42, telegram::TelegramChatAction::Typing))
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn foreground_orchestration_surfaces_blocked_governed_action_into_follow_up_turn()
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
        transport.push_response(Ok(provider_response(&blocked_action_model_output())));
        transport.push_response(Ok(provider_response(
            "The requested action was blocked by policy, so I did not execute it.",
        )));
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

        match outcome {
            foreground_orchestration::TelegramForegroundOrchestrationOutcome::Completed(_) => {}
            other => panic!("expected completed foreground outcome, got {other:?}"),
        }
        assert_eq!(delivery.sent_messages().len(), 1);
        assert_eq!(
            delivery.sent_messages()[0].text,
            "The requested action was blocked by policy, so I did not execute it."
        );

        let seen_requests = transport.seen_requests();
        assert_eq!(seen_requests.len(), 2);
        assert!(
            seen_requests[1]
                .body
                .to_string()
                .contains("Harness governed-action observations")
        );

        let blocked_rows: Vec<(String, Option<String>)> = sqlx::query_as(
            r#"
            SELECT status, blocked_reason
            FROM governed_action_executions
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .fetch_all(&ctx.pool)
        .await?;
        assert_eq!(blocked_rows.len(), 1);
        assert_eq!(blocked_rows[0].0, "blocked");
        assert!(
            blocked_rows[0]
                .1
                .as_deref()
                .unwrap_or_default()
                .contains("not allowlisted")
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn execute_governed_action_routes_policy_recheck_failure_through_recovery() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut planning_config = ctx.config.clone();
        planning_config
            .governed_actions
            .allowlisted_environment_variables =
            vec!["HOME".to_string(), "BLUE_LAGOON_DATABASE_URL".to_string()];

        let proposal = GovernedActionProposal {
            proposal_id: Uuid::now_v7(),
            title: "Policy drift check".to_string(),
            rationale: Some("Used to verify execution-time policy re-check recovery.".to_string()),
            action_kind: GovernedActionKind::RunSubprocess,
            requested_risk_tier: None,
            capability_scope: CapabilityScope {
                filesystem: FilesystemCapabilityScope {
                    read_roots: vec![support::workspace_root().display().to_string()],
                    write_roots: Vec::new(),
                },
                network: NetworkAccessPosture::Disabled,
                environment: EnvironmentCapabilityScope {
                    allow_variables: vec!["HOME".to_string()],
                },
                execution: ExecutionCapabilityBudget {
                    timeout_ms: 30_000,
                    max_stdout_bytes: 65_536,
                    max_stderr_bytes: 32_768,
                },
            },
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
                        "Write-Output 'policy drift check'".to_string(),
                    ]
                } else {
                    vec![
                        "-c".to_string(),
                        "printf 'policy drift check\\n'".to_string(),
                    ]
                },
                working_directory: Some(support::workspace_root().display().to_string()),
            }),
        };
        let planned = governed_actions::plan_governed_action(
            &planning_config,
            &ctx.pool,
            &governed_actions::GovernedActionPlanningRequest {
                governed_action_execution_id: Uuid::now_v7(),
                trace_id: Uuid::now_v7(),
                execution_id: None,
                proposal,
            },
        )
        .await?;
        let planned = match planned {
            governed_actions::GovernedActionPlanningOutcome::Planned(planned) => planned,
            other => panic!("expected planned governed action, got {other:?}"),
        };

        let outcome =
            governed_actions::execute_governed_action(&ctx.config, &ctx.pool, &planned.record)
                .await?;
        assert_eq!(
            outcome.outcome.status,
            contracts::GovernedActionStatus::Blocked
        );
        assert!(
            outcome
                .outcome
                .summary
                .contains("environment variable 'HOME' is not allowlisted")
        );

        let checkpoint_row = sqlx::query(
            r#"
            SELECT recovery_reason_code, status, recovery_decision, checkpoint_payload_json
            FROM recovery_checkpoints
            WHERE governed_action_execution_id = $1
            ORDER BY created_at DESC, recovery_checkpoint_id DESC
            LIMIT 1
            "#,
        )
        .bind(planned.record.governed_action_execution_id)
        .fetch_one(&ctx.pool)
        .await?;
        let payload: serde_json::Value = checkpoint_row.get("checkpoint_payload_json");
        assert_eq!(
            checkpoint_row.get::<String, _>("recovery_reason_code"),
            "integrity_or_policy_block"
        );
        assert_eq!(checkpoint_row.get::<String, _>("status"), "abandoned");
        assert_eq!(
            checkpoint_row.get::<Option<String>, _>("recovery_decision"),
            Some("abandon".to_string())
        );
        assert_eq!(payload["source"], serde_json::json!("policy_recheck"));

        let diagnostics = harness::recovery::list_operational_diagnostics(&ctx.pool, 10).await?;
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.reason_code == "governed_action_policy_recheck_failed"
                && diagnostic.diagnostic_payload["governed_action_execution_id"]
                    == serde_json::json!(planned.record.governed_action_execution_id)
        }));
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn web_fetch_proposal_plans_with_approval_and_execution_is_attempted() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let proposal = contracts::GovernedActionProposal {
            proposal_id: Uuid::now_v7(),
            title: "Fetch example.com".to_string(),
            rationale: Some("Integration test for web fetch governed action.".to_string()),
            action_kind: GovernedActionKind::WebFetch,
            requested_risk_tier: None,
            capability_scope: web_fetch_scope(),
            payload: contracts::GovernedActionPayload::WebFetch(contracts::WebFetchAction {
                url: "https://example.com".to_string(),
                timeout_ms: 10_000,
                max_response_bytes: 65_536,
            }),
        };

        let planned = governed_actions::plan_governed_action(
            &ctx.config,
            &ctx.pool,
            &governed_actions::GovernedActionPlanningRequest {
                governed_action_execution_id: Uuid::now_v7(),
                trace_id: Uuid::now_v7(),
                execution_id: None,
                proposal: proposal.clone(),
            },
        )
        .await?;
        let planned = match planned {
            governed_actions::GovernedActionPlanningOutcome::Planned(planned) => planned,
            other => panic!("expected approval-gated web fetch, got {other:?}"),
        };
        assert!(planned.requires_approval);
        assert_eq!(
            planned.record.risk_tier,
            contracts::GovernedActionRiskTier::Tier2
        );

        let approval_request = approval::create_approval_request(
            &ctx.config,
            &ctx.pool,
            &NewApprovalRequestRecord {
                approval_request_id: Uuid::now_v7(),
                trace_id: planned.record.trace_id,
                execution_id: None,
                action_proposal_id: planned.record.action_proposal_id,
                action_fingerprint: planned.record.action_fingerprint.clone(),
                action_kind: planned.record.action_kind,
                risk_tier: planned.record.risk_tier,
                title: proposal.title.clone(),
                consequence_summary: "Fetch example.com for integration test.".to_string(),
                capability_scope: proposal.capability_scope,
                requested_by: "telegram:primary-user".to_string(),
                token: "web-fetch-test-token".to_string(),
                requested_at: Utc::now(),
                expires_at: Utc::now() + Duration::minutes(15),
            },
        )
        .await?;
        governed_actions::attach_approval_request(
            &ctx.pool,
            planned.record.governed_action_execution_id,
            approval_request.approval_request_id,
        )
        .await?;

        approval::resolve_approval_request(
            &ctx.pool,
            &approval::ApprovalResolutionAttempt {
                token: approval_request.token.clone(),
                actor_ref: "telegram:primary-user".to_string(),
                expected_action_fingerprint: planned.record.action_fingerprint.clone(),
                decision: contracts::ApprovalResolutionDecision::Approved,
                reason: Some("integration test approval".to_string()),
                resolved_at: Utc::now(),
            },
        )
        .await?;

        let synced = governed_actions::sync_status_from_approval_resolution(
            &ctx.pool,
            planned.record.governed_action_execution_id,
            contracts::ApprovalResolutionDecision::Approved,
            None,
            Some("integration test approval"),
        )
        .await?;

        let outcome =
            governed_actions::execute_governed_action(&ctx.config, &ctx.pool, &synced).await?;

        assert!(
            outcome.record.status == contracts::GovernedActionStatus::Executed
                || outcome.record.status == contracts::GovernedActionStatus::Failed,
            "web fetch execution should reach Executed or Failed (not Blocked), got {:?}",
            outcome.record.status
        );
        Ok(())
    })
    .await
}

fn web_fetch_scope() -> CapabilityScope {
    CapabilityScope {
        filesystem: FilesystemCapabilityScope {
            read_roots: Vec::new(),
            write_roots: Vec::new(),
        },
        network: NetworkAccessPosture::Enabled,
        environment: EnvironmentCapabilityScope {
            allow_variables: Vec::new(),
        },
        execution: ExecutionCapabilityBudget {
            timeout_ms: 0,
            max_stdout_bytes: 0,
            max_stderr_bytes: 0,
        },
    }
}

fn provider_response(message: &str) -> ProviderHttpResponse {
    ProviderHttpResponse {
        status: 200,
        body: serde_json::json!({
            "choices": [{
                "message": { "content": message },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 12,
                "completion_tokens": 6
            }
        }),
    }
}

fn immediate_action_model_output() -> String {
    let workspace_root = support::workspace_root().display().to_string();
    let action_block = serde_json::json!({
        "actions": [{
            "proposal_id": Uuid::now_v7(),
            "title": "Immediate bounded check",
            "rationale": "Need one scoped local check before replying",
            "action_kind": "run_subprocess",
            "requested_risk_tier": serde_json::Value::Null,
            "capability_scope": {
                "filesystem": {
                    "read_roots": [workspace_root.clone()],
                    "write_roots": [],
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
                        serde_json::json!(["-NoProfile", "-Command", "Write-Output 'immediate bounded check'"])
                    } else {
                        serde_json::json!(["-c", "printf 'immediate bounded check\\n'"])
                    },
                    "working_directory": workspace_root.clone(),
                },
            },
        }],
    });
    format!(
        "I will run a bounded workspace check.\n```blue-lagoon-governed-actions\n{}\n```",
        action_block,
    )
}

fn harness_native_artifact_list_model_output(title: &str, rationale: &str) -> String {
    let action_block = serde_json::json!({
        "actions": [{
            "proposal_id": Uuid::now_v7(),
            "title": title,
            "rationale": rationale,
            "action_kind": "list_workspace_artifacts",
            "requested_risk_tier": serde_json::Value::Null,
            "capability_scope": {
                "filesystem": {
                    "read_roots": [],
                    "write_roots": [],
                },
                "network": "disabled",
                "environment": {
                    "allow_variables": [],
                },
                "execution": {
                    "timeout_ms": 0,
                    "max_stdout_bytes": 0,
                    "max_stderr_bytes": 0,
                },
            },
            "payload": {
                "kind": "list_workspace_artifacts",
                "value": {
                    "artifact_kind": serde_json::Value::Null,
                    "status": "active",
                    "query": serde_json::Value::Null,
                    "limit": 5,
                },
            },
        }],
    });
    format!(
        "I will inspect current workspace artifacts.\n```blue-lagoon-governed-actions\n{}\n```",
        action_block,
    )
}

fn blocked_action_model_output() -> String {
    let workspace_root = support::workspace_root().display().to_string();
    let action_block = serde_json::json!({
        "actions": [{
            "proposal_id": Uuid::now_v7(),
            "title": "Blocked bounded check",
            "rationale": "Need one local check, but request is intentionally invalid for integration coverage.",
            "action_kind": "run_subprocess",
            "requested_risk_tier": serde_json::Value::Null,
            "capability_scope": {
                "filesystem": {
                    "read_roots": [workspace_root.clone()],
                    "write_roots": [],
                },
                "network": "disabled",
                "environment": {
                    "allow_variables": ["HOME"],
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
                        serde_json::json!(["-NoProfile", "-Command", "Write-Output 'blocked integration'"])
                    } else {
                        serde_json::json!(["-c", "printf 'blocked integration\\n'"])
                    },
                    "working_directory": workspace_root.clone(),
                },
            },
        }],
    });
    format!(
        "I want to run a local check.\n```blue-lagoon-governed-actions\n{}\n```",
        action_block,
    )
}

fn sample_telegram_config() -> harness::config::ResolvedTelegramConfig {
    harness::config::ResolvedTelegramConfig {
        api_base_url: "https://api.telegram.org".to_string(),
        bot_token: "secret".to_string(),
        allowed_user_id: 42,
        allowed_chat_id: 42,
        internal_principal_ref: "primary-user".to_string(),
        internal_conversation_ref: "telegram-primary".to_string(),
        poll_limit: 10,
    }
}

fn sample_model_gateway_config() -> harness::config::ResolvedModelGatewayConfig {
    harness::config::ResolvedModelGatewayConfig {
        foreground: harness::config::ResolvedForegroundModelRouteConfig {
            provider: contracts::ModelProviderKind::ZAi,
            model: "z-ai-foreground".to_string(),
            api_base_url: "https://api.z.ai/api/paas/v4".to_string(),
            api_key: "secret".to_string(),
            provider_headers: Vec::new(),
            timeout_ms: 20_000,
        },
    }
}

fn telegram_fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("telegram")
        .join(name)
}

fn immediate_scope() -> CapabilityScope {
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

fn approval_required_scope() -> CapabilityScope {
    CapabilityScope {
        filesystem: FilesystemCapabilityScope {
            read_roots: vec![support::workspace_root().display().to_string()],
            write_roots: vec![support::workspace_root().join("docs").display().to_string()],
        },
        ..immediate_scope()
    }
}

fn platform_echo_action(message: &str) -> contracts::SubprocessAction {
    if cfg!(windows) {
        contracts::SubprocessAction {
            command: "powershell".to_string(),
            args: vec![
                "-NoProfile".to_string(),
                "-Command".to_string(),
                format!("Write-Output '{}'", message.replace('\'', "''")),
            ],
            working_directory: Some(support::workspace_root().display().to_string()),
        }
    } else {
        contracts::SubprocessAction {
            command: "sh".to_string(),
            args: vec![
                "-c".to_string(),
                format!("printf '%s\\n' '{}'", message.replace('\'', "'\\''")),
            ],
            working_directory: Some(support::workspace_root().display().to_string()),
        }
    }
}
