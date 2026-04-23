use anyhow::{Context, Result, bail};
use chrono::{DateTime, Duration, Utc};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;

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

async fn recover_worker_lease_timeout(
    pool: &PgPool,
    lease: WorkerLeaseRecord,
    now: DateTime<Utc>,
    source: &str,
    diagnostic_reason_code: &str,
    diagnostic_severity: OperationalDiagnosticSeverity,
    error_message: Option<&str>,
) -> Result<WorkerLeaseRecoveryOutcome> {
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
