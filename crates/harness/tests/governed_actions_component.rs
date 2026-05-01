mod support;

use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use contracts::{
    ApprovalRequestStatus, ApprovalResolutionDecision, CapabilityScope, EnvironmentCapabilityScope,
    ExecutionCapabilityBudget, FilesystemCapabilityScope, GovernedActionFingerprint,
    GovernedActionKind, GovernedActionRiskTier, NetworkAccessPosture, WorkspaceArtifactKind,
    WorkspaceScriptRunStatus,
};
use serde_json::json;
use serial_test::serial;
use uuid::Uuid;

use harness::{
    approval::{self, ApprovalResolutionAttempt, NewApprovalRequestRecord},
    audit,
    governed_actions::{self, GovernedActionPlanningOutcome},
    identity,
    workspace::{
        self, NewWorkspaceArtifact, NewWorkspaceScript, NewWorkspaceScriptRun,
        NewWorkspaceScriptVersion, UpdateWorkspaceArtifact, UpdateWorkspaceScriptRunStatus,
        WorkspaceArtifactStatus,
    },
};

/// Exhaustive match — adding a GovernedActionKind variant without adding an arm here
/// is a compile error, forcing the developer to also provide the DB string and write a migration.
fn kind_str(kind: GovernedActionKind) -> &'static str {
    match kind {
        GovernedActionKind::InspectWorkspaceArtifact => "inspect_workspace_artifact",
        GovernedActionKind::ListWorkspaceArtifacts => "list_workspace_artifacts",
        GovernedActionKind::CreateWorkspaceArtifact => "create_workspace_artifact",
        GovernedActionKind::UpdateWorkspaceArtifact => "update_workspace_artifact",
        GovernedActionKind::ListWorkspaceScripts => "list_workspace_scripts",
        GovernedActionKind::InspectWorkspaceScript => "inspect_workspace_script",
        GovernedActionKind::CreateWorkspaceScript => "create_workspace_script",
        GovernedActionKind::AppendWorkspaceScriptVersion => "append_workspace_script_version",
        GovernedActionKind::ListWorkspaceScriptRuns => "list_workspace_script_runs",
        GovernedActionKind::UpsertScheduledForegroundTask => "upsert_scheduled_foreground_task",
        GovernedActionKind::RequestBackgroundJob => "request_background_job",
        GovernedActionKind::RunSubprocess => "run_subprocess",
        GovernedActionKind::RunWorkspaceScript => "run_workspace_script",
        GovernedActionKind::WebFetch => "web_fetch",
    }
}

