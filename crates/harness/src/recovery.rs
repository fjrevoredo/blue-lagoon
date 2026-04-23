use anyhow::{Context, Result, bail};
use chrono::{DateTime, Duration, Utc};
use contracts::{GovernedActionKind, GovernedActionStatus, WorkspaceScriptRunStatus};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::{
    approval::ApprovalRequestRecord,
    audit::{self, NewAuditEvent},
    background::{self, WakeSignalRecord},
    execution, governed_actions, workspace,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryCheckpointKind {
    Foreground,
    Background,
    GovernedAction,
}

impl RecoveryCheckpointKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Foreground => "foreground",
            Self::Background => "background",
            Self::GovernedAction => "governed_action",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "foreground" => Ok(Self::Foreground),
            "background" => Ok(Self::Background),
            "governed_action" => Ok(Self::GovernedAction),
            other => bail!("unrecognized recovery checkpoint kind '{other}'"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryReasonCode {
    Crash,
    TimeoutOrStall,
    SupervisorRestart,
    ApprovalTransition,
    IntegrityOrPolicyBlock,
}

impl RecoveryReasonCode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Crash => "crash",
            Self::TimeoutOrStall => "timeout_or_stall",
            Self::SupervisorRestart => "supervisor_restart",
            Self::ApprovalTransition => "approval_transition",
            Self::IntegrityOrPolicyBlock => "integrity_or_policy_block",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "crash" => Ok(Self::Crash),
            "timeout_or_stall" => Ok(Self::TimeoutOrStall),
            "supervisor_restart" => Ok(Self::SupervisorRestart),
            "approval_transition" => Ok(Self::ApprovalTransition),
            "integrity_or_policy_block" => Ok(Self::IntegrityOrPolicyBlock),
            other => bail!("unrecognized recovery reason code '{other}'"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryCheckpointStatus {
    Open,
    Resolved,
    Abandoned,
    Invalidated,
}

impl RecoveryCheckpointStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Resolved => "resolved",
            Self::Abandoned => "abandoned",
            Self::Invalidated => "invalidated",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "open" => Ok(Self::Open),
            "resolved" => Ok(Self::Resolved),
            "abandoned" => Ok(Self::Abandoned),
            "invalidated" => Ok(Self::Invalidated),
            other => bail!("unrecognized recovery checkpoint status '{other}'"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryDecision {
    Continue,
    Retry,
    Defer,
    Reapprove,
    Clarify,
    Abandon,
}

impl RecoveryDecision {
    fn as_str(self) -> &'static str {
        match self {
            Self::Continue => "continue",
            Self::Retry => "retry",
            Self::Defer => "defer",
            Self::Reapprove => "reapprove",
            Self::Clarify => "clarify",
            Self::Abandon => "abandon",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "continue" => Ok(Self::Continue),
            "retry" => Ok(Self::Retry),
            "defer" => Ok(Self::Defer),
            "reapprove" => Ok(Self::Reapprove),
            "clarify" => Ok(Self::Clarify),
            "abandon" => Ok(Self::Abandon),
            other => bail!("unrecognized recovery decision '{other}'"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryActionClassification {
    SafeReplay,
    ProvablyIdempotentExternal,
    AmbiguousOrNonrepeatable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryEvidenceState {
    NotStarted,
    DurableIncomplete,
    DurableCompleted,
    Ambiguous,
    Corrupted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryApprovalState {
    NotRequired,
    Pending,
    ApprovedFresh,
    Expired,
    Invalidated,
    Rejected,
    MissingRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryPolicyState {
    Valid,
    RequiresRecheck,
    RecheckFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryDecisionRequest {
    pub checkpoint_kind: RecoveryCheckpointKind,
    pub reason_code: RecoveryReasonCode,
    pub action_classification: RecoveryActionClassification,
    pub evidence_state: RecoveryEvidenceState,
    pub approval_state: RecoveryApprovalState,
    pub policy_state: RecoveryPolicyState,
    pub recovery_budget_remaining: i32,
    pub clarification_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryDecisionOutcome {
    pub decision: RecoveryDecision,
    pub checkpoint_status: RecoveryCheckpointStatus,
    pub summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerLeaseSupervisionDecision {
    Healthy,
    SoftWarning,
    HardExpired,
}

#[derive(Debug, Clone)]
pub struct WorkerLeaseRecoveryOutcome {
    pub lease: WorkerLeaseRecord,
    pub checkpoint: RecoveryCheckpointRecord,
    pub decision: RecoveryDecisionOutcome,
    pub diagnostic: OperationalDiagnosticRecord,
}

#[derive(Debug, Clone)]
pub struct RecoveryTriggerOutcome {
    pub checkpoint: RecoveryCheckpointRecord,
    pub decision: RecoveryDecisionOutcome,
    pub diagnostic: OperationalDiagnosticRecord,
}

#[derive(Debug, Clone)]
pub struct ForegroundRestartRecoveryRequest<'a> {
    pub trace_id: Uuid,
    pub execution_id: Uuid,
    pub internal_conversation_ref: &'a str,
    pub recovery_reason_code: RecoveryReasonCode,
    pub trigger_source: &'a str,
    pub decision_reason: &'a str,
    pub selected_ingress_ids: &'a [Uuid],
    pub primary_ingress_id: Uuid,
    pub recovery_mode: &'a str,
}

#[derive(Debug, Clone)]
pub struct WorkerLeaseSupervisionSummary {
    pub soft_warning_diagnostics: Vec<OperationalDiagnosticRecord>,
    pub recovered_expired_leases: Vec<WorkerLeaseRecoveryOutcome>,
}

pub fn evaluate_recovery_decision(
    request: &RecoveryDecisionRequest,
) -> Result<RecoveryDecisionOutcome> {
    validate_recovery_decision_request(request)?;

    let decision = classify_recovery_decision(request);
    let checkpoint_status = checkpoint_status_for_decision(decision);
    Ok(RecoveryDecisionOutcome {
        decision,
        checkpoint_status,
        summary: recovery_decision_summary(request, decision),
    })
}

fn validate_recovery_decision_request(request: &RecoveryDecisionRequest) -> Result<()> {
    if request.recovery_budget_remaining < 0 {
        bail!("recovery budget remaining must not be negative");
    }
    Ok(())
}

fn classify_recovery_decision(request: &RecoveryDecisionRequest) -> RecoveryDecision {
    if request.recovery_budget_remaining == 0 {
        return RecoveryDecision::Abandon;
    }

    if request.evidence_state == RecoveryEvidenceState::Corrupted
        || request.policy_state == RecoveryPolicyState::RecheckFailed
        || request.reason_code == RecoveryReasonCode::IntegrityOrPolicyBlock
        || request.approval_state == RecoveryApprovalState::Rejected
    {
        return RecoveryDecision::Abandon;
    }

    match request.approval_state {
        RecoveryApprovalState::Pending => return RecoveryDecision::Defer,
        RecoveryApprovalState::Expired
        | RecoveryApprovalState::Invalidated
        | RecoveryApprovalState::MissingRequired => return RecoveryDecision::Reapprove,
        RecoveryApprovalState::NotRequired | RecoveryApprovalState::ApprovedFresh => {}
        RecoveryApprovalState::Rejected => unreachable!("rejected approvals are handled above"),
    }

    if request.evidence_state == RecoveryEvidenceState::DurableCompleted {
        return RecoveryDecision::Continue;
    }

    match request.action_classification {
        RecoveryActionClassification::SafeReplay => match request.checkpoint_kind {
            RecoveryCheckpointKind::Foreground => RecoveryDecision::Continue,
            RecoveryCheckpointKind::Background | RecoveryCheckpointKind::GovernedAction => {
                RecoveryDecision::Retry
            }
        },
        RecoveryActionClassification::ProvablyIdempotentExternal => RecoveryDecision::Retry,
        RecoveryActionClassification::AmbiguousOrNonrepeatable => {
            if request.clarification_available {
                RecoveryDecision::Clarify
            } else {
                RecoveryDecision::Abandon
            }
        }
    }
}

fn checkpoint_status_for_decision(decision: RecoveryDecision) -> RecoveryCheckpointStatus {
    match decision {
        RecoveryDecision::Abandon => RecoveryCheckpointStatus::Abandoned,
        RecoveryDecision::Continue
        | RecoveryDecision::Retry
        | RecoveryDecision::Defer
        | RecoveryDecision::Reapprove
        | RecoveryDecision::Clarify => RecoveryCheckpointStatus::Resolved,
    }
}

fn recovery_decision_summary(
    request: &RecoveryDecisionRequest,
    decision: RecoveryDecision,
) -> String {
    match decision {
        RecoveryDecision::Continue => {
            "durable evidence supports fresh-worker continuation".to_string()
        }
        RecoveryDecision::Retry => {
            "work can be retried within the remaining recovery budget".to_string()
        }
        RecoveryDecision::Defer => {
            "recovery is deferred until the pending approval state resolves".to_string()
        }
        RecoveryDecision::Reapprove => {
            "recovery requires a fresh approval before execution may continue".to_string()
        }
        RecoveryDecision::Clarify => {
            "side-effect state is ambiguous and requires user clarification".to_string()
        }
        RecoveryDecision::Abandon => recovery_abandonment_summary(request),
    }
}

fn recovery_abandonment_summary(request: &RecoveryDecisionRequest) -> String {
    if request.recovery_budget_remaining == 0 {
        return "recovery budget is exhausted".to_string();
    }
    if request.evidence_state == RecoveryEvidenceState::Corrupted {
        return "checkpoint evidence is corrupted or incomplete".to_string();
    }
    if request.policy_state == RecoveryPolicyState::RecheckFailed
        || request.reason_code == RecoveryReasonCode::IntegrityOrPolicyBlock
    {
        return "policy or integrity validation blocks recovery".to_string();
    }
    if request.approval_state == RecoveryApprovalState::Rejected {
        return "approval was rejected".to_string();
    }
    if request.action_classification == RecoveryActionClassification::AmbiguousOrNonrepeatable {
        return "side-effect state is ambiguous and no safe clarification path is available"
            .to_string();
    }
    "recovery cannot continue safely".to_string()
}

pub fn classify_worker_lease_supervision(
    lease: &WorkerLeaseRecord,
    now: DateTime<Utc>,
    soft_warning_threshold_percent: u8,
) -> Result<WorkerLeaseSupervisionDecision> {
    if !(1..=100).contains(&soft_warning_threshold_percent) {
        bail!("soft warning threshold percent must be between 1 and 100");
    }

    if lease.status != WorkerLeaseStatus::Active {
        return Ok(WorkerLeaseSupervisionDecision::Healthy);
    }
    if lease.lease_expires_at <= now {
        return Ok(WorkerLeaseSupervisionDecision::HardExpired);
    }

    let total_ms = (lease.lease_expires_at - lease.lease_acquired_at).num_milliseconds();
    if total_ms <= 0 {
        return Ok(WorkerLeaseSupervisionDecision::HardExpired);
    }
    let elapsed_ms = (now - lease.lease_acquired_at).num_milliseconds().max(0);
    let warning_ms = total_ms * i64::from(soft_warning_threshold_percent) / 100;
    if elapsed_ms >= warning_ms {
        Ok(WorkerLeaseSupervisionDecision::SoftWarning)
    } else {
        Ok(WorkerLeaseSupervisionDecision::Healthy)
    }
}

pub async fn recover_expired_worker_leases(
    pool: &PgPool,
    now: DateTime<Utc>,
) -> Result<Vec<WorkerLeaseRecoveryOutcome>> {
    let expired_leases = expire_due_worker_leases(pool, now).await?;
    let mut outcomes = Vec::with_capacity(expired_leases.len());

    for lease in expired_leases {
        outcomes.push(
            recover_worker_lease_timeout(
                pool,
                lease,
                now,
                "worker_lease_expiry",
                "worker_lease_expired",
                OperationalDiagnosticSeverity::Warn,
                None,
            )
            .await?,
        );
    }

    Ok(outcomes)
}

pub async fn recover_observed_worker_timeout(
    pool: &PgPool,
    worker_lease_id: Uuid,
    now: DateTime<Utc>,
    observed_source: &str,
    error_message: &str,
) -> Result<WorkerLeaseRecoveryOutcome> {
    if observed_source.trim().is_empty() {
        bail!("observed worker timeout source must not be empty");
    }
    if error_message.trim().is_empty() {
        bail!("observed worker timeout error message must not be empty");
    }

    let lease = terminate_active_worker_lease(pool, worker_lease_id, now).await?;
    recover_worker_lease_timeout(
        pool,
        lease,
        now,
        observed_source,
        "worker_lease_timeout_observed",
        OperationalDiagnosticSeverity::Error,
        Some(error_message),
    )
    .await
}

pub async fn recover_approval_request_transition(
    pool: &PgPool,
    request: &ApprovalRequestRecord,
    approval_state: RecoveryApprovalState,
    now: DateTime<Utc>,
    diagnostic_reason_code: &str,
) -> Result<RecoveryTriggerOutcome> {
    match approval_state {
        RecoveryApprovalState::Pending
        | RecoveryApprovalState::Expired
        | RecoveryApprovalState::Invalidated
        | RecoveryApprovalState::MissingRequired => {}
        other => bail!("approval transition recovery does not support state {other:?}"),
    }

    let linked_execution = governed_actions::get_governed_action_execution_by_approval_request_id(
        pool,
        request.approval_request_id,
    )
    .await?;
    let checkpoint_kind = if linked_execution.is_some() {
        RecoveryCheckpointKind::GovernedAction
    } else {
        RecoveryCheckpointKind::Foreground
    };
    let governed_action_execution_id = linked_execution
        .as_ref()
        .map(|record| record.governed_action_execution_id);
    let action_classification = if governed_action_execution_id.is_some() {
        RecoveryActionClassification::AmbiguousOrNonrepeatable
    } else {
        RecoveryActionClassification::SafeReplay
    };
    let checkpoint_payload = json!({
        "approval_request_id": request.approval_request_id,
        "action_proposal_id": request.action_proposal_id,
        "action_fingerprint": request.action_fingerprint.value,
        "action_kind": request.action_kind,
        "risk_tier": request.risk_tier,
        "title": request.title,
        "consequence_summary": request.consequence_summary,
        "requested_by": request.requested_by,
        "requested_at": request.requested_at,
        "expires_at": request.expires_at,
        "approval_state": recovery_approval_state_label(approval_state),
        "governed_action_execution_id": governed_action_execution_id,
    });
    let diagnostic_summary = match approval_state {
        RecoveryApprovalState::Pending => {
            "governed action execution is deferred pending approval resolution"
        }
        RecoveryApprovalState::Expired => "approval request expired before resolution",
        RecoveryApprovalState::Invalidated => {
            "approval request was invalidated and requires explicit follow-up"
        }
        RecoveryApprovalState::MissingRequired => {
            "required approval request is missing and must be recreated"
        }
        _ => unreachable!("unsupported approval transition state was rejected above"),
    };

    issue_recovery_trigger(
        pool,
        NewRecoveryCheckpoint {
            recovery_checkpoint_id: Uuid::now_v7(),
            trace_id: request.trace_id,
            execution_id: request.execution_id,
            background_job_id: None,
            background_job_run_id: None,
            governed_action_execution_id,
            approval_request_id: Some(request.approval_request_id),
            checkpoint_kind,
            recovery_reason_code: RecoveryReasonCode::ApprovalTransition,
            recovery_budget_remaining: 1,
            checkpoint_payload: checkpoint_payload.clone(),
        },
        RecoveryDecisionRequest {
            checkpoint_kind,
            reason_code: RecoveryReasonCode::ApprovalTransition,
            action_classification,
            evidence_state: RecoveryEvidenceState::NotStarted,
            approval_state,
            policy_state: RecoveryPolicyState::Valid,
            recovery_budget_remaining: 1,
            clarification_available: false,
        },
        NewOperationalDiagnostic {
            operational_diagnostic_id: Uuid::now_v7(),
            trace_id: Some(request.trace_id),
            execution_id: request.execution_id,
            subsystem: "approval".to_string(),
            severity: OperationalDiagnosticSeverity::Warn,
            reason_code: diagnostic_reason_code.to_string(),
            summary: diagnostic_summary.to_string(),
            diagnostic_payload: checkpoint_payload,
        },
        now,
    )
    .await
}

pub async fn recover_wake_signal_policy_block(
    pool: &PgPool,
    wake_signal: &WakeSignalRecord,
    now: DateTime<Utc>,
    diagnostic_reason_code: &str,
    diagnostic_summary: &str,
) -> Result<RecoveryTriggerOutcome> {
    let checkpoint_payload = json!({
        "wake_signal_id": wake_signal.wake_signal_id,
        "background_job_id": wake_signal.background_job_id,
        "background_job_run_id": wake_signal.background_job_run_id,
        "execution_id": wake_signal.execution_id,
        "status": format!("{:?}", wake_signal.status).to_lowercase(),
        "priority": wake_signal.signal.priority,
        "reason_code": wake_signal.signal.reason_code,
        "summary": wake_signal.signal.summary,
        "requested_at": wake_signal.requested_at,
    });
    issue_recovery_trigger(
        pool,
        NewRecoveryCheckpoint {
            recovery_checkpoint_id: Uuid::now_v7(),
            trace_id: wake_signal.trace_id,
            execution_id: wake_signal.execution_id,
            background_job_id: Some(wake_signal.background_job_id),
            background_job_run_id: wake_signal.background_job_run_id,
            governed_action_execution_id: None,
            approval_request_id: None,
            checkpoint_kind: RecoveryCheckpointKind::Background,
            recovery_reason_code: RecoveryReasonCode::IntegrityOrPolicyBlock,
            recovery_budget_remaining: 1,
            checkpoint_payload: checkpoint_payload.clone(),
        },
        RecoveryDecisionRequest {
            checkpoint_kind: RecoveryCheckpointKind::Background,
            reason_code: RecoveryReasonCode::IntegrityOrPolicyBlock,
            action_classification: RecoveryActionClassification::SafeReplay,
            evidence_state: RecoveryEvidenceState::NotStarted,
            approval_state: RecoveryApprovalState::NotRequired,
            policy_state: RecoveryPolicyState::Valid,
            recovery_budget_remaining: 1,
            clarification_available: false,
        },
        NewOperationalDiagnostic {
            operational_diagnostic_id: Uuid::now_v7(),
            trace_id: Some(wake_signal.trace_id),
            execution_id: wake_signal.execution_id,
            subsystem: "background_execution".to_string(),
            severity: OperationalDiagnosticSeverity::Warn,
            reason_code: diagnostic_reason_code.to_string(),
            summary: diagnostic_summary.to_string(),
            diagnostic_payload: checkpoint_payload,
        },
        now,
    )
    .await
}

pub async fn recover_governed_action_policy_recheck_failure(
    pool: &PgPool,
    record: &governed_actions::GovernedActionExecutionRecord,
    now: DateTime<Utc>,
    failure_summary: &str,
) -> Result<RecoveryTriggerOutcome> {
    if failure_summary.trim().is_empty() {
        bail!("governed action policy re-check failure summary must not be empty");
    }

    let context = load_governed_action_recovery_context(
        pool,
        record.clone(),
        RecoveryReasonCode::IntegrityOrPolicyBlock,
        RecoveryPolicyState::RecheckFailed,
    )
    .await?;
    let checkpoint_payload = governed_action_recovery_checkpoint_payload(
        &context,
        None,
        "policy_recheck",
        Some(failure_summary),
    );
    let decision = evaluate_recovery_decision(&RecoveryDecisionRequest {
        checkpoint_kind: RecoveryCheckpointKind::GovernedAction,
        reason_code: RecoveryReasonCode::IntegrityOrPolicyBlock,
        action_classification: context.action_classification,
        evidence_state: context.evidence_state,
        approval_state: context.approval_state,
        policy_state: RecoveryPolicyState::RecheckFailed,
        recovery_budget_remaining: 1,
        clarification_available: context.clarification_available,
    })?;
    let checkpoint = create_recovery_checkpoint(
        pool,
        &NewRecoveryCheckpoint {
            recovery_checkpoint_id: Uuid::now_v7(),
            trace_id: record.trace_id,
            execution_id: record.execution_id,
            background_job_id: None,
            background_job_run_id: None,
            governed_action_execution_id: Some(record.governed_action_execution_id),
            approval_request_id: record.approval_request_id,
            checkpoint_kind: RecoveryCheckpointKind::GovernedAction,
            recovery_reason_code: RecoveryReasonCode::IntegrityOrPolicyBlock,
            recovery_budget_remaining: 1,
            checkpoint_payload: checkpoint_payload.clone(),
        },
    )
    .await?;
    let checkpoint = resolve_recovery_checkpoint(
        pool,
        &RecoveryCheckpointResolution {
            recovery_checkpoint_id: checkpoint.recovery_checkpoint_id,
            status: decision.checkpoint_status,
            recovery_decision: decision.decision,
            resolved_summary: Some(decision.summary.clone()),
            resolved_at: now,
        },
    )
    .await?;
    let reconciled_record = reconcile_governed_action_recovery(
        pool,
        &context,
        decision.decision,
        RecoveryPolicyState::RecheckFailed,
        now,
        &decision.summary,
    )
    .await?;
    governed_actions::write_governed_action_audit_event(
        pool,
        &reconciled_record,
        "governed_action_recovery_resolved",
        "warn",
        json!({
            "recovery_checkpoint_id": checkpoint.recovery_checkpoint_id,
            "recovery_decision": decision.decision.as_str(),
            "recovery_reason_code": RecoveryReasonCode::IntegrityOrPolicyBlock.as_str(),
            "action_classification": recovery_action_classification_label(context.action_classification),
            "evidence_state": recovery_evidence_state_label(context.evidence_state),
            "approval_state": recovery_approval_state_label(context.approval_state),
            "policy_state": recovery_policy_state_label(RecoveryPolicyState::RecheckFailed),
        }),
    )
    .await?;
    let diagnostic = insert_operational_diagnostic(
        pool,
        &NewOperationalDiagnostic {
            operational_diagnostic_id: Uuid::now_v7(),
            trace_id: Some(record.trace_id),
            execution_id: record.execution_id,
            subsystem: "governed_actions".to_string(),
            severity: OperationalDiagnosticSeverity::Warn,
            reason_code: "governed_action_policy_recheck_failed".to_string(),
            summary: format!(
                "governed action execution was blocked during policy re-check; recovery decision: {}",
                decision.decision.as_str()
            ),
            diagnostic_payload: checkpoint_payload,
        },
    )
    .await?;

    Ok(RecoveryTriggerOutcome {
        checkpoint,
        decision,
        diagnostic,
    })
}

pub async fn recover_foreground_restart_trigger(
    pool: &PgPool,
    request: ForegroundRestartRecoveryRequest<'_>,
    now: DateTime<Utc>,
) -> Result<RecoveryTriggerOutcome> {
    if !matches!(
        request.recovery_reason_code,
        RecoveryReasonCode::Crash | RecoveryReasonCode::SupervisorRestart
    ) {
        bail!("foreground restart recovery requires crash or supervisor-restart reason code");
    }
    if request.internal_conversation_ref.trim().is_empty() {
        bail!("foreground restart recovery requires an internal conversation reference");
    }
    if request.trigger_source.trim().is_empty() {
        bail!("foreground restart recovery requires a trigger source");
    }
    if request.decision_reason.trim().is_empty() {
        bail!("foreground restart recovery requires a decision reason");
    }
    if request.recovery_mode.trim().is_empty() {
        bail!("foreground restart recovery requires a recovery mode");
    }
    if request.selected_ingress_ids.is_empty() {
        bail!("foreground restart recovery requires at least one selected ingress");
    }

    let checkpoint_payload = json!({
        "source": request.trigger_source,
        "internal_conversation_ref": request.internal_conversation_ref,
        "decision_reason": request.decision_reason,
        "recovery_mode": request.recovery_mode,
        "selected_ingress_ids": request.selected_ingress_ids,
        "primary_ingress_id": request.primary_ingress_id,
    });
    let diagnostic_reason_code = match request.recovery_reason_code {
        RecoveryReasonCode::Crash => "foreground_processing_crash_recovered",
        RecoveryReasonCode::SupervisorRestart => "foreground_supervisor_restart_recovered",
        _ => unreachable!("guarded above"),
    };
    let reason_summary = match request.recovery_reason_code {
        RecoveryReasonCode::Crash => {
            "foreground processing was resumed after a crash or interrupted execution"
        }
        RecoveryReasonCode::SupervisorRestart => {
            "foreground backlog recovery was resumed during supervisor startup"
        }
        _ => unreachable!("guarded above"),
    };

    issue_recovery_trigger(
        pool,
        NewRecoveryCheckpoint {
            recovery_checkpoint_id: Uuid::now_v7(),
            trace_id: request.trace_id,
            execution_id: Some(request.execution_id),
            background_job_id: None,
            background_job_run_id: None,
            governed_action_execution_id: None,
            approval_request_id: None,
            checkpoint_kind: RecoveryCheckpointKind::Foreground,
            recovery_reason_code: request.recovery_reason_code,
            recovery_budget_remaining: 1,
            checkpoint_payload: checkpoint_payload.clone(),
        },
        RecoveryDecisionRequest {
            checkpoint_kind: RecoveryCheckpointKind::Foreground,
            reason_code: request.recovery_reason_code,
            action_classification: RecoveryActionClassification::SafeReplay,
            evidence_state: RecoveryEvidenceState::DurableIncomplete,
            approval_state: RecoveryApprovalState::NotRequired,
            policy_state: RecoveryPolicyState::Valid,
            recovery_budget_remaining: 1,
            clarification_available: false,
        },
        NewOperationalDiagnostic {
            operational_diagnostic_id: Uuid::now_v7(),
            trace_id: Some(request.trace_id),
            execution_id: Some(request.execution_id),
            subsystem: "recovery".to_string(),
            severity: OperationalDiagnosticSeverity::Warn,
            reason_code: diagnostic_reason_code.to_string(),
            summary: format!(
                "{reason_summary}; recovery decision: {}",
                RecoveryDecision::Continue.as_str()
            ),
            diagnostic_payload: checkpoint_payload,
        },
        now,
    )
    .await
}

pub async fn recover_background_supervisor_restart(
    pool: &PgPool,
    now: DateTime<Utc>,
    limit: u32,
) -> Result<Vec<RecoveryTriggerOutcome>> {
    let stranded_pairs =
        background::list_active_job_run_pairs_without_worker_leases(pool, limit).await?;
    let mut outcomes = Vec::with_capacity(stranded_pairs.len());

    for (background_job_id, background_job_run_id) in stranded_pairs {
        let job = background::get_job(pool, background_job_id).await?;
        let run = background::get_job_run(pool, background_job_run_id).await?;
        let evidence_state = if run.status == background::BackgroundJobRunStatus::Leased
            && run.started_at.is_none()
        {
            RecoveryEvidenceState::NotStarted
        } else {
            RecoveryEvidenceState::DurableIncomplete
        };
        let checkpoint_payload = json!({
            "source": "background_supervisor_startup",
            "background_job_id": job.background_job_id,
            "background_job_run_id": run.background_job_run_id,
            "job_status": format!("{:?}", job.status).to_lowercase(),
            "run_status": format!("{:?}", run.status).to_lowercase(),
            "lease_acquired_at": run.lease_acquired_at,
            "lease_expires_at": run.lease_expires_at,
            "started_at": run.started_at,
            "execution_id": run.execution_id,
        });
        let outcome = issue_recovery_trigger(
            pool,
            NewRecoveryCheckpoint {
                recovery_checkpoint_id: Uuid::now_v7(),
                trace_id: job.trace_id,
                execution_id: run.execution_id,
                background_job_id: Some(job.background_job_id),
                background_job_run_id: Some(run.background_job_run_id),
                governed_action_execution_id: None,
                approval_request_id: None,
                checkpoint_kind: RecoveryCheckpointKind::Background,
                recovery_reason_code: RecoveryReasonCode::SupervisorRestart,
                recovery_budget_remaining: 1,
                checkpoint_payload: checkpoint_payload.clone(),
            },
            RecoveryDecisionRequest {
                checkpoint_kind: RecoveryCheckpointKind::Background,
                reason_code: RecoveryReasonCode::SupervisorRestart,
                action_classification: RecoveryActionClassification::SafeReplay,
                evidence_state,
                approval_state: RecoveryApprovalState::NotRequired,
                policy_state: RecoveryPolicyState::Valid,
                recovery_budget_remaining: 1,
                clarification_available: false,
            },
            NewOperationalDiagnostic {
                operational_diagnostic_id: Uuid::now_v7(),
                trace_id: Some(job.trace_id),
                execution_id: run.execution_id,
                subsystem: "recovery".to_string(),
                severity: OperationalDiagnosticSeverity::Warn,
                reason_code: "background_supervisor_restart_recovered".to_string(),
                summary:
                    "background execution was stranded across supervisor restart; recovery was evaluated"
                        .to_string(),
                diagnostic_payload: checkpoint_payload,
            },
            now,
        )
        .await?;

        let failure_payload = json!({
            "kind": "supervisor_restart_recovery",
            "background_job_id": job.background_job_id,
            "background_job_run_id": run.background_job_run_id,
            "recovery_checkpoint_id": outcome.checkpoint.recovery_checkpoint_id,
            "recovery_decision": outcome.decision.decision.as_str(),
            "source": "background_supervisor_startup",
        });

        background::update_job_run_status(
            pool,
            run.background_job_run_id,
            &background::UpdateBackgroundJobRunStatus {
                status: background::BackgroundJobRunStatus::Failed,
                worker_pid: run.worker_pid,
                started_at: run.started_at,
                completed_at: Some(now),
                result_payload: None,
                failure_payload: Some(failure_payload.clone()),
            },
        )
        .await?;

        let next_job_status = if matches!(
            outcome.decision.decision,
            RecoveryDecision::Retry | RecoveryDecision::Continue
        ) {
            background::BackgroundJobStatus::Planned
        } else {
            background::BackgroundJobStatus::Failed
        };
        background::update_job_status(
            pool,
            job.background_job_id,
            &background::UpdateBackgroundJobStatus {
                status: next_job_status,
                lease_expires_at: None,
                last_started_at: job.last_started_at.or(run.started_at),
                last_completed_at: job.last_completed_at,
            },
        )
        .await?;

        if let Some(execution_id) = run.execution_id {
            execution::mark_failed(pool, execution_id, &failure_payload).await?;
        }

        audit::insert(
            pool,
            &NewAuditEvent {
                loop_kind: "unconscious".to_string(),
                subsystem: "recovery".to_string(),
                event_kind: "background_job_supervisor_restart_recovered".to_string(),
                severity: "warn".to_string(),
                trace_id: job.trace_id,
                execution_id: run.execution_id,
                worker_pid: run.worker_pid,
                payload: json!({
                    "background_job_id": job.background_job_id,
                    "background_job_run_id": run.background_job_run_id,
                    "recovery_checkpoint_id": outcome.checkpoint.recovery_checkpoint_id,
                    "recovery_decision": outcome.decision.decision.as_str(),
                    "next_job_status": format!("{:?}", next_job_status).to_lowercase(),
                }),
            },
        )
        .await?;

        outcomes.push(outcome);
    }

    Ok(outcomes)
}

async fn recover_worker_lease_timeout(
    pool: &PgPool,
    lease: WorkerLeaseRecord,
    now: DateTime<Utc>,
    source: &str,
    diagnostic_reason_code: &str,
    diagnostic_severity: OperationalDiagnosticSeverity,
    error_message: Option<&str>,
) -> Result<WorkerLeaseRecoveryOutcome> {
    if lease.worker_kind == WorkerLeaseKind::GovernedAction {
        return recover_governed_action_worker_lease_timeout(
            pool,
            lease,
            now,
            source,
            diagnostic_reason_code,
            diagnostic_severity,
            error_message,
        )
        .await;
    }

    let checkpoint = create_recovery_checkpoint(
        pool,
        &NewRecoveryCheckpoint {
            recovery_checkpoint_id: Uuid::now_v7(),
            trace_id: lease.trace_id,
            execution_id: lease.execution_id,
            background_job_id: lease.background_job_id,
            background_job_run_id: lease.background_job_run_id,
            governed_action_execution_id: lease.governed_action_execution_id,
            approval_request_id: None,
            checkpoint_kind: recovery_checkpoint_kind_for_worker_lease(lease.worker_kind),
            recovery_reason_code: RecoveryReasonCode::TimeoutOrStall,
            recovery_budget_remaining: 1,
            checkpoint_payload: json!({
                "source": source,
                "worker_lease_id": lease.worker_lease_id,
                "worker_kind": lease.worker_kind.as_str(),
                "worker_lease_status": lease.status.as_str(),
                "worker_pid": lease.worker_pid,
                "lease_acquired_at": lease.lease_acquired_at,
                "lease_expires_at": lease.lease_expires_at,
                "last_heartbeat_at": lease.last_heartbeat_at,
                "error_message": error_message,
                "metadata": lease.metadata.clone(),
            }),
        },
    )
    .await?;

    let decision = evaluate_recovery_decision(&RecoveryDecisionRequest {
        checkpoint_kind: checkpoint.checkpoint_kind,
        reason_code: checkpoint.recovery_reason_code,
        action_classification: recovery_action_classification_for_lease(&lease),
        evidence_state: RecoveryEvidenceState::DurableIncomplete,
        approval_state: RecoveryApprovalState::NotRequired,
        policy_state: RecoveryPolicyState::Valid,
        recovery_budget_remaining: checkpoint.recovery_budget_remaining,
        clarification_available: false,
    })?;
    let checkpoint = resolve_recovery_checkpoint(
        pool,
        &RecoveryCheckpointResolution {
            recovery_checkpoint_id: checkpoint.recovery_checkpoint_id,
            status: decision.checkpoint_status,
            recovery_decision: decision.decision,
            resolved_summary: Some(decision.summary.clone()),
            resolved_at: now,
        },
    )
    .await?;
    let diagnostic = insert_operational_diagnostic(
        pool,
        &NewOperationalDiagnostic {
            operational_diagnostic_id: Uuid::now_v7(),
            trace_id: Some(lease.trace_id),
            execution_id: lease.execution_id,
            subsystem: "recovery".to_string(),
            severity: diagnostic_severity,
            reason_code: diagnostic_reason_code.to_string(),
            summary: format!(
                "worker lease timeout or stall observed; recovery decision: {}",
                decision.decision.as_str()
            ),
            diagnostic_payload: json!({
                "source": source,
                "worker_lease_id": lease.worker_lease_id,
                "worker_kind": lease.worker_kind.as_str(),
                "worker_lease_status": lease.status.as_str(),
                "recovery_checkpoint_id": checkpoint.recovery_checkpoint_id,
                "recovery_decision": decision.decision.as_str(),
                "error_message": error_message,
            }),
        },
    )
    .await?;

    Ok(WorkerLeaseRecoveryOutcome {
        lease,
        checkpoint,
        decision,
        diagnostic,
    })
}

#[derive(Debug, Clone)]
struct GovernedActionRecoveryContext {
    record: governed_actions::GovernedActionExecutionRecord,
    execution_record: Option<execution::ExecutionRecord>,
    workspace_script_run: Option<workspace::WorkspaceScriptRunRecord>,
    action_classification: RecoveryActionClassification,
    evidence_state: RecoveryEvidenceState,
    approval_state: RecoveryApprovalState,
    clarification_available: bool,
}

async fn recover_governed_action_worker_lease_timeout(
    pool: &PgPool,
    lease: WorkerLeaseRecord,
    now: DateTime<Utc>,
    source: &str,
    diagnostic_reason_code: &str,
    diagnostic_severity: OperationalDiagnosticSeverity,
    error_message: Option<&str>,
) -> Result<WorkerLeaseRecoveryOutcome> {
    let governed_action_execution_id = lease
        .governed_action_execution_id
        .context("governed-action recovery requires a governed action execution id")?;
    let record =
        governed_actions::get_governed_action_execution(pool, governed_action_execution_id).await?;
    let context = load_governed_action_recovery_context(
        pool,
        record,
        RecoveryReasonCode::TimeoutOrStall,
        RecoveryPolicyState::Valid,
    )
    .await?;
    let checkpoint_payload =
        governed_action_recovery_checkpoint_payload(&context, Some(&lease), source, error_message);
    let checkpoint = create_recovery_checkpoint(
        pool,
        &NewRecoveryCheckpoint {
            recovery_checkpoint_id: Uuid::now_v7(),
            trace_id: lease.trace_id,
            execution_id: lease.execution_id,
            background_job_id: None,
            background_job_run_id: None,
            governed_action_execution_id: Some(governed_action_execution_id),
            approval_request_id: context.record.approval_request_id,
            checkpoint_kind: RecoveryCheckpointKind::GovernedAction,
            recovery_reason_code: RecoveryReasonCode::TimeoutOrStall,
            recovery_budget_remaining: 1,
            checkpoint_payload: checkpoint_payload.clone(),
        },
    )
    .await?;
    let decision = evaluate_recovery_decision(&RecoveryDecisionRequest {
        checkpoint_kind: RecoveryCheckpointKind::GovernedAction,
        reason_code: RecoveryReasonCode::TimeoutOrStall,
        action_classification: context.action_classification,
        evidence_state: context.evidence_state,
        approval_state: context.approval_state,
        policy_state: RecoveryPolicyState::Valid,
        recovery_budget_remaining: checkpoint.recovery_budget_remaining,
        clarification_available: context.clarification_available,
    })?;
    let checkpoint = resolve_recovery_checkpoint(
        pool,
        &RecoveryCheckpointResolution {
            recovery_checkpoint_id: checkpoint.recovery_checkpoint_id,
            status: decision.checkpoint_status,
            recovery_decision: decision.decision,
            resolved_summary: Some(decision.summary.clone()),
            resolved_at: now,
        },
    )
    .await?;
    let reconciled_record = reconcile_governed_action_recovery(
        pool,
        &context,
        decision.decision,
        RecoveryPolicyState::Valid,
        now,
        &decision.summary,
    )
    .await?;
    governed_actions::write_governed_action_audit_event(
        pool,
        &reconciled_record,
        "governed_action_recovery_resolved",
        "warn",
        json!({
            "worker_lease_id": lease.worker_lease_id,
            "recovery_checkpoint_id": checkpoint.recovery_checkpoint_id,
            "recovery_decision": decision.decision.as_str(),
            "recovery_reason_code": RecoveryReasonCode::TimeoutOrStall.as_str(),
            "action_classification": recovery_action_classification_label(context.action_classification),
            "evidence_state": recovery_evidence_state_label(context.evidence_state),
            "approval_state": recovery_approval_state_label(context.approval_state),
            "policy_state": recovery_policy_state_label(RecoveryPolicyState::Valid),
            "source": source,
        }),
    )
    .await?;
    let diagnostic = insert_operational_diagnostic(
        pool,
        &NewOperationalDiagnostic {
            operational_diagnostic_id: Uuid::now_v7(),
            trace_id: Some(lease.trace_id),
            execution_id: lease.execution_id,
            subsystem: "governed_actions".to_string(),
            severity: diagnostic_severity,
            reason_code: diagnostic_reason_code.to_string(),
            summary: format!(
                "governed action execution interruption observed; recovery decision: {}",
                decision.decision.as_str()
            ),
            diagnostic_payload: checkpoint_payload,
        },
    )
    .await?;

    Ok(WorkerLeaseRecoveryOutcome {
        lease,
        checkpoint,
        decision,
        diagnostic,
    })
}

async fn load_governed_action_recovery_context(
    pool: &PgPool,
    record: governed_actions::GovernedActionExecutionRecord,
    reason_code: RecoveryReasonCode,
    policy_state: RecoveryPolicyState,
) -> Result<GovernedActionRecoveryContext> {
    let execution_record = match record.execution_id {
        Some(execution_id) => Some(execution::get(pool, execution_id).await?),
        None => None,
    };
    let workspace_script_run = match record.workspace_script_id {
        Some(_) => {
            workspace::get_latest_workspace_script_run_by_governed_action_execution_id(
                pool,
                record.governed_action_execution_id,
            )
            .await?
        }
        None => None,
    };
    let action_classification = governed_action_recovery_action_classification(&record);
    let evidence_state = determine_governed_action_evidence_state(
        &record,
        execution_record.as_ref(),
        workspace_script_run.as_ref(),
    );
    let approval_state = determine_governed_action_approval_state(
        pool,
        &record,
        action_classification,
        evidence_state,
        reason_code,
        policy_state,
    )
    .await?;
    let clarification_available = action_classification
        == RecoveryActionClassification::AmbiguousOrNonrepeatable
        && matches!(approval_state, RecoveryApprovalState::NotRequired);

    Ok(GovernedActionRecoveryContext {
        record,
        execution_record,
        workspace_script_run,
        action_classification,
        evidence_state,
        approval_state,
        clarification_available,
    })
}

async fn determine_governed_action_approval_state(
    pool: &PgPool,
    record: &governed_actions::GovernedActionExecutionRecord,
    action_classification: RecoveryActionClassification,
    evidence_state: RecoveryEvidenceState,
    reason_code: RecoveryReasonCode,
    policy_state: RecoveryPolicyState,
) -> Result<RecoveryApprovalState> {
    let Some(approval_request_id) = record.approval_request_id else {
        return Ok(RecoveryApprovalState::NotRequired);
    };

    let status: Option<String> = sqlx::query_scalar(
        r#"
        SELECT status
        FROM approval_requests
        WHERE approval_request_id = $1
        "#,
    )
    .bind(approval_request_id)
    .fetch_optional(pool)
    .await
    .context("failed to load approval status for governed-action recovery")?;

    let Some(status) = status else {
        return Ok(RecoveryApprovalState::MissingRequired);
    };

    let mapped = match status.as_str() {
        "pending" => RecoveryApprovalState::Pending,
        "approved" => RecoveryApprovalState::ApprovedFresh,
        "rejected" => RecoveryApprovalState::Rejected,
        "expired" => RecoveryApprovalState::Expired,
        "invalidated" => RecoveryApprovalState::Invalidated,
        other => bail!("unrecognized approval request status '{other}'"),
    };

    if mapped == RecoveryApprovalState::ApprovedFresh
        && action_classification == RecoveryActionClassification::AmbiguousOrNonrepeatable
        && reason_code == RecoveryReasonCode::TimeoutOrStall
        && policy_state == RecoveryPolicyState::Valid
        && evidence_state != RecoveryEvidenceState::DurableCompleted
    {
        return Ok(RecoveryApprovalState::MissingRequired);
    }

    Ok(mapped)
}

fn determine_governed_action_evidence_state(
    record: &governed_actions::GovernedActionExecutionRecord,
    execution_record: Option<&execution::ExecutionRecord>,
    workspace_script_run: Option<&workspace::WorkspaceScriptRunRecord>,
) -> RecoveryEvidenceState {
    if governed_action_has_terminal_evidence(record, execution_record) {
        return RecoveryEvidenceState::DurableCompleted;
    }

    if let Some(script_run) = workspace_script_run {
        if workspace_script_run_is_terminal(script_run.status) {
            return RecoveryEvidenceState::DurableCompleted;
        }
    }

    if record.started_at.is_some() {
        if matches!(
            record.action_kind,
            GovernedActionKind::RunSubprocess | GovernedActionKind::RunWorkspaceScript
        ) {
            RecoveryEvidenceState::Ambiguous
        } else {
            RecoveryEvidenceState::DurableIncomplete
        }
    } else {
        RecoveryEvidenceState::NotStarted
    }
}

fn governed_action_has_terminal_evidence(
    record: &governed_actions::GovernedActionExecutionRecord,
    execution_record: Option<&execution::ExecutionRecord>,
) -> bool {
    if record.completed_at.is_some() {
        return true;
    }

    execution_record.is_some_and(|execution_record| {
        execution_record.status == "completed" || execution_record.status == "failed"
    })
}

fn governed_action_recovery_action_classification(
    record: &governed_actions::GovernedActionExecutionRecord,
) -> RecoveryActionClassification {
    match record.action_kind {
        GovernedActionKind::InspectWorkspaceArtifact => RecoveryActionClassification::SafeReplay,
        GovernedActionKind::RunSubprocess | GovernedActionKind::RunWorkspaceScript => {
            RecoveryActionClassification::AmbiguousOrNonrepeatable
        }
    }
}

fn workspace_script_run_is_terminal(status: WorkspaceScriptRunStatus) -> bool {
    matches!(
        status,
        WorkspaceScriptRunStatus::Completed
            | WorkspaceScriptRunStatus::Failed
            | WorkspaceScriptRunStatus::TimedOut
            | WorkspaceScriptRunStatus::Blocked
    )
}

fn governed_action_recovery_checkpoint_payload(
    context: &GovernedActionRecoveryContext,
    lease: Option<&WorkerLeaseRecord>,
    source: &str,
    error_message: Option<&str>,
) -> Value {
    json!({
        "source": source,
        "error_message": error_message,
        "governed_action_execution_id": context.record.governed_action_execution_id,
        "approval_request_id": context.record.approval_request_id,
        "action_proposal_id": context.record.action_proposal_id,
        "action_fingerprint": context.record.action_fingerprint.value.clone(),
        "action_kind": context.record.action_kind,
        "risk_tier": context.record.risk_tier,
        "governed_action_status": context.record.status,
        "output_ref": context.record.output_ref.clone(),
        "started_at": context.record.started_at,
        "completed_at": context.record.completed_at,
        "recovery_action_classification": recovery_action_classification_label(context.action_classification),
        "recovery_evidence_state": recovery_evidence_state_label(context.evidence_state),
        "recovery_approval_state": recovery_approval_state_label(context.approval_state),
        "execution_record": context.execution_record.as_ref().map(|record| json!({
            "execution_id": record.execution_id,
            "status": record.status.clone(),
            "completed_at": record.completed_at,
            "response_payload": record.response_payload.clone(),
        })),
        "workspace_script_run": context.workspace_script_run.as_ref().map(|run| json!({
            "workspace_script_run_id": run.workspace_script_run_id,
            "status": run.status,
            "output_ref": run.output_ref.clone(),
            "failure_summary": run.failure_summary.clone(),
            "started_at": run.started_at,
            "completed_at": run.completed_at,
        })),
        "worker_lease": lease.map(|lease| json!({
            "worker_lease_id": lease.worker_lease_id,
            "worker_kind": lease.worker_kind.as_str(),
            "worker_lease_status": lease.status.as_str(),
            "worker_pid": lease.worker_pid,
            "lease_acquired_at": lease.lease_acquired_at,
            "lease_expires_at": lease.lease_expires_at,
            "last_heartbeat_at": lease.last_heartbeat_at,
            "metadata": lease.metadata.clone(),
        })),
    })
}

async fn reconcile_governed_action_recovery(
    pool: &PgPool,
    context: &GovernedActionRecoveryContext,
    decision: RecoveryDecision,
    policy_state: RecoveryPolicyState,
    now: DateTime<Utc>,
    summary: &str,
) -> Result<governed_actions::GovernedActionExecutionRecord> {
    if decision == RecoveryDecision::Continue {
        if let Some((status, blocked_reason, completed_at)) =
            governed_action_terminal_state(context, now)
        {
            if let Some(script_run) = &context.workspace_script_run {
                if !workspace_script_run_is_terminal(script_run.status) {
                    if let Some(run_status) =
                        workspace_script_run_status_from_governed_action_status(
                            status,
                            blocked_reason.is_some(),
                        )
                    {
                        workspace::update_workspace_script_run_status(
                            pool,
                            &workspace::UpdateWorkspaceScriptRunStatus {
                                workspace_script_run_id: script_run.workspace_script_run_id,
                                status: run_status,
                                output_ref: context
                                    .record
                                    .execution_id
                                    .map(|execution_id| format!("execution_record:{execution_id}"))
                                    .or_else(|| script_run.output_ref.clone()),
                                failure_summary: blocked_reason.clone(),
                                started_at: script_run.started_at.or(context.record.started_at),
                                completed_at: Some(completed_at),
                            },
                        )
                        .await?;
                    }
                }
            }

            let output_ref = context
                .record
                .execution_id
                .map(|execution_id| format!("execution_record:{execution_id}"));
            return governed_actions::update_governed_action_execution(
                pool,
                governed_actions::GovernedActionExecutionUpdate {
                    governed_action_execution_id: context.record.governed_action_execution_id,
                    status,
                    execution_id: context.record.execution_id,
                    output_ref: output_ref.as_deref(),
                    blocked_reason: blocked_reason.as_deref(),
                    approval_request_id: context.record.approval_request_id,
                    started_at: context.record.started_at,
                    completed_at: Some(completed_at),
                },
            )
            .await;
        }

        return Ok(context.record.clone());
    }

    let status = match decision {
        RecoveryDecision::Reapprove => GovernedActionStatus::Invalidated,
        RecoveryDecision::Clarify | RecoveryDecision::Abandon => {
            if policy_state == RecoveryPolicyState::RecheckFailed {
                GovernedActionStatus::Blocked
            } else {
                GovernedActionStatus::Failed
            }
        }
        RecoveryDecision::Retry => {
            if context.record.approval_request_id.is_some() {
                GovernedActionStatus::Approved
            } else {
                GovernedActionStatus::Proposed
            }
        }
        RecoveryDecision::Defer => GovernedActionStatus::AwaitingApproval,
        RecoveryDecision::Continue => unreachable!("continue branch returned earlier"),
    };
    let blocked_reason = match decision {
        RecoveryDecision::Retry => None,
        _ => Some(summary),
    };
    let output_ref = context
        .record
        .execution_id
        .map(|execution_id| format!("execution_record:{execution_id}"));
    governed_actions::update_governed_action_execution(
        pool,
        governed_actions::GovernedActionExecutionUpdate {
            governed_action_execution_id: context.record.governed_action_execution_id,
            status,
            execution_id: context.record.execution_id,
            output_ref: output_ref.as_deref(),
            blocked_reason,
            approval_request_id: context.record.approval_request_id,
            started_at: context.record.started_at,
            completed_at: Some(now),
        },
    )
    .await
}

fn governed_action_terminal_state(
    context: &GovernedActionRecoveryContext,
    now: DateTime<Utc>,
) -> Option<(GovernedActionStatus, Option<String>, DateTime<Utc>)> {
    if let Some(execution_record) = &context.execution_record {
        if execution_record.status == "completed" {
            return Some((
                GovernedActionStatus::Executed,
                None,
                execution_record.completed_at.unwrap_or(now),
            ));
        }
        if execution_record.status == "failed" {
            let payload = execution_record.response_payload.as_ref();
            let summary = payload
                .and_then(|payload| payload.get("summary"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            let status = match payload
                .and_then(|payload| payload.get("status"))
                .and_then(Value::as_str)
            {
                Some("blocked") => GovernedActionStatus::Blocked,
                Some("completed") => GovernedActionStatus::Executed,
                Some("failed") | Some("timed_out") => GovernedActionStatus::Failed,
                _ => GovernedActionStatus::Failed,
            };
            return Some((
                status,
                summary,
                execution_record.completed_at.unwrap_or(now),
            ));
        }
    }

    if let Some(script_run) = &context.workspace_script_run {
        let status = match script_run.status {
            WorkspaceScriptRunStatus::Completed => GovernedActionStatus::Executed,
            WorkspaceScriptRunStatus::Blocked => GovernedActionStatus::Blocked,
            WorkspaceScriptRunStatus::Failed | WorkspaceScriptRunStatus::TimedOut => {
                GovernedActionStatus::Failed
            }
            WorkspaceScriptRunStatus::Pending | WorkspaceScriptRunStatus::Running => {
                return None;
            }
        };
        return Some((
            status,
            script_run.failure_summary.clone(),
            script_run.completed_at.unwrap_or(now),
        ));
    }

    context.record.completed_at.map(|completed_at| {
        (
            context.record.status,
            context.record.blocked_reason.clone(),
            completed_at,
        )
    })
}

fn workspace_script_run_status_from_governed_action_status(
    status: GovernedActionStatus,
    has_failure_summary: bool,
) -> Option<WorkspaceScriptRunStatus> {
    match status {
        GovernedActionStatus::Executed => Some(WorkspaceScriptRunStatus::Completed),
        GovernedActionStatus::Blocked => Some(WorkspaceScriptRunStatus::Blocked),
        GovernedActionStatus::Failed => {
            if has_failure_summary {
                Some(WorkspaceScriptRunStatus::Failed)
            } else {
                Some(WorkspaceScriptRunStatus::TimedOut)
            }
        }
        GovernedActionStatus::Proposed
        | GovernedActionStatus::AwaitingApproval
        | GovernedActionStatus::Approved
        | GovernedActionStatus::Rejected
        | GovernedActionStatus::Expired
        | GovernedActionStatus::Invalidated => None,
    }
}

async fn issue_recovery_trigger(
    pool: &PgPool,
    checkpoint: NewRecoveryCheckpoint,
    request: RecoveryDecisionRequest,
    diagnostic: NewOperationalDiagnostic,
    now: DateTime<Utc>,
) -> Result<RecoveryTriggerOutcome> {
    let checkpoint = create_recovery_checkpoint(pool, &checkpoint).await?;
    let decision = evaluate_recovery_decision(&request)?;
    let checkpoint = resolve_recovery_checkpoint(
        pool,
        &RecoveryCheckpointResolution {
            recovery_checkpoint_id: checkpoint.recovery_checkpoint_id,
            status: decision.checkpoint_status,
            recovery_decision: decision.decision,
            resolved_summary: Some(decision.summary.clone()),
            resolved_at: now,
        },
    )
    .await?;
    let diagnostic = insert_operational_diagnostic(pool, &diagnostic).await?;
    Ok(RecoveryTriggerOutcome {
        checkpoint,
        decision,
        diagnostic,
    })
}

pub async fn supervise_worker_leases(
    pool: &PgPool,
    now: DateTime<Utc>,
    soft_warning_threshold_percent: u8,
) -> Result<WorkerLeaseSupervisionSummary> {
    if !(1..=100).contains(&soft_warning_threshold_percent) {
        bail!("soft warning threshold percent must be between 1 and 100");
    }

    let recovered_expired_leases = recover_expired_worker_leases(pool, now).await?;
    let mut soft_warning_diagnostics = Vec::new();
    for lease in list_active_worker_leases(pool, 100).await? {
        let decision =
            classify_worker_lease_supervision(&lease, now, soft_warning_threshold_percent)?;
        if decision != WorkerLeaseSupervisionDecision::SoftWarning {
            continue;
        }
        if diagnostic_exists_for_worker_lease(
            pool,
            "worker_lease_soft_warning",
            lease.worker_lease_id,
        )
        .await?
        {
            continue;
        }
        soft_warning_diagnostics.push(
            insert_operational_diagnostic(
                pool,
                &NewOperationalDiagnostic {
                    operational_diagnostic_id: Uuid::now_v7(),
                    trace_id: Some(lease.trace_id),
                    execution_id: lease.execution_id,
                    subsystem: "recovery".to_string(),
                    severity: OperationalDiagnosticSeverity::Warn,
                    reason_code: "worker_lease_soft_warning".to_string(),
                    summary: "worker lease is nearing expiry without completion".to_string(),
                    diagnostic_payload: json!({
                        "worker_lease_id": lease.worker_lease_id,
                        "worker_kind": lease.worker_kind.as_str(),
                        "lease_acquired_at": lease.lease_acquired_at,
                        "lease_expires_at": lease.lease_expires_at,
                        "last_heartbeat_at": lease.last_heartbeat_at,
                        "soft_warning_threshold_percent": soft_warning_threshold_percent,
                    }),
                },
            )
            .await?,
        );
    }

    Ok(WorkerLeaseSupervisionSummary {
        soft_warning_diagnostics,
        recovered_expired_leases,
    })
}

fn recovery_checkpoint_kind_for_worker_lease(kind: WorkerLeaseKind) -> RecoveryCheckpointKind {
    match kind {
        WorkerLeaseKind::Foreground => RecoveryCheckpointKind::Foreground,
        WorkerLeaseKind::Background => RecoveryCheckpointKind::Background,
        WorkerLeaseKind::GovernedAction => RecoveryCheckpointKind::GovernedAction,
    }
}

fn recovery_action_classification_for_lease(
    lease: &WorkerLeaseRecord,
) -> RecoveryActionClassification {
    match lease.worker_kind {
        WorkerLeaseKind::Foreground | WorkerLeaseKind::Background => {
            RecoveryActionClassification::SafeReplay
        }
        WorkerLeaseKind::GovernedAction => RecoveryActionClassification::AmbiguousOrNonrepeatable,
    }
}

fn recovery_approval_state_label(state: RecoveryApprovalState) -> &'static str {
    match state {
        RecoveryApprovalState::NotRequired => "not_required",
        RecoveryApprovalState::Pending => "pending",
        RecoveryApprovalState::ApprovedFresh => "approved_fresh",
        RecoveryApprovalState::Expired => "expired",
        RecoveryApprovalState::Invalidated => "invalidated",
        RecoveryApprovalState::Rejected => "rejected",
        RecoveryApprovalState::MissingRequired => "missing_required",
    }
}

fn recovery_action_classification_label(
    classification: RecoveryActionClassification,
) -> &'static str {
    match classification {
        RecoveryActionClassification::SafeReplay => "safe_replay",
        RecoveryActionClassification::ProvablyIdempotentExternal => "provably_idempotent_external",
        RecoveryActionClassification::AmbiguousOrNonrepeatable => "ambiguous_or_nonrepeatable",
    }
}

fn recovery_evidence_state_label(state: RecoveryEvidenceState) -> &'static str {
    match state {
        RecoveryEvidenceState::NotStarted => "not_started",
        RecoveryEvidenceState::DurableIncomplete => "durable_incomplete",
        RecoveryEvidenceState::DurableCompleted => "durable_completed",
        RecoveryEvidenceState::Ambiguous => "ambiguous",
        RecoveryEvidenceState::Corrupted => "corrupted",
    }
}

fn recovery_policy_state_label(state: RecoveryPolicyState) -> &'static str {
    match state {
        RecoveryPolicyState::Valid => "valid",
        RecoveryPolicyState::RequiresRecheck => "requires_recheck",
        RecoveryPolicyState::RecheckFailed => "recheck_failed",
    }
}

#[derive(Debug, Clone)]
pub struct NewRecoveryCheckpoint {
    pub recovery_checkpoint_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub background_job_id: Option<Uuid>,
    pub background_job_run_id: Option<Uuid>,
    pub governed_action_execution_id: Option<Uuid>,
    pub approval_request_id: Option<Uuid>,
    pub checkpoint_kind: RecoveryCheckpointKind,
    pub recovery_reason_code: RecoveryReasonCode,
    pub recovery_budget_remaining: i32,
    pub checkpoint_payload: Value,
}

#[derive(Debug, Clone)]
pub struct RecoveryCheckpointRecord {
    pub recovery_checkpoint_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub background_job_id: Option<Uuid>,
    pub background_job_run_id: Option<Uuid>,
    pub governed_action_execution_id: Option<Uuid>,
    pub approval_request_id: Option<Uuid>,
    pub checkpoint_kind: RecoveryCheckpointKind,
    pub recovery_reason_code: RecoveryReasonCode,
    pub status: RecoveryCheckpointStatus,
    pub recovery_decision: Option<RecoveryDecision>,
    pub recovery_budget_remaining: i32,
    pub checkpoint_payload: Value,
    pub resolved_summary: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct RecoveryCheckpointResolution {
    pub recovery_checkpoint_id: Uuid,
    pub status: RecoveryCheckpointStatus,
    pub recovery_decision: RecoveryDecision,
    pub resolved_summary: Option<String>,
    pub resolved_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerLeaseKind {
    Foreground,
    Background,
    GovernedAction,
}

impl WorkerLeaseKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Foreground => "foreground",
            Self::Background => "background",
            Self::GovernedAction => "governed_action",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "foreground" => Ok(Self::Foreground),
            "background" => Ok(Self::Background),
            "governed_action" => Ok(Self::GovernedAction),
            other => bail!("unrecognized worker lease kind '{other}'"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerLeaseStatus {
    Active,
    Released,
    Expired,
    Terminated,
}

impl WorkerLeaseStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Released => "released",
            Self::Expired => "expired",
            Self::Terminated => "terminated",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "active" => Ok(Self::Active),
            "released" => Ok(Self::Released),
            "expired" => Ok(Self::Expired),
            "terminated" => Ok(Self::Terminated),
            other => bail!("unrecognized worker lease status '{other}'"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NewWorkerLease {
    pub worker_lease_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub background_job_id: Option<Uuid>,
    pub background_job_run_id: Option<Uuid>,
    pub governed_action_execution_id: Option<Uuid>,
    pub worker_kind: WorkerLeaseKind,
    pub lease_token: Uuid,
    pub worker_pid: Option<i32>,
    pub lease_acquired_at: DateTime<Utc>,
    pub lease_expires_at: DateTime<Utc>,
    pub last_heartbeat_at: DateTime<Utc>,
    pub metadata: Value,
}

#[derive(Debug, Clone)]
pub struct WorkerLeaseRecord {
    pub worker_lease_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub background_job_id: Option<Uuid>,
    pub background_job_run_id: Option<Uuid>,
    pub governed_action_execution_id: Option<Uuid>,
    pub worker_kind: WorkerLeaseKind,
    pub status: WorkerLeaseStatus,
    pub lease_token: Uuid,
    pub worker_pid: Option<i32>,
    pub lease_acquired_at: DateTime<Utc>,
    pub lease_expires_at: DateTime<Utc>,
    pub last_heartbeat_at: DateTime<Utc>,
    pub released_at: Option<DateTime<Utc>>,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationalDiagnosticSeverity {
    Info,
    Warn,
    Error,
    Critical,
}

impl OperationalDiagnosticSeverity {
    fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
            Self::Critical => "critical",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "info" => Ok(Self::Info),
            "warn" => Ok(Self::Warn),
            "error" => Ok(Self::Error),
            "critical" => Ok(Self::Critical),
            other => bail!("unrecognized operational diagnostic severity '{other}'"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NewOperationalDiagnostic {
    pub operational_diagnostic_id: Uuid,
    pub trace_id: Option<Uuid>,
    pub execution_id: Option<Uuid>,
    pub subsystem: String,
    pub severity: OperationalDiagnosticSeverity,
    pub reason_code: String,
    pub summary: String,
    pub diagnostic_payload: Value,
}

#[derive(Debug, Clone)]
pub struct OperationalDiagnosticRecord {
    pub operational_diagnostic_id: Uuid,
    pub trace_id: Option<Uuid>,
    pub execution_id: Option<Uuid>,
    pub subsystem: String,
    pub severity: OperationalDiagnosticSeverity,
    pub reason_code: String,
    pub summary: String,
    pub diagnostic_payload: Value,
    pub created_at: DateTime<Utc>,
}

pub async fn create_recovery_checkpoint(
    pool: &PgPool,
    checkpoint: &NewRecoveryCheckpoint,
) -> Result<RecoveryCheckpointRecord> {
    validate_new_recovery_checkpoint(checkpoint)?;

    sqlx::query(
        r#"
        INSERT INTO recovery_checkpoints (
            recovery_checkpoint_id,
            trace_id,
            execution_id,
            background_job_id,
            background_job_run_id,
            governed_action_execution_id,
            approval_request_id,
            checkpoint_kind,
            recovery_reason_code,
            status,
            recovery_decision,
            recovery_budget_remaining,
            checkpoint_payload_json,
            resolved_summary,
            created_at,
            updated_at,
            resolved_at
        ) VALUES (
            $1,
            $2,
            $3,
            $4,
            $5,
            $6,
            $7,
            $8,
            $9,
            'open',
            NULL,
            $10,
            $11,
            NULL,
            NOW(),
            NOW(),
            NULL
        )
        "#,
    )
    .bind(checkpoint.recovery_checkpoint_id)
    .bind(checkpoint.trace_id)
    .bind(checkpoint.execution_id)
    .bind(checkpoint.background_job_id)
    .bind(checkpoint.background_job_run_id)
    .bind(checkpoint.governed_action_execution_id)
    .bind(checkpoint.approval_request_id)
    .bind(checkpoint.checkpoint_kind.as_str())
    .bind(checkpoint.recovery_reason_code.as_str())
    .bind(checkpoint.recovery_budget_remaining)
    .bind(&checkpoint.checkpoint_payload)
    .execute(pool)
    .await
    .context("failed to insert recovery checkpoint")?;

    get_recovery_checkpoint(pool, checkpoint.recovery_checkpoint_id).await
}

pub async fn get_recovery_checkpoint(
    pool: &PgPool,
    recovery_checkpoint_id: Uuid,
) -> Result<RecoveryCheckpointRecord> {
    let row = sqlx::query(
        r#"
        SELECT
            recovery_checkpoint_id,
            trace_id,
            execution_id,
            background_job_id,
            background_job_run_id,
            governed_action_execution_id,
            approval_request_id,
            checkpoint_kind,
            recovery_reason_code,
            status,
            recovery_decision,
            recovery_budget_remaining,
            checkpoint_payload_json,
            resolved_summary,
            created_at,
            updated_at,
            resolved_at
        FROM recovery_checkpoints
        WHERE recovery_checkpoint_id = $1
        "#,
    )
    .bind(recovery_checkpoint_id)
    .fetch_one(pool)
    .await
    .context("failed to fetch recovery checkpoint")?;

    recovery_checkpoint_from_row(&row)
}

pub async fn list_open_recovery_checkpoints(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<RecoveryCheckpointRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT
            recovery_checkpoint_id,
            trace_id,
            execution_id,
            background_job_id,
            background_job_run_id,
            governed_action_execution_id,
            approval_request_id,
            checkpoint_kind,
            recovery_reason_code,
            status,
            recovery_decision,
            recovery_budget_remaining,
            checkpoint_payload_json,
            resolved_summary,
            created_at,
            updated_at,
            resolved_at
        FROM recovery_checkpoints
        WHERE status = 'open'
        ORDER BY created_at ASC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("failed to list open recovery checkpoints")?;

    rows.iter().map(recovery_checkpoint_from_row).collect()
}

pub async fn resolve_recovery_checkpoint(
    pool: &PgPool,
    resolution: &RecoveryCheckpointResolution,
) -> Result<RecoveryCheckpointRecord> {
    if resolution.status == RecoveryCheckpointStatus::Open {
        bail!("resolved recovery checkpoint status must not be open");
    }

    sqlx::query(
        r#"
        UPDATE recovery_checkpoints
        SET
            status = $2,
            recovery_decision = $3,
            resolved_summary = $4,
            resolved_at = $5,
            updated_at = NOW()
        WHERE recovery_checkpoint_id = $1
        "#,
    )
    .bind(resolution.recovery_checkpoint_id)
    .bind(resolution.status.as_str())
    .bind(resolution.recovery_decision.as_str())
    .bind(&resolution.resolved_summary)
    .bind(resolution.resolved_at)
    .execute(pool)
    .await
    .context("failed to resolve recovery checkpoint")?;

    get_recovery_checkpoint(pool, resolution.recovery_checkpoint_id).await
}

pub async fn create_worker_lease(
    pool: &PgPool,
    lease: &NewWorkerLease,
) -> Result<WorkerLeaseRecord> {
    validate_new_worker_lease(lease)?;

    sqlx::query(
        r#"
        INSERT INTO worker_leases (
            worker_lease_id,
            trace_id,
            execution_id,
            background_job_id,
            background_job_run_id,
            governed_action_execution_id,
            worker_kind,
            status,
            lease_token,
            worker_pid,
            lease_acquired_at,
            lease_expires_at,
            last_heartbeat_at,
            released_at,
            metadata_json,
            created_at,
            updated_at
        ) VALUES (
            $1,
            $2,
            $3,
            $4,
            $5,
            $6,
            $7,
            'active',
            $8,
            $9,
            $10,
            $11,
            $12,
            NULL,
            $13,
            NOW(),
            NOW()
        )
        "#,
    )
    .bind(lease.worker_lease_id)
    .bind(lease.trace_id)
    .bind(lease.execution_id)
    .bind(lease.background_job_id)
    .bind(lease.background_job_run_id)
    .bind(lease.governed_action_execution_id)
    .bind(lease.worker_kind.as_str())
    .bind(lease.lease_token)
    .bind(lease.worker_pid)
    .bind(lease.lease_acquired_at)
    .bind(lease.lease_expires_at)
    .bind(lease.last_heartbeat_at)
    .bind(&lease.metadata)
    .execute(pool)
    .await
    .context("failed to insert worker lease")?;

    get_worker_lease(pool, lease.worker_lease_id).await
}

pub async fn get_worker_lease(pool: &PgPool, worker_lease_id: Uuid) -> Result<WorkerLeaseRecord> {
    let row = sqlx::query(
        r#"
        SELECT
            worker_lease_id,
            trace_id,
            execution_id,
            background_job_id,
            background_job_run_id,
            governed_action_execution_id,
            worker_kind,
            status,
            lease_token,
            worker_pid,
            lease_acquired_at,
            lease_expires_at,
            last_heartbeat_at,
            released_at,
            metadata_json,
            created_at,
            updated_at
        FROM worker_leases
        WHERE worker_lease_id = $1
        "#,
    )
    .bind(worker_lease_id)
    .fetch_one(pool)
    .await
    .context("failed to fetch worker lease")?;

    worker_lease_from_row(&row)
}

pub async fn list_active_worker_leases(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<WorkerLeaseRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT
            worker_lease_id,
            trace_id,
            execution_id,
            background_job_id,
            background_job_run_id,
            governed_action_execution_id,
            worker_kind,
            status,
            lease_token,
            worker_pid,
            lease_acquired_at,
            lease_expires_at,
            last_heartbeat_at,
            released_at,
            metadata_json,
            created_at,
            updated_at
        FROM worker_leases
        WHERE status = 'active'
        ORDER BY lease_expires_at ASC, created_at ASC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("failed to list active worker leases")?;

    rows.iter().map(worker_lease_from_row).collect()
}

pub async fn refresh_worker_lease(
    pool: &PgPool,
    worker_lease_id: Uuid,
    heartbeat_at: DateTime<Utc>,
    lease_expires_at: DateTime<Utc>,
) -> Result<WorkerLeaseRecord> {
    if lease_expires_at < heartbeat_at {
        bail!("worker lease expiry must not be earlier than the heartbeat");
    }

    sqlx::query(
        r#"
        UPDATE worker_leases
        SET
            last_heartbeat_at = $2,
            lease_expires_at = $3,
            updated_at = NOW()
        WHERE worker_lease_id = $1
          AND status = 'active'
        "#,
    )
    .bind(worker_lease_id)
    .bind(heartbeat_at)
    .bind(lease_expires_at)
    .execute(pool)
    .await
    .context("failed to refresh worker lease")?;

    get_worker_lease(pool, worker_lease_id).await
}

pub async fn refresh_worker_lease_progress(
    pool: &PgPool,
    worker_lease_id: Uuid,
    heartbeat_at: DateTime<Utc>,
) -> Result<WorkerLeaseRecord> {
    let lease = get_worker_lease(pool, worker_lease_id).await?;
    if lease.status != WorkerLeaseStatus::Active {
        bail!(
            "worker lease {} cannot be refreshed because status is {}",
            worker_lease_id,
            lease.status.as_str()
        );
    }
    let lease_duration = lease.lease_expires_at - lease.lease_acquired_at;
    if lease_duration <= Duration::zero() {
        bail!("worker lease duration must be positive for progress refresh");
    }
    refresh_worker_lease(
        pool,
        worker_lease_id,
        heartbeat_at,
        heartbeat_at + lease_duration,
    )
    .await
}

pub async fn release_worker_lease(
    pool: &PgPool,
    worker_lease_id: Uuid,
    released_at: DateTime<Utc>,
) -> Result<WorkerLeaseRecord> {
    sqlx::query(
        r#"
        UPDATE worker_leases
        SET
            status = $3,
            released_at = $2,
            updated_at = NOW()
        WHERE worker_lease_id = $1
          AND status = 'active'
        "#,
    )
    .bind(worker_lease_id)
    .bind(released_at)
    .bind(WorkerLeaseStatus::Released.as_str())
    .execute(pool)
    .await
    .context("failed to release worker lease")?;

    get_worker_lease(pool, worker_lease_id).await
}

pub async fn expire_due_worker_leases(
    pool: &PgPool,
    now: DateTime<Utc>,
) -> Result<Vec<WorkerLeaseRecord>> {
    let rows = sqlx::query(
        r#"
        UPDATE worker_leases
        SET
            status = $2,
            released_at = $1,
            updated_at = NOW()
        WHERE status = 'active'
          AND lease_expires_at <= $1
        RETURNING
            worker_lease_id,
            trace_id,
            execution_id,
            background_job_id,
            background_job_run_id,
            governed_action_execution_id,
            worker_kind,
            status,
            lease_token,
            worker_pid,
            lease_acquired_at,
            lease_expires_at,
            last_heartbeat_at,
            released_at,
            metadata_json,
            created_at,
            updated_at
        "#,
    )
    .bind(now)
    .bind(WorkerLeaseStatus::Expired.as_str())
    .fetch_all(pool)
    .await
    .context("failed to expire due worker leases")?;

    rows.iter().map(worker_lease_from_row).collect()
}

async fn terminate_active_worker_lease(
    pool: &PgPool,
    worker_lease_id: Uuid,
    terminated_at: DateTime<Utc>,
) -> Result<WorkerLeaseRecord> {
    let row = sqlx::query(
        r#"
        UPDATE worker_leases
        SET
            status = $2,
            released_at = $3,
            updated_at = NOW()
        WHERE worker_lease_id = $1
          AND status = 'active'
        RETURNING
            worker_lease_id,
            trace_id,
            execution_id,
            background_job_id,
            background_job_run_id,
            governed_action_execution_id,
            worker_kind,
            status,
            lease_token,
            worker_pid,
            lease_acquired_at,
            lease_expires_at,
            last_heartbeat_at,
            released_at,
            metadata_json,
            created_at,
            updated_at
        "#,
    )
    .bind(worker_lease_id)
    .bind(WorkerLeaseStatus::Terminated.as_str())
    .bind(terminated_at)
    .fetch_optional(pool)
    .await
    .context("failed to terminate active worker lease")?;

    match row {
        Some(row) => worker_lease_from_row(&row),
        None => {
            let lease = get_worker_lease(pool, worker_lease_id).await?;
            bail!(
                "worker lease {} cannot be recovered from observed timeout because status is {}",
                worker_lease_id,
                lease.status.as_str()
            )
        }
    }
}

pub async fn insert_operational_diagnostic(
    pool: &PgPool,
    diagnostic: &NewOperationalDiagnostic,
) -> Result<OperationalDiagnosticRecord> {
    validate_new_operational_diagnostic(diagnostic)?;

    sqlx::query(
        r#"
        INSERT INTO operational_diagnostics (
            operational_diagnostic_id,
            trace_id,
            execution_id,
            subsystem,
            severity,
            reason_code,
            summary,
            diagnostic_payload_json,
            created_at
        ) VALUES (
            $1,
            $2,
            $3,
            $4,
            $5,
            $6,
            $7,
            $8,
            NOW()
        )
        "#,
    )
    .bind(diagnostic.operational_diagnostic_id)
    .bind(diagnostic.trace_id)
    .bind(diagnostic.execution_id)
    .bind(&diagnostic.subsystem)
    .bind(diagnostic.severity.as_str())
    .bind(&diagnostic.reason_code)
    .bind(&diagnostic.summary)
    .bind(&diagnostic.diagnostic_payload)
    .execute(pool)
    .await
    .context("failed to insert operational diagnostic")?;

    get_operational_diagnostic(pool, diagnostic.operational_diagnostic_id).await
}

async fn diagnostic_exists_for_worker_lease(
    pool: &PgPool,
    reason_code: &str,
    worker_lease_id: Uuid,
) -> Result<bool> {
    let exists: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM operational_diagnostics
            WHERE reason_code = $1
              AND diagnostic_payload_json ->> 'worker_lease_id' = $2
        )
        "#,
    )
    .bind(reason_code)
    .bind(worker_lease_id.to_string())
    .fetch_one(pool)
    .await
    .context("failed to check worker lease diagnostic existence")?;
    Ok(exists)
}

pub async fn get_operational_diagnostic(
    pool: &PgPool,
    operational_diagnostic_id: Uuid,
) -> Result<OperationalDiagnosticRecord> {
    let row = sqlx::query(
        r#"
        SELECT
            operational_diagnostic_id,
            trace_id,
            execution_id,
            subsystem,
            severity,
            reason_code,
            summary,
            diagnostic_payload_json,
            created_at
        FROM operational_diagnostics
        WHERE operational_diagnostic_id = $1
        "#,
    )
    .bind(operational_diagnostic_id)
    .fetch_one(pool)
    .await
    .context("failed to fetch operational diagnostic")?;

    operational_diagnostic_from_row(&row)
}

pub async fn list_operational_diagnostics(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<OperationalDiagnosticRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT
            operational_diagnostic_id,
            trace_id,
            execution_id,
            subsystem,
            severity,
            reason_code,
            summary,
            diagnostic_payload_json,
            created_at
        FROM operational_diagnostics
        ORDER BY created_at DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("failed to list operational diagnostics")?;

    rows.iter().map(operational_diagnostic_from_row).collect()
}

fn validate_new_recovery_checkpoint(checkpoint: &NewRecoveryCheckpoint) -> Result<()> {
    if checkpoint.recovery_budget_remaining < 0 {
        bail!("recovery budget remaining must not be negative");
    }
    Ok(())
}

fn validate_new_worker_lease(lease: &NewWorkerLease) -> Result<()> {
    if lease.lease_expires_at < lease.lease_acquired_at {
        bail!("worker lease expiry must not be earlier than acquisition");
    }
    if lease.last_heartbeat_at < lease.lease_acquired_at {
        bail!("worker lease heartbeat must not be earlier than acquisition");
    }
    Ok(())
}

fn validate_new_operational_diagnostic(diagnostic: &NewOperationalDiagnostic) -> Result<()> {
    if diagnostic.subsystem.trim().is_empty() {
        bail!("operational diagnostic subsystem must not be empty");
    }
    if diagnostic.reason_code.trim().is_empty() {
        bail!("operational diagnostic reason code must not be empty");
    }
    if diagnostic.summary.trim().is_empty() {
        bail!("operational diagnostic summary must not be empty");
    }
    Ok(())
}

fn recovery_checkpoint_from_row(row: &sqlx::postgres::PgRow) -> Result<RecoveryCheckpointRecord> {
    let checkpoint_kind = RecoveryCheckpointKind::parse(row.get("checkpoint_kind"))?;
    let recovery_reason_code = RecoveryReasonCode::parse(row.get("recovery_reason_code"))?;
    let status = RecoveryCheckpointStatus::parse(row.get("status"))?;
    let recovery_decision = row
        .get::<Option<String>, _>("recovery_decision")
        .as_deref()
        .map(RecoveryDecision::parse)
        .transpose()?;

    Ok(RecoveryCheckpointRecord {
        recovery_checkpoint_id: row.get("recovery_checkpoint_id"),
        trace_id: row.get("trace_id"),
        execution_id: row.get("execution_id"),
        background_job_id: row.get("background_job_id"),
        background_job_run_id: row.get("background_job_run_id"),
        governed_action_execution_id: row.get("governed_action_execution_id"),
        approval_request_id: row.get("approval_request_id"),
        checkpoint_kind,
        recovery_reason_code,
        status,
        recovery_decision,
        recovery_budget_remaining: row.get("recovery_budget_remaining"),
        checkpoint_payload: row.get("checkpoint_payload_json"),
        resolved_summary: row.get("resolved_summary"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        resolved_at: row.get("resolved_at"),
    })
}

fn worker_lease_from_row(row: &sqlx::postgres::PgRow) -> Result<WorkerLeaseRecord> {
    Ok(WorkerLeaseRecord {
        worker_lease_id: row.get("worker_lease_id"),
        trace_id: row.get("trace_id"),
        execution_id: row.get("execution_id"),
        background_job_id: row.get("background_job_id"),
        background_job_run_id: row.get("background_job_run_id"),
        governed_action_execution_id: row.get("governed_action_execution_id"),
        worker_kind: WorkerLeaseKind::parse(row.get("worker_kind"))?,
        status: WorkerLeaseStatus::parse(row.get("status"))?,
        lease_token: row.get("lease_token"),
        worker_pid: row.get("worker_pid"),
        lease_acquired_at: row.get("lease_acquired_at"),
        lease_expires_at: row.get("lease_expires_at"),
        last_heartbeat_at: row.get("last_heartbeat_at"),
        released_at: row.get("released_at"),
        metadata: row.get("metadata_json"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

fn operational_diagnostic_from_row(
    row: &sqlx::postgres::PgRow,
) -> Result<OperationalDiagnosticRecord> {
    Ok(OperationalDiagnosticRecord {
        operational_diagnostic_id: row.get("operational_diagnostic_id"),
        trace_id: row.get("trace_id"),
        execution_id: row.get("execution_id"),
        subsystem: row.get("subsystem"),
        severity: OperationalDiagnosticSeverity::parse(row.get("severity"))?,
        reason_code: row.get("reason_code"),
        summary: row.get("summary"),
        diagnostic_payload: row.get("diagnostic_payload_json"),
        created_at: row.get("created_at"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use serde_json::json;

    #[test]
    fn validate_new_recovery_checkpoint_rejects_negative_budget() {
        let checkpoint = NewRecoveryCheckpoint {
            recovery_checkpoint_id: Uuid::now_v7(),
            trace_id: Uuid::now_v7(),
            execution_id: None,
            background_job_id: None,
            background_job_run_id: None,
            governed_action_execution_id: None,
            approval_request_id: None,
            checkpoint_kind: RecoveryCheckpointKind::Foreground,
            recovery_reason_code: RecoveryReasonCode::Crash,
            recovery_budget_remaining: -1,
            checkpoint_payload: json!({}),
        };

        let error = validate_new_recovery_checkpoint(&checkpoint)
            .expect_err("negative recovery budget should be rejected");
        assert!(error.to_string().contains("recovery budget"));
    }

    #[test]
    fn validate_new_worker_lease_rejects_expiry_before_acquisition() {
        let acquired_at = Utc::now();
        let lease = NewWorkerLease {
            worker_lease_id: Uuid::now_v7(),
            trace_id: Uuid::now_v7(),
            execution_id: None,
            background_job_id: None,
            background_job_run_id: None,
            governed_action_execution_id: None,
            worker_kind: WorkerLeaseKind::Background,
            lease_token: Uuid::now_v7(),
            worker_pid: None,
            lease_acquired_at: acquired_at,
            lease_expires_at: acquired_at - Duration::seconds(1),
            last_heartbeat_at: acquired_at,
            metadata: json!({}),
        };

        let error =
            validate_new_worker_lease(&lease).expect_err("invalid lease timing should be rejected");
        assert!(error.to_string().contains("expiry"));
    }

    #[test]
    fn validate_new_operational_diagnostic_rejects_empty_fields() {
        let diagnostic = NewOperationalDiagnostic {
            operational_diagnostic_id: Uuid::now_v7(),
            trace_id: None,
            execution_id: None,
            subsystem: " ".to_string(),
            severity: OperationalDiagnosticSeverity::Warn,
            reason_code: "timeout_or_stall".to_string(),
            summary: "worker lease expired".to_string(),
            diagnostic_payload: json!({}),
        };

        let error = validate_new_operational_diagnostic(&diagnostic)
            .expect_err("empty subsystem should be rejected");
        assert!(error.to_string().contains("subsystem"));
    }

    #[test]
    fn recovery_decision_continues_foreground_safe_replay() {
        let outcome = evaluate_recovery_decision(&RecoveryDecisionRequest {
            checkpoint_kind: RecoveryCheckpointKind::Foreground,
            action_classification: RecoveryActionClassification::SafeReplay,
            evidence_state: RecoveryEvidenceState::DurableIncomplete,
            ..base_recovery_decision_request()
        })
        .expect("safe foreground recovery should be classified");

        assert_eq!(outcome.decision, RecoveryDecision::Continue);
        assert_eq!(
            outcome.checkpoint_status,
            RecoveryCheckpointStatus::Resolved
        );
    }

    #[test]
    fn recovery_decision_retries_background_idempotent_work() {
        let outcome = evaluate_recovery_decision(&RecoveryDecisionRequest {
            checkpoint_kind: RecoveryCheckpointKind::Background,
            action_classification: RecoveryActionClassification::ProvablyIdempotentExternal,
            evidence_state: RecoveryEvidenceState::NotStarted,
            ..base_recovery_decision_request()
        })
        .expect("idempotent background recovery should be classified");

        assert_eq!(outcome.decision, RecoveryDecision::Retry);
        assert_eq!(
            outcome.checkpoint_status,
            RecoveryCheckpointStatus::Resolved
        );
    }

    #[test]
    fn recovery_decision_defers_pending_approval_transition() {
        let outcome = evaluate_recovery_decision(&RecoveryDecisionRequest {
            reason_code: RecoveryReasonCode::ApprovalTransition,
            approval_state: RecoveryApprovalState::Pending,
            ..base_recovery_decision_request()
        })
        .expect("pending approval recovery should be classified");

        assert_eq!(outcome.decision, RecoveryDecision::Defer);
        assert!(outcome.summary.contains("pending approval"));
    }

    #[test]
    fn recovery_decision_reapproves_expired_approval() {
        let outcome = evaluate_recovery_decision(&RecoveryDecisionRequest {
            reason_code: RecoveryReasonCode::ApprovalTransition,
            approval_state: RecoveryApprovalState::Expired,
            ..base_recovery_decision_request()
        })
        .expect("expired approval recovery should be classified");

        assert_eq!(outcome.decision, RecoveryDecision::Reapprove);
        assert!(outcome.summary.contains("fresh approval"));
    }

    #[test]
    fn recovery_decision_clarifies_ambiguous_side_effect_when_possible() {
        let outcome = evaluate_recovery_decision(&RecoveryDecisionRequest {
            checkpoint_kind: RecoveryCheckpointKind::GovernedAction,
            action_classification: RecoveryActionClassification::AmbiguousOrNonrepeatable,
            evidence_state: RecoveryEvidenceState::Ambiguous,
            clarification_available: true,
            ..base_recovery_decision_request()
        })
        .expect("ambiguous recoverable side effect should be classified");

        assert_eq!(outcome.decision, RecoveryDecision::Clarify);
        assert!(outcome.summary.contains("clarification"));
    }

    #[test]
    fn recovery_decision_abandons_ambiguous_side_effect_without_safe_path() {
        let outcome = evaluate_recovery_decision(&RecoveryDecisionRequest {
            checkpoint_kind: RecoveryCheckpointKind::GovernedAction,
            action_classification: RecoveryActionClassification::AmbiguousOrNonrepeatable,
            evidence_state: RecoveryEvidenceState::Ambiguous,
            clarification_available: false,
            ..base_recovery_decision_request()
        })
        .expect("ambiguous unrecoverable side effect should be classified");

        assert_eq!(outcome.decision, RecoveryDecision::Abandon);
        assert_eq!(
            outcome.checkpoint_status,
            RecoveryCheckpointStatus::Abandoned
        );
    }

    #[test]
    fn recovery_decision_abandons_policy_block_fail_closed() {
        let outcome = evaluate_recovery_decision(&RecoveryDecisionRequest {
            reason_code: RecoveryReasonCode::IntegrityOrPolicyBlock,
            policy_state: RecoveryPolicyState::RecheckFailed,
            ..base_recovery_decision_request()
        })
        .expect("policy block recovery should be classified");

        assert_eq!(outcome.decision, RecoveryDecision::Abandon);
        assert!(outcome.summary.contains("policy"));
    }

    #[test]
    fn recovery_decision_abandons_exhausted_budget_before_retry() {
        let outcome = evaluate_recovery_decision(&RecoveryDecisionRequest {
            recovery_budget_remaining: 0,
            ..base_recovery_decision_request()
        })
        .expect("exhausted recovery budget should be classified");

        assert_eq!(outcome.decision, RecoveryDecision::Abandon);
        assert!(outcome.summary.contains("budget"));
    }

    #[test]
    fn recovery_decision_rejects_negative_budget() {
        let error = evaluate_recovery_decision(&RecoveryDecisionRequest {
            recovery_budget_remaining: -1,
            ..base_recovery_decision_request()
        })
        .expect_err("negative recovery budget should be rejected");

        assert!(error.to_string().contains("budget"));
    }

    #[test]
    fn worker_lease_supervision_classifies_soft_warning_and_hard_expiry() {
        let acquired_at = Utc::now();
        let lease = WorkerLeaseRecord {
            worker_lease_id: Uuid::now_v7(),
            trace_id: Uuid::now_v7(),
            execution_id: None,
            background_job_id: None,
            background_job_run_id: None,
            governed_action_execution_id: None,
            worker_kind: WorkerLeaseKind::Background,
            status: WorkerLeaseStatus::Active,
            lease_token: Uuid::now_v7(),
            worker_pid: None,
            lease_acquired_at: acquired_at,
            lease_expires_at: acquired_at + Duration::seconds(100),
            last_heartbeat_at: acquired_at,
            released_at: None,
            metadata: json!({}),
            created_at: acquired_at,
            updated_at: acquired_at,
        };

        assert_eq!(
            classify_worker_lease_supervision(&lease, acquired_at + Duration::seconds(10), 80)
                .expect("healthy lease should classify"),
            WorkerLeaseSupervisionDecision::Healthy
        );
        assert_eq!(
            classify_worker_lease_supervision(&lease, acquired_at + Duration::seconds(80), 80)
                .expect("soft warning lease should classify"),
            WorkerLeaseSupervisionDecision::SoftWarning
        );
        assert_eq!(
            classify_worker_lease_supervision(&lease, acquired_at + Duration::seconds(101), 80)
                .expect("expired lease should classify"),
            WorkerLeaseSupervisionDecision::HardExpired
        );
    }

    fn base_recovery_decision_request() -> RecoveryDecisionRequest {
        RecoveryDecisionRequest {
            checkpoint_kind: RecoveryCheckpointKind::Foreground,
            reason_code: RecoveryReasonCode::Crash,
            action_classification: RecoveryActionClassification::SafeReplay,
            evidence_state: RecoveryEvidenceState::NotStarted,
            approval_state: RecoveryApprovalState::NotRequired,
            policy_state: RecoveryPolicyState::Valid,
            recovery_budget_remaining: 1,
            clarification_available: false,
        }
    }
}
