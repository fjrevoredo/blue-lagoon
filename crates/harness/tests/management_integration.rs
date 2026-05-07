mod support;

use anyhow::Result;
use chrono::{Duration, Utc};
use contracts::{
    ApprovalRequestStatus, CapabilityScope, EnvironmentCapabilityScope, ExecutionCapabilityBudget,
    FilesystemCapabilityScope, GovernedActionKind, NetworkAccessPosture,
};
use harness::{
    approval::{self, NewApprovalRequestRecord},
    audit,
    config::{ForegroundModelRouteConfig, ModelGatewayConfig},
    execution::{self, NewExecutionRecord},
    governed_actions,
    management::{
        self, BackgroundEnqueueOutcome, BackgroundRunNextOutcome, EnqueueBackgroundJobRequest,
        ResolveApprovalRequest, SuperviseWorkerLeasesRequest,
    },
    model_gateway::{FakeModelProviderTransport, ProviderHttpResponse},
    recovery,
};
use serde_json::json;
use serial_test::serial;
use uuid::Uuid;

#[tokio::test]
#[serial]
async fn enqueue_background_job_plans_once_and_then_suppresses_duplicates() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let _ = &ctx.pool;
        let request = EnqueueBackgroundJobRequest {
            job_kind: contracts::UnconsciousJobKind::MemoryConsolidation,
            trigger_kind: contracts::BackgroundTriggerKind::MaintenanceTrigger,
            internal_conversation_ref: Some("telegram-primary".to_string()),
            reason: Some("manual verification".to_string()),
        };

        let first = management::enqueue_background_job(&ctx.config, request.clone()).await?;
        let second = management::enqueue_background_job(&ctx.config, request).await?;

        match first {
            BackgroundEnqueueOutcome::Planned { .. } => {}
            other => panic!("expected planned background job, got {other:?}"),
        }
        match second {
            BackgroundEnqueueOutcome::SuppressedDuplicate { .. } => {}
            other => panic!("expected duplicate suppression, got {other:?}"),
        }
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn run_next_background_job_returns_no_due_job_without_work() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        configure_management_execution(&mut config);

        let outcome = management::run_next_background_job_with_transport(
            &config,
            &FakeModelProviderTransport::new(),
        )
        .await?;
        assert!(matches!(outcome, BackgroundRunNextOutcome::NoDueJob));
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn run_next_background_job_executes_due_job_from_management_surface() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut config = ctx.config.clone();
        configure_management_execution(&mut config);

        let enqueue = management::enqueue_background_job(
            &config,
            EnqueueBackgroundJobRequest {
                job_kind: contracts::UnconsciousJobKind::MemoryConsolidation,
                trigger_kind: contracts::BackgroundTriggerKind::MaintenanceTrigger,
                internal_conversation_ref: Some("telegram-primary".to_string()),
                reason: Some("manual verification".to_string()),
            },
        )
        .await?;
        assert!(matches!(enqueue, BackgroundEnqueueOutcome::Planned { .. }));

        let transport = FakeModelProviderTransport::new();
        transport.push_response(Ok(ProviderHttpResponse {
            status: 200,
            body: json!({
                "choices": [{
                    "message": { "content": "maintenance lexical summary" },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 14,
                    "completion_tokens": 8
                }
            }),
        }));

        let outcome =
            management::run_next_background_job_with_transport(&config, &transport).await?;
        match outcome {
            BackgroundRunNextOutcome::Completed { summary, .. } => {
                assert!(summary.contains("memory_consolidation"));
            }
            other => panic!("expected completed background job, got {other:?}"),
        }
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn resolve_approval_request_executes_linked_governed_action_from_management_surface()
-> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let proposal = contracts::GovernedActionProposal {
            proposal_id: Uuid::now_v7(),
            title: "Management approval flow".to_string(),
            rationale: Some("Used to verify CLI approval resolution.".to_string()),
            action_kind: GovernedActionKind::RunSubprocess,
            requested_risk_tier: None,
            capability_scope: approval_required_scope(),
            payload: contracts::GovernedActionPayload::RunSubprocess(platform_echo_action(
                "management-approval-ok",
            )),
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
            other => panic!("expected approval-gated governed action, got {other:?}"),
        };

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
                title: proposal.title,
                consequence_summary: "Used to verify management resolution.".to_string(),
                capability_scope: proposal.capability_scope,
                requested_by: "telegram:primary-user".to_string(),
                token: "management-resolution-token".to_string(),
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

        let resolution = management::resolve_approval_request(
            &ctx.config,
            ResolveApprovalRequest {
                approval_request_id: approval_request.approval_request_id,
                decision: contracts::ApprovalResolutionDecision::Approved,
                actor_ref: Some("cli:primary-user".to_string()),
                reason: Some("management integration test".to_string()),
            },
        )
        .await?;

        assert_eq!(resolution.approval_request.status, "approved");
        let governed_action = resolution
            .governed_action
            .expect("governed action should be included in management resolution output");
        assert_eq!(governed_action.status, "executed");

        let stored_request =
            approval::get_approval_request(&ctx.pool, approval_request.approval_request_id).await?;
        assert_eq!(stored_request.status, ApprovalRequestStatus::Approved);
        let stored_action = governed_actions::get_governed_action_execution_by_approval_request_id(
            &ctx.pool,
            approval_request.approval_request_id,
        )
        .await?
        .expect("governed action should stay linked to approval request");
        assert_eq!(
            stored_action.status,
            contracts::GovernedActionStatus::Executed
        );
        assert!(stored_action.output_ref.is_some());
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn supervise_worker_leases_recovers_expired_leases_from_management_surface() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let trace_id = Uuid::now_v7();
        let execution_id = seed_execution(&ctx.pool, trace_id).await?;
        let now = Utc::now();

        recovery::create_worker_lease(
            &ctx.pool,
            &recovery::NewWorkerLease {
                worker_lease_id: Uuid::now_v7(),
                trace_id,
                execution_id: Some(execution_id),
                background_job_id: None,
                background_job_run_id: None,
                governed_action_execution_id: None,
                worker_kind: recovery::WorkerLeaseKind::Foreground,
                lease_token: Uuid::now_v7(),
                worker_pid: Some(4242),
                lease_acquired_at: now - Duration::minutes(5),
                lease_expires_at: now - Duration::seconds(30),
                last_heartbeat_at: now - Duration::minutes(1),
                metadata: json!({
                    "source": "management_integration"
                }),
            },
        )
        .await?;

        let report = management::supervise_worker_leases(
            &ctx.config,
            SuperviseWorkerLeasesRequest {
                soft_warning_threshold_percent: 80,
                actor_ref: "cli:test-operator".to_string(),
                reason: Some("management integration test".to_string()),
            },
        )
        .await?;
        assert_eq!(report.actor_ref, "cli:test-operator");
        assert_eq!(
            report.reason.as_deref(),
            Some("management integration test")
        );
        assert_eq!(report.recovered_expired_lease_count, 1);
        assert_eq!(report.soft_warning_count, 0);
        assert_eq!(report.recovered_expired_leases.len(), 1);
        assert_eq!(report.recovered_expired_leases[0].worker_kind, "foreground");
        assert_eq!(
            report.recovered_expired_leases[0].recovery_decision,
            "continue"
        );
        assert_eq!(
            report.recovered_expired_leases[0].diagnostic_reason_code,
            "worker_lease_expired"
        );

        let checkpoints = management::list_recovery_checkpoints(&ctx.config, true, 10).await?;
        assert!(checkpoints.is_empty());

        let audit_events = audit::list_for_trace(&ctx.pool, report.trace_id).await?;
        let event_kinds = audit_events
            .iter()
            .map(|event| event.event_kind.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            event_kinds,
            vec![
                "management_recovery_supervision_requested",
                "management_recovery_supervision_completed",
            ]
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn recovery_lease_list_surfaces_active_soft_warning_leases_from_management_surface()
-> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let trace_id = Uuid::now_v7();
        let execution_id = seed_execution(&ctx.pool, trace_id).await?;
        let now = Utc::now();

        let lease = recovery::create_worker_lease(
            &ctx.pool,
            &recovery::NewWorkerLease {
                worker_lease_id: Uuid::now_v7(),
                trace_id,
                execution_id: Some(execution_id),
                background_job_id: None,
                background_job_run_id: None,
                governed_action_execution_id: None,
                worker_kind: recovery::WorkerLeaseKind::Background,
                lease_token: Uuid::now_v7(),
                worker_pid: Some(4242),
                lease_acquired_at: now - Duration::minutes(8),
                lease_expires_at: now + Duration::minutes(1),
                last_heartbeat_at: now - Duration::seconds(20),
                metadata: json!({
                    "source": "management_integration_recovery_lease_list"
                }),
            },
        )
        .await?;

        let leases = management::list_active_worker_leases(&ctx.config, 10, 80).await?;
        assert_eq!(leases.len(), 1);
        assert_eq!(leases[0].worker_lease_id, lease.worker_lease_id);
        assert_eq!(leases[0].worker_kind, "background");
        assert_eq!(leases[0].lease_status, "active");
        assert_eq!(leases[0].supervision_status, "soft_warning");
        assert_eq!(leases[0].execution_id, Some(execution_id));
        Ok(())
    })
    .await
}