#[tokio::test]
#[serial]
async fn governed_action_planning_persists_planned_and_blocked_outcomes() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let planned = governed_actions::plan_governed_action(
            &ctx.config,
            &ctx.pool,
            &governed_actions::GovernedActionPlanningRequest {
                governed_action_execution_id: Uuid::now_v7(),
                trace_id: Uuid::now_v7(),
                execution_id: None,
                proposal: contracts::GovernedActionProposal {
                    proposal_id: Uuid::now_v7(),
                    title: "Run bounded subprocess".to_string(),
                    rationale: Some("Used to verify planning persistence.".to_string()),
                    action_kind: GovernedActionKind::RunSubprocess,
                    requested_risk_tier: None,
                    capability_scope: sample_capability_scope(),
                    payload: contracts::GovernedActionPayload::RunSubprocess(
                        contracts::SubprocessAction {
                            command: "cmd".to_string(),
                            args: vec!["/c".to_string(), "echo".to_string(), "hello".to_string()],
                            working_directory: Some("D:/Repos/blue-lagoon".to_string()),
                        },
                    ),
                },
            },
        )
        .await?;

        let planned = match planned {
            GovernedActionPlanningOutcome::Planned(planned) => planned,
            other => panic!("expected planned governed action, got {other:?}"),
        };
        assert!(planned.requires_approval);
        assert_eq!(
            planned.record.status,
            contracts::GovernedActionStatus::AwaitingApproval
        );
        assert_eq!(planned.record.risk_tier, GovernedActionRiskTier::Tier2);
        assert!(
            planned
                .record
                .action_fingerprint
                .value
                .starts_with("sha256:")
        );

        let by_fingerprint = governed_actions::get_latest_governed_action_execution_by_fingerprint(
            &ctx.pool,
            &planned.record.action_fingerprint,
        )
        .await?
        .expect("planned governed action should be queryable by fingerprint");
        assert_eq!(
            by_fingerprint.governed_action_execution_id,
            planned.record.governed_action_execution_id
        );

        let blocked = governed_actions::plan_governed_action(
            &ctx.config,
            &ctx.pool,
            &governed_actions::GovernedActionPlanningRequest {
                governed_action_execution_id: Uuid::now_v7(),
                trace_id: Uuid::now_v7(),
                execution_id: None,
                proposal: contracts::GovernedActionProposal {
                    proposal_id: Uuid::now_v7(),
                    title: "Blocked subprocess".to_string(),
                    rationale: Some("Used to verify blocked-action persistence.".to_string()),
                    action_kind: GovernedActionKind::RunSubprocess,
                    requested_risk_tier: None,
                    capability_scope: CapabilityScope {
                        environment: EnvironmentCapabilityScope {
                            allow_variables: vec!["HOME".to_string()],
                        },
                        ..sample_capability_scope()
                    },
                    payload: contracts::GovernedActionPayload::RunSubprocess(
                        contracts::SubprocessAction {
                            command: "cmd".to_string(),
                            args: vec!["/c".to_string(), "echo".to_string(), "blocked".to_string()],
                            working_directory: Some("D:/Repos/blue-lagoon".to_string()),
                        },
                    ),
                },
            },
        )
        .await?;

        let blocked = match blocked {
            GovernedActionPlanningOutcome::Blocked(blocked) => blocked,
            other => panic!("expected blocked governed action, got {other:?}"),
        };
        assert_eq!(
            blocked.record.status,
            contracts::GovernedActionStatus::Blocked
        );
        assert!(
            blocked
                .record
                .blocked_reason
                .as_deref()
                .unwrap_or_default()
                .contains("not allowlisted")
        );
        assert_eq!(
            blocked.outcome.status,
            contracts::GovernedActionStatus::Blocked
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn identity_boundary_blocks_matching_governed_action_policy() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        identity::insert_identity_item(
            &ctx.pool,
            &identity::NewIdentityItem {
                identity_item_id: Uuid::now_v7(),
                self_model_artifact_id: None,
                proposal_id: None,
                trace_id: Some(Uuid::now_v7()),
                stability_class: "stable".to_string(),
                category: "enduring_boundary".to_string(),
                item_key: "network_governance".to_string(),
                value_text: "Do not use network access for governed actions.".to_string(),
                confidence: 1.0,
                weight: Some(1.0),
                provenance_kind: "operator_authored".to_string(),
                source_kind: "operator_authored".to_string(),
                merge_policy: "protected_core".to_string(),
                status: "active".to_string(),
                evidence_refs: json!([]),
                valid_from: Some(Utc::now()),
                valid_to: None,
                supersedes_item_id: None,
                payload: json!({ "component_test": true }),
            },
        )
        .await?;

        let outcome = governed_actions::plan_governed_action(
            &ctx.config,
            &ctx.pool,
            &governed_actions::GovernedActionPlanningRequest {
                governed_action_execution_id: Uuid::now_v7(),
                trace_id: Uuid::now_v7(),
                execution_id: None,
                proposal: contracts::GovernedActionProposal {
                    proposal_id: Uuid::now_v7(),
                    title: "Fetch under identity boundary".to_string(),
                    rationale: Some("Verify identity boundary policy.".to_string()),
                    action_kind: GovernedActionKind::WebFetch,
                    requested_risk_tier: None,
                    capability_scope: web_fetch_capability_scope(),
                    payload: contracts::GovernedActionPayload::WebFetch(sample_web_fetch_action()),
                },
            },
        )
        .await?;

        let blocked = match outcome {
            GovernedActionPlanningOutcome::Blocked(blocked) => blocked,
            other => panic!("expected identity boundary to block action, got {other:?}"),
        };
        let reason = blocked.record.blocked_reason.as_deref().unwrap_or_default();
        assert!(reason.contains("identity boundary"));
        assert!(reason.contains("network access"));
        assert_eq!(
            blocked.record.status,
            contracts::GovernedActionStatus::Blocked
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn workspace_service_persists_artifacts_scripts_versions_and_runs() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let note_id = Uuid::now_v7();
        let note = workspace::create_workspace_artifact(
            &ctx.config,
            &ctx.pool,
            &NewWorkspaceArtifact {
                workspace_artifact_id: note_id,
                trace_id: Some(Uuid::now_v7()),
                execution_id: None,
                artifact_kind: WorkspaceArtifactKind::Note,
                title: "Operator note".to_string(),
                content_text: Some("Workspace service smoke".to_string()),
                metadata: json!({ "source": "component_test" }),
            },
        )
        .await?;
        assert_eq!(note.workspace_artifact_id, note_id);
        assert_eq!(note.artifact_kind, WorkspaceArtifactKind::Note);
        assert_eq!(note.status, WorkspaceArtifactStatus::Active);

        let updated_note = workspace::update_workspace_artifact(
            &ctx.config,
            &ctx.pool,
            &UpdateWorkspaceArtifact {
                workspace_artifact_id: note_id,
                title: "Operator note updated".to_string(),
                content_text: Some("Updated workspace note".to_string()),
                status: WorkspaceArtifactStatus::Archived,
                metadata: json!({ "source": "component_test", "revision": 2 }),
            },
        )
        .await?;
        assert_eq!(updated_note.status, WorkspaceArtifactStatus::Archived);
        assert_eq!(updated_note.title, "Operator note updated");

        let script_artifact_id = Uuid::now_v7();
        let script_id = Uuid::now_v7();
        let version_one_id = Uuid::now_v7();
        let created_script = workspace::create_workspace_script(
            &ctx.config,
            &ctx.pool,
            &NewWorkspaceScript {
                workspace_script_id: script_id,
                workspace_artifact_id: script_artifact_id,
                workspace_script_version_id: version_one_id,
                trace_id: Some(Uuid::now_v7()),
                execution_id: None,
                title: "Verification script".to_string(),
                metadata: json!({ "purpose": "governed_action_component_test" }),
                language: "python".to_string(),
                entrypoint: Some("main.py".to_string()),
                content_text: "print('v1')\n".to_string(),
                change_summary: Some("initial version".to_string()),
            },
        )
        .await?;
        assert_eq!(created_script.script.latest_version, 1);
        assert_eq!(created_script.initial_version.version, 1);

        let version_two = workspace::append_workspace_script_version(
            &ctx.config,
            &ctx.pool,
            &NewWorkspaceScriptVersion {
                workspace_script_version_id: Uuid::now_v7(),
                workspace_script_id: script_id,
                content_text: "print('v2')\n".to_string(),
                change_summary: Some("add second revision".to_string()),
            },
        )
        .await?;
        assert_eq!(version_two.version, 2);

        let script = workspace::get_workspace_script(&ctx.pool, script_id).await?;
        assert_eq!(script.latest_version, 2);

        let latest_version = workspace::get_latest_workspace_script_version(&ctx.pool, script_id)
            .await?
            .expect("latest script version should exist");
        assert_eq!(latest_version.version, 2);
        assert_eq!(latest_version.content_text, "print('v2')\n");

        let version_summaries =
            workspace::list_workspace_script_versions(&ctx.pool, script_id, 10).await?;
        assert_eq!(version_summaries.len(), 2);
        assert_eq!(version_summaries[0].version, 2);
        assert_eq!(version_summaries[1].version, 1);

        let run_id = Uuid::now_v7();
        let recorded_run = workspace::record_workspace_script_run(
            &ctx.pool,
            &NewWorkspaceScriptRun {
                workspace_script_run_id: run_id,
                workspace_script_id: script_id,
                workspace_script_version_id: version_two.workspace_script_version_id,
                trace_id: Uuid::now_v7(),
                execution_id: None,
                governed_action_execution_id: None,
                approval_request_id: None,
                status: WorkspaceScriptRunStatus::Pending,
                risk_tier: GovernedActionRiskTier::Tier1,
                args: vec!["--dry-run".to_string()],
                output_ref: None,
                failure_summary: None,
                started_at: None,
                completed_at: None,
            },
        )
        .await?;
        assert_eq!(recorded_run.status, WorkspaceScriptRunStatus::Pending);
        assert_eq!(recorded_run.args, vec!["--dry-run".to_string()]);

        let started_at = Utc::now();
        let completed_run = workspace::update_workspace_script_run_status(
            &ctx.pool,
            &UpdateWorkspaceScriptRunStatus {
                workspace_script_run_id: run_id,
                status: WorkspaceScriptRunStatus::Completed,
                output_ref: Some("workspace://runs/output-1".to_string()),
                failure_summary: None,
                started_at: Some(started_at),
                completed_at: Some(started_at + Duration::seconds(3)),
            },
        )
        .await?;
        assert_eq!(completed_run.status, WorkspaceScriptRunStatus::Completed);
        assert_eq!(
            completed_run.output_ref.as_deref(),
            Some("workspace://runs/output-1")
        );

        let artifact_summaries =
            workspace::list_workspace_artifact_summaries(&ctx.pool, 10).await?;
        assert_eq!(artifact_summaries.len(), 2);
        assert!(
            artifact_summaries
                .iter()
                .any(|summary| summary.artifact_id == note_id && summary.latest_version == 1)
        );
        assert!(artifact_summaries.iter().any(
            |summary| summary.artifact_id == script_artifact_id && summary.latest_version == 2
        ));

        let script_summaries = workspace::list_workspace_scripts(&ctx.pool, 10).await?;
        assert_eq!(script_summaries.len(), 1);
        assert_eq!(script_summaries[0].script_id, script_id);
        assert_eq!(script_summaries[0].latest_version, 2);

        let run_summaries = workspace::list_workspace_script_runs(&ctx.pool, script_id, 10).await?;
        assert_eq!(run_summaries.len(), 1);
        assert_eq!(run_summaries[0].script_run_id, run_id);
        assert_eq!(run_summaries[0].status, WorkspaceScriptRunStatus::Completed);
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn approval_service_persists_resolution_and_audit_history() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let trace_id = Uuid::now_v7();
        let fingerprint = GovernedActionFingerprint {
            value: "sha256:approval-ok".to_string(),
        };
        let created = approval::create_approval_request(
            &ctx.config,
            &ctx.pool,
            &NewApprovalRequestRecord {
                approval_request_id: Uuid::now_v7(),
                trace_id,
                execution_id: None,
                action_proposal_id: Uuid::now_v7(),
                action_fingerprint: fingerprint.clone(),
                action_kind: GovernedActionKind::RunSubprocess,
                risk_tier: GovernedActionRiskTier::Tier2,
                title: "Run bounded subprocess".to_string(),
                consequence_summary: "Runs a scoped subprocess inside the workspace boundary."
                    .to_string(),
                capability_scope: sample_capability_scope(),
                requested_by: "telegram:primary-user".to_string(),
                token: "approval-token-ok".to_string(),
                requested_at: Utc::now(),
                expires_at: Utc::now() + Duration::minutes(15),
            },
        )
        .await?;
        assert_eq!(created.status, ApprovalRequestStatus::Pending);
        assert_eq!(created.to_contract().status, ApprovalRequestStatus::Pending);

        let by_token = approval::get_approval_request_by_token(&ctx.pool, "approval-token-ok")
            .await?
            .expect("approval request should be found by token");
        assert_eq!(by_token.approval_request_id, created.approval_request_id);

        let by_fingerprint =
            approval::get_pending_approval_request_by_fingerprint(&ctx.pool, &fingerprint)
                .await?
                .expect("pending approval should be found by fingerprint");
        assert_eq!(
            by_fingerprint.approval_request_id,
            created.approval_request_id
        );

        let resolved = approval::resolve_approval_request(
            &ctx.pool,
            &ApprovalResolutionAttempt {
                token: "approval-token-ok".to_string(),
                actor_ref: "telegram:primary-user".to_string(),
                expected_action_fingerprint: fingerprint,
                decision: ApprovalResolutionDecision::Approved,
                reason: Some("verified during component test".to_string()),
                resolved_at: Utc::now(),
            },
        )
        .await?;
        assert_eq!(resolved.request.status, ApprovalRequestStatus::Approved);
        assert_eq!(
            resolved.event.decision,
            ApprovalResolutionDecision::Approved
        );

        let all_requests = approval::list_approval_requests(&ctx.pool, None, 10).await?;
        assert_eq!(all_requests.len(), 1);
        assert_eq!(all_requests[0].status, ApprovalRequestStatus::Approved);

        let events = audit::list_for_trace(&ctx.pool, trace_id).await?;
        let event_kinds = events
            .into_iter()
            .map(|event| event.event_kind)
            .collect::<Vec<_>>();
        assert!(event_kinds.contains(&"approval_request_created".to_string()));
        assert!(event_kinds.contains(&"approval_request_approved".to_string()));

        let second_attempt = approval::resolve_approval_request(
            &ctx.pool,
            &ApprovalResolutionAttempt {
                token: "approval-token-ok".to_string(),
                actor_ref: "telegram:primary-user".to_string(),
                expected_action_fingerprint: GovernedActionFingerprint {
                    value: "sha256:approval-ok".to_string(),
                },
                decision: ApprovalResolutionDecision::Approved,
                reason: None,
                resolved_at: Utc::now(),
            },
        )
        .await
        .expect_err("approved request should be one-shot");
        assert!(second_attempt.to_string().contains("no longer pending"));
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn approval_service_rejects_mismatched_actor_and_non_user_decisions() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let fingerprint = GovernedActionFingerprint {
            value: "sha256:actor-guard".to_string(),
        };
        approval::create_approval_request(
            &ctx.config,
            &ctx.pool,
            &NewApprovalRequestRecord {
                approval_request_id: Uuid::now_v7(),
                trace_id: Uuid::now_v7(),
                execution_id: None,
                action_proposal_id: Uuid::now_v7(),
                action_fingerprint: fingerprint.clone(),
                action_kind: GovernedActionKind::RunSubprocess,
                risk_tier: GovernedActionRiskTier::Tier2,
                title: "Actor guard".to_string(),
                consequence_summary: "Used to verify actor identity validation.".to_string(),
                capability_scope: sample_capability_scope(),
                requested_by: "telegram:primary-user".to_string(),
                token: "approval-token-actor-guard".to_string(),
                requested_at: Utc::now(),
                expires_at: Utc::now() + Duration::minutes(15),
            },
        )
        .await?;

        let actor_error = approval::resolve_approval_request(
            &ctx.pool,
            &ApprovalResolutionAttempt {
                token: "approval-token-actor-guard".to_string(),
                actor_ref: "telegram:other-user".to_string(),
                expected_action_fingerprint: fingerprint.clone(),
                decision: ApprovalResolutionDecision::Approved,
                reason: None,
                resolved_at: Utc::now(),
            },
        )
        .await
        .expect_err("mismatched actor should be rejected");
        assert!(actor_error.to_string().contains("requested principal"));

        let decision_error = approval::resolve_approval_request(
            &ctx.pool,
            &ApprovalResolutionAttempt {
                token: "approval-token-actor-guard".to_string(),
                actor_ref: "telegram:primary-user".to_string(),
                expected_action_fingerprint: fingerprint,
                decision: ApprovalResolutionDecision::Expired,
                reason: None,
                resolved_at: Utc::now(),
            },
        )
        .await
        .expect_err("direct expiry decisions should be rejected");
        assert!(decision_error.to_string().contains("approve/reject"));

        let request =
            approval::get_approval_request_by_token(&ctx.pool, "approval-token-actor-guard")
                .await?
                .expect("approval request should still exist");
        assert_eq!(request.status, ApprovalRequestStatus::Pending);
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn approval_service_invalidates_mismatched_fingerprint_and_expires_due_requests() -> Result<()>
{
    support::with_migrated_database(|ctx| async move {
        let invalidated_trace_id = Uuid::now_v7();
        let invalidated = approval::create_approval_request(
            &ctx.config,
            &ctx.pool,
            &NewApprovalRequestRecord {
                approval_request_id: Uuid::now_v7(),
                trace_id: invalidated_trace_id,
                execution_id: None,
                action_proposal_id: Uuid::now_v7(),
                action_fingerprint: GovernedActionFingerprint {
                    value: "sha256:expected".to_string(),
                },
                action_kind: GovernedActionKind::RunSubprocess,
                risk_tier: GovernedActionRiskTier::Tier2,
                title: "Fingerprint guard".to_string(),
                consequence_summary: "Used to verify invalidation when the proposal changes."
                    .to_string(),
                capability_scope: sample_capability_scope(),
                requested_by: "telegram:primary-user".to_string(),
                token: "approval-token-mismatch".to_string(),
                requested_at: Utc::now(),
                expires_at: Utc::now() + Duration::minutes(15),
            },
        )
        .await?;

        let invalidated_result = approval::resolve_approval_request(
            &ctx.pool,
            &ApprovalResolutionAttempt {
                token: "approval-token-mismatch".to_string(),
                actor_ref: "telegram:primary-user".to_string(),
                expected_action_fingerprint: GovernedActionFingerprint {
                    value: "sha256:changed".to_string(),
                },
                decision: ApprovalResolutionDecision::Approved,
                reason: Some("should not be applied".to_string()),
                resolved_at: Utc::now(),
            },
        )
        .await?;
        assert_eq!(
            invalidated_result.request.approval_request_id,
            invalidated.approval_request_id
        );
        assert_eq!(
            invalidated_result.request.status,
            ApprovalRequestStatus::Invalidated
        );
        assert_eq!(
            invalidated_result.event.decision,
            ApprovalResolutionDecision::Invalidated
        );

        let expiry_trace_id = Uuid::now_v7();
        let expired_request = approval::create_approval_request(
            &ctx.config,
            &ctx.pool,
            &NewApprovalRequestRecord {
                approval_request_id: Uuid::now_v7(),
                trace_id: expiry_trace_id,
                execution_id: None,
                action_proposal_id: Uuid::now_v7(),
                action_fingerprint: GovernedActionFingerprint {
                    value: "sha256:expired".to_string(),
                },
                action_kind: GovernedActionKind::RunSubprocess,
                risk_tier: GovernedActionRiskTier::Tier2,
                title: "Expiry guard".to_string(),
                consequence_summary: "Used to verify pending approval expiry.".to_string(),
                capability_scope: sample_capability_scope(),
                requested_by: "telegram:primary-user".to_string(),
                token: "approval-token-expired".to_string(),
                requested_at: Utc::now() - Duration::minutes(20),
                expires_at: Utc::now() - Duration::minutes(5),
            },
        )
        .await?;

        let expired = approval::expire_due_approval_requests(&ctx.pool, Utc::now()).await?;
        assert_eq!(expired.len(), 1);
        assert_eq!(
            expired[0].request.approval_request_id,
            expired_request.approval_request_id
        );
        assert_eq!(expired[0].request.status, ApprovalRequestStatus::Expired);
        assert_eq!(
            expired[0].event.decision,
            ApprovalResolutionDecision::Expired
        );

        let events = audit::list_for_trace(&ctx.pool, expiry_trace_id).await?;
        let event_kinds = events
            .into_iter()
            .map(|event| event.event_kind)
            .collect::<Vec<_>>();
        assert!(event_kinds.contains(&"approval_request_created".to_string()));
        assert!(event_kinds.contains(&"approval_request_expired".to_string()));
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn governed_action_execution_runs_bounded_subprocess_and_persists_outcome() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let proposal = contracts::GovernedActionProposal {
            proposal_id: Uuid::now_v7(),
            title: "Immediate subprocess".to_string(),
            rationale: Some("Used to verify bounded subprocess execution.".to_string()),
            action_kind: GovernedActionKind::RunSubprocess,
            requested_risk_tier: None,
            capability_scope: execution_capability_scope(),
            payload: contracts::GovernedActionPayload::RunSubprocess(platform_echo_action(
                "governed-subprocess",
            )),
        };

        let planned = governed_actions::plan_governed_action(
            &ctx.config,
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
        assert!(!planned.requires_approval);

        let executed =
            governed_actions::execute_governed_action(&ctx.config, &ctx.pool, &planned.record)
                .await?;
        assert_eq!(
            executed.record.status,
            contracts::GovernedActionStatus::Executed
        );
        assert_eq!(
            executed.outcome.status,
            contracts::GovernedActionStatus::Executed
        );
        assert!(executed.record.execution_id.is_some());
        assert!(executed.record.output_ref.is_some());

        let execution_record = harness::execution::get(
            &ctx.pool,
            executed
                .record
                .execution_id
                .expect("governed action execution id should be set"),
        )
        .await?;
        assert_eq!(execution_record.status, "completed");
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn governed_action_execution_records_workspace_script_runs() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let script_language = if cfg!(windows) { "powershell" } else { "sh" };
        let script_content = if cfg!(windows) {
            "Write-Output 'workspace script ok'\n"
        } else {
            "printf 'workspace script ok\\n'\n"
        };
        let created_script = workspace::create_workspace_script(
            &ctx.config,
            &ctx.pool,
            &NewWorkspaceScript {
                workspace_script_id: Uuid::now_v7(),
                workspace_artifact_id: Uuid::now_v7(),
                workspace_script_version_id: Uuid::now_v7(),
                trace_id: Some(Uuid::now_v7()),
                execution_id: None,
                title: "Governed workspace script".to_string(),
                metadata: json!({ "source": "component_test" }),
                language: script_language.to_string(),
                entrypoint: None,
                content_text: script_content.to_string(),
                change_summary: Some("initial".to_string()),
            },
        )
        .await?;

        let proposal = contracts::GovernedActionProposal {
            proposal_id: Uuid::now_v7(),
            title: "Run workspace script".to_string(),
            rationale: Some("Used to verify script run history.".to_string()),
            action_kind: GovernedActionKind::RunWorkspaceScript,
            requested_risk_tier: None,
            capability_scope: execution_capability_scope(),
            payload: contracts::GovernedActionPayload::RunWorkspaceScript(
                contracts::WorkspaceScriptAction {
                    script_id: created_script.script.workspace_script_id,
                    script_version_id: Some(
                        created_script.initial_version.workspace_script_version_id,
                    ),
                    args: Vec::new(),
                },
            ),
        };

        let planned = governed_actions::plan_governed_action(
            &ctx.config,
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
        assert!(!planned.requires_approval);

        let executed =
            governed_actions::execute_governed_action(&ctx.config, &ctx.pool, &planned.record)
                .await?;
        assert_eq!(
            executed.record.status,
            contracts::GovernedActionStatus::Executed
        );
        let script_run = executed
            .script_run
            .expect("workspace script execution should record a run");
        assert_eq!(script_run.status, WorkspaceScriptRunStatus::Completed);

        let run_summaries = workspace::list_workspace_script_runs(
            &ctx.pool,
            created_script.script.workspace_script_id,
            10,
        )
        .await?;
        assert_eq!(run_summaries.len(), 1);
        assert_eq!(run_summaries[0].status, WorkspaceScriptRunStatus::Completed);
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn governed_action_execution_blocks_unsupported_network_enabled_backend() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut scope = execution_capability_scope();
        scope.network = NetworkAccessPosture::Enabled;
        let proposal = contracts::GovernedActionProposal {
            proposal_id: Uuid::now_v7(),
            title: "Network subprocess".to_string(),
            rationale: Some("Used to verify fail-closed backend blocking.".to_string()),
            action_kind: GovernedActionKind::RunSubprocess,
            requested_risk_tier: Some(GovernedActionRiskTier::Tier2),
            capability_scope: scope,
            payload: contracts::GovernedActionPayload::RunSubprocess(platform_echo_action(
                "network-subprocess",
            )),
        };

        let planned = governed_actions::plan_governed_action(
            &ctx.config,
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
            other => panic!("expected approval-gated governed action, got {other:?}"),
        };
        assert!(planned.requires_approval);

        let blocked =
            governed_actions::execute_governed_action(&ctx.config, &ctx.pool, &planned.record)
                .await?;
        assert_eq!(
            blocked.record.status,
            contracts::GovernedActionStatus::Blocked
        );
        assert!(
            blocked
                .outcome
                .summary
                .contains("network-enabled execution")
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn governed_action_workspace_and_script_tools_execute_through_harness() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let created_artifact = execute_test_action(
            &ctx.config,
            &ctx.pool,
            GovernedActionKind::CreateWorkspaceArtifact,
            contracts::GovernedActionPayload::CreateWorkspaceArtifact(
                contracts::CreateWorkspaceArtifactAction {
                    artifact_kind: WorkspaceArtifactKind::Scratchpad,
                    title: "Governed scratchpad".to_string(),
                    content_text: "first content".to_string(),
                    provenance: Some("component test".to_string()),
                },
            ),
        )
        .await?;
        assert_eq!(
            created_artifact.record.status,
            contracts::GovernedActionStatus::Executed
        );
        let artifact_id = created_artifact.outcome.summary.split_whitespace().nth(3);
        assert!(artifact_id.is_some());

        let artifacts = workspace::list_workspace_artifacts(&ctx.pool, 10).await?;
        let artifact = artifacts
            .iter()
            .find(|artifact| artifact.title == "Governed scratchpad")
            .expect("created artifact should be persisted");

        let inspected = execute_test_action(
            &ctx.config,
            &ctx.pool,
            GovernedActionKind::InspectWorkspaceArtifact,
            contracts::GovernedActionPayload::InspectWorkspaceArtifact(
                contracts::InspectWorkspaceArtifactAction {
                    artifact_id: artifact.workspace_artifact_id,
                    artifact_kind: WorkspaceArtifactKind::Scratchpad,
                },
            ),
        )
        .await?;
        assert!(inspected.outcome.summary.contains("first content"));

        let listed = execute_test_action(
            &ctx.config,
            &ctx.pool,
            GovernedActionKind::ListWorkspaceArtifacts,
            contracts::GovernedActionPayload::ListWorkspaceArtifacts(
                contracts::ListWorkspaceArtifactsAction {
                    artifact_kind: Some(WorkspaceArtifactKind::Scratchpad),
                    status: contracts::WorkspaceArtifactStatusFilter::Active,
                    query: Some("first".to_string()),
                    limit: 5,
                },
            ),
        )
        .await?;
        assert!(
            listed
                .outcome
                .summary
                .contains("listed 1 workspace artifacts")
        );

        let updated = execute_test_action(
            &ctx.config,
            &ctx.pool,
            GovernedActionKind::UpdateWorkspaceArtifact,
            contracts::GovernedActionPayload::UpdateWorkspaceArtifact(
                contracts::UpdateWorkspaceArtifactAction {
                    artifact_id: artifact.workspace_artifact_id,
                    expected_updated_at: Some(artifact.updated_at),
                    title: Some("Governed scratchpad updated".to_string()),
                    content_text: "second content".to_string(),
                    change_summary: "replace content".to_string(),
                },
            ),
        )
        .await?;
        assert!(
            updated
                .outcome
                .summary
                .contains("updated workspace artifact")
        );

        let created_script = execute_test_action(
            &ctx.config,
            &ctx.pool,
            GovernedActionKind::CreateWorkspaceScript,
            contracts::GovernedActionPayload::CreateWorkspaceScript(
                contracts::CreateWorkspaceScriptAction {
                    title: "Governed script".to_string(),
                    language: "python".to_string(),
                    content_text: "print('v1')".to_string(),
                    description: Some("component test".to_string()),
                    requested_capabilities: Vec::new(),
                },
            ),
        )
        .await?;
        assert!(
            created_script
                .outcome
                .summary
                .contains("created workspace script")
        );

        let scripts = workspace::list_workspace_scripts(&ctx.pool, 10).await?;
        let script = scripts
            .iter()
            .find(|script| script.language == "python")
            .expect("created script should be persisted");
        let latest = workspace::get_latest_workspace_script_version(&ctx.pool, script.script_id)
            .await?
            .expect("script should have initial version");

        let inspected_script = execute_test_action(
            &ctx.config,
            &ctx.pool,
            GovernedActionKind::InspectWorkspaceScript,
            contracts::GovernedActionPayload::InspectWorkspaceScript(
                contracts::InspectWorkspaceScriptAction {
                    script_id: script.script_id,
                    script_version_id: None,
                },
            ),
        )
        .await?;
        assert!(inspected_script.outcome.summary.contains("print('v1')"));

        let appended = execute_test_action(
            &ctx.config,
            &ctx.pool,
            GovernedActionKind::AppendWorkspaceScriptVersion,
            contracts::GovernedActionPayload::AppendWorkspaceScriptVersion(
                contracts::AppendWorkspaceScriptVersionAction {
                    script_id: script.script_id,
                    expected_latest_version_id: Some(latest.workspace_script_version_id),
                    expected_content_sha256: Some(latest.content_sha256),
                    language: "python".to_string(),
                    content_text: "print('v2')".to_string(),
                    change_summary: "version two".to_string(),
                },
            ),
        )
        .await?;
        assert!(appended.outcome.summary.contains("version 2"));

        let listed_scripts = execute_test_action(
            &ctx.config,
            &ctx.pool,
            GovernedActionKind::ListWorkspaceScripts,
            contracts::GovernedActionPayload::ListWorkspaceScripts(
                contracts::ListWorkspaceScriptsAction {
                    status: contracts::WorkspaceArtifactStatusFilter::Active,
                    language: Some("python".to_string()),
                    query: Some("Governed script".to_string()),
                    limit: 5,
                },
            ),
        )
        .await?;
        assert!(
            listed_scripts
                .outcome
                .summary
                .contains("listed 1 workspace scripts")
        );

        workspace::record_workspace_script_run(
            &ctx.pool,
            &NewWorkspaceScriptRun {
                workspace_script_run_id: Uuid::now_v7(),
                workspace_script_id: script.script_id,
                workspace_script_version_id: latest.workspace_script_version_id,
                trace_id: Uuid::now_v7(),
                execution_id: None,
                governed_action_execution_id: None,
                approval_request_id: None,
                status: WorkspaceScriptRunStatus::Completed,
                risk_tier: GovernedActionRiskTier::Tier1,
                args: Vec::new(),
                output_ref: Some("execution_record:test".to_string()),
                failure_summary: None,
                started_at: Some(Utc::now()),
                completed_at: Some(Utc::now()),
            },
        )
        .await?;
        let listed_runs = execute_test_action(
            &ctx.config,
            &ctx.pool,
            GovernedActionKind::ListWorkspaceScriptRuns,
            contracts::GovernedActionPayload::ListWorkspaceScriptRuns(
                contracts::ListWorkspaceScriptRunsAction {
                    script_id: script.script_id,
                    status: Some(WorkspaceScriptRunStatus::Completed),
                    limit: 5,
                },
            ),
        )
        .await?;
        assert!(
            listed_runs
                .outcome
                .summary
                .contains("listed 1 workspace script runs")
        );

        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn governed_action_schedule_and_background_tools_execute_through_harness() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let scheduled = execute_test_action(
            &ctx.config,
            &ctx.pool,
            GovernedActionKind::UpsertScheduledForegroundTask,
            contracts::GovernedActionPayload::UpsertScheduledForegroundTask(
                contracts::UpsertScheduledForegroundTaskAction {
                    task_key: "component_check_in".to_string(),
                    title: "Component check in".to_string(),
                    user_facing_prompt: "Check in from component test".to_string(),
                    next_due_at_utc: Some(Utc::now() + Duration::minutes(10)),
                    cadence_seconds: ctx.config.scheduled_foreground.min_cadence_seconds,
                    cooldown_seconds: Some(
                        ctx.config.scheduled_foreground.default_cooldown_seconds,
                    ),
                    internal_principal_ref: "primary-user".to_string(),
                    internal_conversation_ref: "telegram-primary".to_string(),
                    active: true,
                },
            ),
        )
        .await?;
        assert!(
            scheduled
                .outcome
                .summary
                .contains("scheduled foreground task")
        );

        let task =
            harness::scheduled_foreground::get_task_by_key(&ctx.pool, "component_check_in").await?;
        assert!(task.is_some());

        let background = execute_test_action(
            &ctx.config,
            &ctx.pool,
            GovernedActionKind::RequestBackgroundJob,
            contracts::GovernedActionPayload::RequestBackgroundJob(
                contracts::RequestBackgroundJobAction {
                    job_kind: contracts::UnconsciousJobKind::MemoryConsolidation,
                    rationale: "component test background request".to_string(),
                    input_scope_ref: Some("component-test".to_string()),
                    urgency: Some("normal".to_string()),
                    wake_preference: None,
                    internal_conversation_ref: Some("telegram-primary".to_string()),
                },
            ),
        )
        .await?;
        assert!(
            background
                .outcome
                .summary
                .contains("accepted background job request")
                || background
                    .outcome
                    .summary
                    .contains("suppressed duplicate background job request")
        );
        Ok(())
    })
    .await
}

fn sample_capability_scope() -> CapabilityScope {
    CapabilityScope {
        filesystem: FilesystemCapabilityScope {
            read_roots: vec![support::workspace_root().display().to_string()],
            write_roots: vec![support::workspace_root().join("docs").display().to_string()],
        },
        network: NetworkAccessPosture::Disabled,
        environment: EnvironmentCapabilityScope {
            allow_variables: vec!["BLUE_LAGOON_DATABASE_URL".to_string()],
        },
        execution: ExecutionCapabilityBudget {
            timeout_ms: 30_000,
            max_stdout_bytes: 65_536,
            max_stderr_bytes: 32_768,
        },
    }
}

fn execution_capability_scope() -> CapabilityScope {
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

fn web_fetch_capability_scope() -> CapabilityScope {
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

fn harness_native_capability_scope() -> CapabilityScope {
    CapabilityScope {
        filesystem: FilesystemCapabilityScope {
            read_roots: Vec::new(),
            write_roots: Vec::new(),
        },
        network: NetworkAccessPosture::Disabled,
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

async fn execute_test_action(
    config: &harness::config::RuntimeConfig,
    pool: &sqlx::PgPool,
    action_kind: GovernedActionKind,
    payload: contracts::GovernedActionPayload,
) -> Result<governed_actions::GovernedActionExecutionResult> {
    let planned = governed_actions::plan_governed_action(
        config,
        pool,
        &governed_actions::GovernedActionPlanningRequest {
            governed_action_execution_id: Uuid::now_v7(),
            trace_id: Uuid::now_v7(),
            execution_id: None,
            proposal: contracts::GovernedActionProposal {
                proposal_id: Uuid::now_v7(),
                title: format!("{action_kind:?}"),
                rationale: Some("component test".to_string()),
                action_kind,
                requested_risk_tier: Some(GovernedActionRiskTier::Tier0),
                capability_scope: harness_native_capability_scope(),
                payload,
            },
        },
    )
    .await?;
    let planned = match planned {
        GovernedActionPlanningOutcome::Planned(planned) => planned,
        other => panic!("expected planned governed action, got {other:?}"),
    };
    governed_actions::execute_governed_action(config, pool, &planned.record).await
}

fn sample_web_fetch_action() -> contracts::WebFetchAction {
    contracts::WebFetchAction {
        url: "https://example.com".to_string(),
        timeout_ms: 10_000,
        max_response_bytes: 65_536,
    }
}

#[tokio::test]
#[serial]
async fn web_fetch_proposal_is_planned_with_tier2_and_requires_approval() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let planned = governed_actions::plan_governed_action(
            &ctx.config,
            &ctx.pool,
            &governed_actions::GovernedActionPlanningRequest {
                governed_action_execution_id: Uuid::now_v7(),
                trace_id: Uuid::now_v7(),
                execution_id: None,
                proposal: contracts::GovernedActionProposal {
                    proposal_id: Uuid::now_v7(),
                    title: "Fetch example.com".to_string(),
                    rationale: Some("Verify web fetch planning.".to_string()),
                    action_kind: GovernedActionKind::RunSubprocess,
                    requested_risk_tier: None,
                    capability_scope: web_fetch_capability_scope(),
                    payload: contracts::GovernedActionPayload::WebFetch(sample_web_fetch_action()),
                },
            },
        )
        .await;
        assert!(
            planned.is_err(),
            "web_fetch payload with run_subprocess kind should be rejected"
        );
        Ok(())
    })
    .await?;

    support::with_migrated_database(|ctx| async move {
        let planned = governed_actions::plan_governed_action(
            &ctx.config,
            &ctx.pool,
            &governed_actions::GovernedActionPlanningRequest {
                governed_action_execution_id: Uuid::now_v7(),
                trace_id: Uuid::now_v7(),
                execution_id: None,
                proposal: contracts::GovernedActionProposal {
                    proposal_id: Uuid::now_v7(),
                    title: "Fetch example.com".to_string(),
                    rationale: Some("Verify web fetch planning.".to_string()),
                    action_kind: GovernedActionKind::WebFetch,
                    requested_risk_tier: None,
                    capability_scope: web_fetch_capability_scope(),
                    payload: contracts::GovernedActionPayload::WebFetch(sample_web_fetch_action()),
                },
            },
        )
        .await?;

        let planned = match planned {
            governed_actions::GovernedActionPlanningOutcome::Planned(planned) => planned,
            other => panic!("expected planned web fetch, got {other:?}"),
        };
        assert!(planned.requires_approval);
        assert_eq!(planned.record.risk_tier, GovernedActionRiskTier::Tier2);
        assert_eq!(
            planned.record.status,
            contracts::GovernedActionStatus::AwaitingApproval
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn web_fetch_validation_rejects_invalid_url() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut action = sample_web_fetch_action();
        action.url = "not-a-url".to_string();
        let outcome = governed_actions::plan_governed_action(
            &ctx.config,
            &ctx.pool,
            &governed_actions::GovernedActionPlanningRequest {
                governed_action_execution_id: Uuid::now_v7(),
                trace_id: Uuid::now_v7(),
                execution_id: None,
                proposal: contracts::GovernedActionProposal {
                    proposal_id: Uuid::now_v7(),
                    title: "Fetch invalid".to_string(),
                    rationale: None,
                    action_kind: GovernedActionKind::WebFetch,
                    requested_risk_tier: None,
                    capability_scope: web_fetch_capability_scope(),
                    payload: contracts::GovernedActionPayload::WebFetch(action),
                },
            },
        )
        .await?;
        assert!(
            matches!(
                outcome,
                governed_actions::GovernedActionPlanningOutcome::Blocked(_)
            ),
            "invalid URL should produce a blocked outcome"
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn web_fetch_validation_rejects_non_http_scheme() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut action = sample_web_fetch_action();
        action.url = "ftp://example.com/file.txt".to_string();
        let outcome = governed_actions::plan_governed_action(
            &ctx.config,
            &ctx.pool,
            &governed_actions::GovernedActionPlanningRequest {
                governed_action_execution_id: Uuid::now_v7(),
                trace_id: Uuid::now_v7(),
                execution_id: None,
                proposal: contracts::GovernedActionProposal {
                    proposal_id: Uuid::now_v7(),
                    title: "Fetch ftp".to_string(),
                    rationale: None,
                    action_kind: GovernedActionKind::WebFetch,
                    requested_risk_tier: None,
                    capability_scope: web_fetch_capability_scope(),
                    payload: contracts::GovernedActionPayload::WebFetch(action),
                },
            },
        )
        .await?;
        assert!(
            matches!(
                outcome,
                governed_actions::GovernedActionPlanningOutcome::Blocked(_)
            ),
            "non-http/https scheme should produce a blocked outcome"
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn web_fetch_validation_rejects_disabled_network() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut scope = web_fetch_capability_scope();
        scope.network = NetworkAccessPosture::Disabled;
        let outcome = governed_actions::plan_governed_action(
            &ctx.config,
            &ctx.pool,
            &governed_actions::GovernedActionPlanningRequest {
                governed_action_execution_id: Uuid::now_v7(),
                trace_id: Uuid::now_v7(),
                execution_id: None,
                proposal: contracts::GovernedActionProposal {
                    proposal_id: Uuid::now_v7(),
                    title: "Fetch with disabled network".to_string(),
                    rationale: None,
                    action_kind: GovernedActionKind::WebFetch,
                    requested_risk_tier: None,
                    capability_scope: scope,
                    payload: contracts::GovernedActionPayload::WebFetch(sample_web_fetch_action()),
                },
            },
        )
        .await?;
        assert!(
            matches!(
                outcome,
                governed_actions::GovernedActionPlanningOutcome::Blocked(_)
            ),
            "web fetch with disabled network should be blocked"
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn web_fetch_validation_rejects_oversized_timeout() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut action = sample_web_fetch_action();
        action.timeout_ms = ctx.config.governed_actions.max_web_fetch_timeout_ms + 1;
        let outcome = governed_actions::plan_governed_action(
            &ctx.config,
            &ctx.pool,
            &governed_actions::GovernedActionPlanningRequest {
                governed_action_execution_id: Uuid::now_v7(),
                trace_id: Uuid::now_v7(),
                execution_id: None,
                proposal: contracts::GovernedActionProposal {
                    proposal_id: Uuid::now_v7(),
                    title: "Fetch with oversized timeout".to_string(),
                    rationale: None,
                    action_kind: GovernedActionKind::WebFetch,
                    requested_risk_tier: None,
                    capability_scope: web_fetch_capability_scope(),
                    payload: contracts::GovernedActionPayload::WebFetch(action),
                },
            },
        )
        .await?;
        assert!(
            matches!(
                outcome,
                governed_actions::GovernedActionPlanningOutcome::Blocked(_)
            ),
            "timeout exceeding max should produce a blocked outcome"
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn web_fetch_validation_rejects_oversized_response_bytes() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let mut action = sample_web_fetch_action();
        action.max_response_bytes = ctx.config.governed_actions.max_web_fetch_response_bytes + 1;
        let outcome = governed_actions::plan_governed_action(
            &ctx.config,
            &ctx.pool,
            &governed_actions::GovernedActionPlanningRequest {
                governed_action_execution_id: Uuid::now_v7(),
                trace_id: Uuid::now_v7(),
                execution_id: None,
                proposal: contracts::GovernedActionProposal {
                    proposal_id: Uuid::now_v7(),
                    title: "Fetch with oversized response limit".to_string(),
                    rationale: None,
                    action_kind: GovernedActionKind::WebFetch,
                    requested_risk_tier: None,
                    capability_scope: web_fetch_capability_scope(),
                    payload: contracts::GovernedActionPayload::WebFetch(action),
                },
            },
        )
        .await?;
        assert!(
            matches!(
                outcome,
                governed_actions::GovernedActionPlanningOutcome::Blocked(_)
            ),
            "max_response_bytes exceeding max should produce a blocked outcome"
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn all_action_kinds_are_accepted_by_db_constraints() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        // Must stay in sync with kind_str() above — add new variants here too.
        let all_kinds = [
            GovernedActionKind::InspectWorkspaceArtifact,
            GovernedActionKind::ListWorkspaceArtifacts,
            GovernedActionKind::CreateWorkspaceArtifact,
            GovernedActionKind::UpdateWorkspaceArtifact,
            GovernedActionKind::ListWorkspaceScripts,
            GovernedActionKind::InspectWorkspaceScript,
            GovernedActionKind::CreateWorkspaceScript,
            GovernedActionKind::AppendWorkspaceScriptVersion,
            GovernedActionKind::ListWorkspaceScriptRuns,
            GovernedActionKind::UpsertScheduledForegroundTask,
            GovernedActionKind::RequestBackgroundJob,
            GovernedActionKind::RunSubprocess,
            GovernedActionKind::RunWorkspaceScript,
            GovernedActionKind::WebFetch,
        ];

        let targets = [
            (
                "governed_action_executions",
                "governed_action_executions_action_kind_check",
            ),
            ("approval_requests", "approval_requests_action_kind_check"),
        ];

        for (table, constraint) in targets {
            let def: String = sqlx::query_scalar(
                "SELECT pg_get_constraintdef(oid) \
                 FROM pg_constraint \
                 WHERE conrelid = $1::regclass AND conname = $2",
            )
            .bind(table)
            .bind(constraint)
            .fetch_one(&ctx.pool)
            .await
            .with_context(|| {
                format!(
                    "constraint '{constraint}' not found on '{table}' \
                     — was the migration applied?"
                )
            })?;

            for kind in all_kinds {
                let s = kind_str(kind);
                assert!(
                    def.contains(s),
                    "action_kind '{s}' missing from constraint '{constraint}' on '{table}' \
                     — write a migration adding '{s}' to the CHECK IN list"
                );
            }
        }
        Ok(())
    })
    .await
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
