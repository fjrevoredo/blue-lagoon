use std::collections::BTreeSet;

use anyhow::{Context, Result, bail};
use chrono::{Duration, Utc};
use contracts::{
    CapabilityScope, GovernedActionExecutionOutcome, GovernedActionFingerprint, GovernedActionKind,
    GovernedActionObservation, GovernedActionPayload, GovernedActionProposal,
    GovernedActionRiskTier, GovernedActionStatus, InspectWorkspaceArtifactAction,
    NetworkAccessPosture, SubprocessAction, WebFetchAction, WorkspaceScriptAction,
    WorkspaceScriptRunStatus,
};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    audit::{self, NewAuditEvent},
    config::RuntimeConfig,
    execution, policy, recovery, tool_execution,
    workspace::{
        self, NewWorkspaceScriptRun, UpdateWorkspaceScriptRunStatus, WorkspaceScriptRunRecord,
    },
};

const WEB_FETCH_OBSERVATION_PREVIEW_CHARS: usize = 1_500;

#[derive(Debug, Clone)]
pub struct GovernedActionPlanningRequest {
    pub governed_action_execution_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub proposal: GovernedActionProposal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GovernedActionExecutionRecord {
    pub governed_action_execution_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub approval_request_id: Option<Uuid>,
    pub action_proposal_id: Uuid,
    pub action_fingerprint: GovernedActionFingerprint,
    pub action_kind: GovernedActionKind,
    pub risk_tier: GovernedActionRiskTier,
    pub status: GovernedActionStatus,
    pub capability_scope: CapabilityScope,
    pub payload: GovernedActionPayload,
    pub workspace_script_id: Option<Uuid>,
    pub workspace_script_version_id: Option<Uuid>,
    pub blocked_reason: Option<String>,
    pub output_ref: Option<String>,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedGovernedAction {
    pub record: GovernedActionExecutionRecord,
    pub requires_approval: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedGovernedAction {
    pub record: GovernedActionExecutionRecord,
    pub outcome: GovernedActionExecutionOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GovernedActionPlanningOutcome {
    Planned(PlannedGovernedAction),
    Blocked(BlockedGovernedAction),
}

#[derive(Debug, Clone)]
pub struct GovernedActionExecutionResult {
    pub record: GovernedActionExecutionRecord,
    pub outcome: GovernedActionExecutionOutcome,
    pub observation: GovernedActionObservation,
    pub script_run: Option<WorkspaceScriptRunRecord>,
}

pub async fn plan_governed_action(
    config: &RuntimeConfig,
    pool: &PgPool,
    request: &GovernedActionPlanningRequest,
) -> Result<GovernedActionPlanningOutcome> {
    validate_proposal_shape(&request.proposal)?;

    let action_fingerprint = fingerprint_governed_action(&request.proposal)?;
    let risk_tier = policy::classify_governed_action_risk(&request.proposal);
    let requires_approval = policy::governed_action_requires_approval(config, risk_tier);

    let validation_error = validate_capability_scope(config, &request.proposal).err();
    let status = if validation_error.is_some() {
        GovernedActionStatus::Blocked
    } else if requires_approval {
        GovernedActionStatus::AwaitingApproval
    } else {
        GovernedActionStatus::Proposed
    };
    let blocked_reason = validation_error.as_ref().map(ToString::to_string);

    persist_governed_action_execution(
        pool,
        request,
        &action_fingerprint,
        risk_tier,
        status,
        blocked_reason.as_deref(),
    )
    .await?;

    let record = get_governed_action_execution(pool, request.governed_action_execution_id).await?;

    let (event_kind, severity) = match status {
        GovernedActionStatus::Blocked => ("governed_action_blocked", "warn"),
        GovernedActionStatus::AwaitingApproval => ("governed_action_planned_for_approval", "info"),
        GovernedActionStatus::Proposed => ("governed_action_planned", "info"),
        other => bail!("unsupported governed-action planning status '{other:?}'"),
    };
    audit::insert(
        pool,
        &NewAuditEvent {
            loop_kind: "conscious".to_string(),
            subsystem: "governed_actions".to_string(),
            event_kind: event_kind.to_string(),
            severity: severity.to_string(),
            trace_id: request.trace_id,
            execution_id: request.execution_id,
            worker_pid: None,
            payload: json!({
                "governed_action_execution_id": record.governed_action_execution_id,
                "action_proposal_id": record.action_proposal_id,
                "action_fingerprint": record.action_fingerprint.value,
                "action_kind": governed_action_kind_as_str(record.action_kind),
                "risk_tier": governed_action_risk_tier_as_str(record.risk_tier),
                "status": governed_action_status_as_str(record.status),
                "approval_required": requires_approval,
                "blocked_reason": record.blocked_reason,
            }),
        },
    )
    .await?;

    Ok(match status {
        GovernedActionStatus::Blocked => {
            GovernedActionPlanningOutcome::Blocked(BlockedGovernedAction {
                outcome: GovernedActionExecutionOutcome {
                    status,
                    summary: blocked_reason
                        .clone()
                        .unwrap_or_else(|| "governed action was blocked".to_string()),
                    fingerprint: Some(record.action_fingerprint.clone()),
                    output_ref: None,
                },
                record,
            })
        }
        GovernedActionStatus::AwaitingApproval | GovernedActionStatus::Proposed => {
            GovernedActionPlanningOutcome::Planned(PlannedGovernedAction {
                record,
                requires_approval,
            })
        }
        other => bail!("unsupported governed-action planning status '{other:?}'"),
    })
}

pub async fn get_governed_action_execution(
    pool: &PgPool,
    governed_action_execution_id: Uuid,
) -> Result<GovernedActionExecutionRecord> {
    let row = sqlx::query(
        r#"
        SELECT
            governed_action_execution_id,
            trace_id,
            execution_id,
            approval_request_id,
            action_proposal_id,
            action_fingerprint,
            action_kind,
            risk_tier,
            status,
            capability_scope_json,
            payload_json,
            workspace_script_id,
            workspace_script_version_id,
            blocked_reason,
            output_ref,
            started_at,
            completed_at,
            created_at,
            updated_at
        FROM governed_action_executions
        WHERE governed_action_execution_id = $1
        "#,
    )
    .bind(governed_action_execution_id)
    .fetch_one(pool)
    .await
    .context("failed to fetch governed action execution")?;

    decode_governed_action_execution_row(row)
}

pub async fn get_latest_governed_action_execution_by_fingerprint(
    pool: &PgPool,
    action_fingerprint: &GovernedActionFingerprint,
) -> Result<Option<GovernedActionExecutionRecord>> {
    let row = sqlx::query(
        r#"
        SELECT
            governed_action_execution_id,
            trace_id,
            execution_id,
            approval_request_id,
            action_proposal_id,
            action_fingerprint,
            action_kind,
            risk_tier,
            status,
            capability_scope_json,
            payload_json,
            workspace_script_id,
            workspace_script_version_id,
            blocked_reason,
            output_ref,
            started_at,
            completed_at,
            created_at,
            updated_at
        FROM governed_action_executions
        WHERE action_fingerprint = $1
        ORDER BY created_at DESC, governed_action_execution_id DESC
        LIMIT 1
        "#,
    )
    .bind(&action_fingerprint.value)
    .fetch_optional(pool)
    .await
    .context("failed to fetch governed action execution by fingerprint")?;

    row.map(decode_governed_action_execution_row).transpose()
}

pub async fn get_governed_action_execution_by_approval_request_id(
    pool: &PgPool,
    approval_request_id: Uuid,
) -> Result<Option<GovernedActionExecutionRecord>> {
    let row = sqlx::query(
        r#"
        SELECT
            governed_action_execution_id,
            trace_id,
            execution_id,
            approval_request_id,
            action_proposal_id,
            action_fingerprint,
            action_kind,
            risk_tier,
            status,
            capability_scope_json,
            payload_json,
            workspace_script_id,
            workspace_script_version_id,
            blocked_reason,
            output_ref,
            started_at,
            completed_at,
            created_at,
            updated_at
        FROM governed_action_executions
        WHERE approval_request_id = $1
        ORDER BY created_at DESC, governed_action_execution_id DESC
        LIMIT 1
        "#,
    )
    .bind(approval_request_id)
    .fetch_optional(pool)
    .await
    .context("failed to fetch governed action execution by approval request")?;

    row.map(decode_governed_action_execution_row).transpose()
}

pub async fn list_governed_action_executions(
    pool: &PgPool,
    status: Option<GovernedActionStatus>,
    limit: i64,
) -> Result<Vec<GovernedActionExecutionRecord>> {
    let rows = if let Some(status) = status {
        sqlx::query(
            r#"
            SELECT
                governed_action_execution_id,
                trace_id,
                execution_id,
                approval_request_id,
                action_proposal_id,
                action_fingerprint,
                action_kind,
                risk_tier,
                status,
                capability_scope_json,
                payload_json,
                workspace_script_id,
                workspace_script_version_id,
                blocked_reason,
                output_ref,
                started_at,
                completed_at,
                created_at,
                updated_at
            FROM governed_action_executions
            WHERE status = $1
            ORDER BY created_at DESC, governed_action_execution_id DESC
            LIMIT $2
            "#,
        )
        .bind(governed_action_status_as_str(status))
        .bind(limit)
        .fetch_all(pool)
        .await
        .context("failed to list governed action executions by status")?
    } else {
        sqlx::query(
            r#"
            SELECT
                governed_action_execution_id,
                trace_id,
                execution_id,
                approval_request_id,
                action_proposal_id,
                action_fingerprint,
                action_kind,
                risk_tier,
                status,
                capability_scope_json,
                payload_json,
                workspace_script_id,
                workspace_script_version_id,
                blocked_reason,
                output_ref,
                started_at,
                completed_at,
                created_at,
                updated_at
            FROM governed_action_executions
            ORDER BY created_at DESC, governed_action_execution_id DESC
            LIMIT $1
            "#,
        )
        .bind(limit)
        .fetch_all(pool)
        .await
        .context("failed to list governed action executions")?
    };

    rows.into_iter()
        .map(decode_governed_action_execution_row)
        .collect()
}

pub async fn attach_approval_request(
    pool: &PgPool,
    governed_action_execution_id: Uuid,
    approval_request_id: Uuid,
) -> Result<GovernedActionExecutionRecord> {
    sqlx::query(
        r#"
        UPDATE governed_action_executions
        SET
            approval_request_id = $2,
            updated_at = NOW()
        WHERE governed_action_execution_id = $1
        "#,
    )
    .bind(governed_action_execution_id)
    .bind(approval_request_id)
    .execute(pool)
    .await
    .context("failed to attach approval request to governed action execution")?;

    get_governed_action_execution(pool, governed_action_execution_id).await
}

pub async fn sync_status_from_approval_resolution(
    pool: &PgPool,
    governed_action_execution_id: Uuid,
    decision: contracts::ApprovalResolutionDecision,
    execution_id: Option<Uuid>,
    reason: Option<&str>,
) -> Result<GovernedActionExecutionRecord> {
    let status = match decision {
        contracts::ApprovalResolutionDecision::Approved => GovernedActionStatus::Approved,
        contracts::ApprovalResolutionDecision::Rejected => GovernedActionStatus::Rejected,
        contracts::ApprovalResolutionDecision::Expired => GovernedActionStatus::Expired,
        contracts::ApprovalResolutionDecision::Invalidated => GovernedActionStatus::Invalidated,
    };

    let updated = update_governed_action_execution(
        pool,
        GovernedActionExecutionUpdate {
            governed_action_execution_id,
            status,
            execution_id,
            output_ref: None,
            blocked_reason: reason,
            approval_request_id: None,
            started_at: None,
            completed_at: None,
        },
    )
    .await?;
    info!(
        governed_action_execution_id = %updated.governed_action_execution_id,
        approval_request_id = ?updated.approval_request_id,
        action_kind = governed_action_kind_as_str(updated.action_kind),
        status = governed_action_status_as_str(updated.status),
        "governed action status synced from approval resolution"
    );
    Ok(updated)
}

pub async fn execute_governed_action(
    config: &RuntimeConfig,
    pool: &PgPool,
    record: &GovernedActionExecutionRecord,
) -> Result<GovernedActionExecutionResult> {
    let proposal = proposal_from_record(record);
    if let Err(error) = validate_capability_scope(config, &proposal) {
        let summary = error.to_string();
        let completed_at = Utc::now();
        let record = update_governed_action_execution(
            pool,
            GovernedActionExecutionUpdate {
                governed_action_execution_id: record.governed_action_execution_id,
                status: GovernedActionStatus::Blocked,
                execution_id: None,
                output_ref: None,
                blocked_reason: Some(&summary),
                approval_request_id: None,
                started_at: Some(completed_at),
                completed_at: Some(completed_at),
            },
        )
        .await?;
        write_governed_action_audit_event(
            pool,
            &record,
            "governed_action_execution_blocked",
            "warn",
            json!({
                "reason": summary,
                "phase": "policy_recheck",
            }),
        )
        .await?;
        recovery::recover_governed_action_policy_recheck_failure(
            pool,
            &record,
            completed_at,
            &summary,
        )
        .await
        .context("failed to route governed-action policy re-check failure through recovery")?;
        let outcome = GovernedActionExecutionOutcome {
            status: GovernedActionStatus::Blocked,
            summary,
            fingerprint: Some(record.action_fingerprint.clone()),
            output_ref: record.output_ref.clone(),
        };
        return Ok(governed_action_execution_result(record, outcome, None));
    }

    let execution_id = Uuid::now_v7();
    execution::insert(
        pool,
        &execution::NewExecutionRecord {
            execution_id,
            trace_id: record.trace_id,
            trigger_kind: "governed_action".to_string(),
            synthetic_trigger: None,
            status: "started".to_string(),
            request_payload: json!({
                "governed_action_execution_id": record.governed_action_execution_id,
                "action_kind": governed_action_kind_as_str(record.action_kind),
                "risk_tier": governed_action_risk_tier_as_str(record.risk_tier),
            }),
        },
    )
    .await?;

    let started_at = Utc::now();
    let started_record = update_governed_action_execution(
        pool,
        GovernedActionExecutionUpdate {
            governed_action_execution_id: record.governed_action_execution_id,
            status: record.status,
            execution_id: Some(execution_id),
            output_ref: None,
            blocked_reason: None,
            approval_request_id: None,
            started_at: Some(started_at),
            completed_at: None,
        },
    )
    .await?;
    write_governed_action_audit_event(
        pool,
        &started_record,
        "governed_action_execution_started",
        "info",
        json!({
            "execution_id": execution_id,
        }),
    )
    .await?;

    let effective_timeout_ms = match &started_record.payload {
        GovernedActionPayload::WebFetch(action) => action.timeout_ms,
        _ => started_record.capability_scope.execution.timeout_ms,
    };
    let worker_lease = create_governed_action_worker_lease(
        pool,
        &started_record,
        started_at,
        effective_timeout_ms,
    )
    .await?;
    let result = match &started_record.payload {
        GovernedActionPayload::RunSubprocess(action) => {
            execute_subprocess_governed_action(config, pool, &started_record, action).await
        }
        GovernedActionPayload::RunWorkspaceScript(action) => {
            execute_workspace_script_governed_action(config, pool, &started_record, action).await
        }
        GovernedActionPayload::WebFetch(action) => {
            execute_web_fetch_governed_action(pool, &started_record, action).await
        }
        GovernedActionPayload::InspectWorkspaceArtifact(_) => {
            let summary =
                "workspace inspection execution is not implemented in the first governed backend"
                    .to_string();
            let failed_record = update_governed_action_execution(
                pool,
                GovernedActionExecutionUpdate {
                    governed_action_execution_id: started_record.governed_action_execution_id,
                    status: GovernedActionStatus::Blocked,
                    execution_id: Some(execution_id),
                    output_ref: None,
                    blocked_reason: Some(&summary),
                    approval_request_id: None,
                    started_at: Some(started_at),
                    completed_at: Some(Utc::now()),
                },
            )
            .await?;
            execution::mark_failed(
                pool,
                execution_id,
                &json!({
                    "status": "blocked",
                    "summary": summary,
                }),
            )
            .await?;
            write_governed_action_audit_event(
                pool,
                &failed_record,
                "governed_action_execution_blocked",
                "warn",
                json!({
                    "reason": summary,
                    "phase": "backend",
                }),
            )
            .await?;
            let outcome = GovernedActionExecutionOutcome {
                status: GovernedActionStatus::Blocked,
                summary,
                fingerprint: Some(failed_record.action_fingerprint.clone()),
                output_ref: failed_record.output_ref.clone(),
            };
            Ok(governed_action_execution_result(
                failed_record,
                outcome,
                None,
            ))
        }
    };
    let lease_completion_result = if result.as_ref().is_ok_and(governed_action_result_is_timeout) {
        recovery::recover_observed_worker_timeout(
            pool,
            worker_lease.worker_lease_id,
            Utc::now(),
            "governed_action_timeout",
            result
                .as_ref()
                .map(|result| result.outcome.summary.as_str())
                .unwrap_or("governed action timed out"),
        )
        .await
        .map(|_| ())
        .context("failed to route timed-out governed-action worker lease through recovery")
    } else {
        if result.is_ok() {
            recovery::refresh_worker_lease_progress(pool, worker_lease.worker_lease_id, Utc::now())
                .await
                .context("failed to refresh governed-action worker lease after action progress")?;
        }
        recovery::release_worker_lease(pool, worker_lease.worker_lease_id, Utc::now())
            .await
            .map(|_| ())
    };

    match (result, lease_completion_result) {
        (Ok(result), Ok(_)) => Ok(result),
        (Ok(_), Err(error)) => {
            Err(error.context("failed to complete governed-action worker lease after success"))
        }
        (Err(error), Ok(_)) => Err(error),
        (Err(action_error), Err(lease_error)) => Err(lease_error.context(format!(
            "failed to complete governed-action worker lease after action failure: {action_error}"
        ))),
    }
}

fn governed_action_result_is_timeout(result: &GovernedActionExecutionResult) -> bool {
    result.outcome.status == GovernedActionStatus::Failed
        && result.outcome.summary.contains("timed out")
}

async fn create_governed_action_worker_lease(
    pool: &PgPool,
    record: &GovernedActionExecutionRecord,
    started_at: chrono::DateTime<chrono::Utc>,
    timeout_ms: u64,
) -> Result<recovery::WorkerLeaseRecord> {
    let timeout_ms =
        i64::try_from(timeout_ms).context("governed action timeout exceeded chrono range")?;
    recovery::create_worker_lease(
        pool,
        &recovery::NewWorkerLease {
            worker_lease_id: Uuid::now_v7(),
            trace_id: record.trace_id,
            execution_id: record.execution_id,
            background_job_id: None,
            background_job_run_id: None,
            governed_action_execution_id: Some(record.governed_action_execution_id),
            worker_kind: recovery::WorkerLeaseKind::GovernedAction,
            lease_token: Uuid::now_v7(),
            worker_pid: None,
            lease_acquired_at: started_at,
            lease_expires_at: started_at + Duration::milliseconds(timeout_ms),
            last_heartbeat_at: started_at,
            metadata: json!({
                "source": "governed_actions",
                "action_kind": governed_action_kind_as_str(record.action_kind),
                "risk_tier": governed_action_risk_tier_as_str(record.risk_tier),
            }),
        },
    )
    .await
}

pub fn fingerprint_governed_action(
    proposal: &GovernedActionProposal,
) -> Result<GovernedActionFingerprint> {
    let canonical = CanonicalGovernedActionFingerprintInput {
        action_kind: governed_action_kind_as_str(proposal.action_kind),
        risk_tier: governed_action_risk_tier_as_str(policy::classify_governed_action_risk(
            proposal,
        )),
        capability_scope: CanonicalCapabilityScope::from(&proposal.capability_scope),
        payload: CanonicalGovernedActionPayload::from(&proposal.payload),
    };
    let serialized = serde_json::to_vec(&canonical)
        .context("failed to serialize governed action fingerprint input")?;
    let digest = Sha256::digest(&serialized);
    Ok(GovernedActionFingerprint {
        value: format!("sha256:{}", hex::encode(digest)),
    })
}

pub fn validate_capability_scope(
    config: &RuntimeConfig,
    proposal: &GovernedActionProposal,
) -> Result<()> {
    let scope = &proposal.capability_scope;
    let filesystem_roots = normalized_filesystem_roots(scope);
    let is_web_fetch = proposal.action_kind == GovernedActionKind::WebFetch;
    if filesystem_roots.is_empty()
        && proposal.action_kind != GovernedActionKind::InspectWorkspaceArtifact
        && !is_web_fetch
    {
        bail!("governed actions must request at least one filesystem root");
    }
    if filesystem_roots.len() > config.governed_actions.max_filesystem_roots_per_action as usize {
        bail!(
            "governed action requested {} filesystem roots, exceeding the configured limit ({})",
            filesystem_roots.len(),
            config.governed_actions.max_filesystem_roots_per_action
        );
    }

    for root in scope
        .filesystem
        .read_roots
        .iter()
        .chain(scope.filesystem.write_roots.iter())
    {
        if root.trim().is_empty() {
            bail!("governed action filesystem roots must not be empty");
        }
    }

    if !is_web_fetch {
        if scope.execution.timeout_ms == 0 {
            bail!("governed action timeout must be greater than zero");
        }
        if scope.execution.timeout_ms > config.governed_actions.max_subprocess_timeout_ms {
            bail!(
                "governed action timeout {} exceeds the configured maximum ({})",
                scope.execution.timeout_ms,
                config.governed_actions.max_subprocess_timeout_ms
            );
        }
        if scope.execution.max_stdout_bytes == 0 || scope.execution.max_stderr_bytes == 0 {
            bail!("governed action captured output limits must be greater than zero");
        }
        if scope.execution.max_stdout_bytes > config.governed_actions.max_captured_output_bytes
            || scope.execution.max_stderr_bytes > config.governed_actions.max_captured_output_bytes
        {
            bail!(
                "governed action captured output exceeds the configured maximum ({})",
                config.governed_actions.max_captured_output_bytes
            );
        }
    }

    if scope.environment.allow_variables.len()
        > config.governed_actions.max_environment_variables_per_action as usize
    {
        bail!(
            "governed action requested {} environment variables, exceeding the configured limit ({})",
            scope.environment.allow_variables.len(),
            config.governed_actions.max_environment_variables_per_action
        );
    }
    for variable in &scope.environment.allow_variables {
        if variable.trim().is_empty() {
            bail!("governed action environment variable names must not be empty");
        }
        if !config
            .governed_actions
            .allowlisted_environment_variables
            .iter()
            .any(|allowlisted| allowlisted == variable)
        {
            bail!("governed action environment variable '{variable}' is not allowlisted");
        }
    }

    match (&proposal.action_kind, &proposal.payload) {
        (
            GovernedActionKind::InspectWorkspaceArtifact,
            GovernedActionPayload::InspectWorkspaceArtifact(_),
        ) => {
            if !scope.filesystem.write_roots.is_empty() {
                bail!("workspace inspection proposals must not request filesystem write scope");
            }
            if !scope.environment.allow_variables.is_empty() {
                bail!("workspace inspection proposals must not request environment variable scope");
            }
            if scope.network != NetworkAccessPosture::Disabled {
                bail!("workspace inspection proposals must not request network access");
            }
        }
        (GovernedActionKind::RunSubprocess, GovernedActionPayload::RunSubprocess(action)) => {
            if action.command.trim().is_empty() {
                bail!("subprocess proposals must declare a command");
            }
        }
        (
            GovernedActionKind::RunWorkspaceScript,
            GovernedActionPayload::RunWorkspaceScript(action),
        ) => {
            validate_workspace_script_action(action)?;
        }
        (GovernedActionKind::WebFetch, GovernedActionPayload::WebFetch(action)) => {
            validate_web_fetch_action(config, scope, action)?;
        }
        _ => bail!("governed action kind does not match the proposal payload"),
    }

    Ok(())
}

fn validate_proposal_shape(proposal: &GovernedActionProposal) -> Result<()> {
    if proposal.title.trim().is_empty() {
        bail!("governed action title must not be empty");
    }
    match (&proposal.action_kind, &proposal.payload) {
        (
            GovernedActionKind::InspectWorkspaceArtifact,
            GovernedActionPayload::InspectWorkspaceArtifact(action),
        ) => validate_workspace_inspection_action(action),
        (GovernedActionKind::RunSubprocess, GovernedActionPayload::RunSubprocess(action)) => {
            validate_subprocess_action(action)
        }
        (
            GovernedActionKind::RunWorkspaceScript,
            GovernedActionPayload::RunWorkspaceScript(action),
        ) => validate_workspace_script_action(action),
        (GovernedActionKind::WebFetch, GovernedActionPayload::WebFetch(action)) => {
            validate_web_fetch_shape(action)
        }
        _ => bail!("governed action kind does not match the proposal payload"),
    }
}

fn validate_workspace_inspection_action(action: &InspectWorkspaceArtifactAction) -> Result<()> {
    if action.artifact_kind == contracts::WorkspaceArtifactKind::Script {
        bail!("workspace inspection proposals must use run_workspace_script for scripts");
    }
    Ok(())
}

fn validate_subprocess_action(action: &SubprocessAction) -> Result<()> {
    if action.command.trim().is_empty() {
        bail!("subprocess proposals must declare a command");
    }
    Ok(())
}

fn validate_workspace_script_action(action: &WorkspaceScriptAction) -> Result<()> {
    if action.args.iter().any(|arg| arg.trim().is_empty()) {
        bail!("workspace script arguments must not be empty");
    }
    Ok(())
}

fn validate_web_fetch_shape(action: &WebFetchAction) -> Result<()> {
    if action.url.trim().is_empty() {
        bail!("web fetch proposals must declare a URL");
    }
    if action.timeout_ms == 0 {
        bail!("web fetch timeout_ms must be greater than zero");
    }
    if action.max_response_bytes == 0 {
        bail!("web fetch max_response_bytes must be greater than zero");
    }
    Ok(())
}

fn validate_web_fetch_action(
    config: &RuntimeConfig,
    scope: &CapabilityScope,
    action: &WebFetchAction,
) -> Result<()> {
    validate_web_fetch_shape(action)?;
    let parsed = reqwest::Url::parse(&action.url)
        .with_context(|| format!("web fetch URL '{}' is not a valid URL", action.url))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        bail!(
            "web fetch URL must use http or https scheme, got '{}'",
            parsed.scheme()
        );
    }
    if action.timeout_ms > config.governed_actions.max_web_fetch_timeout_ms {
        bail!(
            "web fetch timeout_ms {} exceeds the configured maximum ({})",
            action.timeout_ms,
            config.governed_actions.max_web_fetch_timeout_ms
        );
    }
    if action.max_response_bytes > config.governed_actions.max_web_fetch_response_bytes {
        bail!(
            "web fetch max_response_bytes {} exceeds the configured maximum ({})",
            action.max_response_bytes,
            config.governed_actions.max_web_fetch_response_bytes
        );
    }
    if scope.network != NetworkAccessPosture::Enabled {
        bail!("web fetch proposals must set capability_scope.network to \"enabled\"");
    }
    Ok(())
}

async fn execute_subprocess_governed_action(
    config: &RuntimeConfig,
    pool: &PgPool,
    record: &GovernedActionExecutionRecord,
    action: &SubprocessAction,
) -> Result<GovernedActionExecutionResult> {
    let Some(execution_id) = record.execution_id else {
        bail!("governed subprocess execution requires an attached execution record");
    };
    let started_at = record.started_at.unwrap_or_else(Utc::now);
    let outcome =
        match tool_execution::execute_bounded_subprocess(config, &record.capability_scope, action)
            .await
        {
            Ok(outcome) => outcome,
            Err(error) => {
                let summary = error.to_string();
                execution::mark_failed(
                    pool,
                    execution_id,
                    &json!({
                        "status": "blocked",
                        "summary": summary,
                    }),
                )
                .await?;
                let blocked_record = update_governed_action_execution(
                    pool,
                    GovernedActionExecutionUpdate {
                        governed_action_execution_id: record.governed_action_execution_id,
                        status: GovernedActionStatus::Blocked,
                        execution_id: Some(execution_id),
                        output_ref: Some(&format!("execution_record:{execution_id}")),
                        blocked_reason: Some(&summary),
                        approval_request_id: None,
                        started_at: Some(started_at),
                        completed_at: Some(Utc::now()),
                    },
                )
                .await?;
                write_governed_action_audit_event(
                    pool,
                    &blocked_record,
                    "governed_action_execution_blocked",
                    "warn",
                    json!({
                        "reason": summary,
                        "phase": "backend",
                    }),
                )
                .await?;
                let execution_outcome = GovernedActionExecutionOutcome {
                    status: GovernedActionStatus::Blocked,
                    summary,
                    fingerprint: Some(blocked_record.action_fingerprint.clone()),
                    output_ref: blocked_record.output_ref.clone(),
                };
                return Ok(governed_action_execution_result(
                    blocked_record,
                    execution_outcome,
                    None,
                ));
            }
        };
    let completed_at = Utc::now();
    let output_ref = format!("execution_record:{execution_id}");

    if outcome.timed_out {
        let summary = format!(
            "bounded subprocess timed out after {} ms",
            record.capability_scope.execution.timeout_ms
        );
        execution::mark_failed(
            pool,
            execution_id,
            &json!({
                "status": "timed_out",
                "summary": summary,
                "stdout": outcome.stdout,
                "stderr": outcome.stderr,
            }),
        )
        .await?;
        let updated_record = update_governed_action_execution(
            pool,
            GovernedActionExecutionUpdate {
                governed_action_execution_id: record.governed_action_execution_id,
                status: GovernedActionStatus::Failed,
                execution_id: Some(execution_id),
                output_ref: Some(&output_ref),
                blocked_reason: Some(&summary),
                approval_request_id: None,
                started_at: Some(started_at),
                completed_at: Some(completed_at),
            },
        )
        .await?;
        write_governed_action_audit_event(
            pool,
            &updated_record,
            "governed_action_execution_timed_out",
            "warn",
            json!({
                "stdout_bytes": updated_record.capability_scope.execution.max_stdout_bytes,
                "stderr_bytes": updated_record.capability_scope.execution.max_stderr_bytes,
            }),
        )
        .await?;
        let execution_outcome = GovernedActionExecutionOutcome {
            status: GovernedActionStatus::Failed,
            summary,
            fingerprint: Some(updated_record.action_fingerprint.clone()),
            output_ref: Some(output_ref),
        };
        return Ok(governed_action_execution_result(
            updated_record,
            execution_outcome,
            None,
        ));
    }

    let success = outcome.exit_code == Some(0);
    let status = if success {
        GovernedActionStatus::Executed
    } else {
        GovernedActionStatus::Failed
    };
    let summary = if success {
        "bounded subprocess completed successfully".to_string()
    } else {
        format!(
            "bounded subprocess exited with status {}",
            outcome
                .exit_code
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        )
    };

    let response_payload = json!({
        "status": if success { "completed" } else { "failed" },
        "summary": summary,
        "exit_code": outcome.exit_code,
        "stdout": outcome.stdout,
        "stderr": outcome.stderr,
    });
    if success {
        execution::mark_succeeded(pool, execution_id, "governed_action", 0, &response_payload)
            .await?;
    } else {
        execution::mark_failed(pool, execution_id, &response_payload).await?;
    }

    let updated_record = update_governed_action_execution(
        pool,
        GovernedActionExecutionUpdate {
            governed_action_execution_id: record.governed_action_execution_id,
            status,
            execution_id: Some(execution_id),
            output_ref: Some(&output_ref),
            blocked_reason: if success { None } else { Some(&summary) },
            approval_request_id: None,
            started_at: Some(started_at),
            completed_at: Some(completed_at),
        },
    )
    .await?;
    write_governed_action_audit_event(
        pool,
        &updated_record,
        if success {
            "governed_action_execution_completed"
        } else {
            "governed_action_execution_failed"
        },
        if success { "info" } else { "warn" },
        json!({
            "exit_code": outcome.exit_code,
            "stdout_excerpt": outcome.stdout,
            "stderr_excerpt": outcome.stderr,
        }),
    )
    .await?;

    let execution_outcome = GovernedActionExecutionOutcome {
        status,
        summary,
        fingerprint: Some(updated_record.action_fingerprint.clone()),
        output_ref: Some(output_ref),
    };
    Ok(governed_action_execution_result(
        updated_record,
        execution_outcome,
        None,
    ))
}

async fn execute_workspace_script_governed_action(
    config: &RuntimeConfig,
    pool: &PgPool,
    record: &GovernedActionExecutionRecord,
    action: &WorkspaceScriptAction,
) -> Result<GovernedActionExecutionResult> {
    let Some(execution_id) = record.execution_id else {
        bail!("governed workspace-script execution requires an attached execution record");
    };
    let script = workspace::get_workspace_script(pool, action.script_id).await?;
    let version = match action.script_version_id {
        Some(version_id) => workspace::get_workspace_script_version(pool, version_id).await?,
        None => workspace::get_latest_workspace_script_version(pool, action.script_id)
            .await?
            .context("workspace script has no canonical versions")?,
    };
    if version.workspace_script_id != action.script_id {
        bail!("workspace script version does not belong to the requested script");
    }

    let subprocess_action =
        build_workspace_script_subprocess_action(config, &script, &version, action)?;
    let script_run_id = Uuid::now_v7();
    let pending_run = workspace::record_workspace_script_run(
        pool,
        &NewWorkspaceScriptRun {
            workspace_script_run_id: script_run_id,
            workspace_script_id: script.workspace_script_id,
            workspace_script_version_id: version.workspace_script_version_id,
            trace_id: record.trace_id,
            execution_id: Some(execution_id),
            governed_action_execution_id: Some(record.governed_action_execution_id),
            approval_request_id: record.approval_request_id,
            status: WorkspaceScriptRunStatus::Pending,
            risk_tier: record.risk_tier,
            args: action.args.clone(),
            output_ref: None,
            failure_summary: None,
            started_at: None,
            completed_at: None,
        },
    )
    .await?;

    let started_at = Utc::now();
    let output_ref = format!("execution_record:{execution_id}");
    let running_run = workspace::update_workspace_script_run_status(
        pool,
        &UpdateWorkspaceScriptRunStatus {
            workspace_script_run_id: pending_run.workspace_script_run_id,
            status: WorkspaceScriptRunStatus::Running,
            output_ref: None,
            failure_summary: None,
            started_at: Some(started_at),
            completed_at: None,
        },
    )
    .await?;

    let subprocess_outcome = match tool_execution::execute_bounded_subprocess(
        config,
        &record.capability_scope,
        &subprocess_action,
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(error) => {
            let summary = error.to_string();
            execution::mark_failed(
                pool,
                execution_id,
                &json!({
                    "status": "blocked",
                    "summary": summary,
                    "workspace_script_id": script.workspace_script_id,
                    "workspace_script_version_id": version.workspace_script_version_id,
                }),
            )
            .await?;
            let blocked_run = workspace::update_workspace_script_run_status(
                pool,
                &UpdateWorkspaceScriptRunStatus {
                    workspace_script_run_id: running_run.workspace_script_run_id,
                    status: WorkspaceScriptRunStatus::Blocked,
                    output_ref: Some(output_ref.clone()),
                    failure_summary: Some(summary.clone()),
                    started_at: Some(started_at),
                    completed_at: Some(Utc::now()),
                },
            )
            .await?;
            let blocked_record = update_governed_action_execution(
                pool,
                GovernedActionExecutionUpdate {
                    governed_action_execution_id: record.governed_action_execution_id,
                    status: GovernedActionStatus::Blocked,
                    execution_id: Some(execution_id),
                    output_ref: Some(&output_ref),
                    blocked_reason: Some(&summary),
                    approval_request_id: None,
                    started_at: Some(started_at),
                    completed_at: Some(Utc::now()),
                },
            )
            .await?;
            write_governed_action_audit_event(
                pool,
                &blocked_record,
                "governed_action_execution_blocked",
                "warn",
                json!({
                    "workspace_script_run_id": blocked_run.workspace_script_run_id,
                    "reason": summary,
                    "phase": "backend",
                }),
            )
            .await?;
            let execution_outcome = GovernedActionExecutionOutcome {
                status: GovernedActionStatus::Blocked,
                summary,
                fingerprint: Some(blocked_record.action_fingerprint.clone()),
                output_ref: Some(output_ref),
            };
            return Ok(governed_action_execution_result(
                blocked_record,
                execution_outcome,
                Some(blocked_run),
            ));
        }
    };
    let completed_at = Utc::now();

    if subprocess_outcome.timed_out {
        let summary = format!(
            "workspace script '{}' timed out after {} ms",
            script.workspace_script_id, record.capability_scope.execution.timeout_ms
        );
        execution::mark_failed(
            pool,
            execution_id,
            &json!({
                "status": "timed_out",
                "summary": summary,
                "workspace_script_id": script.workspace_script_id,
                "workspace_script_version_id": version.workspace_script_version_id,
                "stdout": subprocess_outcome.stdout,
                "stderr": subprocess_outcome.stderr,
            }),
        )
        .await?;
        let updated_run = workspace::update_workspace_script_run_status(
            pool,
            &UpdateWorkspaceScriptRunStatus {
                workspace_script_run_id: running_run.workspace_script_run_id,
                status: WorkspaceScriptRunStatus::TimedOut,
                output_ref: Some(output_ref.clone()),
                failure_summary: Some(summary.clone()),
                started_at: Some(started_at),
                completed_at: Some(completed_at),
            },
        )
        .await?;
        let updated_record = update_governed_action_execution(
            pool,
            GovernedActionExecutionUpdate {
                governed_action_execution_id: record.governed_action_execution_id,
                status: GovernedActionStatus::Failed,
                execution_id: Some(execution_id),
                output_ref: Some(&output_ref),
                blocked_reason: Some(&summary),
                approval_request_id: None,
                started_at: Some(started_at),
                completed_at: Some(completed_at),
            },
        )
        .await?;
        write_governed_action_audit_event(
            pool,
            &updated_record,
            "governed_action_execution_timed_out",
            "warn",
            json!({
                "workspace_script_run_id": updated_run.workspace_script_run_id,
            }),
        )
        .await?;
        let execution_outcome = GovernedActionExecutionOutcome {
            status: GovernedActionStatus::Failed,
            summary,
            fingerprint: Some(updated_record.action_fingerprint.clone()),
            output_ref: Some(output_ref),
        };
        return Ok(governed_action_execution_result(
            updated_record,
            execution_outcome,
            Some(updated_run),
        ));
    }

    let success = subprocess_outcome.exit_code == Some(0);
    let governed_status = if success {
        GovernedActionStatus::Executed
    } else {
        GovernedActionStatus::Failed
    };
    let run_status = if success {
        WorkspaceScriptRunStatus::Completed
    } else {
        WorkspaceScriptRunStatus::Failed
    };
    let summary = if success {
        format!(
            "workspace script '{}' completed successfully",
            script.workspace_script_id
        )
    } else {
        format!(
            "workspace script '{}' exited with status {}",
            script.workspace_script_id,
            subprocess_outcome
                .exit_code
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        )
    };

    let response_payload = json!({
        "status": if success { "completed" } else { "failed" },
        "summary": summary,
        "workspace_script_id": script.workspace_script_id,
        "workspace_script_version_id": version.workspace_script_version_id,
        "exit_code": subprocess_outcome.exit_code,
        "stdout": subprocess_outcome.stdout,
        "stderr": subprocess_outcome.stderr,
    });
    if success {
        execution::mark_succeeded(pool, execution_id, "governed_action", 0, &response_payload)
            .await?;
    } else {
        execution::mark_failed(pool, execution_id, &response_payload).await?;
    }

    let updated_run = workspace::update_workspace_script_run_status(
        pool,
        &UpdateWorkspaceScriptRunStatus {
            workspace_script_run_id: running_run.workspace_script_run_id,
            status: run_status,
            output_ref: Some(output_ref.clone()),
            failure_summary: if success { None } else { Some(summary.clone()) },
            started_at: Some(started_at),
            completed_at: Some(completed_at),
        },
    )
    .await?;
    let updated_record = update_governed_action_execution(
        pool,
        GovernedActionExecutionUpdate {
            governed_action_execution_id: record.governed_action_execution_id,
            status: governed_status,
            execution_id: Some(execution_id),
            output_ref: Some(&output_ref),
            blocked_reason: if success { None } else { Some(&summary) },
            approval_request_id: None,
            started_at: Some(started_at),
            completed_at: Some(completed_at),
        },
    )
    .await?;
    write_governed_action_audit_event(
        pool,
        &updated_record,
        if success {
            "governed_action_execution_completed"
        } else {
            "governed_action_execution_failed"
        },
        if success { "info" } else { "warn" },
        json!({
            "workspace_script_run_id": updated_run.workspace_script_run_id,
            "workspace_script_id": script.workspace_script_id,
            "workspace_script_version_id": version.workspace_script_version_id,
            "exit_code": subprocess_outcome.exit_code,
        }),
    )
    .await?;

    let execution_outcome = GovernedActionExecutionOutcome {
        status: governed_status,
        summary,
        fingerprint: Some(updated_record.action_fingerprint.clone()),
        output_ref: Some(output_ref),
    };
    Ok(governed_action_execution_result(
        updated_record,
        execution_outcome,
        Some(updated_run),
    ))
}

async fn execute_web_fetch_governed_action(
    pool: &PgPool,
    record: &GovernedActionExecutionRecord,
    action: &WebFetchAction,
) -> Result<GovernedActionExecutionResult> {
    let Some(execution_id) = record.execution_id else {
        bail!("governed web fetch execution requires an attached execution record");
    };
    let started_at = record.started_at.unwrap_or_else(Utc::now);
    let output_ref = format!("execution_record:{execution_id}");

    info!(
        governed_action_execution_id = %record.governed_action_execution_id,
        execution_id = %execution_id,
        url = %action.url,
        timeout_ms = action.timeout_ms,
        max_response_bytes = action.max_response_bytes,
        "starting governed web fetch execution"
    );
    let fetch_result = tool_execution::execute_web_fetch(action).await;
    let completed_at = Utc::now();

    match fetch_result {
        Err(error) => {
            let summary = error.to_string();
            warn!(
                governed_action_execution_id = %record.governed_action_execution_id,
                execution_id = %execution_id,
                url = %action.url,
                error = %summary,
                "governed web fetch execution failed"
            );
            execution::mark_failed(
                pool,
                execution_id,
                &json!({
                    "status": "failed",
                    "summary": summary,
                }),
            )
            .await?;
            let failed_record = update_governed_action_execution(
                pool,
                GovernedActionExecutionUpdate {
                    governed_action_execution_id: record.governed_action_execution_id,
                    status: GovernedActionStatus::Failed,
                    execution_id: Some(execution_id),
                    output_ref: Some(&output_ref),
                    blocked_reason: Some(&summary),
                    approval_request_id: None,
                    started_at: Some(started_at),
                    completed_at: Some(completed_at),
                },
            )
            .await?;
            write_governed_action_audit_event(
                pool,
                &failed_record,
                "governed_action_execution_failed",
                "warn",
                json!({
                    "reason": summary,
                    "phase": "backend",
                }),
            )
            .await?;
            let execution_outcome = GovernedActionExecutionOutcome {
                status: GovernedActionStatus::Failed,
                summary,
                fingerprint: Some(failed_record.action_fingerprint.clone()),
                output_ref: Some(output_ref),
            };
            Ok(governed_action_execution_result(
                failed_record,
                execution_outcome,
                None,
            ))
        }
        Ok(outcome) => {
            let summary = web_fetch_execution_summary(action, &outcome.body, outcome.truncated);
            execution::mark_succeeded(
                pool,
                execution_id,
                "governed_action",
                0,
                &json!({
                    "status": "completed",
                    "summary": summary,
                    "url": action.url,
                    "body": outcome.body,
                    "truncated": outcome.truncated,
                }),
            )
            .await?;
            let updated_record = update_governed_action_execution(
                pool,
                GovernedActionExecutionUpdate {
                    governed_action_execution_id: record.governed_action_execution_id,
                    status: GovernedActionStatus::Executed,
                    execution_id: Some(execution_id),
                    output_ref: Some(&output_ref),
                    blocked_reason: None,
                    approval_request_id: None,
                    started_at: Some(started_at),
                    completed_at: Some(completed_at),
                },
            )
            .await?;
            write_governed_action_audit_event(
                pool,
                &updated_record,
                "governed_action_execution_completed",
                "info",
                json!({
                    "url": action.url,
                    "body_bytes": outcome.body.len(),
                    "truncated": outcome.truncated,
                }),
            )
            .await?;
            info!(
                governed_action_execution_id = %updated_record.governed_action_execution_id,
                execution_id = %execution_id,
                url = %action.url,
                body_bytes = outcome.body.len(),
                truncated = outcome.truncated,
                "governed web fetch execution completed"
            );
            let execution_outcome = GovernedActionExecutionOutcome {
                status: GovernedActionStatus::Executed,
                summary,
                fingerprint: Some(updated_record.action_fingerprint.clone()),
                output_ref: Some(output_ref),
            };
            Ok(governed_action_execution_result(
                updated_record,
                execution_outcome,
                None,
            ))
        }
    }
}

fn web_fetch_execution_summary(
    action: &WebFetchAction,
    response_body: &str,
    response_truncated: bool,
) -> String {
    let preview = normalize_preview(response_body, WEB_FETCH_OBSERVATION_PREVIEW_CHARS);
    let response_suffix = if preview.is_empty() {
        "response preview: <empty>".to_string()
    } else {
        format!("response preview: {preview}")
    };
    let truncation_note = if response_truncated {
        format!(
            "; response truncated to {} bytes",
            action.max_response_bytes
        )
    } else if response_body.chars().count() > WEB_FETCH_OBSERVATION_PREVIEW_CHARS {
        format!("; preview truncated to {WEB_FETCH_OBSERVATION_PREVIEW_CHARS} chars")
    } else {
        String::new()
    };

    format!(
        "web fetch completed for {}; {response_suffix}{truncation_note}",
        action.url
    )
}

fn normalize_preview(value: &str, max_chars: usize) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max_chars)
        .collect()
}

fn build_workspace_script_subprocess_action(
    config: &RuntimeConfig,
    script: &workspace::WorkspaceScriptRecord,
    version: &workspace::WorkspaceScriptVersionRecord,
    action: &WorkspaceScriptAction,
) -> Result<SubprocessAction> {
    let workspace_root = config.workspace.root_dir.display().to_string();
    match script.language.to_ascii_lowercase().as_str() {
        "powershell" | "pwsh" => Ok(SubprocessAction {
            command: if script.language.eq_ignore_ascii_case("pwsh") {
                "pwsh".to_string()
            } else {
                "powershell".to_string()
            },
            args: vec![
                "-NoProfile".to_string(),
                "-Command".to_string(),
                version.content_text.clone(),
            ]
            .into_iter()
            .chain(action.args.iter().cloned())
            .collect(),
            working_directory: Some(workspace_root),
        }),
        "sh" | "bash" => Ok(SubprocessAction {
            command: if script.language.eq_ignore_ascii_case("bash") {
                "bash".to_string()
            } else {
                "sh".to_string()
            },
            args: vec!["-c".to_string(), version.content_text.clone()]
                .into_iter()
                .chain(action.args.iter().cloned())
                .collect(),
            working_directory: Some(workspace_root),
        }),
        "python" => Ok(SubprocessAction {
            command: "python".to_string(),
            args: vec!["-c".to_string(), version.content_text.clone()]
                .into_iter()
                .chain(action.args.iter().cloned())
                .collect(),
            working_directory: Some(workspace_root),
        }),
        other => bail!(
            "workspace script language '{other}' is not supported by the first governed backend"
        ),
    }
}

fn proposal_from_record(record: &GovernedActionExecutionRecord) -> GovernedActionProposal {
    GovernedActionProposal {
        proposal_id: record.action_proposal_id,
        title: format!(
            "{}:{}",
            governed_action_kind_as_str(record.action_kind),
            record.governed_action_execution_id
        ),
        rationale: record.blocked_reason.clone(),
        action_kind: record.action_kind,
        requested_risk_tier: Some(record.risk_tier),
        capability_scope: record.capability_scope.clone(),
        payload: record.payload.clone(),
    }
}

fn governed_action_execution_result(
    record: GovernedActionExecutionRecord,
    outcome: GovernedActionExecutionOutcome,
    script_run: Option<WorkspaceScriptRunRecord>,
) -> GovernedActionExecutionResult {
    GovernedActionExecutionResult {
        observation: GovernedActionObservation {
            observation_id: Uuid::now_v7(),
            action_kind: record.action_kind,
            outcome: outcome.clone(),
        },
        record,
        outcome,
        script_run,
    }
}

pub(crate) struct GovernedActionExecutionUpdate<'a> {
    pub(crate) governed_action_execution_id: Uuid,
    pub(crate) status: GovernedActionStatus,
    pub(crate) execution_id: Option<Uuid>,
    pub(crate) output_ref: Option<&'a str>,
    pub(crate) blocked_reason: Option<&'a str>,
    pub(crate) approval_request_id: Option<Uuid>,
    pub(crate) started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub(crate) completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub(crate) async fn update_governed_action_execution(
    pool: &PgPool,
    update: GovernedActionExecutionUpdate<'_>,
) -> Result<GovernedActionExecutionRecord> {
    sqlx::query(
        r#"
        UPDATE governed_action_executions
        SET
            status = $2,
            execution_id = COALESCE($3, execution_id),
            approval_request_id = COALESCE($4, approval_request_id),
            output_ref = COALESCE($5, output_ref),
            blocked_reason = $6,
            started_at = COALESCE($7, started_at),
            completed_at = $8,
            updated_at = NOW()
        WHERE governed_action_execution_id = $1
        "#,
    )
    .bind(update.governed_action_execution_id)
    .bind(governed_action_status_as_str(update.status))
    .bind(update.execution_id)
    .bind(update.approval_request_id)
    .bind(update.output_ref)
    .bind(update.blocked_reason)
    .bind(update.started_at)
    .bind(update.completed_at)
    .execute(pool)
    .await
    .context("failed to update governed action execution")?;

    get_governed_action_execution(pool, update.governed_action_execution_id).await
}

pub(crate) async fn write_governed_action_audit_event(
    pool: &PgPool,
    record: &GovernedActionExecutionRecord,
    event_kind: &str,
    severity: &str,
    payload: serde_json::Value,
) -> Result<()> {
    audit::insert(
        pool,
        &NewAuditEvent {
            loop_kind: "conscious".to_string(),
            subsystem: "governed_actions".to_string(),
            event_kind: event_kind.to_string(),
            severity: severity.to_string(),
            trace_id: record.trace_id,
            execution_id: record.execution_id,
            worker_pid: None,
            payload: json!({
                "governed_action_execution_id": record.governed_action_execution_id,
                "action_fingerprint": record.action_fingerprint.value,
                "action_kind": governed_action_kind_as_str(record.action_kind),
                "risk_tier": governed_action_risk_tier_as_str(record.risk_tier),
                "status": governed_action_status_as_str(record.status),
                "details": payload,
            }),
        },
    )
    .await
    .map(|_| ())
}

async fn persist_governed_action_execution(
    pool: &PgPool,
    request: &GovernedActionPlanningRequest,
    action_fingerprint: &GovernedActionFingerprint,
    risk_tier: GovernedActionRiskTier,
    status: GovernedActionStatus,
    blocked_reason: Option<&str>,
) -> Result<()> {
    let capability_scope_json = serde_json::to_value(&request.proposal.capability_scope)
        .context("failed to encode governed action capability scope")?;
    let payload_json = serde_json::to_value(&request.proposal.payload)
        .context("failed to encode governed action payload")?;
    let (workspace_script_id, workspace_script_version_id) = match &request.proposal.payload {
        GovernedActionPayload::RunWorkspaceScript(action) => {
            (Some(action.script_id), action.script_version_id)
        }
        _ => (None, None),
    };

    sqlx::query(
        r#"
        INSERT INTO governed_action_executions (
            governed_action_execution_id,
            trace_id,
            execution_id,
            approval_request_id,
            action_proposal_id,
            action_fingerprint,
            action_kind,
            risk_tier,
            status,
            capability_scope_json,
            payload_json,
            workspace_script_id,
            workspace_script_version_id,
            blocked_reason,
            output_ref,
            started_at,
            completed_at,
            created_at,
            updated_at
        ) VALUES (
            $1,
            $2,
            $3,
            NULL,
            $4,
            $5,
            $6,
            $7,
            $8,
            $9,
            $10,
            $11,
            $12,
            $13,
            NULL,
            NULL,
            NULL,
            NOW(),
            NOW()
        )
        "#,
    )
    .bind(request.governed_action_execution_id)
    .bind(request.trace_id)
    .bind(request.execution_id)
    .bind(request.proposal.proposal_id)
    .bind(&action_fingerprint.value)
    .bind(governed_action_kind_as_str(request.proposal.action_kind))
    .bind(governed_action_risk_tier_as_str(risk_tier))
    .bind(governed_action_status_as_str(status))
    .bind(capability_scope_json)
    .bind(payload_json)
    .bind(workspace_script_id)
    .bind(workspace_script_version_id)
    .bind(blocked_reason)
    .execute(pool)
    .await
    .context("failed to insert governed action execution")?;

    Ok(())
}

fn decode_governed_action_execution_row(
    row: sqlx::postgres::PgRow,
) -> Result<GovernedActionExecutionRecord> {
    Ok(GovernedActionExecutionRecord {
        governed_action_execution_id: row.get("governed_action_execution_id"),
        trace_id: row.get("trace_id"),
        execution_id: row.get("execution_id"),
        approval_request_id: row.get("approval_request_id"),
        action_proposal_id: row.get("action_proposal_id"),
        action_fingerprint: GovernedActionFingerprint {
            value: row.get("action_fingerprint"),
        },
        action_kind: parse_governed_action_kind(row.get("action_kind"))?,
        risk_tier: parse_governed_action_risk_tier(row.get("risk_tier"))?,
        status: parse_governed_action_status(row.get("status"))?,
        capability_scope: serde_json::from_value(row.get("capability_scope_json"))
            .context("failed to decode governed action capability scope")?,
        payload: serde_json::from_value(row.get("payload_json"))
            .context("failed to decode governed action payload")?,
        workspace_script_id: row.get("workspace_script_id"),
        workspace_script_version_id: row.get("workspace_script_version_id"),
        blocked_reason: row.get("blocked_reason"),
        output_ref: row.get("output_ref"),
        started_at: row.get("started_at"),
        completed_at: row.get("completed_at"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

fn normalized_filesystem_roots(scope: &CapabilityScope) -> Vec<String> {
    scope
        .filesystem
        .read_roots
        .iter()
        .chain(scope.filesystem.write_roots.iter())
        .map(|root| root.trim().to_string())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn governed_action_kind_as_str(kind: GovernedActionKind) -> &'static str {
    match kind {
        GovernedActionKind::InspectWorkspaceArtifact => "inspect_workspace_artifact",
        GovernedActionKind::RunSubprocess => "run_subprocess",
        GovernedActionKind::RunWorkspaceScript => "run_workspace_script",
        GovernedActionKind::WebFetch => "web_fetch",
    }
}

fn parse_governed_action_kind(value: &str) -> Result<GovernedActionKind> {
    match value {
        "inspect_workspace_artifact" => Ok(GovernedActionKind::InspectWorkspaceArtifact),
        "run_subprocess" => Ok(GovernedActionKind::RunSubprocess),
        "run_workspace_script" => Ok(GovernedActionKind::RunWorkspaceScript),
        "web_fetch" => Ok(GovernedActionKind::WebFetch),
        other => bail!("unrecognized governed action kind '{other}'"),
    }
}

fn governed_action_risk_tier_as_str(risk_tier: GovernedActionRiskTier) -> &'static str {
    match risk_tier {
        GovernedActionRiskTier::Tier0 => "tier_0",
        GovernedActionRiskTier::Tier1 => "tier_1",
        GovernedActionRiskTier::Tier2 => "tier_2",
        GovernedActionRiskTier::Tier3 => "tier_3",
    }
}

fn parse_governed_action_risk_tier(value: &str) -> Result<GovernedActionRiskTier> {
    match value {
        "tier_0" => Ok(GovernedActionRiskTier::Tier0),
        "tier_1" => Ok(GovernedActionRiskTier::Tier1),
        "tier_2" => Ok(GovernedActionRiskTier::Tier2),
        "tier_3" => Ok(GovernedActionRiskTier::Tier3),
        other => bail!("unrecognized governed action risk tier '{other}'"),
    }
}

fn governed_action_status_as_str(status: GovernedActionStatus) -> &'static str {
    match status {
        GovernedActionStatus::Proposed => "proposed",
        GovernedActionStatus::AwaitingApproval => "awaiting_approval",
        GovernedActionStatus::Approved => "approved",
        GovernedActionStatus::Rejected => "rejected",
        GovernedActionStatus::Expired => "expired",
        GovernedActionStatus::Invalidated => "invalidated",
        GovernedActionStatus::Blocked => "blocked",
        GovernedActionStatus::Executed => "executed",
        GovernedActionStatus::Failed => "failed",
    }
}

fn parse_governed_action_status(value: &str) -> Result<GovernedActionStatus> {
    match value {
        "proposed" => Ok(GovernedActionStatus::Proposed),
        "awaiting_approval" => Ok(GovernedActionStatus::AwaitingApproval),
        "approved" => Ok(GovernedActionStatus::Approved),
        "rejected" => Ok(GovernedActionStatus::Rejected),
        "expired" => Ok(GovernedActionStatus::Expired),
        "invalidated" => Ok(GovernedActionStatus::Invalidated),
        "blocked" => Ok(GovernedActionStatus::Blocked),
        "executed" => Ok(GovernedActionStatus::Executed),
        "failed" => Ok(GovernedActionStatus::Failed),
        other => bail!("unrecognized governed action status '{other}'"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct CanonicalGovernedActionFingerprintInput {
    action_kind: &'static str,
    risk_tier: &'static str,
    capability_scope: CanonicalCapabilityScope,
    payload: CanonicalGovernedActionPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct CanonicalCapabilityScope {
    filesystem_read_roots: Vec<String>,
    filesystem_write_roots: Vec<String>,
    network: NetworkAccessPosture,
    environment_variables: Vec<String>,
    timeout_ms: u64,
    max_stdout_bytes: u64,
    max_stderr_bytes: u64,
}

impl From<&CapabilityScope> for CanonicalCapabilityScope {
    fn from(scope: &CapabilityScope) -> Self {
        Self {
            filesystem_read_roots: normalized_path_list(&scope.filesystem.read_roots),
            filesystem_write_roots: normalized_path_list(&scope.filesystem.write_roots),
            network: scope.network,
            environment_variables: normalized_path_list(&scope.environment.allow_variables),
            timeout_ms: scope.execution.timeout_ms,
            max_stdout_bytes: scope.execution.max_stdout_bytes,
            max_stderr_bytes: scope.execution.max_stderr_bytes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
enum CanonicalGovernedActionPayload {
    InspectWorkspaceArtifact {
        artifact_id: Uuid,
        artifact_kind: contracts::WorkspaceArtifactKind,
    },
    RunSubprocess {
        command: String,
        args: Vec<String>,
        working_directory: Option<String>,
    },
    RunWorkspaceScript {
        script_id: Uuid,
        script_version_id: Option<Uuid>,
        args: Vec<String>,
    },
    WebFetch {
        url: String,
        timeout_ms: u64,
        max_response_bytes: u64,
    },
}

impl From<&GovernedActionPayload> for CanonicalGovernedActionPayload {
    fn from(payload: &GovernedActionPayload) -> Self {
        match payload {
            GovernedActionPayload::InspectWorkspaceArtifact(action) => {
                Self::InspectWorkspaceArtifact {
                    artifact_id: action.artifact_id,
                    artifact_kind: action.artifact_kind,
                }
            }
            GovernedActionPayload::RunSubprocess(action) => Self::RunSubprocess {
                command: action.command.trim().to_string(),
                args: action.args.clone(),
                working_directory: action
                    .working_directory
                    .as_ref()
                    .map(|path| path.trim().to_string()),
            },
            GovernedActionPayload::RunWorkspaceScript(action) => Self::RunWorkspaceScript {
                script_id: action.script_id,
                script_version_id: action.script_version_id,
                args: action.args.clone(),
            },
            GovernedActionPayload::WebFetch(action) => Self::WebFetch {
                url: action.url.trim().to_string(),
                timeout_ms: action.timeout_ms,
                max_response_bytes: action.max_response_bytes,
            },
        }
    }
}

fn normalized_path_list(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.trim().to_string())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use contracts::{
        EnvironmentCapabilityScope, ExecutionCapabilityBudget, FilesystemCapabilityScope,
        WorkspaceArtifactKind,
    };

    use super::*;

    fn sample_config() -> RuntimeConfig {
        crate::config::RuntimeConfig {
            app: crate::config::AppConfig {
                name: "blue-lagoon".to_string(),
                log_filter: "info".to_string(),
            },
            database: crate::config::DatabaseConfig {
                database_url: "postgres://localhost/blue_lagoon".to_string(),
                minimum_supported_schema_version: 1,
            },
            harness: crate::config::HarnessConfig {
                allow_synthetic_smoke: true,
                default_foreground_iteration_budget: 1,
                default_wall_clock_budget_ms: 30_000,
                default_foreground_token_budget: 4_000,
            },
            background: crate::config::BackgroundConfig {
                scheduler: crate::config::BackgroundSchedulerConfig {
                    poll_interval_seconds: 300,
                    max_due_jobs_per_iteration: 4,
                    lease_timeout_ms: 300_000,
                },
                thresholds: crate::config::BackgroundThresholdsConfig {
                    episode_backlog_threshold: 25,
                    candidate_memory_threshold: 10,
                    contradiction_alert_threshold: 3,
                },
                execution: crate::config::BackgroundExecutionConfig {
                    default_iteration_budget: 2,
                    default_wall_clock_budget_ms: 120_000,
                    default_token_budget: 6_000,
                },
                wake_signals: crate::config::WakeSignalPolicyConfig {
                    allow_foreground_conversion: true,
                    max_pending_signals: 8,
                    cooldown_seconds: 900,
                },
            },
            continuity: crate::config::ContinuityConfig {
                retrieval: crate::config::RetrievalConfig {
                    max_recent_episode_candidates: 3,
                    max_memory_artifact_candidates: 5,
                    max_context_items: 6,
                },
                backlog_recovery: crate::config::BacklogRecoveryConfig {
                    pending_message_count_threshold: 3,
                    pending_message_span_seconds_threshold: 120,
                    stale_pending_ingress_age_seconds_threshold: 300,
                    max_recovery_batch_size: 8,
                },
            },
            scheduled_foreground: crate::config::ScheduledForegroundConfig {
                enabled: true,
                max_due_tasks_per_iteration: 2,
                min_cadence_seconds: 300,
                default_cooldown_seconds: 300,
            },
            workspace: crate::config::WorkspaceConfig {
                root_dir: ".".into(),
                max_artifact_bytes: 1_048_576,
                max_script_bytes: 262_144,
            },
            approvals: crate::config::ApprovalsConfig {
                default_ttl_seconds: 900,
                max_pending_requests: 32,
                allow_cli_resolution: true,
                prompt_mode: crate::config::ApprovalPromptMode::InlineKeyboardWithFallback,
            },
            governed_actions: crate::config::GovernedActionsConfig {
                approval_required_min_risk_tier: GovernedActionRiskTier::Tier2,
                default_subprocess_timeout_ms: 30_000,
                max_subprocess_timeout_ms: 120_000,
                max_filesystem_roots_per_action: 4,
                default_network_access: NetworkAccessPosture::Disabled,
                allowlisted_environment_variables: vec!["BLUE_LAGOON_DATABASE_URL".to_string()],
                max_environment_variables_per_action: 8,
                max_captured_output_bytes: 65_536,
                max_web_fetch_timeout_ms: 15_000,
                max_web_fetch_response_bytes: 524_288,
            },
            worker: crate::config::WorkerConfig {
                timeout_ms: 20_000,
                command: "workers".to_string(),
                args: vec!["conscious-worker".to_string()],
            },
            telegram: None,
            model_gateway: None,
            self_model: None,
        }
    }

    fn sample_subprocess_proposal() -> GovernedActionProposal {
        GovernedActionProposal {
            proposal_id: Uuid::now_v7(),
            title: "Run bounded command".to_string(),
            rationale: Some("verify governed-action planning".to_string()),
            action_kind: GovernedActionKind::RunSubprocess,
            requested_risk_tier: None,
            capability_scope: CapabilityScope {
                filesystem: FilesystemCapabilityScope {
                    read_roots: vec![
                        "D:/Repos/blue-lagoon".to_string(),
                        "D:/Repos/blue-lagoon".to_string(),
                    ],
                    write_roots: vec!["D:/Repos/blue-lagoon/docs".to_string()],
                },
                network: NetworkAccessPosture::Disabled,
                environment: EnvironmentCapabilityScope {
                    allow_variables: vec!["BLUE_LAGOON_DATABASE_URL".to_string()],
                },
                execution: ExecutionCapabilityBudget {
                    timeout_ms: 30_000,
                    max_stdout_bytes: 16_384,
                    max_stderr_bytes: 8_192,
                },
            },
            payload: GovernedActionPayload::RunSubprocess(SubprocessAction {
                command: "cmd".to_string(),
                args: vec!["/c".to_string(), "echo".to_string(), "hello".to_string()],
                working_directory: Some("D:/Repos/blue-lagoon".to_string()),
            }),
        }
    }

    #[test]
    fn governed_action_fingerprint_is_stable_for_equivalent_scope_orderings() {
        let proposal = sample_subprocess_proposal();
        let mut reordered = proposal.clone();
        reordered.capability_scope.filesystem.read_roots = vec![
            "D:/Repos/blue-lagoon".to_string(),
            "D:/Repos/blue-lagoon".to_string(),
        ];
        reordered.capability_scope.environment.allow_variables =
            vec!["BLUE_LAGOON_DATABASE_URL".to_string()];

        let first = fingerprint_governed_action(&proposal).expect("fingerprint should derive");
        let second =
            fingerprint_governed_action(&reordered).expect("fingerprint should derive again");
        assert_eq!(first, second);
    }

    #[test]
    fn governed_action_fingerprint_changes_when_payload_changes() {
        let proposal = sample_subprocess_proposal();
        let mut changed = proposal.clone();
        changed.payload = GovernedActionPayload::RunSubprocess(SubprocessAction {
            command: "cmd".to_string(),
            args: vec![
                "/c".to_string(),
                "echo".to_string(),
                "different".to_string(),
            ],
            working_directory: Some("D:/Repos/blue-lagoon".to_string()),
        });

        let first = fingerprint_governed_action(&proposal).expect("fingerprint should derive");
        let second = fingerprint_governed_action(&changed).expect("fingerprint should derive");
        assert_ne!(first, second);
    }

    #[test]
    fn inspect_workspace_artifact_scope_rejects_side_effecting_capabilities() {
        let mut proposal = GovernedActionProposal {
            proposal_id: Uuid::now_v7(),
            title: "Inspect note".to_string(),
            rationale: None,
            action_kind: GovernedActionKind::InspectWorkspaceArtifact,
            requested_risk_tier: None,
            capability_scope: CapabilityScope {
                filesystem: FilesystemCapabilityScope {
                    read_roots: vec!["D:/Repos/blue-lagoon".to_string()],
                    write_roots: vec!["D:/Repos/blue-lagoon/docs".to_string()],
                },
                network: NetworkAccessPosture::Disabled,
                environment: EnvironmentCapabilityScope {
                    allow_variables: Vec::new(),
                },
                execution: ExecutionCapabilityBudget {
                    timeout_ms: 1_000,
                    max_stdout_bytes: 1_024,
                    max_stderr_bytes: 1_024,
                },
            },
            payload: GovernedActionPayload::InspectWorkspaceArtifact(
                InspectWorkspaceArtifactAction {
                    artifact_id: Uuid::now_v7(),
                    artifact_kind: WorkspaceArtifactKind::Note,
                },
            ),
        };

        let error = validate_capability_scope(&sample_config(), &proposal)
            .expect_err("write-scoped inspection should be rejected");
        assert!(
            error
                .to_string()
                .contains("must not request filesystem write scope")
        );

        proposal.capability_scope.filesystem.write_roots.clear();
        proposal.capability_scope.network = NetworkAccessPosture::Enabled;
        let network_error = validate_capability_scope(&sample_config(), &proposal)
            .expect_err("network-scoped inspection should be rejected");
        assert!(
            network_error
                .to_string()
                .contains("must not request network access")
        );
    }

    #[test]
    fn environment_scope_must_be_allowlisted() {
        let mut proposal = sample_subprocess_proposal();
        proposal.capability_scope.environment.allow_variables =
            vec!["HOME".to_string(), "BLUE_LAGOON_DATABASE_URL".to_string()];

        let error = validate_capability_scope(&sample_config(), &proposal)
            .expect_err("non-allowlisted variables should be rejected");
        assert!(error.to_string().contains("not allowlisted"));
    }

    #[test]
    fn subprocess_scope_requires_at_least_one_filesystem_root() {
        let mut proposal = sample_subprocess_proposal();
        proposal.capability_scope.filesystem.read_roots.clear();
        proposal.capability_scope.filesystem.write_roots.clear();

        let error = validate_capability_scope(&sample_config(), &proposal)
            .expect_err("subprocess without any filesystem scope should be rejected");
        assert!(
            error
                .to_string()
                .contains("must request at least one filesystem root")
        );
    }

    #[test]
    fn subprocess_scope_rejects_timeout_above_configured_limit() {
        let mut proposal = sample_subprocess_proposal();
        proposal.capability_scope.execution.timeout_ms = 120_001;

        let error = validate_capability_scope(&sample_config(), &proposal)
            .expect_err("timeout above configured limit should be rejected");
        assert!(error.to_string().contains("exceeds the configured maximum"));
    }

    #[test]
    fn subprocess_scope_rejects_captured_output_above_configured_limit() {
        let mut proposal = sample_subprocess_proposal();
        proposal.capability_scope.execution.max_stdout_bytes = 70_000;

        let error = validate_capability_scope(&sample_config(), &proposal)
            .expect_err("captured output above configured limit should be rejected");
        assert!(
            error
                .to_string()
                .contains("captured output exceeds the configured maximum")
        );
    }

    #[test]
    fn web_fetch_summary_carries_bounded_response_preview() {
        let action = WebFetchAction {
            url: "https://example.com/weather".to_string(),
            timeout_ms: 10_000,
            max_response_bytes: 65_536,
        };

        let summary =
            web_fetch_execution_summary(&action, "  Weather\n\nReport: sunny and mild.  ", false);

        assert!(summary.contains("web fetch completed for https://example.com/weather"));
        assert!(summary.contains("response preview: Weather Report: sunny and mild."));
    }

    #[test]
    fn web_fetch_summary_marks_truncation() {
        let action = WebFetchAction {
            url: "https://example.com/large".to_string(),
            timeout_ms: 10_000,
            max_response_bytes: 32,
        };

        let summary = web_fetch_execution_summary(&action, "abcdef", true);

        assert!(summary.contains("response truncated to 32 bytes"));
    }
}
