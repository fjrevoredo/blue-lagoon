use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    path::PathBuf,
};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Duration, Utc};
use contracts::{
    BackgroundTrigger, BackgroundTriggerKind, CanonicalProposal, CanonicalProposalKind,
    CanonicalProposalPayload, CanonicalTargetKind, ChannelKind, CompactIdentitySnapshot,
    IdentityDeltaOperation, IdentityDeltaProposal, IdentityEvidenceRef, IdentityItemCategory,
    IdentityItemDelta, IdentityItemSource, IdentityLifecycleState, IdentityMergePolicy,
    IdentityStabilityClass, ProposalConflictPosture, ProposalEvaluationOutcome, ProposalProvenance,
    ProposalProvenanceKind, ScheduledForegroundLastOutcome, ScheduledForegroundTaskStatus,
    UnconsciousJobKind,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::{
    approval,
    audit::{self, NewAuditEvent},
    background_execution,
    background_planning::{self, BackgroundPlanningDecision, BackgroundPlanningRequest},
    causal_links,
    config::RuntimeConfig,
    continuity, db, execution, governed_actions, identity, migration, model_calls, model_gateway,
    proposal, recovery, scheduled_foreground,
    schema::{self, SchemaCompatibility, SchemaPolicy},
    worker, workspace,
};

const DEFAULT_LIST_LIMIT: u32 = 20;
const HEALTH_RECENT_WINDOW_MINUTES: i64 = 60;
const HEALTH_TOP_REASON_LIMIT: usize = 5;
const HEALTH_RECENT_BASE_DIAGNOSTIC_LIMIT: i64 = 200;
const ANOMALY_REPEAT_THRESHOLD: usize = 3;
const ANOMALY_FAILURE_PRESSURE_THRESHOLD: usize = 2;
const ANOMALY_DEDUPE_WINDOW_MINUTES: i64 = 15;
const LEASE_AT_RISK_WINDOW_SECONDS: i64 = 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStatusReport {
    pub schema: SchemaStatusReport,
    pub worker: WorkerStatusReport,
    pub telegram: TelegramStatusReport,
    pub model_gateway: ModelGatewayStatusReport,
    pub self_model: SelfModelStatusReport,
    pub pending_work: PendingWorkSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationalHealthSummary {
    pub evaluated_at: DateTime<Utc>,
    pub overall_status: String,
    pub pending_work: PendingWorkSummary,
    pub recovery: RecoveryHealthSummary,
    pub diagnostics: DiagnosticHealthSummary,
    pub anomalies: Vec<OperationalAnomalySummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryHealthSummary {
    pub open_checkpoint_count: u32,
    pub open_foreground_checkpoint_count: u32,
    pub open_background_checkpoint_count: u32,
    pub open_governed_action_checkpoint_count: u32,
    pub recent_resolved_checkpoint_count: u32,
    pub recent_abandoned_checkpoint_count: u32,
    pub active_worker_lease_count: u32,
    pub overdue_active_worker_lease_count: u32,
    pub at_risk_active_worker_lease_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticHealthSummary {
    pub recent_window_minutes: u32,
    pub observed_count: u32,
    pub info_count: u32,
    pub warn_count: u32,
    pub error_count: u32,
    pub critical_count: u32,
    pub top_reason_codes: Vec<OperationalReasonRollup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationalReasonRollup {
    pub reason_code: String,
    pub count: u32,
    pub latest_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationalAnomalySummary {
    pub anomaly_kind: String,
    pub severity: String,
    pub reason_code: String,
    pub summary: String,
    pub occurrence_count: u32,
    pub latest_trace_id: Option<Uuid>,
    pub latest_execution_id: Option<Uuid>,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationalDiagnosticSummary {
    pub operational_diagnostic_id: Uuid,
    pub trace_id: Option<Uuid>,
    pub execution_id: Option<Uuid>,
    pub subsystem: String,
    pub severity: String,
    pub reason_code: String,
    pub summary: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryCheckpointSummary {
    pub recovery_checkpoint_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub background_job_id: Option<Uuid>,
    pub background_job_run_id: Option<Uuid>,
    pub governed_action_execution_id: Option<Uuid>,
    pub approval_request_id: Option<Uuid>,
    pub checkpoint_kind: String,
    pub recovery_reason_code: String,
    pub status: String,
    pub recovery_decision: Option<String>,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoverySupervisionReport {
    pub trace_id: Uuid,
    pub supervised_at: DateTime<Utc>,
    pub actor_ref: String,
    pub reason: Option<String>,
    pub soft_warning_count: u32,
    pub recovered_expired_lease_count: u32,
    pub soft_warning_diagnostics: Vec<OperationalDiagnosticSummary>,
    pub recovered_expired_leases: Vec<RecoveredWorkerLeaseSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveredWorkerLeaseSummary {
    pub worker_lease_id: Uuid,
    pub worker_kind: String,
    pub checkpoint_id: Uuid,
    pub checkpoint_status: String,
    pub recovery_decision: String,
    pub diagnostic_reason_code: String,
    pub diagnostic_severity: String,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerLeaseInspectionSummary {
    pub worker_lease_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub background_job_id: Option<Uuid>,
    pub background_job_run_id: Option<Uuid>,
    pub governed_action_execution_id: Option<Uuid>,
    pub worker_kind: String,
    pub lease_status: String,
    pub supervision_status: String,
    pub lease_acquired_at: DateTime<Utc>,
    pub lease_expires_at: DateTime<Utc>,
    pub last_heartbeat_at: DateTime<Utc>,
    pub released_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledForegroundTaskSummary {
    pub scheduled_foreground_task_id: Uuid,
    pub task_key: String,
    pub channel_kind: String,
    pub status: String,
    pub internal_principal_ref: String,
    pub internal_conversation_ref: String,
    pub conversation_binding_present: bool,
    pub message_text: String,
    pub cadence_seconds: u64,
    pub cooldown_seconds: u64,
    pub next_due_at: DateTime<Utc>,
    pub current_execution_id: Option<Uuid>,
    pub current_run_started_at: Option<DateTime<Utc>>,
    pub last_execution_id: Option<Uuid>,
    pub last_run_started_at: Option<DateTime<Utc>>,
    pub last_run_completed_at: Option<DateTime<Utc>>,
    pub last_outcome: Option<String>,
    pub last_outcome_reason: Option<String>,
    pub last_outcome_summary: Option<String>,
    pub created_by: String,
    pub updated_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledForegroundTaskUpsertSummary {
    pub trace_id: Uuid,
    pub action: String,
    pub actor_ref: String,
    pub reason: Option<String>,
    pub task: ScheduledForegroundTaskSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaStatusReport {
    pub compatibility: String,
    pub current_version: Option<i64>,
    pub expected_version: i64,
    pub minimum_supported_version: i64,
    pub applied_migration_count: usize,
    pub history_valid: bool,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaUpgradeAssessmentReport {
    pub compatibility: String,
    pub current_version: Option<i64>,
    pub expected_version: i64,
    pub minimum_supported_version: i64,
    pub discovered_versions: Vec<i64>,
    pub applied_versions: Vec<i64>,
    pub pending_versions: Vec<i64>,
    pub history_valid: bool,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerStatusReport {
    pub resolution_kind: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub timeout_ms: u64,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramStatusReport {
    pub configured: bool,
    pub binding_present: bool,
    pub binding_internal_conversation_ref: Option<String>,
    pub binding_internal_principal_ref: Option<String>,
    pub bot_token_env: Option<String>,
    pub bot_token_present: bool,
    pub poll_limit: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelGatewayStatusReport {
    pub configured: bool,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_base_url: Option<String>,
    pub api_key_env: Option<String>,
    pub api_key_present: bool,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfModelStatusReport {
    pub configured: bool,
    pub seed_path: Option<String>,
    pub seed_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingWorkSummary {
    pub pending_foreground_conversation_count: usize,
    pub pending_background_job_count: u32,
    pub due_background_job_count: u32,
    pub pending_wake_signal_count: u32,
    pub pending_approval_request_count: u32,
    pub awaiting_approval_governed_action_count: u32,
    pub blocked_governed_action_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingForegroundConversationSummary {
    pub internal_conversation_ref: String,
    pub pending_count: u32,
    pub oldest_occurred_at: DateTime<Utc>,
    pub newest_occurred_at: DateTime<Utc>,
    pub oldest_touch_at: DateTime<Utc>,
    pub pending_span_seconds: u64,
    pub stale_pending_age_seconds: u64,
    pub includes_stale_processing: bool,
    pub suggested_mode: String,
    pub decision_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundJobSummary {
    pub background_job_id: Uuid,
    pub trace_id: Uuid,
    pub job_kind: String,
    pub trigger_kind: String,
    pub status: String,
    pub available_at: DateTime<Utc>,
    pub last_started_at: Option<DateTime<Utc>>,
    pub last_completed_at: Option<DateTime<Utc>>,
    pub internal_conversation_ref: Option<String>,
    pub scope_summary: String,
    pub latest_run_status: Option<String>,
    pub latest_run_completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakeSignalSummary {
    pub wake_signal_id: Uuid,
    pub background_job_id: Uuid,
    pub reason_code: String,
    pub reason: String,
    pub priority: String,
    pub status: String,
    pub decision_kind: Option<String>,
    pub requested_at: DateTime<Utc>,
    pub reviewed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequestSummary {
    pub approval_request_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub action_proposal_id: Uuid,
    pub action_fingerprint: String,
    pub action_kind: String,
    pub risk_tier: String,
    pub capability_scope: contracts::CapabilityScope,
    pub status: String,
    pub title: String,
    pub consequence_summary: String,
    pub requested_by: String,
    pub requested_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub resolution_kind: Option<String>,
    pub resolved_by: Option<String>,
    pub resolution_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernedActionSummary {
    pub governed_action_execution_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub approval_request_id: Option<Uuid>,
    pub action_proposal_id: Uuid,
    pub action_fingerprint: String,
    pub action_kind: String,
    pub risk_tier: String,
    pub status: String,
    pub workspace_script_id: Option<Uuid>,
    pub workspace_script_version_id: Option<Uuid>,
    pub blocked_reason: Option<String>,
    pub output_ref: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceScriptRunSummary {
    pub workspace_script_run_id: Uuid,
    pub workspace_script_id: Uuid,
    pub workspace_script_version_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub governed_action_execution_id: Option<Uuid>,
    pub approval_request_id: Option<Uuid>,
    pub status: String,
    pub risk_tier: String,
    pub args: Vec<String>,
    pub output_ref: Option<String>,
    pub failure_summary: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceLookupRequest {
    pub trace_id: Option<Uuid>,
    pub execution_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceReport {
    pub trace_id: Uuid,
    pub root_execution_id: Option<Uuid>,
    pub generated_at: DateTime<Utc>,
    pub node_count: usize,
    pub edge_count: usize,
    pub nodes: Vec<TraceNode>,
    pub edges: Vec<TraceEdge>,
    pub scheduling: Vec<SchedulingTraceSummary>,
    pub notes: Vec<TraceNote>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulingTraceSummary {
    pub scheduled_foreground_task_id: Uuid,
    pub task_key: String,
    pub status: String,
    pub message_text: String,
    pub cadence_seconds: i64,
    pub cooldown_seconds: i64,
    pub next_due_at: DateTime<Utc>,
    pub current_execution_id: Option<Uuid>,
    pub current_run_started_at: Option<DateTime<Utc>>,
    pub last_execution_id: Option<Uuid>,
    pub last_run_started_at: Option<DateTime<Utc>>,
    pub last_run_completed_at: Option<DateTime<Utc>>,
    pub last_outcome: Option<String>,
    pub last_outcome_reason: Option<String>,
    pub last_outcome_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceSummary {
    pub trace_id: Uuid,
    pub latest_execution_id: Option<Uuid>,
    pub latest_trigger_kind: Option<String>,
    pub latest_status: Option<String>,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub execution_count: u32,
    pub audit_event_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceNode {
    pub node_id: String,
    pub node_kind: String,
    pub source_id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub status: Option<String>,
    pub title: String,
    pub summary: String,
    pub payload: JsonValue,
    pub related_ids: BTreeMap<String, Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEdge {
    pub source_node_id: String,
    pub target_node_id: String,
    pub edge_kind: String,
    pub occurred_at: DateTime<Utc>,
    pub detail: Option<String>,
    pub inference: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceNote {
    pub note_kind: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceDiagnosisVerdict {
    Succeeded,
    Failed,
    AwaitingApproval,
    Blocked,
    Inconclusive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceFailureClass {
    ModelGatewayTransportFailure,
    ProviderRejected,
    TelegramDeliveryFailure,
    PersistenceFailure,
    ContextAssemblyFailure,
    MalformedActionProposal,
    WorkerProtocolFailure,
    ScheduledForegroundValidationFailure,
    ApprovalPending,
    ApprovalRejected,
    ApprovalExpired,
    GovernedActionBlocked,
    RecoveryInterrupted,
    UnknownFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceLikelyCauseKind {
    DirectFact,
    Inference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceSideEffectStatus {
    NoneExecuted,
    Executed,
    Possible,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceUserReplyStatus {
    Produced,
    NotProduced,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceRetrySafety {
    Safe,
    Unsafe,
    RequiresOperator,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceNodeReference {
    pub node_id: String,
    pub node_kind: String,
    pub source_id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub status: Option<String>,
    pub title: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceDiagnosisSummary {
    pub trace_id: Uuid,
    pub root_execution_id: Option<Uuid>,
    pub verdict: TraceDiagnosisVerdict,
    pub failure_class: Option<TraceFailureClass>,
    pub first_failing_node: Option<TraceNodeReference>,
    pub last_successful_node: Option<TraceNodeReference>,
    pub side_effect_status: TraceSideEffectStatus,
    pub user_reply_status: TraceUserReplyStatus,
    pub retry_safety: TraceRetrySafety,
    pub likely_cause: Option<String>,
    pub likely_cause_kind: Option<TraceLikelyCauseKind>,
    pub suggested_next_steps: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceFocusSelector {
    FailingNode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceFocusPayloadAvailability {
    Available,
    Partial,
    RetentionExpired,
    NotRecorded,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceFocusReport {
    pub selector: TraceFocusSelector,
    pub resolved_node: Option<TraceNode>,
    pub payload_availability: TraceFocusPayloadAvailability,
    pub payload_availability_reason: Option<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceExplanationReport {
    pub diagnosis: TraceDiagnosisSummary,
    pub focus: Option<TraceFocusReport>,
}

#[derive(Debug, Clone)]
pub struct ResolveApprovalRequest {
    pub approval_request_id: Uuid,
    pub decision: contracts::ApprovalResolutionDecision,
    pub actor_ref: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalResolutionSummary {
    pub approval_request: ApprovalRequestSummary,
    pub governed_action: Option<GovernedActionSummary>,
}

#[derive(Debug, Clone)]
pub struct EnqueueBackgroundJobRequest {
    pub job_kind: UnconsciousJobKind,
    pub trigger_kind: BackgroundTriggerKind,
    pub internal_conversation_ref: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SuperviseWorkerLeasesRequest {
    pub soft_warning_threshold_percent: u8,
    pub actor_ref: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct IdentityResetRequest {
    pub actor_ref: String,
    pub reason: Option<String>,
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityResetReport {
    pub trace_id: Uuid,
    pub reset_at: DateTime<Utc>,
    pub actor_ref: String,
    pub reason: Option<String>,
    pub previous_lifecycle_state: Option<String>,
    pub lifecycle_state: String,
    pub superseded_identity_item_count: u32,
    pub cancelled_interview_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityStatusReport {
    pub lifecycle_state: String,
    pub lifecycle_status: Option<String>,
    pub lifecycle_transition_reason: Option<String>,
    pub kickstart_available: bool,
    pub active_item_count: u32,
    pub stable_item_count: u32,
    pub evolving_item_count: u32,
    pub boundary_count: u32,
    pub value_count: u32,
    pub self_description_present: bool,
    pub compact_summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityShowReport {
    pub status: IdentityStatusReport,
    pub compact_identity: CompactIdentitySnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityHistorySummary {
    pub identity_item_id: Uuid,
    pub proposal_id: Option<Uuid>,
    pub trace_id: Option<Uuid>,
    pub stability_class: String,
    pub category: String,
    pub item_key: String,
    pub value_text: String,
    pub status: String,
    pub supersedes_item_id: Option<Uuid>,
    pub superseded_by_item_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityDiagnosticSummary {
    pub identity_diagnostic_id: Uuid,
    pub diagnostic_kind: String,
    pub severity: String,
    pub status: String,
    pub identity_item_id: Option<Uuid>,
    pub proposal_id: Option<Uuid>,
    pub trace_id: Option<Uuid>,
    pub message: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct IdentityEditProposalRequest {
    pub actor_ref: String,
    pub reason: String,
    pub operation: String,
    pub stability_class: String,
    pub category: String,
    pub item_key: String,
    pub value: String,
    pub confidence_pct: u8,
    pub weight_pct: Option<u8>,
    pub target_identity_item_id: Option<Uuid>,
    pub confirm_stable: bool,
}

#[derive(Debug, Clone)]
pub struct IdentityEditResolutionRequest {
    pub proposal_id: Uuid,
    pub actor_ref: String,
    pub decision: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityEditProposalReport {
    pub trace_id: Uuid,
    pub execution_id: Uuid,
    pub proposal_id: Uuid,
    pub status: String,
    pub validation_reason: String,
    pub stable_identity_change: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityEditProposalSummary {
    pub proposal_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Uuid,
    pub status: String,
    pub confidence_pct: u8,
    pub category: Option<String>,
    pub item_key: Option<String>,
    pub value_text: String,
    pub rationale: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityEditResolutionReport {
    pub proposal_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Uuid,
    pub decision: String,
    pub status: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct UpsertScheduledForegroundTaskRequest {
    pub task_key: String,
    pub internal_principal_ref: String,
    pub internal_conversation_ref: String,
    pub message_text: String,
    pub cadence_seconds: u64,
    pub cooldown_seconds: Option<u64>,
    pub next_due_at: Option<DateTime<Utc>>,
    pub status: ScheduledForegroundTaskStatus,
    pub actor_ref: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum BackgroundEnqueueOutcome {
    Planned {
        background_job_id: Uuid,
        deduplication_key: String,
        scope_summary: String,
    },
    SuppressedDuplicate {
        existing_job_id: Uuid,
        deduplication_key: String,
        reason: String,
    },
    Rejected {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum BackgroundRunNextOutcome {
    NoDueJob,
    Completed {
        background_job_id: Uuid,
        execution_id: Uuid,
        trace_id: Uuid,
        summary: String,
    },
}

pub async fn load_runtime_status(config: &RuntimeConfig) -> Result<RuntimeStatusReport> {
    let pool = db::connect(config).await?;
    let schema = inspect_schema_status(&pool, config).await?;
    let pending_work = load_pending_work_summary(&pool, config).await?;
    Ok(RuntimeStatusReport {
        schema,
        worker: inspect_worker_status(config),
        telegram: inspect_telegram_status(config),
        model_gateway: inspect_model_gateway_status(config),
        self_model: inspect_self_model_status(config),
        pending_work,
    })
}

pub async fn load_schema_status(config: &RuntimeConfig) -> Result<SchemaStatusReport> {
    let pool = db::connect(config).await?;
    inspect_schema_status(&pool, config).await
}

pub async fn load_schema_upgrade_assessment(
    config: &RuntimeConfig,
) -> Result<SchemaUpgradeAssessmentReport> {
    let pool = db::connect(config).await?;
    inspect_schema_upgrade_assessment(&pool, config).await
}

pub async fn load_identity_status(config: &RuntimeConfig) -> Result<IdentityStatusReport> {
    let pool = db::connect(config).await?;
    verify_schema(&pool, config).await?;
    load_identity_status_from_pool(&pool).await
}

pub async fn load_identity_show(config: &RuntimeConfig) -> Result<IdentityShowReport> {
    let pool = db::connect(config).await?;
    verify_schema(&pool, config).await?;
    let status = load_identity_status_from_pool(&pool).await?;
    let compact_identity = identity::reconstruct_compact_identity_snapshot(&pool, 64).await?;
    Ok(IdentityShowReport {
        status,
        compact_identity,
    })
}

pub async fn list_identity_history(
    config: &RuntimeConfig,
    limit: u32,
) -> Result<Vec<IdentityHistorySummary>> {
    let pool = db::connect(config).await?;
    verify_schema(&pool, config).await?;
    let rows = sqlx::query(
        r#"
        SELECT identity_item_id, proposal_id, trace_id, stability_class, category, item_key,
               value_text, status, supersedes_item_id, superseded_by_item_id, created_at,
               updated_at
        FROM identity_items
        ORDER BY updated_at DESC, created_at DESC
        LIMIT $1
        "#,
    )
    .bind(i64::from(limit))
    .fetch_all(&pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(IdentityHistorySummary {
                identity_item_id: row.try_get("identity_item_id")?,
                proposal_id: row.try_get("proposal_id")?,
                trace_id: row.try_get("trace_id")?,
                stability_class: row.try_get("stability_class")?,
                category: row.try_get("category")?,
                item_key: row.try_get("item_key")?,
                value_text: row.try_get("value_text")?,
                status: row.try_get("status")?,
                supersedes_item_id: row.try_get("supersedes_item_id")?,
                superseded_by_item_id: row.try_get("superseded_by_item_id")?,
                created_at: row.try_get("created_at")?,
                updated_at: row.try_get("updated_at")?,
            })
        })
        .collect()
}

pub async fn list_identity_diagnostics(
    config: &RuntimeConfig,
    limit: u32,
) -> Result<Vec<IdentityDiagnosticSummary>> {
    let pool = db::connect(config).await?;
    verify_schema(&pool, config).await?;
    let rows = sqlx::query(
        r#"
        SELECT identity_diagnostic_id, diagnostic_kind, severity, status, identity_item_id,
               proposal_id, trace_id, message, created_at
        FROM identity_diagnostics
        ORDER BY created_at DESC
        LIMIT $1
        "#,
    )
    .bind(i64::from(limit))
    .fetch_all(&pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(IdentityDiagnosticSummary {
                identity_diagnostic_id: row.try_get("identity_diagnostic_id")?,
                diagnostic_kind: row.try_get("diagnostic_kind")?,
                severity: row.try_get("severity")?,
                status: row.try_get("status")?,
                identity_item_id: row.try_get("identity_item_id")?,
                proposal_id: row.try_get("proposal_id")?,
                trace_id: row.try_get("trace_id")?,
                message: row.try_get("message")?,
                created_at: row.try_get("created_at")?,
            })
        })
        .collect()
}

pub async fn propose_identity_edit(
    config: &RuntimeConfig,
    request: IdentityEditProposalRequest,
) -> Result<IdentityEditProposalReport> {
    let pool = db::connect(config).await?;
    verify_schema(&pool, config).await?;

    let actor_ref = request.actor_ref.trim();
    if actor_ref.is_empty() {
        bail!("identity edit actor_ref must not be empty");
    }
    let reason = request.reason.trim();
    if reason.is_empty() {
        bail!("identity edit reason must not be empty");
    }

    let stability_class = parse_identity_stability_class(&request.stability_class)?;
    let stable_identity_change = matches!(stability_class, IdentityStabilityClass::Stable);
    if stable_identity_change && !request.confirm_stable {
        bail!("stable identity edit proposals require --confirm-stable");
    }

    let operation = parse_identity_delta_operation(&request.operation)?;
    let category = parse_identity_item_category(&request.category)?;
    let now = Utc::now();
    let trace_id = Uuid::now_v7();
    let execution_id = Uuid::now_v7();
    let proposal_id = Uuid::now_v7();
    let proposal = CanonicalProposal {
        proposal_id,
        proposal_kind: CanonicalProposalKind::IdentityDelta,
        canonical_target: CanonicalTargetKind::IdentityItems,
        confidence_pct: request.confidence_pct,
        conflict_posture: match operation {
            IdentityDeltaOperation::Revise => ProposalConflictPosture::Revises,
            IdentityDeltaOperation::Supersede => ProposalConflictPosture::Supersedes,
            _ => ProposalConflictPosture::Independent,
        },
        subject_ref: "self:blue-lagoon".to_string(),
        rationale: Some(reason.to_string()),
        valid_from: Some(now),
        valid_to: None,
        supersedes_artifact_id: None,
        provenance: ProposalProvenance {
            provenance_kind: ProposalProvenanceKind::EpisodeObservation,
            source_ingress_ids: vec![trace_id],
            source_episode_id: None,
        },
        payload: CanonicalProposalPayload::IdentityDelta(IdentityDeltaProposal {
            lifecycle_state: IdentityLifecycleState::CompleteIdentityActive,
            item_deltas: vec![IdentityItemDelta {
                operation,
                stability_class,
                category,
                item_key: request.item_key.trim().to_string(),
                value: request.value.trim().to_string(),
                confidence_pct: request.confidence_pct,
                weight_pct: request.weight_pct,
                source: IdentityItemSource::OperatorAuthored,
                merge_policy: if stable_identity_change {
                    IdentityMergePolicy::ApprovalRequired
                } else {
                    IdentityMergePolicy::Revisable
                },
                evidence_refs: vec![IdentityEvidenceRef {
                    source_kind: "operator".to_string(),
                    source_id: None,
                    summary: reason.to_string(),
                }],
                valid_from: Some(now),
                valid_to: None,
                target_identity_item_id: request.target_identity_item_id,
            }],
            self_description_delta: None,
            interview_action: None,
            rationale: reason.to_string(),
        }),
    };

    let validation = proposal::validate_proposal(&proposal);
    if validation.outcome == ProposalEvaluationOutcome::Rejected {
        bail!(validation.reason);
    }

    execution::insert(
        &pool,
        &execution::NewExecutionRecord {
            execution_id,
            trace_id,
            trigger_kind: "operator_identity_edit".to_string(),
            synthetic_trigger: Some("identity_edit_proposal".to_string()),
            status: "completed".to_string(),
            request_payload: json!({
                "actor_ref": actor_ref,
                "reason": reason,
                "proposal_id": proposal_id,
            }),
        },
    )
    .await?;
    continuity::insert_proposal(
        &pool,
        &continuity::NewProposalRecord {
            proposal_id,
            trace_id,
            execution_id,
            episode_id: None,
            source_ingress_id: None,
            source_loop_kind: "operator".to_string(),
            proposal_kind: "identity_delta".to_string(),
            canonical_target: "identity_items".to_string(),
            status: "pending_operator_review".to_string(),
            confidence: f64::from(request.confidence_pct) / 100.0,
            conflict_posture: conflict_posture_as_str(proposal.conflict_posture).to_string(),
            subject_ref: proposal.subject_ref.clone(),
            content_text: request.value.trim().to_string(),
            rationale: proposal.rationale.clone(),
            valid_from: proposal.valid_from,
            valid_to: None,
            supersedes_artifact_id: None,
            supersedes_artifact_kind: None,
            payload: serde_json::to_value(&proposal.payload)?,
        },
    )
    .await?;
    continuity::insert_merge_decision(
        &pool,
        &continuity::NewMergeDecision {
            merge_decision_id: Uuid::now_v7(),
            proposal_id,
            trace_id,
            execution_id,
            episode_id: None,
            decision_kind: "pending_operator_review".to_string(),
            decision_reason: validation.reason.clone(),
            accepted_memory_artifact_id: None,
            accepted_self_model_artifact_id: None,
            payload: json!({
                "actor_ref": actor_ref,
                "stable_identity_change": stable_identity_change,
            }),
        },
    )
    .await?;
    audit::insert(
        &pool,
        &NewAuditEvent {
            loop_kind: "operator".to_string(),
            subsystem: "management".to_string(),
            event_kind: "management_identity_edit_proposed".to_string(),
            severity: "info".to_string(),
            trace_id,
            execution_id: Some(execution_id),
            worker_pid: None,
            payload: json!({
                "actor_ref": actor_ref,
                "proposal_id": proposal_id,
                "stable_identity_change": stable_identity_change,
            }),
        },
    )
    .await?;

    Ok(IdentityEditProposalReport {
        trace_id,
        execution_id,
        proposal_id,
        status: "pending_operator_review".to_string(),
        validation_reason: validation.reason,
        stable_identity_change,
    })
}

pub async fn list_identity_edit_proposals(
    config: &RuntimeConfig,
    limit: u32,
) -> Result<Vec<IdentityEditProposalSummary>> {
    let pool = db::connect(config).await?;
    verify_schema(&pool, config).await?;
    let rows = sqlx::query(
        r#"
        SELECT proposal_id, trace_id, execution_id, status, confidence, content_text,
               rationale, payload_json, created_at
        FROM proposals
        WHERE proposal_kind = 'identity_delta'
          AND canonical_target = 'identity_items'
          AND source_loop_kind = 'operator'
        ORDER BY created_at DESC
        LIMIT $1
        "#,
    )
    .bind(i64::from(limit))
    .fetch_all(&pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            let payload: JsonValue = row.try_get("payload_json")?;
            Ok(IdentityEditProposalSummary {
                proposal_id: row.try_get("proposal_id")?,
                trace_id: row.try_get("trace_id")?,
                execution_id: row.try_get("execution_id")?,
                status: row.try_get("status")?,
                confidence_pct: pct_from_f64(row.try_get::<f64, _>("confidence")?),
                category: payload
                    .pointer("/value/item_deltas/0/category")
                    .and_then(JsonValue::as_str)
                    .map(str::to_string),
                item_key: payload
                    .pointer("/value/item_deltas/0/item_key")
                    .and_then(JsonValue::as_str)
                    .map(str::to_string),
                value_text: row.try_get("content_text")?,
                rationale: row.try_get("rationale")?,
                created_at: row.try_get("created_at")?,
            })
        })
        .collect()
}

pub async fn resolve_identity_edit_proposal(
    config: &RuntimeConfig,
    request: IdentityEditResolutionRequest,
) -> Result<IdentityEditResolutionReport> {
    let pool = db::connect(config).await?;
    verify_schema(&pool, config).await?;
    let actor_ref = request.actor_ref.trim();
    if actor_ref.is_empty() {
        bail!("identity edit resolution actor_ref must not be empty");
    }
    let decision = request.decision.trim();
    if decision != "approve" && decision != "reject" {
        bail!("identity edit resolution decision must be approve or reject");
    }

    let proposal = load_identity_edit_proposal(&pool, request.proposal_id).await?;
    let reason = request
        .reason
        .unwrap_or_else(|| format!("operator {decision}"));

    if decision == "reject" {
        sqlx::query("UPDATE proposals SET status = 'rejected' WHERE proposal_id = $1")
            .bind(request.proposal_id)
            .execute(&pool)
            .await?;
        continuity::update_merge_decision_outcome(&pool, request.proposal_id, "rejected", &reason)
            .await?;
        return Ok(IdentityEditResolutionReport {
            proposal_id: request.proposal_id,
            trace_id: proposal.0.trace_id,
            execution_id: proposal.0.execution_id,
            decision: decision.to_string(),
            status: "rejected".to_string(),
            reason,
        });
    }

    let context = proposal::ProposalProcessingContext {
        trace_id: proposal.0.trace_id,
        execution_id: proposal.0.execution_id,
        episode_id: None,
        source_ingress_id: None,
        source_loop_kind: "operator".to_string(),
    };
    let evaluation =
        identity::apply_identity_delta_proposal_merge(&pool, &context, &proposal.1).await?;
    let status = if evaluation.outcome == ProposalEvaluationOutcome::Accepted {
        "merged"
    } else {
        "rejected"
    };
    sqlx::query("UPDATE proposals SET status = $2 WHERE proposal_id = $1")
        .bind(request.proposal_id)
        .bind(status)
        .execute(&pool)
        .await?;

    Ok(IdentityEditResolutionReport {
        proposal_id: request.proposal_id,
        trace_id: proposal.0.trace_id,
        execution_id: proposal.0.execution_id,
        decision: decision.to_string(),
        status: status.to_string(),
        reason: evaluation.reason,
    })
}

pub async fn load_operational_health_summary(
    config: &RuntimeConfig,
) -> Result<OperationalHealthSummary> {
    let pool = db::connect(config).await?;
    let now = Utc::now();
    let recent_window_start = now - Duration::minutes(HEALTH_RECENT_WINDOW_MINUTES);
    let pending_work = load_pending_work_summary(&pool, config).await?;
    let recovery = load_recovery_health_summary(&pool, now, recent_window_start).await?;
    let base_diagnostics = load_recent_base_diagnostics(&pool, recent_window_start).await?;
    record_operational_anomaly_rollups(&pool, now, &recovery, &base_diagnostics).await?;
    let diagnostics = summarize_diagnostic_health(&base_diagnostics);
    let anomalies = load_recent_operational_anomalies(&pool, recent_window_start).await?;

    Ok(OperationalHealthSummary {
        evaluated_at: now,
        overall_status: classify_overall_health(&pending_work, &recovery, &diagnostics),
        pending_work,
        recovery,
        diagnostics,
        anomalies,
    })
}

pub async fn supervise_worker_leases(
    config: &RuntimeConfig,
    request: SuperviseWorkerLeasesRequest,
) -> Result<RecoverySupervisionReport> {
    let pool = db::connect(config).await?;
    verify_schema(&pool, config).await?;

    let actor_ref = request.actor_ref.trim();
    if actor_ref.is_empty() {
        bail!("management recovery supervision actor_ref must not be empty");
    }

    let trace_id = Uuid::now_v7();
    let supervised_at = Utc::now();
    audit::insert(
        &pool,
        &NewAuditEvent {
            loop_kind: "operator".to_string(),
            subsystem: "management".to_string(),
            event_kind: "management_recovery_supervision_requested".to_string(),
            severity: "info".to_string(),
            trace_id,
            execution_id: None,
            worker_pid: None,
            payload: json!({
                "actor_ref": actor_ref,
                "reason": request.reason,
                "soft_warning_threshold_percent": request.soft_warning_threshold_percent,
            }),
        },
    )
    .await?;

    let summary = match recovery::supervise_worker_leases(
        &pool,
        supervised_at,
        request.soft_warning_threshold_percent,
    )
    .await
    {
        Ok(summary) => summary,
        Err(error) => {
            let _ = audit::insert(
                &pool,
                &NewAuditEvent {
                    loop_kind: "operator".to_string(),
                    subsystem: "management".to_string(),
                    event_kind: "management_recovery_supervision_failed".to_string(),
                    severity: "error".to_string(),
                    trace_id,
                    execution_id: None,
                    worker_pid: None,
                    payload: json!({
                        "actor_ref": actor_ref,
                        "reason": request.reason,
                        "soft_warning_threshold_percent": request.soft_warning_threshold_percent,
                        "error": error.to_string(),
                    }),
                },
            )
            .await;
            return Err(error);
        }
    };

    audit::insert(
        &pool,
        &NewAuditEvent {
            loop_kind: "operator".to_string(),
            subsystem: "management".to_string(),
            event_kind: "management_recovery_supervision_completed".to_string(),
            severity: "info".to_string(),
            trace_id,
            execution_id: None,
            worker_pid: None,
            payload: json!({
                "actor_ref": actor_ref,
                "reason": request.reason,
                "soft_warning_threshold_percent": request.soft_warning_threshold_percent,
                "soft_warning_count": summary.soft_warning_diagnostics.len(),
                "recovered_expired_lease_count": summary.recovered_expired_leases.len(),
            }),
        },
    )
    .await?;

    Ok(RecoverySupervisionReport {
        trace_id,
        supervised_at,
        actor_ref: actor_ref.to_string(),
        reason: request.reason,
        soft_warning_count: summary.soft_warning_diagnostics.len() as u32,
        recovered_expired_lease_count: summary.recovered_expired_leases.len() as u32,
        soft_warning_diagnostics: summary
            .soft_warning_diagnostics
            .into_iter()
            .map(operational_diagnostic_summary)
            .collect(),
        recovered_expired_leases: summary
            .recovered_expired_leases
            .into_iter()
            .map(recovered_worker_lease_summary)
            .collect(),
    })
}

pub async fn reset_identity(
    config: &RuntimeConfig,
    request: IdentityResetRequest,
) -> Result<IdentityResetReport> {
    let pool = db::connect(config).await?;
    verify_schema(&pool, config).await?;

    if !request.force {
        bail!("identity reset requires --force");
    }

    let actor_ref = request.actor_ref.trim();
    if actor_ref.is_empty() {
        bail!("identity reset actor_ref must not be empty");
    }

    let trace_id = Uuid::now_v7();
    let reason = request.reason.clone();
    audit::insert(
        &pool,
        &NewAuditEvent {
            loop_kind: "operator".to_string(),
            subsystem: "management".to_string(),
            event_kind: "management_identity_reset_requested".to_string(),
            severity: "warn".to_string(),
            trace_id,
            execution_id: None,
            worker_pid: None,
            payload: json!({
                "actor_ref": actor_ref,
                "reason": reason.clone(),
                "force": request.force,
            }),
        },
    )
    .await?;

    let outcome =
        match identity::reset_to_bootstrap(&pool, trace_id, actor_ref, reason.as_deref()).await {
            Ok(outcome) => outcome,
            Err(error) => {
                let _ = audit::insert(
                    &pool,
                    &NewAuditEvent {
                        loop_kind: "operator".to_string(),
                        subsystem: "management".to_string(),
                        event_kind: "management_identity_reset_failed".to_string(),
                        severity: "error".to_string(),
                        trace_id,
                        execution_id: None,
                        worker_pid: None,
                        payload: json!({
                            "actor_ref": actor_ref,
                            "reason": reason.clone(),
                            "error": error.to_string(),
                        }),
                    },
                )
                .await;
                return Err(error);
            }
        };

    audit::insert(
        &pool,
        &NewAuditEvent {
            loop_kind: "operator".to_string(),
            subsystem: "management".to_string(),
            event_kind: "management_identity_reset_completed".to_string(),
            severity: "info".to_string(),
            trace_id,
            execution_id: None,
            worker_pid: None,
            payload: json!({
                "actor_ref": actor_ref,
                "reason": reason.clone(),
                "previous_lifecycle_state": outcome.previous_lifecycle_state.clone(),
                "lifecycle_state": "bootstrap_seed_only",
                "superseded_identity_item_count": outcome.superseded_identity_item_count,
                "cancelled_interview_count": outcome.cancelled_interview_count,
            }),
        },
    )
    .await?;

    Ok(IdentityResetReport {
        trace_id,
        reset_at: outcome.reset_at,
        actor_ref: actor_ref.to_string(),
        reason,
        previous_lifecycle_state: outcome.previous_lifecycle_state,
        lifecycle_state: "bootstrap_seed_only".to_string(),
        superseded_identity_item_count: outcome.superseded_identity_item_count,
        cancelled_interview_count: outcome.cancelled_interview_count,
    })
}

async fn load_identity_status_from_pool(pool: &PgPool) -> Result<IdentityStatusReport> {
    let lifecycle = identity::get_current_lifecycle(pool).await?;
    let compact_identity = identity::reconstruct_compact_identity_snapshot(pool, 64).await?;
    let active_item_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM identity_items WHERE status = 'active'")
            .fetch_one(pool)
            .await?;
    let stable_item_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM identity_items WHERE status = 'active' AND stability_class = 'stable'",
    )
    .fetch_one(pool)
    .await?;
    let evolving_item_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM identity_items WHERE status = 'active' AND stability_class = 'evolving'",
    )
    .fetch_one(pool)
    .await?;

    let lifecycle_state = lifecycle
        .as_ref()
        .map(|record| record.lifecycle_state.clone())
        .unwrap_or_else(|| "bootstrap_seed_only".to_string());
    let compact_summary = if compact_identity.identity_summary.trim().is_empty() {
        "(not formed)".to_string()
    } else {
        compact_identity.identity_summary.clone()
    };

    Ok(IdentityStatusReport {
        kickstart_available: matches!(
            lifecycle_state.as_str(),
            "bootstrap_seed_only" | "identity_kickstart_in_progress"
        ),
        lifecycle_status: lifecycle.as_ref().map(|record| record.status.clone()),
        lifecycle_transition_reason: lifecycle
            .as_ref()
            .map(|record| record.transition_reason.clone()),
        lifecycle_state,
        active_item_count: active_item_count.try_into().unwrap_or(u32::MAX),
        stable_item_count: stable_item_count.try_into().unwrap_or(u32::MAX),
        evolving_item_count: evolving_item_count.try_into().unwrap_or(u32::MAX),
        boundary_count: compact_identity
            .boundaries
            .len()
            .try_into()
            .unwrap_or(u32::MAX),
        value_count: compact_identity.values.len().try_into().unwrap_or(u32::MAX),
        self_description_present: compact_identity.self_description.is_some(),
        compact_summary,
    })
}

pub async fn list_active_worker_leases(
    config: &RuntimeConfig,
    limit: u32,
    soft_warning_threshold_percent: u8,
) -> Result<Vec<WorkerLeaseInspectionSummary>> {
    let pool = db::connect(config).await?;
    verify_schema(&pool, config).await?;

    let now = Utc::now();
    recovery::list_active_worker_leases(&pool, i64::from(limit))
        .await?
        .into_iter()
        .map(|lease| {
            let supervision_status = recovery::classify_worker_lease_supervision(
                &lease,
                now,
                soft_warning_threshold_percent,
            )?;
            Ok(WorkerLeaseInspectionSummary {
                worker_lease_id: lease.worker_lease_id,
                trace_id: lease.trace_id,
                execution_id: lease.execution_id,
                background_job_id: lease.background_job_id,
                background_job_run_id: lease.background_job_run_id,
                governed_action_execution_id: lease.governed_action_execution_id,
                worker_kind: worker_lease_kind_label(lease.worker_kind),
                lease_status: worker_lease_status_label(lease.status),
                supervision_status: worker_lease_supervision_status_label(supervision_status),
                lease_acquired_at: lease.lease_acquired_at,
                lease_expires_at: lease.lease_expires_at,
                last_heartbeat_at: lease.last_heartbeat_at,
                released_at: lease.released_at,
            })
        })
        .collect()
}

pub async fn list_scheduled_foreground_tasks(
    config: &RuntimeConfig,
    status: Option<ScheduledForegroundTaskStatus>,
    due_only: bool,
    limit: u32,
) -> Result<Vec<ScheduledForegroundTaskSummary>> {
    let pool = db::connect(config).await?;
    verify_schema(&pool, config).await?;

    let records = scheduled_foreground::list_tasks(
        &pool,
        scheduled_foreground::ScheduledForegroundTaskListFilter {
            status,
            due_only,
            limit: i64::from(limit),
        },
    )
    .await?;

    let mut summaries = Vec::with_capacity(records.len());
    for record in records {
        summaries.push(scheduled_foreground_task_summary_from_record(&pool, record).await?);
    }
    Ok(summaries)
}

pub async fn get_scheduled_foreground_task(
    config: &RuntimeConfig,
    task_key: &str,
) -> Result<Option<ScheduledForegroundTaskSummary>> {
    let pool = db::connect(config).await?;
    verify_schema(&pool, config).await?;

    let task_key = task_key.trim();
    if task_key.is_empty() {
        bail!("scheduled foreground task key must not be empty");
    }

    let Some(record) = scheduled_foreground::get_task_by_key(&pool, task_key).await? else {
        return Ok(None);
    };

    Ok(Some(
        scheduled_foreground_task_summary_from_record(&pool, record).await?,
    ))
}

pub async fn upsert_scheduled_foreground_task(
    config: &RuntimeConfig,
    request: UpsertScheduledForegroundTaskRequest,
) -> Result<ScheduledForegroundTaskUpsertSummary> {
    let pool = db::connect(config).await?;
    verify_schema(&pool, config).await?;

    let actor_ref = request.actor_ref.trim();
    if actor_ref.is_empty() {
        bail!("scheduled foreground actor_ref must not be empty");
    }

    let task_key = request.task_key.trim();
    if task_key.is_empty() {
        bail!("scheduled foreground task key must not be empty");
    }

    let internal_conversation_ref = request.internal_conversation_ref.trim();
    if internal_conversation_ref.is_empty() {
        bail!("scheduled foreground internal_conversation_ref must not be empty");
    }

    let binding_present =
        scheduled_foreground::conversation_binding_present(&pool, internal_conversation_ref)
            .await?;
    if !binding_present {
        bail!(
            "scheduled foreground internal_conversation_ref '{}' does not have a bound conversation",
            internal_conversation_ref
        );
    }

    let trace_id = Uuid::now_v7();
    let reason = request.reason.clone();
    audit::insert(
        &pool,
        &NewAuditEvent {
            loop_kind: "operator".to_string(),
            subsystem: "management".to_string(),
            event_kind: "management_scheduled_foreground_upsert_requested".to_string(),
            severity: "info".to_string(),
            trace_id,
            execution_id: None,
            worker_pid: None,
            payload: json!({
                "actor_ref": actor_ref,
                "reason": reason.clone(),
                "task_key": task_key,
                "internal_principal_ref": request.internal_principal_ref.trim(),
                "internal_conversation_ref": internal_conversation_ref,
                "binding_present": binding_present,
                "cadence_seconds": request.cadence_seconds,
                "cooldown_seconds": request.cooldown_seconds,
                "next_due_at": request.next_due_at,
                "status": scheduled_foreground_task_status_label(request.status),
            }),
        },
    )
    .await?;

    let upsert_result = match scheduled_foreground::upsert_task(
        &pool,
        config,
        &scheduled_foreground::UpsertScheduledForegroundTask {
            task_key: task_key.to_string(),
            internal_principal_ref: request.internal_principal_ref,
            internal_conversation_ref: request.internal_conversation_ref,
            message_text: request.message_text,
            cadence_seconds: request.cadence_seconds,
            cooldown_seconds: request.cooldown_seconds,
            next_due_at: request.next_due_at,
            status: request.status,
            actor_ref: actor_ref.to_string(),
        },
    )
    .await
    {
        Ok(result) => result,
        Err(error) => {
            let _ = audit::insert(
                &pool,
                &NewAuditEvent {
                    loop_kind: "operator".to_string(),
                    subsystem: "management".to_string(),
                    event_kind: "management_scheduled_foreground_upsert_failed".to_string(),
                    severity: "error".to_string(),
                    trace_id,
                    execution_id: None,
                    worker_pid: None,
                    payload: json!({
                        "actor_ref": actor_ref,
                        "reason": reason.clone(),
                        "task_key": task_key,
                        "error": error.to_string(),
                    }),
                },
            )
            .await;
            return Err(error);
        }
    };

    let action = scheduled_foreground_task_write_action_label(upsert_result.action);
    let task = scheduled_foreground_task_summary_from_record(&pool, upsert_result.record).await?;
    audit::insert(
        &pool,
        &NewAuditEvent {
            loop_kind: "operator".to_string(),
            subsystem: "management".to_string(),
            event_kind: "management_scheduled_foreground_upsert_completed".to_string(),
            severity: "info".to_string(),
            trace_id,
            execution_id: None,
            worker_pid: None,
            payload: json!({
                "actor_ref": actor_ref,
                "reason": reason.clone(),
                "action": action,
                "task_key": task.task_key.clone(),
                "scheduled_foreground_task_id": task.scheduled_foreground_task_id,
                "status": task.status.clone(),
                "next_due_at": task.next_due_at,
            }),
        },
    )
    .await?;

    Ok(ScheduledForegroundTaskUpsertSummary {
        trace_id,
        action,
        actor_ref: actor_ref.to_string(),
        reason,
        task,
    })
}

pub async fn list_recent_operational_diagnostics(
    config: &RuntimeConfig,
    limit: u32,
) -> Result<Vec<OperationalDiagnosticSummary>> {
    let pool = db::connect(config).await?;
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
            created_at
        FROM operational_diagnostics
        ORDER BY created_at DESC, operational_diagnostic_id DESC
        LIMIT $1
        "#,
    )
    .bind(i64::from(limit))
    .fetch_all(&pool)
    .await
    .context("failed to list recent operational diagnostics for management")?;

    Ok(rows
        .into_iter()
        .map(|row| OperationalDiagnosticSummary {
            operational_diagnostic_id: row.get("operational_diagnostic_id"),
            trace_id: row.get("trace_id"),
            execution_id: row.get("execution_id"),
            subsystem: row.get("subsystem"),
            severity: row.get("severity"),
            reason_code: row.get("reason_code"),
            summary: row.get("summary"),
            created_at: row.get("created_at"),
        })
        .collect())
}

pub async fn list_recovery_checkpoints(
    config: &RuntimeConfig,
    open_only: bool,
    limit: u32,
) -> Result<Vec<RecoveryCheckpointSummary>> {
    let pool = db::connect(config).await?;
    let rows = if open_only {
        sqlx::query(
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
                created_at,
                resolved_at
            FROM recovery_checkpoints
            WHERE status = 'open'
            ORDER BY created_at DESC, recovery_checkpoint_id DESC
            LIMIT $1
            "#,
        )
        .bind(i64::from(limit))
        .fetch_all(&pool)
        .await
        .context("failed to list open recovery checkpoints for management")?
    } else {
        sqlx::query(
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
                created_at,
                resolved_at
            FROM recovery_checkpoints
            ORDER BY created_at DESC, recovery_checkpoint_id DESC
            LIMIT $1
            "#,
        )
        .bind(i64::from(limit))
        .fetch_all(&pool)
        .await
        .context("failed to list recovery checkpoints for management")?
    };

    Ok(rows
        .into_iter()
        .map(|row| RecoveryCheckpointSummary {
            recovery_checkpoint_id: row.get("recovery_checkpoint_id"),
            trace_id: row.get("trace_id"),
            execution_id: row.get("execution_id"),
            background_job_id: row.get("background_job_id"),
            background_job_run_id: row.get("background_job_run_id"),
            governed_action_execution_id: row.get("governed_action_execution_id"),
            approval_request_id: row.get("approval_request_id"),
            checkpoint_kind: row.get("checkpoint_kind"),
            recovery_reason_code: row.get("recovery_reason_code"),
            status: row.get("status"),
            recovery_decision: row.get("recovery_decision"),
            created_at: row.get("created_at"),
            resolved_at: row.get("resolved_at"),
        })
        .collect())
}

pub async fn list_pending_foreground_conversations(
    config: &RuntimeConfig,
    limit: u32,
) -> Result<Vec<PendingForegroundConversationSummary>> {
    let pool = db::connect(config).await?;
    let stale_cutoff = foreground_stale_cutoff(config);
    let rows = sqlx::query(
        r#"
        SELECT
            internal_conversation_ref,
            COUNT(*) AS pending_count,
            MIN(occurred_at) AS oldest_occurred_at,
            MAX(occurred_at) AS newest_occurred_at,
            MIN(COALESCE(last_processed_at, received_at)) AS oldest_touch_at,
            BOOL_OR(foreground_status = 'processing') AS includes_stale_processing
        FROM ingress_events
        WHERE internal_conversation_ref IS NOT NULL
          AND status = 'accepted'
          AND (
              foreground_status = 'pending'
              OR (
                  foreground_status = 'processing'
                  AND COALESCE(last_processed_at, received_at) <= $1
              )
          )
        GROUP BY internal_conversation_ref
        ORDER BY oldest_occurred_at ASC, internal_conversation_ref ASC
        LIMIT $2
        "#,
    )
    .bind(stale_cutoff)
    .bind(i64::from(limit))
    .fetch_all(&pool)
    .await
    .context("failed to list pending foreground conversations")?;

    let now = Utc::now();
    Ok(rows
        .into_iter()
        .map(|row| {
            let pending_count = row.get::<i64, _>("pending_count");
            let oldest_occurred_at: DateTime<Utc> = row.get("oldest_occurred_at");
            let newest_occurred_at: DateTime<Utc> = row.get("newest_occurred_at");
            let oldest_touch_at: DateTime<Utc> = row.get("oldest_touch_at");
            let pending_span_seconds = newest_occurred_at
                .signed_duration_since(oldest_occurred_at)
                .num_seconds()
                .max(0) as u64;
            let stale_pending_age_seconds = now
                .signed_duration_since(oldest_touch_at)
                .num_seconds()
                .max(0) as u64;
            let includes_stale_processing = row.get::<bool, _>("includes_stale_processing");
            let (suggested_mode, decision_reason) = classify_pending_foreground_summary(
                config,
                pending_count as usize,
                pending_span_seconds,
                stale_pending_age_seconds,
                includes_stale_processing,
            );

            PendingForegroundConversationSummary {
                internal_conversation_ref: row.get("internal_conversation_ref"),
                pending_count: pending_count as u32,
                oldest_occurred_at,
                newest_occurred_at,
                oldest_touch_at,
                pending_span_seconds,
                stale_pending_age_seconds,
                includes_stale_processing,
                suggested_mode: suggested_mode.to_string(),
                decision_reason: decision_reason.to_string(),
            }
        })
        .collect())
}

pub async fn list_background_jobs(
    config: &RuntimeConfig,
    limit: u32,
) -> Result<Vec<BackgroundJobSummary>> {
    let pool = db::connect(config).await?;
    let rows = sqlx::query(
        r#"
        SELECT
            job.background_job_id,
            job.trace_id,
            job.job_kind,
            job.trigger_kind,
            job.status,
            job.available_at,
            job.last_started_at,
            job.last_completed_at,
            job.scope_summary,
            job.scope_json,
            latest_run.status AS latest_run_status,
            latest_run.completed_at AS latest_run_completed_at
        FROM background_jobs job
        LEFT JOIN LATERAL (
            SELECT status, completed_at
            FROM background_job_runs
            WHERE background_job_id = job.background_job_id
            ORDER BY COALESCE(completed_at, started_at, lease_acquired_at) DESC,
                     background_job_run_id DESC
            LIMIT 1
        ) latest_run ON TRUE
        ORDER BY job.created_at DESC, job.background_job_id DESC
        LIMIT $1
        "#,
    )
    .bind(i64::from(limit))
    .fetch_all(&pool)
    .await
    .context("failed to list background jobs for management CLI")?;

    rows.into_iter()
        .map(|row| {
            let scope: contracts::UnconsciousScope = serde_json::from_value(row.get("scope_json"))
                .context("failed to decode background job scope for management CLI")?;

            Ok(BackgroundJobSummary {
                background_job_id: row.get("background_job_id"),
                trace_id: row.get("trace_id"),
                job_kind: row.get("job_kind"),
                trigger_kind: row.get("trigger_kind"),
                status: row.get("status"),
                available_at: row.get("available_at"),
                last_started_at: row.get("last_started_at"),
                last_completed_at: row.get("last_completed_at"),
                internal_conversation_ref: scope.internal_conversation_ref,
                scope_summary: row.get("scope_summary"),
                latest_run_status: row.get("latest_run_status"),
                latest_run_completed_at: row.get("latest_run_completed_at"),
            })
        })
        .collect()
}

pub async fn enqueue_background_job(
    config: &RuntimeConfig,
    request: EnqueueBackgroundJobRequest,
) -> Result<BackgroundEnqueueOutcome> {
    let pool = db::connect(config).await?;
    verify_schema(&pool, config).await?;
    let trigger = BackgroundTrigger {
        trigger_id: Uuid::now_v7(),
        trigger_kind: request.trigger_kind,
        requested_at: Utc::now(),
        reason_summary: request
            .reason
            .clone()
            .unwrap_or_else(|| "manual management CLI enqueue".to_string()),
        payload_ref: request.internal_conversation_ref.clone(),
    };
    let decision = background_planning::plan_background_job(
        &pool,
        config,
        BackgroundPlanningRequest {
            trace_id: Uuid::now_v7(),
            job_kind: request.job_kind,
            trigger,
            internal_conversation_ref: request.internal_conversation_ref,
            available_at: Utc::now(),
        },
    )
    .await?;

    Ok(match decision {
        BackgroundPlanningDecision::Planned(job) => BackgroundEnqueueOutcome::Planned {
            background_job_id: job.background_job_id,
            deduplication_key: job.deduplication_key,
            scope_summary: job.scope.summary,
        },
        BackgroundPlanningDecision::SuppressedDuplicate {
            existing_job_id,
            deduplication_key,
            reason,
        } => BackgroundEnqueueOutcome::SuppressedDuplicate {
            existing_job_id,
            deduplication_key,
            reason,
        },
        BackgroundPlanningDecision::Rejected { reason } => {
            BackgroundEnqueueOutcome::Rejected { reason }
        }
    })
}

pub async fn run_next_background_job(config: &RuntimeConfig) -> Result<BackgroundRunNextOutcome> {
    let transport = model_gateway::ReqwestModelProviderTransport::new();
    run_next_background_job_with_transport(config, &transport).await
}

pub async fn run_next_background_job_with_transport<T: model_gateway::ModelProviderTransport>(
    config: &RuntimeConfig,
    transport: &T,
) -> Result<BackgroundRunNextOutcome> {
    let pool = db::connect(config).await?;
    verify_schema(&pool, config).await?;
    let gateway = config.require_model_gateway_config()?;

    let outcome =
        background_execution::execute_next_due_job(&pool, config, &gateway, transport, Utc::now())
            .await?;

    Ok(match outcome {
        Some(result) => BackgroundRunNextOutcome::Completed {
            background_job_id: result.background_job_id,
            execution_id: result.execution_id,
            trace_id: result.trace_id,
            summary: result.summary,
        },
        None => BackgroundRunNextOutcome::NoDueJob,
    })
}

pub async fn list_wake_signals(
    config: &RuntimeConfig,
    limit: u32,
) -> Result<Vec<WakeSignalSummary>> {
    let pool = db::connect(config).await?;
    let rows = sqlx::query(
        r#"
        SELECT
            wake_signal_id,
            background_job_id,
            reason,
            priority,
            reason_code,
            status,
            decision_kind,
            requested_at,
            reviewed_at
        FROM wake_signals
        ORDER BY requested_at DESC, wake_signal_id DESC
        LIMIT $1
        "#,
    )
    .bind(i64::from(limit))
    .fetch_all(&pool)
    .await
    .context("failed to list wake signals for management CLI")?;

    Ok(rows
        .into_iter()
        .map(|row| WakeSignalSummary {
            wake_signal_id: row.get("wake_signal_id"),
            background_job_id: row.get("background_job_id"),
            reason_code: row.get("reason_code"),
            reason: row.get("reason"),
            priority: row.get("priority"),
            status: row.get("status"),
            decision_kind: row.get("decision_kind"),
            requested_at: row.get("requested_at"),
            reviewed_at: row.get("reviewed_at"),
        })
        .collect())
}

pub async fn list_approval_requests(
    config: &RuntimeConfig,
    status: Option<contracts::ApprovalRequestStatus>,
    limit: u32,
) -> Result<Vec<ApprovalRequestSummary>> {
    let pool = db::connect(config).await?;
    approval::list_approval_requests(&pool, status, i64::from(limit))
        .await?
        .into_iter()
        .map(|record| Ok(approval_request_summary(&record)))
        .collect()
}

pub async fn resolve_approval_request(
    config: &RuntimeConfig,
    request: ResolveApprovalRequest,
) -> Result<ApprovalResolutionSummary> {
    if !config.approvals.allow_cli_resolution {
        bail!("CLI approval resolution is disabled by configuration");
    }

    let pool = db::connect(config).await?;
    verify_schema(&pool, config).await?;

    let approval_request =
        approval::get_approval_request(&pool, request.approval_request_id).await?;
    let actor_ref = request
        .actor_ref
        .unwrap_or_else(|| default_cli_actor_ref(&approval_request.requested_by));
    let resolution = approval::resolve_approval_request(
        &pool,
        &approval::ApprovalResolutionAttempt {
            token: approval_request.token.clone(),
            actor_ref,
            expected_action_fingerprint: approval_request.action_fingerprint.clone(),
            decision: request.decision,
            reason: request.reason,
            resolved_at: Utc::now(),
        },
    )
    .await?;

    let governed_action =
        match governed_actions::get_governed_action_execution_by_approval_request_id(
            &pool,
            resolution.request.approval_request_id,
        )
        .await?
        {
            Some(record) => {
                let synced = governed_actions::sync_status_from_approval_resolution(
                    &pool,
                    record.governed_action_execution_id,
                    resolution.event.decision,
                    resolution.request.execution_id,
                    resolution.event.reason.as_deref(),
                )
                .await?;

                let record = if resolution.event.decision
                    == contracts::ApprovalResolutionDecision::Approved
                {
                    governed_actions::execute_governed_action(config, &pool, &synced)
                        .await?
                        .record
                } else {
                    synced
                };
                Some(governed_action_summary(&record))
            }
            None => None,
        };

    Ok(ApprovalResolutionSummary {
        approval_request: approval_request_summary(&resolution.request),
        governed_action,
    })
}

pub async fn list_governed_actions(
    config: &RuntimeConfig,
    status: Option<contracts::GovernedActionStatus>,
    limit: u32,
) -> Result<Vec<GovernedActionSummary>> {
    let pool = db::connect(config).await?;
    governed_actions::list_governed_action_executions(&pool, status, i64::from(limit))
        .await?
        .into_iter()
        .map(|record| Ok(governed_action_summary(&record)))
        .collect()
}

pub async fn load_trace_report(
    config: &RuntimeConfig,
    request: TraceLookupRequest,
) -> Result<TraceReport> {
    let pool = db::connect(config).await?;
    verify_schema(&pool, config).await?;

    let trace_id = resolve_trace_lookup(&pool, &request).await?;
    let mut builder = TraceReportBuilder::new(trace_id, request.execution_id);

    load_trace_execution_nodes(&pool, trace_id, &mut builder).await?;
    load_trace_ingress_nodes(&pool, trace_id, &mut builder).await?;
    load_trace_episode_nodes(&pool, trace_id, &mut builder).await?;
    load_trace_audit_nodes(&pool, trace_id, &mut builder).await?;
    load_trace_model_call_nodes(&pool, trace_id, &mut builder).await?;
    load_trace_background_nodes(&pool, trace_id, &mut builder).await?;
    load_trace_wake_signal_nodes(&pool, trace_id, &mut builder).await?;
    load_trace_approval_nodes(&pool, trace_id, &mut builder).await?;
    load_trace_governed_action_nodes(&pool, trace_id, &mut builder).await?;
    load_trace_scheduled_task_nodes(&pool, trace_id, &mut builder).await?;
    load_trace_explicit_causal_links(&pool, trace_id, &mut builder).await?;

    Ok(builder.finish())
}

pub fn explain_trace_report(
    report: &TraceReport,
    focus: Option<TraceFocusSelector>,
) -> TraceExplanationReport {
    let diagnosis = diagnose_trace_report(report);
    let focus = focus.map(|selector| inspect_trace_report_focus(report, &diagnosis, selector));
    TraceExplanationReport { diagnosis, focus }
}

pub fn diagnose_trace_report(report: &TraceReport) -> TraceDiagnosisSummary {
    let pending_approval = find_pending_approval_node(report);
    let blocked_node = find_blocked_node(report);
    let first_failing_node = find_first_failing_node(report);
    let primary_failure = pending_approval
        .or(blocked_node)
        .or(first_failing_node)
        .map(trace_node_reference);
    let last_successful_node = primary_failure
        .as_ref()
        .and_then(|failure| find_last_nonfailing_before(report, &failure.node_id))
        .or_else(|| {
            report
                .nodes
                .iter()
                .rev()
                .find(|node| node_is_nonfailing(node))
                .map(trace_node_reference)
        });
    let failure_class =
        classify_trace_failure(report, pending_approval, blocked_node, first_failing_node);
    let verdict = derive_trace_verdict(report, pending_approval, blocked_node, failure_class);
    let side_effect_status = derive_side_effect_status(report);
    let user_reply_status = derive_user_reply_status(report);
    let retry_safety = derive_retry_safety(verdict, side_effect_status, failure_class);
    let (likely_cause, likely_cause_kind) = derive_likely_cause(
        report,
        failure_class,
        pending_approval,
        blocked_node,
        first_failing_node,
    );
    let mut notes = report
        .notes
        .iter()
        .map(|note| format!("{}: {}", note.note_kind, note.message))
        .collect::<Vec<_>>();
    if first_failing_node.is_none() && pending_approval.is_none() && blocked_node.is_none() {
        notes.push("No explicit failing node was found in the durable trace.".to_string());
    }
    let suggested_next_steps = derive_next_steps(
        failure_class,
        retry_safety,
        side_effect_status,
        pending_approval.is_some(),
    );

    TraceDiagnosisSummary {
        trace_id: report.trace_id,
        root_execution_id: report.root_execution_id,
        verdict,
        failure_class,
        first_failing_node: primary_failure,
        last_successful_node,
        side_effect_status,
        user_reply_status,
        retry_safety,
        likely_cause,
        likely_cause_kind,
        suggested_next_steps,
        notes,
    }
}

fn find_last_nonfailing_before(
    report: &TraceReport,
    failing_node_id: &str,
) -> Option<TraceNodeReference> {
    let failure_index = report
        .nodes
        .iter()
        .position(|node| node.node_id == failing_node_id)?;
    report.nodes[..failure_index]
        .iter()
        .rev()
        .find(|node| node_is_nonfailing(node))
        .map(trace_node_reference)
}

pub fn inspect_trace_report_focus(
    report: &TraceReport,
    diagnosis: &TraceDiagnosisSummary,
    selector: TraceFocusSelector,
) -> TraceFocusReport {
    let resolved_node = match selector {
        TraceFocusSelector::FailingNode => diagnosis.first_failing_node.as_ref().and_then(|node| {
            report
                .nodes
                .iter()
                .find(|candidate| candidate.node_id == node.node_id)
                .cloned()
        }),
    };
    let (payload_availability, payload_availability_reason, mut notes) =
        classify_focus_payload_availability(resolved_node.as_ref());
    if resolved_node.is_none() {
        notes.push("The requested focus target is not present in this trace.".to_string());
    }

    TraceFocusReport {
        selector,
        resolved_node,
        payload_availability,
        payload_availability_reason,
        notes,
    }
}

pub async fn list_recent_traces(config: &RuntimeConfig, limit: u32) -> Result<Vec<TraceSummary>> {
    let pool = db::connect(config).await?;
    verify_schema(&pool, config).await?;

    let rows = sqlx::query(
        r#"
        WITH execution_rollup AS (
            SELECT
                trace_id,
                COUNT(*)::INT AS execution_count,
                MIN(created_at) AS first_seen_at,
                MAX(COALESCE(completed_at, updated_at, created_at)) AS last_seen_at,
                (ARRAY_AGG(execution_id ORDER BY COALESCE(completed_at, updated_at, created_at) DESC, execution_id DESC))[1] AS latest_execution_id,
                (ARRAY_AGG(trigger_kind ORDER BY COALESCE(completed_at, updated_at, created_at) DESC, execution_id DESC))[1] AS latest_trigger_kind,
                (ARRAY_AGG(status ORDER BY COALESCE(completed_at, updated_at, created_at) DESC, execution_id DESC))[1] AS latest_status
            FROM execution_records
            GROUP BY trace_id
        ),
        audit_rollup AS (
            SELECT trace_id, COUNT(*)::INT AS audit_event_count
            FROM audit_events
            GROUP BY trace_id
        )
        SELECT
            execution_rollup.trace_id,
            latest_execution_id,
            latest_trigger_kind,
            latest_status,
            first_seen_at,
            last_seen_at,
            execution_count,
            COALESCE(audit_event_count, 0)::INT AS audit_event_count
        FROM execution_rollup
        LEFT JOIN audit_rollup USING (trace_id)
        ORDER BY last_seen_at DESC, trace_id DESC
        LIMIT $1
        "#,
    )
    .bind(i64::from(limit))
    .fetch_all(&pool)
    .await
    .context("failed to list recent traces for management CLI")?;

    Ok(rows
        .into_iter()
        .map(|row| TraceSummary {
            trace_id: row.get("trace_id"),
            latest_execution_id: row.get("latest_execution_id"),
            latest_trigger_kind: row.get("latest_trigger_kind"),
            latest_status: row.get("latest_status"),
            first_seen_at: row.get("first_seen_at"),
            last_seen_at: row.get("last_seen_at"),
            execution_count: row.get::<i32, _>("execution_count") as u32,
            audit_event_count: row.get::<i32, _>("audit_event_count") as u32,
        })
        .collect())
}

pub async fn clear_expired_model_call_payloads(config: &RuntimeConfig) -> Result<u64> {
    let pool = db::connect(config).await?;
    verify_schema(&pool, config).await?;
    model_calls::clear_expired_model_call_payloads(&pool, Utc::now()).await
}

pub async fn list_workspace_artifact_summaries(
    config: &RuntimeConfig,
    limit: u32,
) -> Result<Vec<contracts::WorkspaceArtifactSummary>> {
    let pool = db::connect(config).await?;
    workspace::list_workspace_artifact_summaries(&pool, i64::from(limit)).await
}

pub async fn list_workspace_scripts(
    config: &RuntimeConfig,
    limit: u32,
) -> Result<Vec<contracts::WorkspaceScriptSummary>> {
    let pool = db::connect(config).await?;
    workspace::list_workspace_scripts(&pool, i64::from(limit)).await
}

pub async fn list_workspace_script_runs(
    config: &RuntimeConfig,
    workspace_script_id: Option<Uuid>,
    limit: u32,
) -> Result<Vec<WorkspaceScriptRunSummary>> {
    let pool = db::connect(config).await?;
    workspace::list_workspace_script_run_records(&pool, workspace_script_id, i64::from(limit))
        .await?
        .into_iter()
        .map(|record| Ok(workspace_script_run_summary(&record)))
        .collect()
}

struct TraceReportBuilder {
    trace_id: Uuid,
    root_execution_id: Option<Uuid>,
    nodes: Vec<TraceNode>,
    edges: Vec<TraceEdge>,
    scheduling: Vec<SchedulingTraceSummary>,
    notes: Vec<TraceNote>,
    node_ids: BTreeSet<String>,
    edge_ids: BTreeSet<String>,
    execution_ids: BTreeSet<Uuid>,
    episode_ids: BTreeSet<Uuid>,
}

impl TraceReportBuilder {
    fn new(trace_id: Uuid, root_execution_id: Option<Uuid>) -> Self {
        Self {
            trace_id,
            root_execution_id,
            nodes: Vec::new(),
            edges: Vec::new(),
            scheduling: Vec::new(),
            notes: Vec::new(),
            node_ids: BTreeSet::new(),
            edge_ids: BTreeSet::new(),
            execution_ids: BTreeSet::new(),
            episode_ids: BTreeSet::new(),
        }
    }

    fn add_node(&mut self, node: TraceNode) {
        if node.node_kind == "execution" {
            self.execution_ids.insert(node.source_id);
        }
        if node.node_kind == "episode" {
            self.episode_ids.insert(node.source_id);
        }
        if self.node_ids.insert(node.node_id.clone()) {
            self.nodes.push(node);
        }
    }

    fn add_edge(&mut self, edge: TraceEdge) {
        let edge_id = format!(
            "{}|{}|{}",
            edge.source_node_id, edge.target_node_id, edge.edge_kind
        );
        if self.edge_ids.insert(edge_id) {
            self.edges.push(edge);
        } else if edge.inference == "explicit" {
            if let Some(existing) = self.edges.iter_mut().find(|existing| {
                existing.source_node_id == edge.source_node_id
                    && existing.target_node_id == edge.target_node_id
                    && existing.edge_kind == edge.edge_kind
            }) {
                *existing = edge;
            }
        }
    }

    fn add_note(&mut self, note_kind: impl Into<String>, message: impl Into<String>) {
        self.notes.push(TraceNote {
            note_kind: note_kind.into(),
            message: message.into(),
        });
    }

    fn add_scheduling_summary(&mut self, summary: SchedulingTraceSummary) {
        self.scheduling.push(summary);
    }

    fn finish(mut self) -> TraceReport {
        self.nodes.sort_by(|left, right| {
            left.occurred_at
                .cmp(&right.occurred_at)
                .then_with(|| left.node_kind.cmp(&right.node_kind))
                .then_with(|| left.node_id.cmp(&right.node_id))
        });
        self.edges.sort_by(|left, right| {
            left.occurred_at
                .cmp(&right.occurred_at)
                .then_with(|| left.edge_kind.cmp(&right.edge_kind))
                .then_with(|| left.source_node_id.cmp(&right.source_node_id))
                .then_with(|| left.target_node_id.cmp(&right.target_node_id))
        });
        if self.nodes.is_empty() {
            self.add_note(
                "empty_trace",
                "No durable records were found for this trace identifier.",
            );
        }
        TraceReport {
            trace_id: self.trace_id,
            root_execution_id: self.root_execution_id,
            generated_at: Utc::now(),
            node_count: self.nodes.len(),
            edge_count: self.edges.len(),
            nodes: self.nodes,
            edges: self.edges,
            scheduling: self.scheduling,
            notes: self.notes,
        }
    }
}

async fn resolve_trace_lookup(pool: &PgPool, request: &TraceLookupRequest) -> Result<Uuid> {
    match (request.trace_id, request.execution_id) {
        (Some(trace_id), Some(execution_id)) => {
            let record_trace_id = sqlx::query_scalar::<_, Uuid>(
                "SELECT trace_id FROM execution_records WHERE execution_id = $1",
            )
            .bind(execution_id)
            .fetch_optional(pool)
            .await
            .context("failed to resolve execution trace for trace lookup")?;
            match record_trace_id {
                Some(record_trace_id) if record_trace_id == trace_id => Ok(trace_id),
                Some(record_trace_id) => bail!(
                    "execution_id {execution_id} belongs to trace_id {record_trace_id}, not {trace_id}"
                ),
                None => bail!("execution_id {execution_id} was not found"),
            }
        }
        (Some(trace_id), None) => Ok(trace_id),
        (None, Some(execution_id)) => sqlx::query_scalar::<_, Uuid>(
            "SELECT trace_id FROM execution_records WHERE execution_id = $1",
        )
        .bind(execution_id)
        .fetch_optional(pool)
        .await
        .context("failed to resolve trace from execution_id")?
        .with_context(|| format!("execution_id {execution_id} was not found")),
        (None, None) => bail!("either trace_id or execution_id is required"),
    }
}

async fn load_trace_execution_nodes(
    pool: &PgPool,
    trace_id: Uuid,
    builder: &mut TraceReportBuilder,
) -> Result<()> {
    let rows = sqlx::query(
        r#"
        SELECT execution_id, trigger_kind, synthetic_trigger, status, worker_kind, worker_pid,
               request_payload, response_payload, created_at, updated_at, completed_at
        FROM execution_records
        WHERE trace_id = $1
        ORDER BY created_at ASC, execution_id ASC
        "#,
    )
    .bind(trace_id)
    .fetch_all(pool)
    .await
    .context("failed to load execution records for trace")?;

    for row in rows {
        let execution_id: Uuid = row.get("execution_id");
        let trigger_kind: String = row.get("trigger_kind");
        let status: String = row.get("status");
        let worker_kind: Option<String> = row.get("worker_kind");
        let completed_at: Option<DateTime<Utc>> = row.get("completed_at");
        let created_at: DateTime<Utc> = row.get("created_at");
        builder.add_node(TraceNode {
            node_id: trace_node_id("execution", execution_id),
            node_kind: "execution".to_string(),
            source_id: execution_id,
            occurred_at: created_at,
            status: Some(status.clone()),
            title: format!("Execution {trigger_kind}"),
            summary: format!(
                "status={status} worker={} completed_at={}",
                worker_kind.as_deref().unwrap_or("none"),
                completed_at
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "none".to_string())
            ),
            payload: json!({
                "trigger_kind": trigger_kind,
                "synthetic_trigger": row.get::<Option<String>, _>("synthetic_trigger"),
                "worker_kind": worker_kind,
                "worker_pid": row.get::<Option<i32>, _>("worker_pid"),
                "request_payload": row.get::<JsonValue, _>("request_payload"),
                "response_payload": row.get::<Option<JsonValue>, _>("response_payload"),
                "updated_at": row.get::<DateTime<Utc>, _>("updated_at"),
                "completed_at": completed_at,
            }),
            related_ids: BTreeMap::new(),
        });
    }

    Ok(())
}

async fn load_trace_ingress_nodes(
    pool: &PgPool,
    trace_id: Uuid,
    builder: &mut TraceReportBuilder,
) -> Result<()> {
    let rows = sqlx::query(
        r#"
        SELECT ingress_id, execution_id, channel_kind, external_user_id, external_conversation_id,
               external_event_id, external_message_id, internal_principal_ref,
               internal_conversation_ref, event_kind, occurred_at, received_at, status,
               rejection_reason, text_body, reply_to_external_message_id, attachment_count,
               command_name, raw_payload_ref
        FROM ingress_events
        WHERE trace_id = $1
        ORDER BY occurred_at ASC, ingress_id ASC
        "#,
    )
    .bind(trace_id)
    .fetch_all(pool)
    .await
    .context("failed to load ingress events for trace")?;

    for row in rows {
        let ingress_id: Uuid = row.get("ingress_id");
        let execution_id: Option<Uuid> = row.get("execution_id");
        let event_kind: String = row.get("event_kind");
        let channel_kind: String = row.get("channel_kind");
        let status: String = row.get("status");
        let occurred_at: DateTime<Utc> = row.get("occurred_at");
        let text_body: Option<String> = row.get("text_body");
        let mut related_ids = BTreeMap::new();
        if let Some(execution_id) = execution_id {
            related_ids.insert("execution_id".to_string(), execution_id);
        }

        builder.add_node(TraceNode {
            node_id: trace_node_id("ingress", ingress_id),
            node_kind: "ingress".to_string(),
            source_id: ingress_id,
            occurred_at,
            status: Some(status.clone()),
            title: format!("{channel_kind} {event_kind}"),
            summary: text_body
                .as_deref()
                .map(short_trace_text)
                .unwrap_or_else(|| format!("status={status}")),
            payload: json!({
                "channel_kind": channel_kind,
                "external_user_id": row.get::<String, _>("external_user_id"),
                "external_conversation_id": row.get::<String, _>("external_conversation_id"),
                "external_event_id": row.get::<String, _>("external_event_id"),
                "external_message_id": row.get::<Option<String>, _>("external_message_id"),
                "internal_principal_ref": row.get::<Option<String>, _>("internal_principal_ref"),
                "internal_conversation_ref": row.get::<Option<String>, _>("internal_conversation_ref"),
                "event_kind": event_kind,
                "received_at": row.get::<DateTime<Utc>, _>("received_at"),
                "rejection_reason": row.get::<Option<String>, _>("rejection_reason"),
                "text_body": text_body,
                "reply_to_external_message_id": row.get::<Option<String>, _>("reply_to_external_message_id"),
                "attachment_count": row.get::<i32, _>("attachment_count"),
                "command_name": row.get::<Option<String>, _>("command_name"),
                "raw_payload_ref": row.get::<Option<String>, _>("raw_payload_ref"),
            }),
            related_ids,
        });

        if let Some(execution_id) = execution_id {
            builder.add_edge(TraceEdge {
                source_node_id: trace_node_id("ingress", ingress_id),
                target_node_id: trace_node_id("execution", execution_id),
                edge_kind: "triggered_execution".to_string(),
                occurred_at,
                detail: Some("ingress_events.execution_id".to_string()),
                inference: "inferred".to_string(),
            });
        }
    }

    load_trace_execution_ingress_edges(pool, builder).await
}

async fn load_trace_execution_ingress_edges(
    pool: &PgPool,
    builder: &mut TraceReportBuilder,
) -> Result<()> {
    if builder.execution_ids.is_empty() {
        return Ok(());
    }
    let execution_ids: Vec<Uuid> = builder.execution_ids.iter().copied().collect();
    let rows = sqlx::query(
        r#"
        SELECT execution_id, ingress_id, link_role, created_at
        FROM execution_ingress_links
        WHERE execution_id = ANY($1)
        ORDER BY created_at ASC, sequence_index ASC
        "#,
    )
    .bind(&execution_ids)
    .fetch_all(pool)
    .await
    .context("failed to load execution ingress links for trace")?;

    for row in rows {
        let ingress_id: Uuid = row.get("ingress_id");
        let execution_id: Uuid = row.get("execution_id");
        builder.add_edge(TraceEdge {
            source_node_id: trace_node_id("ingress", ingress_id),
            target_node_id: trace_node_id("execution", execution_id),
            edge_kind: "linked_to_execution".to_string(),
            occurred_at: row.get("created_at"),
            detail: Some(row.get("link_role")),
            inference: "inferred".to_string(),
        });
    }

    Ok(())
}

async fn load_trace_episode_nodes(
    pool: &PgPool,
    trace_id: Uuid,
    builder: &mut TraceReportBuilder,
) -> Result<()> {
    let rows = sqlx::query(
        r#"
        SELECT episode_id, execution_id, ingress_id, internal_principal_ref,
               internal_conversation_ref, trigger_kind, trigger_source, status,
               started_at, completed_at, outcome, summary
        FROM episodes
        WHERE trace_id = $1
        ORDER BY started_at ASC, episode_id ASC
        "#,
    )
    .bind(trace_id)
    .fetch_all(pool)
    .await
    .context("failed to load episodes for trace")?;

    for row in rows {
        let episode_id: Uuid = row.get("episode_id");
        let execution_id: Uuid = row.get("execution_id");
        let ingress_id: Option<Uuid> = row.get("ingress_id");
        let status: String = row.get("status");
        let started_at: DateTime<Utc> = row.get("started_at");
        let summary: Option<String> = row.get("summary");
        let mut related_ids = BTreeMap::new();
        related_ids.insert("execution_id".to_string(), execution_id);
        if let Some(ingress_id) = ingress_id {
            related_ids.insert("ingress_id".to_string(), ingress_id);
        }

        builder.add_node(TraceNode {
            node_id: trace_node_id("episode", episode_id),
            node_kind: "episode".to_string(),
            source_id: episode_id,
            occurred_at: started_at,
            status: Some(status.clone()),
            title: "Foreground episode".to_string(),
            summary: summary
                .as_deref()
                .map(short_trace_text)
                .unwrap_or_else(|| format!("status={status}")),
            payload: json!({
                "execution_id": execution_id,
                "ingress_id": ingress_id,
                "internal_principal_ref": row.get::<String, _>("internal_principal_ref"),
                "internal_conversation_ref": row.get::<String, _>("internal_conversation_ref"),
                "trigger_kind": row.get::<String, _>("trigger_kind"),
                "trigger_source": row.get::<String, _>("trigger_source"),
                "completed_at": row.get::<Option<DateTime<Utc>>, _>("completed_at"),
                "outcome": row.get::<Option<String>, _>("outcome"),
                "summary": summary,
            }),
            related_ids,
        });
        builder.add_edge(TraceEdge {
            source_node_id: trace_node_id("execution", execution_id),
            target_node_id: trace_node_id("episode", episode_id),
            edge_kind: "opened_episode".to_string(),
            occurred_at: started_at,
            detail: None,
            inference: "inferred".to_string(),
        });
        if let Some(ingress_id) = ingress_id {
            builder.add_edge(TraceEdge {
                source_node_id: trace_node_id("ingress", ingress_id),
                target_node_id: trace_node_id("episode", episode_id),
                edge_kind: "opened_episode".to_string(),
                occurred_at: started_at,
                detail: None,
                inference: "inferred".to_string(),
            });
        }
    }

    load_trace_episode_message_nodes(pool, trace_id, builder).await
}

async fn load_trace_episode_message_nodes(
    pool: &PgPool,
    trace_id: Uuid,
    builder: &mut TraceReportBuilder,
) -> Result<()> {
    let rows = sqlx::query(
        r#"
        SELECT episode_message_id, episode_id, execution_id, message_order, message_role,
               channel_kind, text_body, external_message_id, created_at
        FROM episode_messages
        WHERE trace_id = $1
        ORDER BY created_at ASC, message_order ASC
        "#,
    )
    .bind(trace_id)
    .fetch_all(pool)
    .await
    .context("failed to load episode messages for trace")?;

    for row in rows {
        let episode_message_id: Uuid = row.get("episode_message_id");
        let episode_id: Uuid = row.get("episode_id");
        let execution_id: Uuid = row.get("execution_id");
        let message_role: String = row.get("message_role");
        let created_at: DateTime<Utc> = row.get("created_at");
        let text_body: Option<String> = row.get("text_body");
        let mut related_ids = BTreeMap::new();
        related_ids.insert("episode_id".to_string(), episode_id);
        related_ids.insert("execution_id".to_string(), execution_id);

        builder.add_node(TraceNode {
            node_id: trace_node_id("episode_message", episode_message_id),
            node_kind: "episode_message".to_string(),
            source_id: episode_message_id,
            occurred_at: created_at,
            status: None,
            title: format!("Episode message {message_role}"),
            summary: text_body
                .as_deref()
                .map(short_trace_text)
                .unwrap_or_else(|| "no text body".to_string()),
            payload: json!({
                "episode_id": episode_id,
                "execution_id": execution_id,
                "message_order": row.get::<i32, _>("message_order"),
                "message_role": message_role,
                "channel_kind": row.get::<String, _>("channel_kind"),
                "text_body": text_body,
                "external_message_id": row.get::<Option<String>, _>("external_message_id"),
            }),
            related_ids,
        });
        builder.add_edge(TraceEdge {
            source_node_id: trace_node_id("episode", episode_id),
            target_node_id: trace_node_id("episode_message", episode_message_id),
            edge_kind: "contains_message".to_string(),
            occurred_at: created_at,
            detail: Some(row.get::<i32, _>("message_order").to_string()),
            inference: "inferred".to_string(),
        });
    }

    Ok(())
}

async fn load_trace_audit_nodes(
    pool: &PgPool,
    trace_id: Uuid,
    builder: &mut TraceReportBuilder,
) -> Result<()> {
    let rows = sqlx::query(
        r#"
        SELECT event_id, occurred_at, loop_kind, subsystem, event_kind, severity,
               execution_id, model_tier, payload
        FROM audit_events
        WHERE trace_id = $1
        ORDER BY occurred_at ASC, event_id ASC
        "#,
    )
    .bind(trace_id)
    .fetch_all(pool)
    .await
    .context("failed to load audit events for trace")?;

    for row in rows {
        let event_id: Uuid = row.get("event_id");
        let occurred_at: DateTime<Utc> = row.get("occurred_at");
        let subsystem: String = row.get("subsystem");
        let event_kind: String = row.get("event_kind");
        let severity: String = row.get("severity");
        let execution_id: Option<Uuid> = row.get("execution_id");
        let payload: JsonValue = row.get("payload");
        let mut related_ids = BTreeMap::new();
        if let Some(execution_id) = execution_id {
            related_ids.insert("execution_id".to_string(), execution_id);
        }
        builder.add_node(TraceNode {
            node_id: trace_node_id("audit_event", event_id),
            node_kind: "audit_event".to_string(),
            source_id: event_id,
            occurred_at,
            status: Some(severity.clone()),
            title: format!("{subsystem}.{event_kind}"),
            summary: audit_payload_summary(&payload).unwrap_or_else(|| severity.clone()),
            payload: json!({
                "loop_kind": row.get::<String, _>("loop_kind"),
                "subsystem": subsystem,
                "event_kind": event_kind,
                "severity": severity,
                "execution_id": execution_id,
                "model_tier": row.get::<Option<String>, _>("model_tier"),
                "payload": payload,
            }),
            related_ids,
        });
        if let Some(execution_id) = execution_id {
            builder.add_edge(TraceEdge {
                source_node_id: trace_node_id("execution", execution_id),
                target_node_id: trace_node_id("audit_event", event_id),
                edge_kind: "emitted_audit_event".to_string(),
                occurred_at,
                detail: None,
                inference: "inferred".to_string(),
            });
        }
    }

    Ok(())
}

async fn load_trace_model_call_nodes(
    pool: &PgPool,
    trace_id: Uuid,
    builder: &mut TraceReportBuilder,
) -> Result<()> {
    let records = model_calls::list_model_call_records_for_trace(pool, trace_id).await?;
    for record in records {
        let mut related_ids = BTreeMap::new();
        if let Some(execution_id) = record.execution_id {
            related_ids.insert("execution_id".to_string(), execution_id);
        }
        builder.add_node(TraceNode {
            node_id: trace_node_id("model_call", record.model_call_id),
            node_kind: "model_call".to_string(),
            source_id: record.model_call_id,
            occurred_at: record.started_at,
            status: Some(record.status.clone()),
            title: format!("Model call {}", record.purpose),
            summary: format!(
                "provider={} model={} task_class={} input_tokens={} output_tokens={} finish_reason={}",
                record.provider,
                record.model,
                record.task_class.as_deref().unwrap_or("none"),
                record
                    .input_tokens
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
                record
                    .output_tokens
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
                record.finish_reason.as_deref().unwrap_or("none")
            ),
            payload: json!({
                "trace_id": record.trace_id,
                "execution_id": record.execution_id,
                "loop_kind": record.loop_kind,
                "purpose": record.purpose,
                "task_class": record.task_class,
                "provider": record.provider,
                "model": record.model,
                "request_payload_json": record.request_payload_json,
                "response_payload_json": record.response_payload_json,
                "system_prompt_text": record.system_prompt_text,
                "messages_json": record.messages_json,
                "input_tokens": record.input_tokens,
                "output_tokens": record.output_tokens,
                "finish_reason": record.finish_reason,
                "error_summary": record.error_summary,
                "completed_at": record.completed_at,
                "payload_retention_expires_at": record.payload_retention_expires_at,
                "payload_cleared_at": record.payload_cleared_at,
                "payload_retention_reason": record.payload_retention_reason,
            }),
            related_ids,
        });
        if let Some(execution_id) = record.execution_id {
            builder.add_edge(TraceEdge {
                source_node_id: trace_node_id("execution", execution_id),
                target_node_id: trace_node_id("model_call", record.model_call_id),
                edge_kind: "invoked_model".to_string(),
                occurred_at: record.started_at,
                detail: Some(record.purpose),
                inference: "inferred".to_string(),
            });
        }
    }

    Ok(())
}

async fn load_trace_background_nodes(
    pool: &PgPool,
    trace_id: Uuid,
    builder: &mut TraceReportBuilder,
) -> Result<()> {
    let jobs = sqlx::query(
        r#"
        SELECT background_job_id, job_kind, trigger_kind, trigger_reason_summary,
               status, created_at, updated_at
        FROM background_jobs
        WHERE trace_id = $1
        ORDER BY created_at ASC, background_job_id ASC
        "#,
    )
    .bind(trace_id)
    .fetch_all(pool)
    .await
    .context("failed to load background jobs for trace")?;

    for row in jobs {
        let background_job_id: Uuid = row.get("background_job_id");
        let job_kind: String = row.get("job_kind");
        let status: String = row.get("status");
        let created_at: DateTime<Utc> = row.get("created_at");
        builder.add_node(TraceNode {
            node_id: trace_node_id("background_job", background_job_id),
            node_kind: "background_job".to_string(),
            source_id: background_job_id,
            occurred_at: created_at,
            status: Some(status.clone()),
            title: format!("Background job {job_kind}"),
            summary: row.get("trigger_reason_summary"),
            payload: json!({
                "job_kind": job_kind,
                "trigger_kind": row.get::<String, _>("trigger_kind"),
                "status": status,
                "updated_at": row.get::<DateTime<Utc>, _>("updated_at"),
            }),
            related_ids: BTreeMap::new(),
        });
    }

    let runs = sqlx::query(
        r#"
        SELECT background_job_run_id, background_job_id, execution_id, status,
               started_at, completed_at, result_payload, failure_payload, created_at
        FROM background_job_runs
        WHERE trace_id = $1
        ORDER BY created_at ASC, background_job_run_id ASC
        "#,
    )
    .bind(trace_id)
    .fetch_all(pool)
    .await
    .context("failed to load background job runs for trace")?;

    for row in runs {
        let background_job_run_id: Uuid = row.get("background_job_run_id");
        let background_job_id: Uuid = row.get("background_job_id");
        let execution_id: Option<Uuid> = row.get("execution_id");
        let status: String = row.get("status");
        let created_at: DateTime<Utc> = row.get("created_at");
        let mut related_ids = BTreeMap::new();
        related_ids.insert("background_job_id".to_string(), background_job_id);
        if let Some(execution_id) = execution_id {
            related_ids.insert("execution_id".to_string(), execution_id);
        }
        builder.add_node(TraceNode {
            node_id: trace_node_id("background_job_run", background_job_run_id),
            node_kind: "background_job_run".to_string(),
            source_id: background_job_run_id,
            occurred_at: created_at,
            status: Some(status.clone()),
            title: "Background job run".to_string(),
            summary: format!("status={status}"),
            payload: json!({
                "background_job_id": background_job_id,
                "execution_id": execution_id,
                "started_at": row.get::<Option<DateTime<Utc>>, _>("started_at"),
                "completed_at": row.get::<Option<DateTime<Utc>>, _>("completed_at"),
                "result_payload": row.get::<Option<JsonValue>, _>("result_payload"),
                "failure_payload": row.get::<Option<JsonValue>, _>("failure_payload"),
            }),
            related_ids,
        });
        builder.add_edge(TraceEdge {
            source_node_id: trace_node_id("background_job", background_job_id),
            target_node_id: trace_node_id("background_job_run", background_job_run_id),
            edge_kind: "started_run".to_string(),
            occurred_at: created_at,
            detail: None,
            inference: "inferred".to_string(),
        });
        if let Some(execution_id) = execution_id {
            builder.add_edge(TraceEdge {
                source_node_id: trace_node_id("background_job_run", background_job_run_id),
                target_node_id: trace_node_id("execution", execution_id),
                edge_kind: "used_execution".to_string(),
                occurred_at: created_at,
                detail: None,
                inference: "inferred".to_string(),
            });
        }
    }

    Ok(())
}

async fn load_trace_wake_signal_nodes(
    pool: &PgPool,
    trace_id: Uuid,
    builder: &mut TraceReportBuilder,
) -> Result<()> {
    let rows = sqlx::query(
        r#"
        SELECT wake_signal_id, background_job_id, background_job_run_id, execution_id,
               reason, priority, reason_code, summary, payload_ref, status,
               decision_kind, decision_reason, requested_at, reviewed_at
        FROM wake_signals
        WHERE trace_id = $1
        ORDER BY requested_at ASC, wake_signal_id ASC
        "#,
    )
    .bind(trace_id)
    .fetch_all(pool)
    .await
    .context("failed to load wake signals for trace")?;

    for row in rows {
        let wake_signal_id: Uuid = row.get("wake_signal_id");
        let background_job_id: Uuid = row.get("background_job_id");
        let background_job_run_id: Option<Uuid> = row.get("background_job_run_id");
        let execution_id: Option<Uuid> = row.get("execution_id");
        let status: String = row.get("status");
        let requested_at: DateTime<Utc> = row.get("requested_at");
        let mut related_ids = BTreeMap::new();
        related_ids.insert("background_job_id".to_string(), background_job_id);
        if let Some(background_job_run_id) = background_job_run_id {
            related_ids.insert("background_job_run_id".to_string(), background_job_run_id);
        }
        if let Some(execution_id) = execution_id {
            related_ids.insert("execution_id".to_string(), execution_id);
        }
        builder.add_node(TraceNode {
            node_id: trace_node_id("wake_signal", wake_signal_id),
            node_kind: "wake_signal".to_string(),
            source_id: wake_signal_id,
            occurred_at: requested_at,
            status: Some(status),
            title: format!("Wake signal {}", row.get::<String, _>("reason_code")),
            summary: row.get("summary"),
            payload: json!({
                "background_job_id": background_job_id,
                "background_job_run_id": background_job_run_id,
                "execution_id": execution_id,
                "reason": row.get::<String, _>("reason"),
                "priority": row.get::<String, _>("priority"),
                "reason_code": row.get::<String, _>("reason_code"),
                "payload_ref": row.get::<Option<String>, _>("payload_ref"),
                "decision_kind": row.get::<Option<String>, _>("decision_kind"),
                "decision_reason": row.get::<Option<String>, _>("decision_reason"),
                "reviewed_at": row.get::<Option<DateTime<Utc>>, _>("reviewed_at"),
            }),
            related_ids,
        });
        if let Some(background_job_run_id) = background_job_run_id {
            builder.add_edge(TraceEdge {
                source_node_id: trace_node_id("background_job_run", background_job_run_id),
                target_node_id: trace_node_id("wake_signal", wake_signal_id),
                edge_kind: "recorded_wake_signal".to_string(),
                occurred_at: requested_at,
                detail: None,
                inference: "inferred".to_string(),
            });
        }
    }

    Ok(())
}

async fn load_trace_approval_nodes(
    pool: &PgPool,
    trace_id: Uuid,
    builder: &mut TraceReportBuilder,
) -> Result<()> {
    let rows = sqlx::query(
        r#"
        SELECT approval_request_id, execution_id, action_proposal_id, action_fingerprint,
               action_kind, risk_tier, title, consequence_summary, status, requested_by,
               requested_at, expires_at, resolved_at, resolution_kind, resolved_by,
               resolution_reason
        FROM approval_requests
        WHERE trace_id = $1
        ORDER BY requested_at ASC, approval_request_id ASC
        "#,
    )
    .bind(trace_id)
    .fetch_all(pool)
    .await
    .context("failed to load approval requests for trace")?;

    for row in rows {
        let approval_request_id: Uuid = row.get("approval_request_id");
        let execution_id: Option<Uuid> = row.get("execution_id");
        let status: String = row.get("status");
        let requested_at: DateTime<Utc> = row.get("requested_at");
        let mut related_ids = BTreeMap::new();
        if let Some(execution_id) = execution_id {
            related_ids.insert("execution_id".to_string(), execution_id);
        }
        builder.add_node(TraceNode {
            node_id: trace_node_id("approval_request", approval_request_id),
            node_kind: "approval_request".to_string(),
            source_id: approval_request_id,
            occurred_at: requested_at,
            status: Some(status),
            title: row.get("title"),
            summary: row.get("consequence_summary"),
            payload: json!({
                "execution_id": execution_id,
                "action_proposal_id": row.get::<Uuid, _>("action_proposal_id"),
                "action_fingerprint": row.get::<String, _>("action_fingerprint"),
                "action_kind": row.get::<String, _>("action_kind"),
                "risk_tier": row.get::<String, _>("risk_tier"),
                "requested_by": row.get::<String, _>("requested_by"),
                "expires_at": row.get::<DateTime<Utc>, _>("expires_at"),
                "resolved_at": row.get::<Option<DateTime<Utc>>, _>("resolved_at"),
                "resolution_kind": row.get::<Option<String>, _>("resolution_kind"),
                "resolved_by": row.get::<Option<String>, _>("resolved_by"),
                "resolution_reason": row.get::<Option<String>, _>("resolution_reason"),
            }),
            related_ids,
        });
        if let Some(execution_id) = execution_id {
            builder.add_edge(TraceEdge {
                source_node_id: trace_node_id("execution", execution_id),
                target_node_id: trace_node_id("approval_request", approval_request_id),
                edge_kind: "requested_approval".to_string(),
                occurred_at: requested_at,
                detail: None,
                inference: "inferred".to_string(),
            });
        }
    }

    Ok(())
}

async fn load_trace_governed_action_nodes(
    pool: &PgPool,
    trace_id: Uuid,
    builder: &mut TraceReportBuilder,
) -> Result<()> {
    let rows = sqlx::query(
        r#"
        SELECT governed_action_execution_id, execution_id, approval_request_id,
               action_proposal_id, action_fingerprint, action_kind, risk_tier,
               status, payload_json, blocked_reason, output_ref, started_at,
               completed_at, created_at
        FROM governed_action_executions
        WHERE trace_id = $1
        ORDER BY created_at ASC, governed_action_execution_id ASC
        "#,
    )
    .bind(trace_id)
    .fetch_all(pool)
    .await
    .context("failed to load governed actions for trace")?;

    for row in rows {
        let governed_action_execution_id: Uuid = row.get("governed_action_execution_id");
        let execution_id: Option<Uuid> = row.get("execution_id");
        let approval_request_id: Option<Uuid> = row.get("approval_request_id");
        let action_kind: String = row.get("action_kind");
        let status: String = row.get("status");
        let created_at: DateTime<Utc> = row.get("created_at");
        let mut related_ids = BTreeMap::new();
        if let Some(execution_id) = execution_id {
            related_ids.insert("execution_id".to_string(), execution_id);
        }
        if let Some(approval_request_id) = approval_request_id {
            related_ids.insert("approval_request_id".to_string(), approval_request_id);
        }
        builder.add_node(TraceNode {
            node_id: trace_node_id("governed_action", governed_action_execution_id),
            node_kind: "governed_action".to_string(),
            source_id: governed_action_execution_id,
            occurred_at: created_at,
            status: Some(status.clone()),
            title: format!("Governed action {action_kind}"),
            summary: row
                .get::<Option<String>, _>("blocked_reason")
                .unwrap_or_else(|| format!("status={status}")),
            payload: json!({
                "execution_id": execution_id,
                "approval_request_id": approval_request_id,
                "action_proposal_id": row.get::<Uuid, _>("action_proposal_id"),
                "action_fingerprint": row.get::<String, _>("action_fingerprint"),
                "action_kind": action_kind,
                "risk_tier": row.get::<String, _>("risk_tier"),
                "payload_json": row.get::<JsonValue, _>("payload_json"),
                "blocked_reason": row.get::<Option<String>, _>("blocked_reason"),
                "output_ref": row.get::<Option<String>, _>("output_ref"),
                "started_at": row.get::<Option<DateTime<Utc>>, _>("started_at"),
                "completed_at": row.get::<Option<DateTime<Utc>>, _>("completed_at"),
            }),
            related_ids,
        });
        if let Some(execution_id) = execution_id {
            builder.add_edge(TraceEdge {
                source_node_id: trace_node_id("execution", execution_id),
                target_node_id: trace_node_id("governed_action", governed_action_execution_id),
                edge_kind: "proposed_action".to_string(),
                occurred_at: created_at,
                detail: None,
                inference: "inferred".to_string(),
            });
        }
        if let Some(approval_request_id) = approval_request_id {
            builder.add_edge(TraceEdge {
                source_node_id: trace_node_id("governed_action", governed_action_execution_id),
                target_node_id: trace_node_id("approval_request", approval_request_id),
                edge_kind: "required_approval".to_string(),
                occurred_at: created_at,
                detail: None,
                inference: "inferred".to_string(),
            });
        }
    }

    Ok(())
}

async fn load_trace_scheduled_task_nodes(
    pool: &PgPool,
    trace_id: Uuid,
    builder: &mut TraceReportBuilder,
) -> Result<()> {
    let execution_ids: Vec<Uuid> = builder.execution_ids.iter().copied().collect();
    let rows = sqlx::query(
        r#"
        SELECT scheduled_foreground_task_id, task_key, channel_kind, status,
               internal_principal_ref, internal_conversation_ref, message_text,
               cadence_seconds, cooldown_seconds, next_due_at, current_execution_id,
               current_run_started_at, last_execution_id, last_run_started_at,
               last_run_completed_at, last_outcome, last_outcome_reason,
               last_outcome_summary, created_by, updated_by, created_at, updated_at
        FROM scheduled_foreground_tasks
        WHERE current_execution_id = ANY($1)
           OR last_execution_id = ANY($1)
           OR scheduled_foreground_task_id IN (
                SELECT target_id
                FROM causal_links
                WHERE trace_id = $2
                  AND target_kind = 'scheduled_foreground_task'
           )
           OR scheduled_foreground_task_id IN (
                SELECT source_id
                FROM causal_links
                WHERE trace_id = $2
                  AND source_kind = 'scheduled_foreground_task'
           )
        ORDER BY updated_at ASC, scheduled_foreground_task_id ASC
        "#,
    )
    .bind(&execution_ids)
    .bind(trace_id)
    .fetch_all(pool)
    .await
    .context("failed to load scheduled foreground tasks for trace")?;

    for row in rows {
        let scheduled_foreground_task_id: Uuid = row.get("scheduled_foreground_task_id");
        let current_execution_id: Option<Uuid> = row.get("current_execution_id");
        let last_execution_id: Option<Uuid> = row.get("last_execution_id");
        let status: String = row.get("status");
        let updated_at: DateTime<Utc> = row.get("updated_at");
        let task_key: String = row.get("task_key");
        let message_text: String = row.get("message_text");
        let cadence_seconds: i64 = row.get("cadence_seconds");
        let cooldown_seconds: i64 = row.get("cooldown_seconds");
        let next_due_at: DateTime<Utc> = row.get("next_due_at");
        let current_run_started_at: Option<DateTime<Utc>> = row.get("current_run_started_at");
        let last_run_started_at: Option<DateTime<Utc>> = row.get("last_run_started_at");
        let last_run_completed_at: Option<DateTime<Utc>> = row.get("last_run_completed_at");
        let last_outcome: Option<String> = row.get("last_outcome");
        let last_outcome_reason: Option<String> = row.get("last_outcome_reason");
        let last_outcome_summary: Option<String> = row.get("last_outcome_summary");
        let mut related_ids = BTreeMap::new();
        if let Some(current_execution_id) = current_execution_id {
            related_ids.insert("current_execution_id".to_string(), current_execution_id);
        }
        if let Some(last_execution_id) = last_execution_id {
            related_ids.insert("last_execution_id".to_string(), last_execution_id);
        }
        builder.add_node(TraceNode {
            node_id: trace_node_id("scheduled_foreground_task", scheduled_foreground_task_id),
            node_kind: "scheduled_foreground_task".to_string(),
            source_id: scheduled_foreground_task_id,
            occurred_at: updated_at,
            status: Some(status.clone()),
            title: format!("Scheduled task {task_key}"),
            summary: message_text.clone(),
            payload: json!({
                "task_key": task_key,
                "channel_kind": row.get::<String, _>("channel_kind"),
                "internal_principal_ref": row.get::<String, _>("internal_principal_ref"),
                "internal_conversation_ref": row.get::<String, _>("internal_conversation_ref"),
                "cadence_seconds": cadence_seconds,
                "cooldown_seconds": cooldown_seconds,
                "next_due_at": next_due_at,
                "current_execution_id": current_execution_id,
                "current_run_started_at": current_run_started_at,
                "last_execution_id": last_execution_id,
                "last_run_started_at": last_run_started_at,
                "last_run_completed_at": last_run_completed_at,
                "last_outcome": last_outcome,
                "last_outcome_reason": last_outcome_reason,
                "last_outcome_summary": last_outcome_summary,
                "created_by": row.get::<String, _>("created_by"),
                "updated_by": row.get::<String, _>("updated_by"),
                "created_at": row.get::<DateTime<Utc>, _>("created_at"),
            }),
            related_ids,
        });
        builder.add_scheduling_summary(SchedulingTraceSummary {
            scheduled_foreground_task_id,
            task_key,
            status,
            message_text,
            cadence_seconds,
            cooldown_seconds,
            next_due_at,
            current_execution_id,
            current_run_started_at,
            last_execution_id,
            last_run_started_at,
            last_run_completed_at,
            last_outcome,
            last_outcome_reason,
            last_outcome_summary,
        });
        for (edge_kind, execution_id) in [
            ("current_scheduled_execution", current_execution_id),
            ("last_scheduled_execution", last_execution_id),
        ] {
            if let Some(execution_id) = execution_id {
                builder.add_edge(TraceEdge {
                    source_node_id: trace_node_id(
                        "scheduled_foreground_task",
                        scheduled_foreground_task_id,
                    ),
                    target_node_id: trace_node_id("execution", execution_id),
                    edge_kind: edge_kind.to_string(),
                    occurred_at: updated_at,
                    detail: None,
                    inference: "inferred".to_string(),
                });
            }
        }
    }

    Ok(())
}

async fn load_trace_explicit_causal_links(
    pool: &PgPool,
    trace_id: Uuid,
    builder: &mut TraceReportBuilder,
) -> Result<()> {
    for link in causal_links::list_for_trace(pool, trace_id).await? {
        let source_node_id = match trace_node_kind_for_causal_kind(&link.source_kind) {
            Some(kind) => trace_node_id(kind, link.source_id),
            None => {
                builder.add_note(
                    "unknown_causal_source_kind",
                    format!(
                        "causal link {} used unknown source kind {}",
                        link.causal_link_id, link.source_kind
                    ),
                );
                continue;
            }
        };
        let target_node_id = match trace_node_kind_for_causal_kind(&link.target_kind) {
            Some(kind) => trace_node_id(kind, link.target_id),
            None => {
                builder.add_note(
                    "unknown_causal_target_kind",
                    format!(
                        "causal link {} used unknown target kind {}",
                        link.causal_link_id, link.target_kind
                    ),
                );
                continue;
            }
        };
        builder.add_edge(TraceEdge {
            source_node_id,
            target_node_id,
            edge_kind: link.edge_kind,
            occurred_at: link.created_at,
            detail: Some(link.payload.to_string()),
            inference: "explicit".to_string(),
        });
    }

    Ok(())
}

fn trace_node_kind_for_causal_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "execution_record" => Some("execution"),
        "ingress_event" => Some("ingress"),
        "episode" => Some("episode"),
        "episode_message" => Some("episode_message"),
        "audit_event" => Some("audit_event"),
        "background_job" => Some("background_job"),
        "background_job_run" => Some("background_job_run"),
        "wake_signal" => Some("wake_signal"),
        "approval_request" => Some("approval_request"),
        "governed_action_execution" => Some("governed_action"),
        "model_call_record" => Some("model_call"),
        "scheduled_foreground_task" => Some("scheduled_foreground_task"),
        _ => None,
    }
}

fn trace_node_id(kind: &str, id: Uuid) -> String {
    format!("{kind}:{id}")
}

fn trace_node_reference(node: &TraceNode) -> TraceNodeReference {
    TraceNodeReference {
        node_id: node.node_id.clone(),
        node_kind: node.node_kind.clone(),
        source_id: node.source_id,
        occurred_at: node.occurred_at,
        status: node.status.clone(),
        title: node.title.clone(),
        summary: node.summary.clone(),
    }
}

fn find_pending_approval_node(report: &TraceReport) -> Option<&TraceNode> {
    report.nodes.iter().find(|node| {
        node.node_kind == "approval_request" && matches!(node.status.as_deref(), Some("pending"))
    })
}

fn find_blocked_node(report: &TraceReport) -> Option<&TraceNode> {
    report.nodes.iter().find(|node| {
        (node.node_kind == "governed_action_execution"
            && matches!(node.status.as_deref(), Some("blocked")))
            || (node.node_kind == "approval_request"
                && matches!(node.status.as_deref(), Some("rejected" | "expired")))
    })
}

fn find_first_failing_node(report: &TraceReport) -> Option<&TraceNode> {
    report
        .nodes
        .iter()
        .filter(|node| node_is_failure(node))
        .min_by(|left, right| {
            trace_failure_priority(left)
                .cmp(&trace_failure_priority(right))
                .then_with(|| left.occurred_at.cmp(&right.occurred_at))
                .then_with(|| left.node_id.cmp(&right.node_id))
        })
}

fn node_is_failure(node: &TraceNode) -> bool {
    match node.node_kind.as_str() {
        "audit_event" => matches!(node.status.as_deref(), Some("error" | "critical")),
        _ => matches!(
            node.status.as_deref(),
            Some("failed" | "blocked" | "rejected" | "expired" | "invalidated")
        ),
    }
}

fn node_is_nonfailing(node: &TraceNode) -> bool {
    !node_is_failure(node) && !matches!(node.status.as_deref(), Some("pending"))
}

fn trace_failure_priority(node: &TraceNode) -> u8 {
    match node.node_kind.as_str() {
        "model_call" | "governed_action_execution" | "approval_request" => 0,
        "audit_event" => 1,
        "execution" | "episode" => 2,
        _ => 3,
    }
}

fn derive_trace_verdict(
    report: &TraceReport,
    pending_approval: Option<&TraceNode>,
    blocked_node: Option<&TraceNode>,
    failure_class: Option<TraceFailureClass>,
) -> TraceDiagnosisVerdict {
    if pending_approval.is_some() {
        return TraceDiagnosisVerdict::AwaitingApproval;
    }
    if blocked_node.is_some() {
        return TraceDiagnosisVerdict::Blocked;
    }
    if failure_class.is_some() {
        return TraceDiagnosisVerdict::Failed;
    }
    if report.nodes.iter().any(|node| {
        matches!(
            node.status.as_deref(),
            Some("succeeded" | "executed" | "resolved" | "completed")
        )
    }) || report.nodes.iter().any(|node| {
        node.node_kind == "episode_message"
            && node.payload.get("message_role").and_then(JsonValue::as_str) == Some("assistant")
    }) {
        return TraceDiagnosisVerdict::Succeeded;
    }
    TraceDiagnosisVerdict::Inconclusive
}

fn classify_trace_failure(
    report: &TraceReport,
    pending_approval: Option<&TraceNode>,
    blocked_node: Option<&TraceNode>,
    first_failing_node: Option<&TraceNode>,
) -> Option<TraceFailureClass> {
    if pending_approval.is_some() {
        return Some(TraceFailureClass::ApprovalPending);
    }
    if let Some(node) = blocked_node {
        return match node.status.as_deref() {
            Some("rejected") => Some(TraceFailureClass::ApprovalRejected),
            Some("expired") => Some(TraceFailureClass::ApprovalExpired),
            _ => Some(TraceFailureClass::GovernedActionBlocked),
        };
    }
    let node = first_failing_node?;
    match node.node_kind.as_str() {
        "model_call" => {
            classify_model_call_failure(report, node).or(Some(TraceFailureClass::UnknownFailure))
        }
        "audit_event" => classify_audit_failure(node),
        "execution" | "episode" => classify_failure_from_related_audit(report, node)
            .or(Some(TraceFailureClass::UnknownFailure)),
        "governed_action_execution" => Some(TraceFailureClass::GovernedActionBlocked),
        _ => Some(TraceFailureClass::UnknownFailure),
    }
}

fn classify_model_call_failure(
    report: &TraceReport,
    node: &TraceNode,
) -> Option<TraceFailureClass> {
    if let Some(failure_class) = classify_failure_from_related_audit(report, node) {
        return Some(failure_class);
    }
    let error_summary = node
        .payload
        .get("error_summary")
        .and_then(JsonValue::as_str)
        .map(|value| value.to_ascii_lowercase());
    match error_summary.as_deref() {
        Some(summary) if summary.contains("timeout") || summary.contains("transport") => {
            Some(TraceFailureClass::ModelGatewayTransportFailure)
        }
        Some(summary) if summary.contains("status 4") || summary.contains("status 5") => {
            Some(TraceFailureClass::ProviderRejected)
        }
        Some(summary) if summary.contains("persist") || summary.contains("database") => {
            Some(TraceFailureClass::PersistenceFailure)
        }
        _ => None,
    }
}

fn classify_failure_from_related_audit(
    report: &TraceReport,
    node: &TraceNode,
) -> Option<TraceFailureClass> {
    let execution_id = node.related_ids.get("execution_id").copied().or_else(|| {
        node.payload
            .get("execution_id")
            .and_then(JsonValue::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
    });
    report
        .nodes
        .iter()
        .filter(|candidate| {
            candidate.node_kind == "audit_event"
                && candidate.occurred_at >= node.occurred_at
                && execution_id.map(|id| candidate.related_ids.get("execution_id") == Some(&id))
                    != Some(false)
        })
        .find_map(classify_audit_failure)
}

fn classify_audit_failure(node: &TraceNode) -> Option<TraceFailureClass> {
    let payload = node.payload.get("payload")?;
    if let Some(class) = classify_failure_text(&format!("{} {}", node.summary, payload)) {
        return Some(class);
    }
    let failure_kind = payload
        .get("failure_kind")
        .and_then(JsonValue::as_str)
        .or_else(|| payload.get("reason_code").and_then(JsonValue::as_str))?;
    match failure_kind {
        "model_gateway_transport_failure" => Some(TraceFailureClass::ModelGatewayTransportFailure),
        "provider_rejected" => Some(TraceFailureClass::ProviderRejected),
        "telegram_delivery_failure" => Some(TraceFailureClass::TelegramDeliveryFailure),
        "persistence_failure" => Some(TraceFailureClass::PersistenceFailure),
        "context_assembly_failure" => Some(TraceFailureClass::ContextAssemblyFailure),
        "malformed_action_proposal" => Some(TraceFailureClass::MalformedActionProposal),
        "worker_protocol_failure" => Some(TraceFailureClass::WorkerProtocolFailure),
        "scheduled_foreground_validation_failure" => {
            Some(TraceFailureClass::ScheduledForegroundValidationFailure)
        }
        "recovery_interrupted" => Some(TraceFailureClass::RecoveryInterrupted),
        _ => Some(TraceFailureClass::UnknownFailure),
    }
}

fn classify_failure_text(text: &str) -> Option<TraceFailureClass> {
    let text = text.to_ascii_lowercase();
    if text.contains("violates check constraint")
        || text.contains("failed to insert governed action execution")
        || text.contains("failed to insert scheduled foreground task")
        || text.contains("failed to update scheduled foreground task")
    {
        return Some(TraceFailureClass::PersistenceFailure);
    }
    if text.contains("scheduled foreground cadence_seconds")
        || text.contains("scheduled foreground task key")
        || text.contains("scheduled foreground message_text")
    {
        return Some(TraceFailureClass::ScheduledForegroundValidationFailure);
    }
    if text.contains("worker_protocol_phase=")
        || text.contains("failed to write worker protocol line")
        || text.contains("broken pipe")
    {
        return Some(TraceFailureClass::WorkerProtocolFailure);
    }
    None
}

fn derive_side_effect_status(report: &TraceReport) -> TraceSideEffectStatus {
    let actions = report
        .nodes
        .iter()
        .filter(|node| node.node_kind == "governed_action_execution")
        .collect::<Vec<_>>();
    if actions.is_empty() {
        return TraceSideEffectStatus::NoneExecuted;
    }
    if actions
        .iter()
        .any(|node| matches!(node.status.as_deref(), Some("executed")))
    {
        return TraceSideEffectStatus::Executed;
    }
    if actions.iter().any(|node| {
        matches!(node.status.as_deref(), Some("failed"))
            || node
                .payload
                .get("started_at")
                .is_some_and(|value| !value.is_null())
    }) {
        return TraceSideEffectStatus::Possible;
    }
    if actions.iter().all(|node| {
        matches!(
            node.status.as_deref(),
            Some(
                "proposed"
                    | "awaiting_approval"
                    | "approved"
                    | "blocked"
                    | "rejected"
                    | "expired"
                    | "invalidated"
            )
        )
    }) {
        return TraceSideEffectStatus::NoneExecuted;
    }
    TraceSideEffectStatus::Unknown
}

fn derive_user_reply_status(report: &TraceReport) -> TraceUserReplyStatus {
    if report.nodes.iter().any(|node| {
        node.node_kind == "episode_message"
            && node.payload.get("message_role").and_then(JsonValue::as_str) == Some("assistant")
    }) {
        return TraceUserReplyStatus::Produced;
    }
    if report
        .nodes
        .iter()
        .any(|node| matches!(node.node_kind.as_str(), "execution" | "episode" | "ingress"))
    {
        return TraceUserReplyStatus::NotProduced;
    }
    TraceUserReplyStatus::Unknown
}

fn derive_retry_safety(
    verdict: TraceDiagnosisVerdict,
    side_effect_status: TraceSideEffectStatus,
    failure_class: Option<TraceFailureClass>,
) -> TraceRetrySafety {
    if verdict == TraceDiagnosisVerdict::AwaitingApproval
        || matches!(
            failure_class,
            Some(TraceFailureClass::ApprovalPending)
                | Some(TraceFailureClass::ApprovalRejected)
                | Some(TraceFailureClass::ApprovalExpired)
        )
    {
        return TraceRetrySafety::RequiresOperator;
    }
    match side_effect_status {
        TraceSideEffectStatus::NoneExecuted => match verdict {
            TraceDiagnosisVerdict::Succeeded => TraceRetrySafety::Unsafe,
            TraceDiagnosisVerdict::Failed
            | TraceDiagnosisVerdict::Blocked
            | TraceDiagnosisVerdict::Inconclusive => TraceRetrySafety::Safe,
            TraceDiagnosisVerdict::AwaitingApproval => TraceRetrySafety::RequiresOperator,
        },
        TraceSideEffectStatus::Executed | TraceSideEffectStatus::Possible => {
            TraceRetrySafety::Unsafe
        }
        TraceSideEffectStatus::Unknown => TraceRetrySafety::Unknown,
    }
}

fn derive_likely_cause(
    report: &TraceReport,
    failure_class: Option<TraceFailureClass>,
    pending_approval: Option<&TraceNode>,
    blocked_node: Option<&TraceNode>,
    first_failing_node: Option<&TraceNode>,
) -> (Option<String>, Option<TraceLikelyCauseKind>) {
    if let Some(node) = pending_approval {
        return (
            Some(format!(
                "Approval request is pending: {}",
                short_trace_text(&node.summary)
            )),
            Some(TraceLikelyCauseKind::DirectFact),
        );
    }
    if let Some(node) = blocked_node {
        let detail = node
            .payload
            .get("blocked_reason")
            .and_then(JsonValue::as_str)
            .or_else(|| {
                node.payload
                    .get("consequence_summary")
                    .and_then(JsonValue::as_str)
            })
            .unwrap_or(node.summary.as_str());
        return (
            Some(short_trace_text(detail)),
            Some(TraceLikelyCauseKind::DirectFact),
        );
    }
    if let Some(node) = first_failing_node {
        if node.node_kind == "model_call" {
            if let Some(error_summary) = node
                .payload
                .get("error_summary")
                .and_then(JsonValue::as_str)
            {
                return (
                    Some(short_trace_text(error_summary)),
                    Some(TraceLikelyCauseKind::DirectFact),
                );
            }
        }
        if node.node_kind == "audit_event" {
            if let Some(error) = node
                .payload
                .get("payload")
                .and_then(|payload| payload.get("error"))
                .and_then(JsonValue::as_str)
            {
                return (
                    Some(short_trace_text(error)),
                    Some(TraceLikelyCauseKind::DirectFact),
                );
            }
        }
    }
    match failure_class {
        Some(class) => (
            Some(short_trace_text(&trace_failure_class_label(class))),
            Some(TraceLikelyCauseKind::Inference),
        ),
        None => {
            if report.nodes.is_empty() {
                (
                    Some(
                        "No durable trace records were found for this trace identifier."
                            .to_string(),
                    ),
                    Some(TraceLikelyCauseKind::DirectFact),
                )
            } else {
                (None, None)
            }
        }
    }
}

fn derive_next_steps(
    failure_class: Option<TraceFailureClass>,
    retry_safety: TraceRetrySafety,
    side_effect_status: TraceSideEffectStatus,
    awaiting_approval: bool,
) -> Vec<String> {
    let mut steps = Vec::new();
    match failure_class {
        Some(TraceFailureClass::ModelGatewayTransportFailure) => {
            steps.push("Check model gateway and provider availability.".to_string());
            steps.push(
                "Inspect recent operational diagnostics for repeated transport failures."
                    .to_string(),
            );
        }
        Some(TraceFailureClass::ProviderRejected) => {
            steps.push(
                "Inspect the failing model-call payload and provider error summary.".to_string(),
            );
            steps.push("Check recent diagnostics for repeated provider rejections.".to_string());
        }
        Some(TraceFailureClass::ContextAssemblyFailure) => {
            steps.push("Inspect the latest foreground context assembly audit events.".to_string());
            steps.push(
                "Review recent diagnostics for self-model or context assembly issues.".to_string(),
            );
        }
        Some(TraceFailureClass::MalformedActionProposal) => {
            steps.push(
                "Inspect the failing worker output and governed-action proposal formatting."
                    .to_string(),
            );
            steps.push(
                "Retry the request only after confirming the assistant produced a valid tagged action block."
                    .to_string(),
            );
        }
        Some(TraceFailureClass::WorkerProtocolFailure) => {
            steps.push(
                "Inspect the worker protocol phase, child exit status, and stderr excerpt."
                    .to_string(),
            );
            steps.push(
                "Restart or rebuild the worker/runtime if the child exited unexpectedly."
                    .to_string(),
            );
        }
        Some(TraceFailureClass::ScheduledForegroundValidationFailure) => {
            steps.push(
                "Inspect the scheduled foreground task payload and one-shot versus recurring schedule shape."
                    .to_string(),
            );
            steps.push(
                "Correct or disable the scheduled foreground task before retrying.".to_string(),
            );
        }
        Some(TraceFailureClass::GovernedActionBlocked) => {
            steps.push(
                "Inspect the blocked reason and capability scope on the governed action."
                    .to_string(),
            );
            steps.push(
                "Use the governed-action and approval management surfaces before retrying."
                    .to_string(),
            );
        }
        Some(TraceFailureClass::ApprovalPending) => {
            steps.push(
                "Resolve the pending approval request through the management CLI.".to_string(),
            );
        }
        Some(TraceFailureClass::ApprovalRejected | TraceFailureClass::ApprovalExpired) => {
            steps.push(
                "Inspect the approval outcome and decide whether a new request is needed."
                    .to_string(),
            );
        }
        Some(TraceFailureClass::PersistenceFailure) => {
            steps.push(
                "Inspect recent diagnostics and recovery health for persistence errors."
                    .to_string(),
            );
        }
        Some(TraceFailureClass::RecoveryInterrupted) => {
            steps.push(
                "Inspect recovery checkpoints and worker leases before retrying.".to_string(),
            );
        }
        Some(TraceFailureClass::TelegramDeliveryFailure) => {
            steps.push(
                "Inspect channel delivery diagnostics before retrying the foreground turn."
                    .to_string(),
            );
        }
        Some(TraceFailureClass::UnknownFailure) | None => {}
    }
    if awaiting_approval && !steps.iter().any(|step| step.contains("approval")) {
        steps.push(
            "Resolve the pending approval request before attempting further work.".to_string(),
        );
    }
    match retry_safety {
        TraceRetrySafety::Safe => {
            steps.push("Retry is safe once the underlying issue is addressed.".to_string());
        }
        TraceRetrySafety::Unsafe => {
            steps.push(
                "Do not retry blindly; the trace may already include side effects.".to_string(),
            );
        }
        TraceRetrySafety::RequiresOperator => {
            steps.push("Operator action is required before continuation.".to_string());
        }
        TraceRetrySafety::Unknown => {
            steps.push(
                "Treat retry safety as unknown until the ambiguous state is resolved.".to_string(),
            );
        }
    }
    if side_effect_status == TraceSideEffectStatus::NoneExecuted
        && !steps
            .iter()
            .any(|step| step.contains("Inspect the failing model-call payload"))
        && failure_class == Some(TraceFailureClass::ModelGatewayTransportFailure)
    {
        steps.push("No governed action side effects executed before the failure.".to_string());
    }
    if steps.is_empty() {
        steps.push(
            "Use trace show for the full timeline and inspect the most relevant node payload."
                .to_string(),
        );
    }
    steps
}

fn trace_failure_class_label(class: TraceFailureClass) -> String {
    match class {
        TraceFailureClass::ModelGatewayTransportFailure => {
            "The trace failed at the model gateway transport boundary.".to_string()
        }
        TraceFailureClass::ProviderRejected => {
            "The model provider rejected or failed the request.".to_string()
        }
        TraceFailureClass::TelegramDeliveryFailure => {
            "Delivery back to the conversation channel failed.".to_string()
        }
        TraceFailureClass::PersistenceFailure => {
            "Persistence failed during foreground processing.".to_string()
        }
        TraceFailureClass::ContextAssemblyFailure => {
            "Foreground context assembly failed before model completion.".to_string()
        }
        TraceFailureClass::MalformedActionProposal => {
            "The assistant attempted a governed action but returned an invalid action proposal shape."
                .to_string()
        }
        TraceFailureClass::WorkerProtocolFailure => {
            "The worker subprocess protocol failed before a valid final response returned."
                .to_string()
        }
        TraceFailureClass::ScheduledForegroundValidationFailure => {
            "A scheduled foreground action failed validation before execution could complete."
                .to_string()
        }
        TraceFailureClass::ApprovalPending => {
            "The foreground turn is waiting on a pending approval request.".to_string()
        }
        TraceFailureClass::ApprovalRejected => "The required approval was rejected.".to_string(),
        TraceFailureClass::ApprovalExpired => {
            "The required approval expired before execution.".to_string()
        }
        TraceFailureClass::GovernedActionBlocked => {
            "A governed action was blocked by policy or validation.".to_string()
        }
        TraceFailureClass::RecoveryInterrupted => {
            "Recovery interrupted the normal foreground continuation path.".to_string()
        }
        TraceFailureClass::UnknownFailure => {
            "The trace contains a failure, but the exact class could not be derived safely."
                .to_string()
        }
    }
}

fn classify_focus_payload_availability(
    node: Option<&TraceNode>,
) -> (TraceFocusPayloadAvailability, Option<String>, Vec<String>) {
    let Some(node) = node else {
        return (
            TraceFocusPayloadAvailability::Unavailable,
            Some("No node was resolved for the requested focus target.".to_string()),
            Vec::new(),
        );
    };

    if node.node_kind == "model_call" {
        let retained_fields = [
            "request_payload_json",
            "response_payload_json",
            "system_prompt_text",
            "messages_json",
        ];
        let available_fields = retained_fields
            .iter()
            .filter(|field| {
                node.payload
                    .get(**field)
                    .is_some_and(|value| !value.is_null())
            })
            .count();
        if let Some(reason) = node
            .payload
            .get("payload_retention_reason")
            .and_then(JsonValue::as_str)
        {
            return (
                TraceFocusPayloadAvailability::RetentionExpired,
                Some(format!(
                    "Model-call payload fields were cleared by retention: {reason}."
                )),
                vec!["Only retained metadata remains available for this model call.".to_string()],
            );
        }
        if available_fields == retained_fields.len() {
            return (TraceFocusPayloadAvailability::Available, None, Vec::new());
        }
        if available_fields == 0 {
            return (
                TraceFocusPayloadAvailability::NotRecorded,
                Some(
                    "The retained model-call payload fields are not available on this trace."
                        .to_string(),
                ),
                Vec::new(),
            );
        }
        return (
            TraceFocusPayloadAvailability::Partial,
            Some(
                "Some model-call payload fields are available, but others are absent.".to_string(),
            ),
            Vec::new(),
        );
    }

    if node.payload.is_null() {
        return (
            TraceFocusPayloadAvailability::Unavailable,
            Some("No payload is stored for this node.".to_string()),
            Vec::new(),
        );
    }

    (TraceFocusPayloadAvailability::Available, None, Vec::new())
}

fn short_trace_text(text: &str) -> String {
    const MAX_CHARS: usize = 240;
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= MAX_CHARS {
        normalized
    } else {
        let mut truncated = normalized.chars().take(MAX_CHARS).collect::<String>();
        truncated.push_str("...");
        truncated
    }
}

fn audit_payload_summary(payload: &JsonValue) -> Option<String> {
    payload
        .get("summary")
        .and_then(JsonValue::as_str)
        .or_else(|| payload.get("reason").and_then(JsonValue::as_str))
        .or_else(|| payload.get("reason_code").and_then(JsonValue::as_str))
        .map(short_trace_text)
}

pub fn default_list_limit() -> u32 {
    DEFAULT_LIST_LIMIT
}

#[derive(Debug, Clone)]
struct LoadedIdentityEditProposal {
    trace_id: Uuid,
    execution_id: Uuid,
    confidence_pct: u8,
    conflict_posture: ProposalConflictPosture,
    subject_ref: String,
    rationale: Option<String>,
    valid_from: Option<DateTime<Utc>>,
}

async fn load_identity_edit_proposal(
    pool: &PgPool,
    proposal_id: Uuid,
) -> Result<(LoadedIdentityEditProposal, CanonicalProposal)> {
    let row = sqlx::query(
        r#"
        SELECT trace_id, execution_id, status, confidence, conflict_posture, subject_ref,
               rationale, valid_from, payload_json
        FROM proposals
        WHERE proposal_id = $1
          AND proposal_kind = 'identity_delta'
          AND canonical_target = 'identity_items'
          AND source_loop_kind = 'operator'
        "#,
    )
    .bind(proposal_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("identity edit proposal {proposal_id} was not found"))?;
    let status: String = row.try_get("status")?;
    if status != "pending_operator_review" {
        bail!("identity edit proposal {proposal_id} is not pending operator review");
    }
    let payload: CanonicalProposalPayload = serde_json::from_value(row.try_get("payload_json")?)?;
    let conflict_posture = parse_proposal_conflict_posture(row.try_get("conflict_posture")?)?;
    let loaded = LoadedIdentityEditProposal {
        trace_id: row.try_get("trace_id")?,
        execution_id: row.try_get("execution_id")?,
        confidence_pct: pct_from_f64(row.try_get("confidence")?),
        conflict_posture,
        subject_ref: row.try_get("subject_ref")?,
        rationale: row.try_get("rationale")?,
        valid_from: row.try_get("valid_from")?,
    };
    let proposal = CanonicalProposal {
        proposal_id,
        proposal_kind: CanonicalProposalKind::IdentityDelta,
        canonical_target: CanonicalTargetKind::IdentityItems,
        confidence_pct: loaded.confidence_pct,
        conflict_posture: loaded.conflict_posture,
        subject_ref: loaded.subject_ref.clone(),
        rationale: loaded.rationale.clone(),
        valid_from: loaded.valid_from,
        valid_to: None,
        supersedes_artifact_id: None,
        provenance: ProposalProvenance {
            provenance_kind: ProposalProvenanceKind::EpisodeObservation,
            source_ingress_ids: vec![loaded.trace_id],
            source_episode_id: None,
        },
        payload,
    };
    Ok((loaded, proposal))
}

fn parse_identity_delta_operation(value: &str) -> Result<IdentityDeltaOperation> {
    match value {
        "add" => Ok(IdentityDeltaOperation::Add),
        "reinforce" => Ok(IdentityDeltaOperation::Reinforce),
        "weaken" => Ok(IdentityDeltaOperation::Weaken),
        "revise" => Ok(IdentityDeltaOperation::Revise),
        "supersede" => Ok(IdentityDeltaOperation::Supersede),
        "expire" => Ok(IdentityDeltaOperation::Expire),
        _ => bail!("unsupported identity edit operation {value}"),
    }
}

fn parse_identity_stability_class(value: &str) -> Result<IdentityStabilityClass> {
    match value {
        "stable" => Ok(IdentityStabilityClass::Stable),
        "evolving" => Ok(IdentityStabilityClass::Evolving),
        "transient_projection" => Ok(IdentityStabilityClass::TransientProjection),
        _ => bail!("unsupported identity stability class {value}"),
    }
}

fn parse_identity_item_category(value: &str) -> Result<IdentityItemCategory> {
    match value {
        "name" => Ok(IdentityItemCategory::Name),
        "identity_form" => Ok(IdentityItemCategory::IdentityForm),
        "role" => Ok(IdentityItemCategory::Role),
        "archetype" => Ok(IdentityItemCategory::Archetype),
        "origin_backstory" => Ok(IdentityItemCategory::OriginBackstory),
        "age_framing" => Ok(IdentityItemCategory::AgeFraming),
        "foundational_trait" => Ok(IdentityItemCategory::FoundationalTrait),
        "foundational_value" => Ok(IdentityItemCategory::FoundationalValue),
        "enduring_boundary" => Ok(IdentityItemCategory::EnduringBoundary),
        "default_communication_style" => Ok(IdentityItemCategory::DefaultCommunicationStyle),
        "preference" => Ok(IdentityItemCategory::Preference),
        "like" => Ok(IdentityItemCategory::Like),
        "dislike" => Ok(IdentityItemCategory::Dislike),
        "habit" => Ok(IdentityItemCategory::Habit),
        "routine" => Ok(IdentityItemCategory::Routine),
        "learned_tendency" => Ok(IdentityItemCategory::LearnedTendency),
        "autobiographical_refinement" => Ok(IdentityItemCategory::AutobiographicalRefinement),
        "recurring_self_description" => Ok(IdentityItemCategory::RecurringSelfDescription),
        "interaction_style_adaptation" => Ok(IdentityItemCategory::InteractionStyleAdaptation),
        "goal" => Ok(IdentityItemCategory::Goal),
        "subgoal" => Ok(IdentityItemCategory::Subgoal),
        _ => bail!("unsupported identity item category {value}"),
    }
}

fn parse_proposal_conflict_posture(value: String) -> Result<ProposalConflictPosture> {
    match value.as_str() {
        "independent" => Ok(ProposalConflictPosture::Independent),
        "revises" => Ok(ProposalConflictPosture::Revises),
        "supersedes" => Ok(ProposalConflictPosture::Supersedes),
        "conflicts" => Ok(ProposalConflictPosture::Conflicts),
        _ => bail!("unsupported proposal conflict posture {value}"),
    }
}

fn conflict_posture_as_str(value: ProposalConflictPosture) -> &'static str {
    match value {
        ProposalConflictPosture::Independent => "independent",
        ProposalConflictPosture::Revises => "revises",
        ProposalConflictPosture::Supersedes => "supersedes",
        ProposalConflictPosture::Conflicts => "conflicts",
    }
}

fn pct_from_f64(value: f64) -> u8 {
    (value.clamp(0.0, 1.0) * 100.0).round() as u8
}

fn inspect_worker_status(config: &RuntimeConfig) -> WorkerStatusReport {
    let resolution = worker::inspect_resolution(config);
    WorkerStatusReport {
        resolution_kind: resolution.resolution_kind.as_str().to_string(),
        command: resolution.command,
        args: resolution.args,
        timeout_ms: config.worker.timeout_ms,
        notes: resolution.notes,
    }
}

fn inspect_telegram_status(config: &RuntimeConfig) -> TelegramStatusReport {
    match &config.telegram {
        Some(telegram) => TelegramStatusReport {
            configured: true,
            binding_present: telegram.foreground_binding.is_some(),
            binding_internal_conversation_ref: telegram
                .foreground_binding
                .as_ref()
                .map(|binding| binding.internal_conversation_ref.clone()),
            binding_internal_principal_ref: telegram
                .foreground_binding
                .as_ref()
                .map(|binding| binding.internal_principal_ref.clone()),
            bot_token_env: Some(telegram.bot_token_env.clone()),
            bot_token_present: env_var_present(&telegram.bot_token_env),
            poll_limit: Some(telegram.poll_limit),
        },
        None => TelegramStatusReport {
            configured: false,
            binding_present: false,
            binding_internal_conversation_ref: None,
            binding_internal_principal_ref: None,
            bot_token_env: None,
            bot_token_present: false,
            poll_limit: None,
        },
    }
}

fn inspect_model_gateway_status(config: &RuntimeConfig) -> ModelGatewayStatusReport {
    match &config.model_gateway {
        Some(model_gateway) => ModelGatewayStatusReport {
            configured: true,
            provider: Some(model_gateway.foreground.provider.identifier().to_string()),
            model: Some(model_gateway.foreground.model.clone()),
            api_base_url: model_gateway.foreground.api_base_url.clone(),
            api_key_env: Some(model_gateway.foreground.api_key_env.clone()),
            api_key_present: env_var_present(&model_gateway.foreground.api_key_env),
            timeout_ms: Some(model_gateway.foreground.timeout_ms),
        },
        None => ModelGatewayStatusReport {
            configured: false,
            provider: None,
            model: None,
            api_base_url: None,
            api_key_env: None,
            api_key_present: false,
            timeout_ms: None,
        },
    }
}

fn inspect_self_model_status(config: &RuntimeConfig) -> SelfModelStatusReport {
    match &config.self_model {
        Some(self_model) => {
            let seed_path = resolve_seed_path(&self_model.seed_path);
            SelfModelStatusReport {
                configured: true,
                seed_path: Some(seed_path.display().to_string()),
                seed_exists: seed_path.is_file(),
            }
        }
        None => SelfModelStatusReport {
            configured: false,
            seed_path: None,
            seed_exists: false,
        },
    }
}

async fn inspect_schema_status(
    pool: &PgPool,
    config: &RuntimeConfig,
) -> Result<SchemaStatusReport> {
    let discovered = migration::load_migrations()?;
    let expected_version = migration::latest_version(&discovered);
    let applied = migration::load_applied_migrations(pool).await?;
    let current_version = applied.last().map(|migration| migration.version);
    let compatibility = match migration::validate_applied_history(&discovered, &applied) {
        Ok(()) => schema::evaluate(
            current_version,
            SchemaPolicy {
                minimum_supported_version: config.database.minimum_supported_schema_version,
                expected_version,
            },
        ),
        Err(error) => SchemaCompatibility::IncompatibleHistory {
            details: error.to_string(),
        },
    };

    let (compatibility_kind, details, history_valid) =
        schema_compatibility_report_fields(&compatibility);

    Ok(SchemaStatusReport {
        compatibility: compatibility_kind,
        current_version,
        expected_version,
        minimum_supported_version: config.database.minimum_supported_schema_version,
        applied_migration_count: applied.len(),
        history_valid,
        details,
    })
}

async fn inspect_schema_upgrade_assessment(
    pool: &PgPool,
    config: &RuntimeConfig,
) -> Result<SchemaUpgradeAssessmentReport> {
    let migrations = migration::load_migrations()?;
    let policy = SchemaPolicy {
        minimum_supported_version: config.database.minimum_supported_schema_version,
        expected_version: migration::latest_version(&migrations),
    };
    let assessment = schema::assess_upgrade_path(pool, policy).await?;
    let (compatibility, details, history_valid) =
        schema_compatibility_report_fields(&assessment.compatibility);

    Ok(SchemaUpgradeAssessmentReport {
        compatibility,
        current_version: assessment.current_version,
        expected_version: assessment.expected_version,
        minimum_supported_version: assessment.policy.minimum_supported_version,
        discovered_versions: assessment.discovered_versions,
        applied_versions: assessment.applied_versions,
        pending_versions: assessment.pending_versions,
        history_valid,
        details,
    })
}

fn schema_compatibility_report_fields(
    compatibility: &SchemaCompatibility,
) -> (String, Option<String>, bool) {
    match compatibility {
        SchemaCompatibility::Supported { .. } => ("supported".to_string(), None, true),
        SchemaCompatibility::Missing => (
            "missing".to_string(),
            Some("database schema is missing required migrations".to_string()),
            true,
        ),
        SchemaCompatibility::TooOld {
            current,
            minimum_supported,
        } => (
            "too_old".to_string(),
            Some(format!(
                "database schema version {current} is below minimum supported version {minimum_supported}"
            )),
            true,
        ),
        SchemaCompatibility::PendingMigrations { current, expected } => (
            "pending_migrations".to_string(),
            Some(format!(
                "database schema version {current} is behind expected version {expected}"
            )),
            true,
        ),
        SchemaCompatibility::TooNew { current, expected } => (
            "too_new".to_string(),
            Some(format!(
                "database schema version {current} is newer than runtime-supported version {expected}"
            )),
            true,
        ),
        SchemaCompatibility::IncompatibleHistory { details } => (
            "incompatible_history".to_string(),
            Some(details.clone()),
            false,
        ),
    }
}

async fn load_pending_work_summary(
    pool: &PgPool,
    config: &RuntimeConfig,
) -> Result<PendingWorkSummary> {
    let pending_foreground_conversation_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(DISTINCT internal_conversation_ref)
        FROM ingress_events
        WHERE internal_conversation_ref IS NOT NULL
          AND status = 'accepted'
          AND (
              foreground_status = 'pending'
              OR (
                  foreground_status = 'processing'
                  AND COALESCE(last_processed_at, received_at) <= $1
              )
          )
        "#,
    )
    .bind(foreground_stale_cutoff(config))
    .fetch_one(pool)
    .await
    .context("failed to count pending foreground conversations")?;
    let due_background_job_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM background_jobs
        WHERE status = 'planned'
          AND available_at <= $1
        "#,
    )
    .bind(Utc::now())
    .fetch_one(pool)
    .await
    .context("failed to count due background jobs")?;
    let pending_background_job_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM background_jobs
        WHERE status = 'planned'
        "#,
    )
    .fetch_one(pool)
    .await
    .context("failed to count planned background jobs")?;
    let pending_wake_signal_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM wake_signals
        WHERE status = 'pending_review'
        "#,
    )
    .fetch_one(pool)
    .await
    .context("failed to count pending wake signals")?;
    let pending_approval_request_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM approval_requests
        WHERE status = 'pending'
        "#,
    )
    .fetch_one(pool)
    .await
    .context("failed to count pending approval requests")?;
    let awaiting_approval_governed_action_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM governed_action_executions
        WHERE status = 'awaiting_approval'
        "#,
    )
    .fetch_one(pool)
    .await
    .context("failed to count awaiting approval governed actions")?;
    let blocked_governed_action_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM governed_action_executions
        WHERE status = 'blocked'
        "#,
    )
    .fetch_one(pool)
    .await
    .context("failed to count blocked governed actions")?;

    Ok(PendingWorkSummary {
        pending_foreground_conversation_count: pending_foreground_conversation_count as usize,
        pending_background_job_count: pending_background_job_count as u32,
        due_background_job_count: due_background_job_count as u32,
        pending_wake_signal_count: pending_wake_signal_count as u32,
        pending_approval_request_count: pending_approval_request_count as u32,
        awaiting_approval_governed_action_count: awaiting_approval_governed_action_count as u32,
        blocked_governed_action_count: blocked_governed_action_count as u32,
    })
}

async fn load_recovery_health_summary(
    pool: &PgPool,
    now: DateTime<Utc>,
    recent_window_start: DateTime<Utc>,
) -> Result<RecoveryHealthSummary> {
    let checkpoint_row = sqlx::query(
        r#"
        SELECT
            COUNT(*) FILTER (WHERE status = 'open') AS open_checkpoint_count,
            COUNT(*) FILTER (WHERE status = 'open' AND checkpoint_kind = 'foreground')
                AS open_foreground_checkpoint_count,
            COUNT(*) FILTER (WHERE status = 'open' AND checkpoint_kind = 'background')
                AS open_background_checkpoint_count,
            COUNT(*) FILTER (WHERE status = 'open' AND checkpoint_kind = 'governed_action')
                AS open_governed_action_checkpoint_count,
            COUNT(*) FILTER (
                WHERE status = 'resolved'
                  AND resolved_at >= $1
            ) AS recent_resolved_checkpoint_count,
            COUNT(*) FILTER (
                WHERE status = 'abandoned'
                  AND resolved_at >= $1
            ) AS recent_abandoned_checkpoint_count
        FROM recovery_checkpoints
        "#,
    )
    .bind(recent_window_start)
    .fetch_one(pool)
    .await
    .context("failed to load recovery checkpoint health summary")?;

    let lease_row = sqlx::query(
        r#"
        SELECT
            COUNT(*) AS active_worker_lease_count,
            COUNT(*) FILTER (WHERE lease_expires_at <= $1) AS overdue_active_worker_lease_count,
            COUNT(*) FILTER (WHERE lease_expires_at <= $2) AS at_risk_active_worker_lease_count
        FROM worker_leases
        WHERE status = 'active'
        "#,
    )
    .bind(now)
    .bind(now + Duration::seconds(LEASE_AT_RISK_WINDOW_SECONDS))
    .fetch_one(pool)
    .await
    .context("failed to load worker lease health summary")?;

    Ok(RecoveryHealthSummary {
        open_checkpoint_count: count_from_row(&checkpoint_row, "open_checkpoint_count"),
        open_foreground_checkpoint_count: count_from_row(
            &checkpoint_row,
            "open_foreground_checkpoint_count",
        ),
        open_background_checkpoint_count: count_from_row(
            &checkpoint_row,
            "open_background_checkpoint_count",
        ),
        open_governed_action_checkpoint_count: count_from_row(
            &checkpoint_row,
            "open_governed_action_checkpoint_count",
        ),
        recent_resolved_checkpoint_count: count_from_row(
            &checkpoint_row,
            "recent_resolved_checkpoint_count",
        ),
        recent_abandoned_checkpoint_count: count_from_row(
            &checkpoint_row,
            "recent_abandoned_checkpoint_count",
        ),
        active_worker_lease_count: count_from_row(&lease_row, "active_worker_lease_count"),
        overdue_active_worker_lease_count: count_from_row(
            &lease_row,
            "overdue_active_worker_lease_count",
        ),
        at_risk_active_worker_lease_count: count_from_row(
            &lease_row,
            "at_risk_active_worker_lease_count",
        ),
    })
}

#[derive(Debug, Clone)]
struct BaseDiagnosticRecord {
    operational_diagnostic_id: Uuid,
    trace_id: Option<Uuid>,
    execution_id: Option<Uuid>,
    subsystem: String,
    severity: String,
    reason_code: String,
    created_at: DateTime<Utc>,
}

async fn load_recent_base_diagnostics(
    pool: &PgPool,
    recent_window_start: DateTime<Utc>,
) -> Result<Vec<BaseDiagnosticRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT
            operational_diagnostic_id,
            trace_id,
            execution_id,
            subsystem,
            severity,
            reason_code,
            created_at
        FROM operational_diagnostics
        WHERE created_at >= $1
          AND subsystem <> 'management_health'
        ORDER BY created_at DESC, operational_diagnostic_id DESC
        LIMIT $2
        "#,
    )
    .bind(recent_window_start)
    .bind(HEALTH_RECENT_BASE_DIAGNOSTIC_LIMIT)
    .fetch_all(pool)
    .await
    .context("failed to load recent operational diagnostics for health summary")?;

    Ok(rows
        .into_iter()
        .map(|row| BaseDiagnosticRecord {
            operational_diagnostic_id: row.get("operational_diagnostic_id"),
            trace_id: row.get("trace_id"),
            execution_id: row.get("execution_id"),
            subsystem: row.get("subsystem"),
            severity: row.get("severity"),
            reason_code: row.get("reason_code"),
            created_at: row.get("created_at"),
        })
        .collect())
}

fn summarize_diagnostic_health(diagnostics: &[BaseDiagnosticRecord]) -> DiagnosticHealthSummary {
    let mut info_count = 0_u32;
    let mut warn_count = 0_u32;
    let mut error_count = 0_u32;
    let mut critical_count = 0_u32;
    let mut reasons: BTreeMap<String, OperationalReasonRollup> = BTreeMap::new();

    for diagnostic in diagnostics {
        match diagnostic.severity.as_str() {
            "info" => info_count += 1,
            "warn" => warn_count += 1,
            "error" => error_count += 1,
            "critical" => critical_count += 1,
            _ => {}
        }

        reasons
            .entry(diagnostic.reason_code.clone())
            .and_modify(|rollup| {
                rollup.count += 1;
                if diagnostic.created_at > rollup.latest_at {
                    rollup.latest_at = diagnostic.created_at;
                }
            })
            .or_insert_with(|| OperationalReasonRollup {
                reason_code: diagnostic.reason_code.clone(),
                count: 1,
                latest_at: diagnostic.created_at,
            });
    }

    let mut top_reason_codes = reasons.into_values().collect::<Vec<_>>();
    top_reason_codes.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| right.latest_at.cmp(&left.latest_at))
            .then_with(|| left.reason_code.cmp(&right.reason_code))
    });
    top_reason_codes.truncate(HEALTH_TOP_REASON_LIMIT);

    DiagnosticHealthSummary {
        recent_window_minutes: HEALTH_RECENT_WINDOW_MINUTES as u32,
        observed_count: diagnostics.len() as u32,
        info_count,
        warn_count,
        error_count,
        critical_count,
        top_reason_codes,
    }
}

async fn record_operational_anomaly_rollups(
    pool: &PgPool,
    now: DateTime<Utc>,
    recovery_summary: &RecoveryHealthSummary,
    diagnostics: &[BaseDiagnosticRecord],
) -> Result<()> {
    let recent_window_start = now - Duration::minutes(HEALTH_RECENT_WINDOW_MINUTES);
    let dedupe_window_start = now - Duration::minutes(ANOMALY_DEDUPE_WINDOW_MINUTES);

    let mut repeated_by_reason: BTreeMap<String, Vec<&BaseDiagnosticRecord>> = BTreeMap::new();
    for diagnostic in diagnostics {
        repeated_by_reason
            .entry(diagnostic.reason_code.clone())
            .or_default()
            .push(diagnostic);
    }
    for (reason_code, records) in repeated_by_reason {
        if records.len() < ANOMALY_REPEAT_THRESHOLD {
            continue;
        }
        let latest = records
            .iter()
            .max_by_key(|record| record.created_at)
            .expect("repeated anomaly records should not be empty");
        let first_seen_at = records
            .iter()
            .map(|record| record.created_at)
            .min()
            .expect("repeated anomaly records should not be empty");
        let last_seen_at = records
            .iter()
            .map(|record| record.created_at)
            .max()
            .expect("repeated anomaly records should not be empty");
        let severity = if records
            .iter()
            .any(|record| matches!(record.severity.as_str(), "critical" | "error"))
        {
            recovery::OperationalDiagnosticSeverity::Error
        } else {
            recovery::OperationalDiagnosticSeverity::Warn
        };
        let aggregate_key = format!("repeated_reason:{reason_code}");
        insert_management_anomaly_if_missing(
            pool,
            now,
            dedupe_window_start,
            recovery::NewOperationalDiagnostic {
                operational_diagnostic_id: Uuid::now_v7(),
                trace_id: latest.trace_id,
                execution_id: latest.execution_id,
                subsystem: "management_health".to_string(),
                severity,
                reason_code: "operational_repeated_condition_detected".to_string(),
                summary: format!(
                    "diagnostic reason '{reason_code}' repeated {} times in the last {} minutes",
                    records.len(),
                    HEALTH_RECENT_WINDOW_MINUTES
                ),
                diagnostic_payload: json!({
                    "aggregate_key": aggregate_key,
                    "anomaly_kind": "repeated_reason",
                    "source_reason_code": reason_code,
                    "occurrence_count": records.len(),
                    "first_seen_at": first_seen_at,
                    "last_seen_at": last_seen_at,
                    "recent_window_minutes": HEALTH_RECENT_WINDOW_MINUTES,
                    "source_operational_diagnostic_id": latest.operational_diagnostic_id,
                    "source_subsystem": latest.subsystem,
                }),
            },
        )
        .await?;
    }

    let failure_pressure_records = diagnostics
        .iter()
        .filter(|record| matches!(record.severity.as_str(), "error" | "critical"))
        .collect::<Vec<_>>();
    if failure_pressure_records.len() >= ANOMALY_FAILURE_PRESSURE_THRESHOLD {
        let latest = failure_pressure_records
            .iter()
            .max_by_key(|record| record.created_at)
            .expect("failure pressure records should not be empty");
        insert_management_anomaly_if_missing(
            pool,
            now,
            dedupe_window_start,
            recovery::NewOperationalDiagnostic {
                operational_diagnostic_id: Uuid::now_v7(),
                trace_id: latest.trace_id,
                execution_id: latest.execution_id,
                subsystem: "management_health".to_string(),
                severity: recovery::OperationalDiagnosticSeverity::Error,
                reason_code: "operational_failure_pressure_detected".to_string(),
                summary: format!(
                    "{} error or critical diagnostics observed in the last {} minutes",
                    failure_pressure_records.len(),
                    HEALTH_RECENT_WINDOW_MINUTES
                ),
                diagnostic_payload: json!({
                    "aggregate_key": "failure_pressure",
                    "anomaly_kind": "failure_pressure",
                    "occurrence_count": failure_pressure_records.len(),
                    "first_seen_at": failure_pressure_records
                        .iter()
                        .map(|record| record.created_at)
                        .min(),
                    "last_seen_at": failure_pressure_records
                        .iter()
                        .map(|record| record.created_at)
                        .max(),
                    "recent_window_minutes": HEALTH_RECENT_WINDOW_MINUTES,
                }),
            },
        )
        .await?;
    }

    if recovery_summary.open_checkpoint_count > 0
        || recovery_summary.overdue_active_worker_lease_count > 0
        || recovery_summary.at_risk_active_worker_lease_count > 0
    {
        let latest_checkpoint = sqlx::query(
            r#"
            SELECT trace_id, execution_id, created_at
            FROM recovery_checkpoints
            WHERE status = 'open'
            ORDER BY created_at DESC, recovery_checkpoint_id DESC
            LIMIT 1
            "#,
        )
        .fetch_optional(pool)
        .await
        .context("failed to load latest open recovery checkpoint for anomaly rollup")?;

        insert_management_anomaly_if_missing(
            pool,
            now,
            dedupe_window_start,
            recovery::NewOperationalDiagnostic {
                operational_diagnostic_id: Uuid::now_v7(),
                trace_id: latest_checkpoint.as_ref().map(|row| row.get("trace_id")),
                execution_id: latest_checkpoint.as_ref().and_then(|row| row.get("execution_id")),
                subsystem: "management_health".to_string(),
                severity: if recovery_summary.open_checkpoint_count > 0
                    || recovery_summary.overdue_active_worker_lease_count > 0
                {
                    recovery::OperationalDiagnosticSeverity::Error
                } else {
                    recovery::OperationalDiagnosticSeverity::Warn
                },
                reason_code: "operational_recovery_pressure_detected".to_string(),
                summary: format!(
                    "recovery pressure detected: {} open checkpoints, {} overdue active leases, {} at-risk active leases",
                    recovery_summary.open_checkpoint_count,
                    recovery_summary.overdue_active_worker_lease_count,
                    recovery_summary.at_risk_active_worker_lease_count
                ),
                diagnostic_payload: json!({
                    "aggregate_key": "recovery_pressure",
                    "anomaly_kind": "recovery_pressure",
                    "open_checkpoint_count": recovery_summary.open_checkpoint_count,
                    "open_foreground_checkpoint_count": recovery_summary.open_foreground_checkpoint_count,
                    "open_background_checkpoint_count": recovery_summary.open_background_checkpoint_count,
                    "open_governed_action_checkpoint_count": recovery_summary.open_governed_action_checkpoint_count,
                    "overdue_active_worker_lease_count": recovery_summary.overdue_active_worker_lease_count,
                    "at_risk_active_worker_lease_count": recovery_summary.at_risk_active_worker_lease_count,
                    "recent_window_minutes": HEALTH_RECENT_WINDOW_MINUTES,
                    "evaluated_at": now,
                    "recent_window_start": recent_window_start,
                }),
            },
        )
        .await?;
    }

    Ok(())
}

async fn insert_management_anomaly_if_missing(
    pool: &PgPool,
    now: DateTime<Utc>,
    dedupe_window_start: DateTime<Utc>,
    diagnostic: recovery::NewOperationalDiagnostic,
) -> Result<()> {
    let aggregate_key = diagnostic.diagnostic_payload["aggregate_key"]
        .as_str()
        .context("management anomaly diagnostic payload must include aggregate_key")?;
    let exists: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM operational_diagnostics
            WHERE subsystem = 'management_health'
              AND reason_code = $1
              AND diagnostic_payload_json ->> 'aggregate_key' = $2
              AND created_at >= $3
        )
        "#,
    )
    .bind(&diagnostic.reason_code)
    .bind(aggregate_key)
    .bind(dedupe_window_start)
    .fetch_one(pool)
    .await
    .context("failed to check management anomaly dedupe window")?;

    if exists {
        return Ok(());
    }

    recovery::insert_operational_diagnostic(pool, &diagnostic)
        .await
        .with_context(|| {
            format!(
                "failed to insert management anomaly diagnostic '{}' at {}",
                diagnostic.reason_code, now
            )
        })?;
    Ok(())
}

async fn load_recent_operational_anomalies(
    pool: &PgPool,
    recent_window_start: DateTime<Utc>,
) -> Result<Vec<OperationalAnomalySummary>> {
    let rows = sqlx::query(
        r#"
        SELECT
            operational_diagnostic_id,
            trace_id,
            execution_id,
            severity,
            reason_code,
            summary,
            diagnostic_payload_json,
            created_at
        FROM operational_diagnostics
        WHERE subsystem = 'management_health'
          AND created_at >= $1
        ORDER BY created_at DESC, operational_diagnostic_id DESC
        LIMIT $2
        "#,
    )
    .bind(recent_window_start)
    .bind(i64::from(DEFAULT_LIST_LIMIT))
    .fetch_all(pool)
    .await
    .context("failed to load recent operational anomalies")?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let payload: serde_json::Value = row.get("diagnostic_payload_json");
            OperationalAnomalySummary {
                anomaly_kind: payload["anomaly_kind"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string(),
                severity: row.get("severity"),
                reason_code: row.get("reason_code"),
                summary: row.get("summary"),
                occurrence_count: payload["occurrence_count"].as_u64().unwrap_or(1) as u32,
                latest_trace_id: row.get("trace_id"),
                latest_execution_id: row.get("execution_id"),
                first_seen_at: payload["first_seen_at"]
                    .as_str()
                    .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                    .map(|value| value.with_timezone(&Utc))
                    .unwrap_or_else(|| row.get("created_at")),
                last_seen_at: payload["last_seen_at"]
                    .as_str()
                    .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                    .map(|value| value.with_timezone(&Utc))
                    .unwrap_or_else(|| row.get("created_at")),
            }
        })
        .collect())
}

fn classify_overall_health(
    pending_work: &PendingWorkSummary,
    recovery: &RecoveryHealthSummary,
    diagnostics: &DiagnosticHealthSummary,
) -> String {
    if recovery.open_checkpoint_count > 0
        || recovery.overdue_active_worker_lease_count > 0
        || diagnostics.error_count > 0
        || diagnostics.critical_count > 0
    {
        return "unhealthy".to_string();
    }

    if recovery.at_risk_active_worker_lease_count > 0
        || diagnostics.warn_count > 0
        || pending_work.pending_wake_signal_count > 0
        || pending_work.pending_approval_request_count > 0
        || pending_work.blocked_governed_action_count > 0
    {
        return "degraded".to_string();
    }

    "healthy".to_string()
}

fn count_from_row(row: &sqlx::postgres::PgRow, field: &str) -> u32 {
    row.get::<i64, _>(field).max(0) as u32
}

async fn scheduled_foreground_task_summary_from_record(
    pool: &PgPool,
    record: scheduled_foreground::ScheduledForegroundTaskRecord,
) -> Result<ScheduledForegroundTaskSummary> {
    let conversation_binding_present =
        scheduled_foreground::conversation_binding_present(pool, &record.internal_conversation_ref)
            .await?;
    Ok(ScheduledForegroundTaskSummary {
        scheduled_foreground_task_id: record.scheduled_foreground_task_id,
        task_key: record.task_key,
        channel_kind: scheduled_foreground_channel_kind_label(record.channel_kind),
        status: scheduled_foreground_task_status_label(record.status),
        internal_principal_ref: record.internal_principal_ref,
        internal_conversation_ref: record.internal_conversation_ref,
        conversation_binding_present,
        message_text: record.message_text,
        cadence_seconds: record.cadence_seconds,
        cooldown_seconds: record.cooldown_seconds,
        next_due_at: record.next_due_at,
        current_execution_id: record.current_execution_id,
        current_run_started_at: record.current_run_started_at,
        last_execution_id: record.last_execution_id,
        last_run_started_at: record.last_run_started_at,
        last_run_completed_at: record.last_run_completed_at,
        last_outcome: record
            .last_outcome
            .map(scheduled_foreground_last_outcome_label),
        last_outcome_reason: record.last_outcome_reason,
        last_outcome_summary: record.last_outcome_summary,
        created_by: record.created_by,
        updated_by: record.updated_by,
        created_at: record.created_at,
        updated_at: record.updated_at,
    })
}

fn recovered_worker_lease_summary(
    outcome: recovery::WorkerLeaseRecoveryOutcome,
) -> RecoveredWorkerLeaseSummary {
    RecoveredWorkerLeaseSummary {
        worker_lease_id: outcome.lease.worker_lease_id,
        worker_kind: worker_lease_kind_label(outcome.lease.worker_kind),
        checkpoint_id: outcome.checkpoint.recovery_checkpoint_id,
        checkpoint_status: recovery_checkpoint_status_label(outcome.checkpoint.status),
        recovery_decision: recovery_decision_label(outcome.decision.decision),
        diagnostic_reason_code: outcome.diagnostic.reason_code,
        diagnostic_severity: operational_diagnostic_severity_label(outcome.diagnostic.severity),
        trace_id: outcome.lease.trace_id,
        execution_id: outcome.lease.execution_id,
    }
}

fn operational_diagnostic_summary(
    record: recovery::OperationalDiagnosticRecord,
) -> OperationalDiagnosticSummary {
    OperationalDiagnosticSummary {
        operational_diagnostic_id: record.operational_diagnostic_id,
        trace_id: record.trace_id,
        execution_id: record.execution_id,
        subsystem: record.subsystem,
        severity: operational_diagnostic_severity_label(record.severity),
        reason_code: record.reason_code,
        summary: record.summary,
        created_at: record.created_at,
    }
}

fn approval_request_summary(record: &approval::ApprovalRequestRecord) -> ApprovalRequestSummary {
    ApprovalRequestSummary {
        approval_request_id: record.approval_request_id,
        trace_id: record.trace_id,
        execution_id: record.execution_id,
        action_proposal_id: record.action_proposal_id,
        action_fingerprint: record.action_fingerprint.value.clone(),
        action_kind: governed_action_kind_label(record.action_kind),
        risk_tier: governed_action_risk_tier_label(record.risk_tier),
        capability_scope: record.capability_scope.clone(),
        status: approval_request_status_label(record.status),
        title: record.title.clone(),
        consequence_summary: record.consequence_summary.clone(),
        requested_by: record.requested_by.clone(),
        requested_at: record.requested_at,
        expires_at: record.expires_at,
        resolved_at: record.resolved_at,
        resolution_kind: record
            .resolution_kind
            .map(approval_resolution_decision_label),
        resolved_by: record.resolved_by.clone(),
        resolution_reason: record.resolution_reason.clone(),
    }
}

fn governed_action_summary(
    record: &governed_actions::GovernedActionExecutionRecord,
) -> GovernedActionSummary {
    GovernedActionSummary {
        governed_action_execution_id: record.governed_action_execution_id,
        trace_id: record.trace_id,
        execution_id: record.execution_id,
        approval_request_id: record.approval_request_id,
        action_proposal_id: record.action_proposal_id,
        action_fingerprint: record.action_fingerprint.value.clone(),
        action_kind: governed_action_kind_label(record.action_kind),
        risk_tier: governed_action_risk_tier_label(record.risk_tier),
        status: governed_action_status_label(record.status),
        workspace_script_id: record.workspace_script_id,
        workspace_script_version_id: record.workspace_script_version_id,
        blocked_reason: record.blocked_reason.clone(),
        output_ref: record.output_ref.clone(),
        started_at: record.started_at,
        completed_at: record.completed_at,
    }
}

fn workspace_script_run_summary(
    record: &workspace::WorkspaceScriptRunRecord,
) -> WorkspaceScriptRunSummary {
    WorkspaceScriptRunSummary {
        workspace_script_run_id: record.workspace_script_run_id,
        workspace_script_id: record.workspace_script_id,
        workspace_script_version_id: record.workspace_script_version_id,
        trace_id: record.trace_id,
        execution_id: record.execution_id,
        governed_action_execution_id: record.governed_action_execution_id,
        approval_request_id: record.approval_request_id,
        status: workspace_script_run_status_label(record.status),
        risk_tier: governed_action_risk_tier_label(record.risk_tier),
        args: record.args.clone(),
        output_ref: record.output_ref.clone(),
        failure_summary: record.failure_summary.clone(),
        started_at: record.started_at,
        completed_at: record.completed_at,
    }
}

fn default_cli_actor_ref(requested_by: &str) -> String {
    match requested_by.split_once(':') {
        Some((_, principal)) if !principal.trim().is_empty() => format!("cli:{principal}"),
        _ => "cli:operator".to_string(),
    }
}

fn approval_request_status_label(status: contracts::ApprovalRequestStatus) -> String {
    match status {
        contracts::ApprovalRequestStatus::Pending => "pending",
        contracts::ApprovalRequestStatus::Approved => "approved",
        contracts::ApprovalRequestStatus::Rejected => "rejected",
        contracts::ApprovalRequestStatus::Expired => "expired",
        contracts::ApprovalRequestStatus::Invalidated => "invalidated",
    }
    .to_string()
}

fn approval_resolution_decision_label(decision: contracts::ApprovalResolutionDecision) -> String {
    match decision {
        contracts::ApprovalResolutionDecision::Approved => "approved",
        contracts::ApprovalResolutionDecision::Rejected => "rejected",
        contracts::ApprovalResolutionDecision::Expired => "expired",
        contracts::ApprovalResolutionDecision::Invalidated => "invalidated",
    }
    .to_string()
}

fn governed_action_kind_label(kind: contracts::GovernedActionKind) -> String {
    match kind {
        contracts::GovernedActionKind::RunSubprocess => "run_subprocess",
        contracts::GovernedActionKind::RunWorkspaceScript => "run_workspace_script",
        contracts::GovernedActionKind::InspectWorkspaceArtifact => "inspect_workspace_artifact",
        contracts::GovernedActionKind::ListWorkspaceArtifacts => "list_workspace_artifacts",
        contracts::GovernedActionKind::CreateWorkspaceArtifact => "create_workspace_artifact",
        contracts::GovernedActionKind::UpdateWorkspaceArtifact => "update_workspace_artifact",
        contracts::GovernedActionKind::ListWorkspaceScripts => "list_workspace_scripts",
        contracts::GovernedActionKind::InspectWorkspaceScript => "inspect_workspace_script",
        contracts::GovernedActionKind::CreateWorkspaceScript => "create_workspace_script",
        contracts::GovernedActionKind::AppendWorkspaceScriptVersion => {
            "append_workspace_script_version"
        }
        contracts::GovernedActionKind::ListWorkspaceScriptRuns => "list_workspace_script_runs",
        contracts::GovernedActionKind::UpsertScheduledForegroundTask => {
            "upsert_scheduled_foreground_task"
        }
        contracts::GovernedActionKind::RequestBackgroundJob => "request_background_job",
        contracts::GovernedActionKind::RunDiagnostic => "run_diagnostic",
        contracts::GovernedActionKind::WebFetch => "web_fetch",
    }
    .to_string()
}

fn governed_action_risk_tier_label(risk_tier: contracts::GovernedActionRiskTier) -> String {
    match risk_tier {
        contracts::GovernedActionRiskTier::Tier0 => "tier_0",
        contracts::GovernedActionRiskTier::Tier1 => "tier_1",
        contracts::GovernedActionRiskTier::Tier2 => "tier_2",
        contracts::GovernedActionRiskTier::Tier3 => "tier_3",
    }
    .to_string()
}

fn governed_action_status_label(status: contracts::GovernedActionStatus) -> String {
    match status {
        contracts::GovernedActionStatus::Proposed => "proposed",
        contracts::GovernedActionStatus::AwaitingApproval => "awaiting_approval",
        contracts::GovernedActionStatus::Approved => "approved",
        contracts::GovernedActionStatus::Rejected => "rejected",
        contracts::GovernedActionStatus::Expired => "expired",
        contracts::GovernedActionStatus::Invalidated => "invalidated",
        contracts::GovernedActionStatus::Blocked => "blocked",
        contracts::GovernedActionStatus::Executed => "executed",
        contracts::GovernedActionStatus::Failed => "failed",
    }
    .to_string()
}

fn workspace_script_run_status_label(status: contracts::WorkspaceScriptRunStatus) -> String {
    match status {
        contracts::WorkspaceScriptRunStatus::Pending => "pending",
        contracts::WorkspaceScriptRunStatus::Running => "running",
        contracts::WorkspaceScriptRunStatus::Completed => "completed",
        contracts::WorkspaceScriptRunStatus::Failed => "failed",
        contracts::WorkspaceScriptRunStatus::TimedOut => "timed_out",
        contracts::WorkspaceScriptRunStatus::Blocked => "blocked",
    }
    .to_string()
}

fn worker_lease_kind_label(kind: recovery::WorkerLeaseKind) -> String {
    match kind {
        recovery::WorkerLeaseKind::Foreground => "foreground",
        recovery::WorkerLeaseKind::Background => "background",
        recovery::WorkerLeaseKind::GovernedAction => "governed_action",
    }
    .to_string()
}

fn worker_lease_status_label(status: recovery::WorkerLeaseStatus) -> String {
    match status {
        recovery::WorkerLeaseStatus::Active => "active",
        recovery::WorkerLeaseStatus::Released => "released",
        recovery::WorkerLeaseStatus::Expired => "expired",
        recovery::WorkerLeaseStatus::Terminated => "terminated",
    }
    .to_string()
}

fn worker_lease_supervision_status_label(
    status: recovery::WorkerLeaseSupervisionDecision,
) -> String {
    match status {
        recovery::WorkerLeaseSupervisionDecision::Healthy => "healthy",
        recovery::WorkerLeaseSupervisionDecision::SoftWarning => "soft_warning",
        recovery::WorkerLeaseSupervisionDecision::HardExpired => "hard_expired",
    }
    .to_string()
}

fn scheduled_foreground_channel_kind_label(kind: ChannelKind) -> String {
    match kind {
        ChannelKind::Telegram => "telegram",
    }
    .to_string()
}

fn scheduled_foreground_task_status_label(status: ScheduledForegroundTaskStatus) -> String {
    match status {
        ScheduledForegroundTaskStatus::Active => "active",
        ScheduledForegroundTaskStatus::Paused => "paused",
        ScheduledForegroundTaskStatus::Disabled => "disabled",
    }
    .to_string()
}

fn scheduled_foreground_last_outcome_label(outcome: ScheduledForegroundLastOutcome) -> String {
    match outcome {
        ScheduledForegroundLastOutcome::Completed => "completed",
        ScheduledForegroundLastOutcome::Suppressed => "suppressed",
        ScheduledForegroundLastOutcome::Failed => "failed",
    }
    .to_string()
}

fn scheduled_foreground_task_write_action_label(
    action: scheduled_foreground::ScheduledForegroundTaskWriteAction,
) -> String {
    match action {
        scheduled_foreground::ScheduledForegroundTaskWriteAction::Created => "created",
        scheduled_foreground::ScheduledForegroundTaskWriteAction::Updated => "updated",
    }
    .to_string()
}

fn recovery_checkpoint_status_label(status: recovery::RecoveryCheckpointStatus) -> String {
    match status {
        recovery::RecoveryCheckpointStatus::Open => "open",
        recovery::RecoveryCheckpointStatus::Resolved => "resolved",
        recovery::RecoveryCheckpointStatus::Abandoned => "abandoned",
        recovery::RecoveryCheckpointStatus::Invalidated => "invalidated",
    }
    .to_string()
}

fn recovery_decision_label(decision: recovery::RecoveryDecision) -> String {
    match decision {
        recovery::RecoveryDecision::Continue => "continue",
        recovery::RecoveryDecision::Retry => "retry",
        recovery::RecoveryDecision::Defer => "defer",
        recovery::RecoveryDecision::Reapprove => "reapprove",
        recovery::RecoveryDecision::Clarify => "clarify",
        recovery::RecoveryDecision::Abandon => "abandon",
    }
    .to_string()
}

fn operational_diagnostic_severity_label(
    severity: recovery::OperationalDiagnosticSeverity,
) -> String {
    match severity {
        recovery::OperationalDiagnosticSeverity::Info => "info",
        recovery::OperationalDiagnosticSeverity::Warn => "warn",
        recovery::OperationalDiagnosticSeverity::Error => "error",
        recovery::OperationalDiagnosticSeverity::Critical => "critical",
    }
    .to_string()
}

fn classify_pending_foreground_summary(
    config: &RuntimeConfig,
    pending_count: usize,
    pending_span_seconds: u64,
    stale_pending_age_seconds: u64,
    includes_stale_processing: bool,
) -> (&'static str, &'static str) {
    let backlog = &config.continuity.backlog_recovery;

    if pending_count < 2 {
        return ("single_ingress", "single_ingress");
    }
    if includes_stale_processing {
        return ("backlog_recovery", "stale_processing_resume");
    }
    if pending_count >= backlog.pending_message_count_threshold as usize
        && pending_span_seconds >= backlog.pending_message_span_seconds_threshold
    {
        return ("backlog_recovery", "pending_span_threshold");
    }
    if pending_count >= backlog.pending_message_count_threshold as usize
        && stale_pending_age_seconds >= backlog.stale_pending_ingress_age_seconds_threshold
    {
        return ("backlog_recovery", "stale_pending_batch");
    }
    ("single_ingress", "single_ingress")
}

fn foreground_stale_cutoff(config: &RuntimeConfig) -> DateTime<Utc> {
    Utc::now()
        - Duration::seconds(
            config
                .continuity
                .backlog_recovery
                .stale_pending_ingress_age_seconds_threshold as i64,
        )
}

fn env_var_present(name: &str) -> bool {
    env::var_os(name).is_some_and(|value| !value.is_empty())
}

fn resolve_seed_path(seed_path: &PathBuf) -> PathBuf {
    if seed_path.is_absolute() {
        return seed_path.clone();
    }
    migration::workspace_root().join(seed_path)
}

async fn verify_schema(pool: &PgPool, config: &RuntimeConfig) -> Result<i64> {
    let migrations = migration::load_migrations()?;
    let policy = SchemaPolicy {
        minimum_supported_version: config.database.minimum_supported_schema_version,
        expected_version: migration::latest_version(&migrations),
    };
    schema::verify(pool, policy).await
}

trait ModelProviderKindExt {
    fn identifier(self) -> &'static str;
}

impl ModelProviderKindExt for contracts::ModelProviderKind {
    fn identifier(self) -> &'static str {
        match self {
            contracts::ModelProviderKind::ZAi => "z_ai",
            contracts::ModelProviderKind::OpenRouter => "openrouter",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        AppConfig, ApprovalPromptMode, ApprovalsConfig, BackgroundConfig,
        BackgroundExecutionConfig, BackgroundSchedulerConfig, BackgroundThresholdsConfig,
        ContinuityConfig, DatabaseConfig, GovernedActionsConfig, HarnessConfig, ModelGatewayConfig,
        ObservabilityConfig, ScheduledForegroundConfig, SelfModelConfig, TelegramConfig,
        TelegramForegroundBindingConfig, WakeSignalPolicyConfig, WorkerConfig, WorkspaceConfig,
    };
    use std::path::PathBuf;

    fn sample_config() -> RuntimeConfig {
        RuntimeConfig {
            app: AppConfig {
                name: "blue-lagoon".to_string(),
                log_filter: "info".to_string(),
            },
            database: DatabaseConfig {
                database_url: "postgres://example".to_string(),
                minimum_supported_schema_version: 1,
            },
            harness: HarnessConfig {
                allow_synthetic_smoke: true,
                default_foreground_iteration_budget: 1,
                default_wall_clock_budget_ms: 60_000,
                default_foreground_token_budget: 4_000,
            },
            background: BackgroundConfig {
                scheduler: BackgroundSchedulerConfig {
                    poll_interval_seconds: 300,
                    max_due_jobs_per_iteration: 4,
                    lease_timeout_ms: 300_000,
                },
                thresholds: BackgroundThresholdsConfig {
                    episode_backlog_threshold: 25,
                    candidate_memory_threshold: 10,
                    contradiction_alert_threshold: 3,
                },
                execution: BackgroundExecutionConfig {
                    default_iteration_budget: 2,
                    default_wall_clock_budget_ms: 120_000,
                    default_token_budget: 6_000,
                },
                wake_signals: WakeSignalPolicyConfig {
                    allow_foreground_conversion: true,
                    max_pending_signals: 8,
                    cooldown_seconds: 900,
                },
            },
            continuity: ContinuityConfig {
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
            scheduled_foreground: ScheduledForegroundConfig {
                enabled: true,
                max_due_tasks_per_iteration: 2,
                min_cadence_seconds: 300,
                default_cooldown_seconds: 300,
            },
            workspace: WorkspaceConfig {
                root_dir: ".".into(),
                max_artifact_bytes: 1_048_576,
                max_script_bytes: 262_144,
            },
            observability: ObservabilityConfig {
                model_call_payload_retention_days: 30,
            },
            approvals: ApprovalsConfig {
                default_ttl_seconds: 900,
                max_pending_requests: 32,
                allow_cli_resolution: true,
                prompt_mode: ApprovalPromptMode::InlineKeyboardWithFallback,
            },
            governed_actions: GovernedActionsConfig {
                approval_required_min_risk_tier: contracts::GovernedActionRiskTier::Tier2,
                default_subprocess_timeout_ms: 30_000,
                max_subprocess_timeout_ms: 120_000,
                max_actions_per_foreground_turn: 10,
                cap_exceeded_behavior: contracts::GovernedActionCapExceededBehavior::Escalate,
                max_filesystem_roots_per_action: 4,
                default_network_access: contracts::NetworkAccessPosture::Disabled,
                allowlisted_environment_variables: vec!["BLUE_LAGOON_DATABASE_URL".to_string()],
                max_environment_variables_per_action: 8,
                max_captured_output_bytes: 65_536,
                max_web_fetch_timeout_ms: 15_000,
                max_web_fetch_response_bytes: 524_288,
            },
            worker: WorkerConfig {
                timeout_ms: 10_000,
                command: String::new(),
                args: Vec::new(),
            },
            telegram: Some(TelegramConfig {
                api_base_url: "https://api.telegram.org".to_string(),
                bot_token_env: "BLUE_LAGOON_TELEGRAM_BOT_TOKEN".to_string(),
                poll_limit: 10,
                foreground_binding: Some(TelegramForegroundBindingConfig {
                    allowed_user_id: 1,
                    allowed_chat_id: 2,
                    internal_principal_ref: "primary-user".to_string(),
                    internal_conversation_ref: "telegram-primary".to_string(),
                }),
            }),
            model_gateway: Some(ModelGatewayConfig {
                foreground: crate::config::ForegroundModelRouteConfig {
                    provider: contracts::ModelProviderKind::ZAi,
                    model: "foreground".to_string(),
                    api_base_url: None,
                    api_key_env: "BLUE_LAGOON_FOREGROUND_API_KEY".to_string(),
                    timeout_ms: 60_000,
                },
                z_ai: None,
                openrouter: None,
            }),
            self_model: Some(SelfModelConfig {
                seed_path: PathBuf::from("config/self_model_seed.toml"),
            }),
        }
    }

    #[test]
    fn classify_pending_foreground_summary_prefers_backlog_for_stale_processing() {
        let config = sample_config();
        let (mode, reason) = classify_pending_foreground_summary(&config, 2, 10, 10, true);
        assert_eq!(mode, "backlog_recovery");
        assert_eq!(reason, "stale_processing_resume");
    }

    #[test]
    fn classify_pending_foreground_summary_defaults_to_single_ingress() {
        let config = sample_config();
        let (mode, reason) = classify_pending_foreground_summary(&config, 1, 0, 0, false);
        assert_eq!(mode, "single_ingress");
        assert_eq!(reason, "single_ingress");
    }

    #[test]
    fn default_cli_actor_ref_reuses_requested_principal() {
        assert_eq!(
            default_cli_actor_ref("telegram:primary-user"),
            "cli:primary-user"
        );
    }

    #[test]
    fn default_cli_actor_ref_falls_back_when_requested_by_is_malformed() {
        assert_eq!(default_cli_actor_ref("primary-user"), "cli:operator");
    }

    #[test]
    fn classify_audit_failure_detects_malformed_action_proposal() {
        let node = TraceNode {
            node_id: "audit_event:test".to_string(),
            node_kind: "audit_event".to_string(),
            source_id: Uuid::now_v7(),
            occurred_at: Utc::now(),
            status: Some("error".to_string()),
            title: "foreground_execution_failed".to_string(),
            summary: "Foreground execution failed.".to_string(),
            payload: json!({
                "payload": {
                    "failure_kind": "malformed_action_proposal",
                    "error": "model attempted a governed action without the required governed-action block"
                }
            }),
            related_ids: BTreeMap::new(),
        };

        assert_eq!(
            classify_audit_failure(&node),
            Some(TraceFailureClass::MalformedActionProposal)
        );
    }
}