fn configure_management_execution(config: &mut harness::config::RuntimeConfig) {
    let worker_binary = support::workers_binary().expect("workers binary should resolve");
    config.worker.command = worker_binary.to_string_lossy().into_owned();
    config.worker.args = vec!["unconscious-worker".to_string()];

    let api_key_env = format!(
        "BLUE_LAGOON_TEST_FOREGROUND_API_KEY_{}",
        uuid::Uuid::now_v7()
    );
    // SAFETY: these serial integration tests set a unique env var name and restore process
    // state only through process exit, so there is no concurrent aliasing across tests.
    unsafe {
        std::env::set_var(&api_key_env, "test-key");
    }

    config.model_gateway = Some(ModelGatewayConfig {
        foreground: ForegroundModelRouteConfig {
            provider: contracts::ModelProviderKind::ZAi,
            model: "z-ai-background".to_string(),
            api_base_url: Some("https://api.z.ai/api/paas/v4".to_string()),
            api_key_env,
            timeout_ms: 20_000,
        },
        z_ai: None,
        openrouter: None,
    });
}

async fn seed_execution(pool: &sqlx::PgPool, trace_id: Uuid) -> Result<Uuid> {
    let execution_id = Uuid::now_v7();
    execution::insert(
        pool,
        &NewExecutionRecord {
            execution_id,
            trace_id,
            trigger_kind: "management_integration_test".to_string(),
            synthetic_trigger: None,
            status: "started".to_string(),
            request_payload: json!({
                "request_id": Uuid::now_v7(),
                "sent_at": Utc::now(),
                "kind": "management_integration_test"
            }),
        },
    )
    .await?;
    Ok(execution_id)
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
