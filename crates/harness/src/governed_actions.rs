use std::collections::BTreeSet;

use anyhow::{Context, Result, bail};
use contracts::{
    CapabilityScope, GovernedActionExecutionOutcome, GovernedActionFingerprint, GovernedActionKind,
    GovernedActionPayload, GovernedActionProposal, GovernedActionRiskTier, GovernedActionStatus,
    InspectWorkspaceArtifactAction, NetworkAccessPosture, SubprocessAction, WorkspaceScriptAction,
};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::{
    audit::{self, NewAuditEvent},
    config::RuntimeConfig,
    policy,
};

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
    if filesystem_roots.is_empty()
        && proposal.action_kind != GovernedActionKind::InspectWorkspaceArtifact
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
    }
}

fn parse_governed_action_kind(value: &str) -> Result<GovernedActionKind> {
    match value {
        "inspect_workspace_artifact" => Ok(GovernedActionKind::InspectWorkspaceArtifact),
        "run_subprocess" => Ok(GovernedActionKind::RunSubprocess),
        "run_workspace_script" => Ok(GovernedActionKind::RunWorkspaceScript),
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
}
