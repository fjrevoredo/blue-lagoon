mod support;

use anyhow::Result;
use chrono::{Duration, Utc};
use contracts::{
    ApprovalRequestStatus, CapabilityScope, ChannelKind, EnvironmentCapabilityScope,
    ExecutionCapabilityBudget, FilesystemCapabilityScope, GovernedActionKind, NetworkAccessPosture,
};
use harness::{
    approval::{self, NewApprovalRequestRecord},
    config::SelfModelConfig,
    foreground, foreground_orchestration, governed_actions, ingress,
    model_gateway::{self, ProviderHttpResponse},
    telegram,
};
use serial_test::serial;
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
        assert!(
            delivery.sent_messages()[0]
                .text
                .contains("bounded subprocess completed successfully")
        );
        Ok(())
    })
    .await
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
                        serde_json::json!(["-NoProfile", "-Command", "Write-Output 'phase5 immediate'"])
                    } else {
                        serde_json::json!(["-c", "printf 'phase5 immediate\\n'"])
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
