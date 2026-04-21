use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerKind {
    Smoke,
    Conscious,
    Unconscious,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerRequest {
    pub request_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Uuid,
    pub sent_at: DateTime<Utc>,
    pub worker_kind: WorkerKind,
    pub payload: WorkerPayload,
}

impl WorkerRequest {
    pub fn smoke(trace_id: Uuid, execution_id: Uuid, synthetic_trigger: impl Into<String>) -> Self {
        Self {
            request_id: Uuid::now_v7(),
            trace_id,
            execution_id,
            sent_at: Utc::now(),
            worker_kind: WorkerKind::Smoke,
            payload: WorkerPayload::Smoke(SmokeWorkerRequest {
                synthetic_trigger: synthetic_trigger.into(),
            }),
        }
    }

    pub fn conscious(trace_id: Uuid, execution_id: Uuid, context: ConsciousContext) -> Self {
        let request_id = Uuid::now_v7();
        let sent_at = Utc::now();
        Self {
            request_id,
            trace_id,
            execution_id,
            sent_at,
            worker_kind: WorkerKind::Conscious,
            payload: WorkerPayload::Conscious(Box::new(ConsciousWorkerRequest {
                request_id,
                trace_id,
                execution_id,
                sent_at,
                context,
            })),
        }
    }

    pub fn unconscious(trace_id: Uuid, execution_id: Uuid, context: UnconsciousContext) -> Self {
        let request_id = Uuid::now_v7();
        let sent_at = Utc::now();
        Self {
            request_id,
            trace_id,
            execution_id,
            sent_at,
            worker_kind: WorkerKind::Unconscious,
            payload: WorkerPayload::Unconscious(Box::new(UnconsciousWorkerRequest {
                request_id,
                trace_id,
                execution_id,
                sent_at,
                context,
            })),
        }
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        match (&self.worker_kind, &self.payload) {
            (WorkerKind::Smoke, WorkerPayload::Smoke(_))
            | (WorkerKind::Conscious, WorkerPayload::Conscious(_))
            | (WorkerKind::Unconscious, WorkerPayload::Unconscious(_)) => Ok(()),
            _ => Err(ContractError::WorkerPayloadMismatch),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum WorkerPayload {
    Smoke(SmokeWorkerRequest),
    Conscious(Box<ConsciousWorkerRequest>),
    Unconscious(Box<UnconsciousWorkerRequest>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmokeWorkerRequest {
    pub synthetic_trigger: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsciousWorkerRequest {
    pub request_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Uuid,
    pub sent_at: DateTime<Utc>,
    pub context: ConsciousContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnconsciousWorkerRequest {
    pub request_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Uuid,
    pub sent_at: DateTime<Utc>,
    pub context: UnconsciousContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum ConsciousWorkerOutboundMessage {
    ModelCallRequest(ModelCallRequest),
    FinalResponse(WorkerResponse),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum ConsciousWorkerInboundMessage {
    ModelCallResponse(ModelCallResponse),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerResponse {
    pub request_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Uuid,
    pub finished_at: DateTime<Utc>,
    pub worker_pid: u32,
    pub result: WorkerResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum WorkerResult {
    Smoke(SmokeWorkerResult),
    Conscious(ConsciousWorkerResult),
    Unconscious(UnconsciousWorkerResult),
    Error(WorkerFailure),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmokeWorkerResult {
    pub status: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsciousWorkerResult {
    pub status: ConsciousWorkerStatus,
    pub assistant_output: AssistantOutput,
    pub episode_summary: EpisodeSummary,
    pub candidate_proposals: Vec<CanonicalProposal>,
    pub governed_action_proposals: Vec<GovernedActionProposal>,
    pub governed_action_observations: Vec<GovernedActionObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnconsciousWorkerResult {
    pub status: UnconsciousWorkerStatus,
    pub summary: String,
    pub maintenance_outputs: UnconsciousMaintenanceOutputs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsciousWorkerStatus {
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnconsciousWorkerStatus {
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantOutput {
    pub channel_kind: ChannelKind,
    pub internal_conversation_ref: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpisodeSummary {
    pub summary: String,
    pub outcome: String,
    pub message_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerFailure {
    pub code: WorkerErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerErrorCode {
    InvalidRequest,
    UnsupportedWorker,
    InternalFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    Telegram,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngressEventKind {
    MessageCreated,
    CommandIssued,
    ApprovalCallback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedIngress {
    pub ingress_id: Uuid,
    pub channel_kind: ChannelKind,
    pub external_user_id: String,
    pub external_conversation_id: String,
    pub external_event_id: String,
    pub external_message_id: Option<String>,
    pub internal_principal_ref: String,
    pub internal_conversation_ref: String,
    pub event_kind: IngressEventKind,
    pub occurred_at: DateTime<Utc>,
    pub text_body: Option<String>,
    pub reply_to: Option<ReplyReference>,
    pub attachments: Vec<AttachmentReference>,
    pub command_hint: Option<CommandHint>,
    pub approval_payload: Option<ApprovalPayload>,
    pub raw_payload_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplyReference {
    pub external_message_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentReference {
    pub attachment_id: String,
    pub media_type: Option<String>,
    pub file_name: Option<String>,
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandHint {
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalPayload {
    pub token: String,
    pub callback_data: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForegroundTriggerKind {
    UserIngress,
    ApprovedWakeSignal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForegroundExecutionMode {
    SingleIngress,
    BacklogRecovery,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForegroundBudget {
    pub iteration_budget: u32,
    pub wall_clock_budget_ms: u64,
    pub token_budget: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForegroundTrigger {
    pub trigger_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Uuid,
    pub trigger_kind: ForegroundTriggerKind,
    pub ingress: NormalizedIngress,
    pub received_at: DateTime<Utc>,
    pub deduplication_key: String,
    pub budget: ForegroundBudget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundTriggerKind {
    TimeSchedule,
    VolumeThreshold,
    DriftOrAnomalySignal,
    ForegroundDelegation,
    ExternalPassiveEvent,
    MaintenanceTrigger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnconsciousJobKind {
    MemoryConsolidation,
    RetrievalMaintenance,
    ContradictionAndDriftScan,
    SelfModelReflection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundExecutionBudget {
    pub iteration_budget: u32,
    pub wall_clock_budget_ms: u64,
    pub token_budget: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundTrigger {
    pub trigger_id: Uuid,
    pub trigger_kind: BackgroundTriggerKind,
    pub requested_at: DateTime<Utc>,
    pub reason_summary: String,
    pub payload_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UnconsciousScope {
    pub episode_ids: Vec<Uuid>,
    pub memory_artifact_ids: Vec<Uuid>,
    pub retrieval_artifact_ids: Vec<Uuid>,
    pub self_model_artifact_id: Option<Uuid>,
    pub internal_principal_ref: Option<String>,
    pub internal_conversation_ref: Option<String>,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnconsciousContext {
    pub context_id: Uuid,
    pub assembled_at: DateTime<Utc>,
    pub job_id: Uuid,
    pub job_kind: UnconsciousJobKind,
    pub trigger: BackgroundTrigger,
    pub scope: UnconsciousScope,
    pub budget: BackgroundExecutionBudget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsciousContext {
    pub context_id: Uuid,
    pub assembled_at: DateTime<Utc>,
    pub trigger: ForegroundTrigger,
    pub self_model: SelfModelSnapshot,
    pub internal_state: InternalStateSnapshot,
    pub recent_history: Vec<EpisodeExcerpt>,
    pub retrieved_context: RetrievedContext,
    pub recovery_context: ForegroundRecoveryContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfModelSnapshot {
    pub stable_identity: String,
    pub role: String,
    pub communication_style: String,
    pub capabilities: Vec<String>,
    pub constraints: Vec<String>,
    pub preferences: Vec<String>,
    pub current_goals: Vec<String>,
    pub current_subgoals: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InternalStateSnapshot {
    pub load_pct: u8,
    pub health_pct: u8,
    pub reliability_pct: u8,
    pub resource_pressure_pct: u8,
    pub confidence_pct: u8,
    pub connection_quality_pct: u8,
    pub active_conditions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpisodeExcerpt {
    pub episode_id: Uuid,
    pub trace_id: Uuid,
    pub started_at: DateTime<Utc>,
    pub user_message: Option<String>,
    pub assistant_message: Option<String>,
    pub outcome: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RetrievedContext {
    pub items: Vec<RetrievedContextItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum RetrievedContextItem {
    Episode(RetrievedEpisodeContext),
    MemoryArtifact(RetrievedMemoryArtifactContext),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievedEpisodeContext {
    pub episode_id: Uuid,
    pub internal_conversation_ref: String,
    pub started_at: DateTime<Utc>,
    pub summary: String,
    pub outcome: String,
    pub relevance_reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievedMemoryArtifactContext {
    pub memory_artifact_id: Uuid,
    pub artifact_kind: String,
    pub subject_ref: String,
    pub content_text: String,
    pub validity_status: String,
    pub relevance_reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForegroundRecoveryContext {
    pub mode: ForegroundExecutionMode,
    pub ordered_ingress: Vec<OrderedIngressReference>,
}

impl Default for ForegroundRecoveryContext {
    fn default() -> Self {
        Self {
            mode: ForegroundExecutionMode::SingleIngress,
            ordered_ingress: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderedIngressReference {
    pub ingress_id: Uuid,
    pub external_message_id: Option<String>,
    pub occurred_at: DateTime<Utc>,
    pub text_body: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalProposalKind {
    MemoryArtifact,
    SelfModelObservation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalTargetKind {
    MemoryArtifacts,
    SelfModelArtifacts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalConflictPosture {
    Independent,
    Revises,
    Supersedes,
    Conflicts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalProvenanceKind {
    EpisodeObservation,
    BacklogRecovery,
    SelfModelReflection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalProvenance {
    pub provenance_kind: ProposalProvenanceKind,
    pub source_ingress_ids: Vec<Uuid>,
    pub source_episode_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalProposal {
    pub proposal_id: Uuid,
    pub proposal_kind: CanonicalProposalKind,
    pub canonical_target: CanonicalTargetKind,
    pub confidence_pct: u8,
    pub conflict_posture: ProposalConflictPosture,
    pub subject_ref: String,
    pub rationale: Option<String>,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_to: Option<DateTime<Utc>>,
    pub supersedes_artifact_id: Option<Uuid>,
    pub provenance: ProposalProvenance,
    pub payload: CanonicalProposalPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum CanonicalProposalPayload {
    MemoryArtifact(MemoryArtifactProposal),
    SelfModelObservation(SelfModelObservationProposal),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryArtifactProposal {
    pub artifact_kind: String,
    pub content_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfModelObservationProposal {
    pub observation_kind: String,
    pub content_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalEvaluationOutcome {
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "artifact_id")]
pub enum MergeDecisionTarget {
    MemoryArtifact(Uuid),
    SelfModelArtifact(Uuid),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalEvaluation {
    pub proposal_id: Uuid,
    pub outcome: ProposalEvaluationOutcome,
    pub reason: String,
    pub target: Option<MergeDecisionTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalUpdateOperation {
    Upsert,
    Archive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalUpdateProposal {
    pub update_id: Uuid,
    pub operation: RetrievalUpdateOperation,
    pub source_ref: String,
    pub lexical_document: String,
    pub relevance_timestamp: DateTime<Utc>,
    pub internal_conversation_ref: Option<String>,
    pub rationale: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticAlert {
    pub alert_id: Uuid,
    pub code: String,
    pub severity: DiagnosticSeverity,
    pub summary: String,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WakeSignalReason {
    CriticalConflict,
    ProactiveBriefingReady,
    SelfStateAnomaly,
    MaintenanceInsightReady,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WakeSignalPriority {
    Low,
    Normal,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WakeSignal {
    pub signal_id: Uuid,
    pub reason: WakeSignalReason,
    pub priority: WakeSignalPriority,
    pub reason_code: String,
    pub summary: String,
    pub payload_ref: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WakeSignalDecisionKind {
    Accepted,
    Rejected,
    Suppressed,
    Deferred,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WakeSignalDecision {
    pub signal_id: Uuid,
    pub decision: WakeSignalDecisionKind,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UnconsciousMaintenanceOutputs {
    pub canonical_proposals: Vec<CanonicalProposal>,
    pub retrieval_updates: Vec<RetrievalUpdateProposal>,
    pub diagnostics: Vec<DiagnosticAlert>,
    pub wake_signals: Vec<WakeSignal>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub enum GovernedActionRiskTier {
    #[serde(rename = "tier_0")]
    Tier0,
    #[default]
    #[serde(rename = "tier_1")]
    Tier1,
    #[serde(rename = "tier_2")]
    Tier2,
    #[serde(rename = "tier_3")]
    Tier3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NetworkAccessPosture {
    #[default]
    Disabled,
    Allowlisted,
    Enabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct FilesystemCapabilityScope {
    pub read_roots: Vec<String>,
    pub write_roots: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EnvironmentCapabilityScope {
    pub allow_variables: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionCapabilityBudget {
    pub timeout_ms: u64,
    pub max_stdout_bytes: u64,
    pub max_stderr_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityScope {
    pub filesystem: FilesystemCapabilityScope,
    pub network: NetworkAccessPosture,
    pub environment: EnvironmentCapabilityScope,
    pub execution: ExecutionCapabilityBudget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernedActionKind {
    InspectWorkspaceArtifact,
    RunSubprocess,
    RunWorkspaceScript,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GovernedActionProposal {
    pub proposal_id: Uuid,
    pub title: String,
    pub rationale: Option<String>,
    pub action_kind: GovernedActionKind,
    pub requested_risk_tier: Option<GovernedActionRiskTier>,
    pub capability_scope: CapabilityScope,
    pub payload: GovernedActionPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectWorkspaceArtifactAction {
    pub artifact_id: Uuid,
    pub artifact_kind: WorkspaceArtifactKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubprocessAction {
    pub command: String,
    pub args: Vec<String>,
    pub working_directory: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceScriptAction {
    pub script_id: Uuid,
    pub script_version_id: Option<Uuid>,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum GovernedActionPayload {
    InspectWorkspaceArtifact(InspectWorkspaceArtifactAction),
    RunSubprocess(SubprocessAction),
    RunWorkspaceScript(WorkspaceScriptAction),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GovernedActionFingerprint {
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernedActionStatus {
    Proposed,
    AwaitingApproval,
    Approved,
    Rejected,
    Expired,
    Invalidated,
    Blocked,
    Executed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GovernedActionExecutionOutcome {
    pub status: GovernedActionStatus,
    pub summary: String,
    pub fingerprint: Option<GovernedActionFingerprint>,
    pub output_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GovernedActionObservation {
    pub observation_id: Uuid,
    pub action_kind: GovernedActionKind,
    pub outcome: GovernedActionExecutionOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalRequestStatus {
    Pending,
    Approved,
    Rejected,
    Expired,
    Invalidated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalResolutionDecision {
    Approved,
    Rejected,
    Expired,
    Invalidated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub approval_request_id: Uuid,
    pub action_proposal_id: Uuid,
    pub action_fingerprint: GovernedActionFingerprint,
    pub status: ApprovalRequestStatus,
    pub risk_tier: GovernedActionRiskTier,
    pub title: String,
    pub consequence_summary: String,
    pub capability_scope: CapabilityScope,
    pub requested_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalResolutionEvent {
    pub resolution_id: Uuid,
    pub approval_request_id: Uuid,
    pub decision: ApprovalResolutionDecision,
    pub resolved_by: String,
    pub resolved_at: DateTime<Utc>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceArtifactKind {
    Note,
    Runbook,
    Scratchpad,
    TaskList,
    Script,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceArtifactSummary {
    pub artifact_id: Uuid,
    pub artifact_kind: WorkspaceArtifactKind,
    pub title: String,
    pub latest_version: u32,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceScriptSummary {
    pub script_id: Uuid,
    pub workspace_artifact_id: Uuid,
    pub language: String,
    pub latest_version: u32,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceScriptVersionSummary {
    pub script_version_id: Uuid,
    pub script_id: Uuid,
    pub version: u32,
    pub content_sha256: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceScriptRunStatus {
    Pending,
    Running,
    Completed,
    Failed,
    TimedOut,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceScriptRunSummary {
    pub script_run_id: Uuid,
    pub script_id: Uuid,
    pub script_version_id: Uuid,
    pub status: WorkspaceScriptRunStatus,
    pub risk_tier: GovernedActionRiskTier,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopKind {
    Conscious,
    Unconscious,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelProviderKind {
    ZAi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelCallPurpose {
    ForegroundResponse,
    BackgroundAnalysis,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelOutputMode {
    PlainText,
    JsonObject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolPolicy {
    NoTools,
    ProposalOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelBudget {
    pub max_input_tokens: u32,
    pub max_output_tokens: u32,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelProviderHint {
    pub preferred_provider: Option<ModelProviderKind>,
    pub preferred_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInput {
    pub system_prompt: String,
    pub messages: Vec<ModelInputMessage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelMessageRole {
    System,
    Developer,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInputMessage {
    pub role: ModelMessageRole,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCallRequest {
    pub request_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Uuid,
    pub loop_kind: LoopKind,
    pub purpose: ModelCallPurpose,
    pub task_class: String,
    pub budget: ModelBudget,
    pub input: ModelInput,
    pub output_mode: ModelOutputMode,
    pub schema_name: Option<String>,
    pub schema_json: Option<Value>,
    pub tool_policy: ToolPolicy,
    pub provider_hint: Option<ModelProviderHint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCallResponse {
    pub request_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Uuid,
    pub provider: ModelProviderKind,
    pub model: String,
    pub received_at: DateTime<Utc>,
    pub output: ModelOutput,
    pub usage: ModelUsage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelOutput {
    pub text: String,
    pub json: Option<Value>,
    pub finish_reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Debug, Error)]
pub enum ContractError {
    #[error("worker request kind does not match payload kind")]
    WorkerPayloadMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke_request_round_trips() {
        let request = WorkerRequest::smoke(Uuid::now_v7(), Uuid::now_v7(), "smoke");
        let json = serde_json::to_string(&request).expect("request should serialize");
        let decoded: WorkerRequest =
            serde_json::from_str(&json).expect("request should deserialize");
        assert_eq!(request, decoded);
        decoded.validate().expect("request should be valid");
    }

    #[test]
    fn conscious_worker_request_round_trips() {
        let request = WorkerRequest::conscious(Uuid::now_v7(), Uuid::now_v7(), sample_context());
        let WorkerPayload::Conscious(payload) = &request.payload else {
            panic!("expected conscious payload");
        };
        assert_eq!(payload.request_id, request.request_id);
        assert_eq!(payload.sent_at, request.sent_at);
        let json = serde_json::to_string(&request).expect("request should serialize");
        let decoded: WorkerRequest =
            serde_json::from_str(&json).expect("request should deserialize");
        assert_eq!(request, decoded);
        decoded.validate().expect("request should be valid");
    }

    #[test]
    fn unconscious_worker_request_round_trips() {
        let request = WorkerRequest::unconscious(
            Uuid::now_v7(),
            Uuid::now_v7(),
            sample_unconscious_context(),
        );
        let WorkerPayload::Unconscious(payload) = &request.payload else {
            panic!("expected unconscious payload");
        };
        assert_eq!(payload.request_id, request.request_id);
        assert_eq!(payload.sent_at, request.sent_at);
        let json = serde_json::to_string(&request).expect("request should serialize");
        let decoded: WorkerRequest =
            serde_json::from_str(&json).expect("request should deserialize");
        assert_eq!(request, decoded);
        decoded.validate().expect("request should be valid");
    }

    #[test]
    fn normalized_ingress_round_trips_transport_fields() {
        let ingress = sample_ingress();
        let json = serde_json::to_string(&ingress).expect("ingress should serialize");
        let decoded: NormalizedIngress =
            serde_json::from_str(&json).expect("ingress should deserialize");
        assert_eq!(decoded.channel_kind, ChannelKind::Telegram);
        assert_eq!(decoded.event_kind, IngressEventKind::MessageCreated);
        assert_eq!(decoded.external_user_id, "telegram-user-42");
        assert_eq!(decoded.external_conversation_id, "telegram-chat-42");
        assert_eq!(decoded.internal_principal_ref, "primary-user");
        assert_eq!(decoded.internal_conversation_ref, "telegram-primary");
        assert_eq!(
            decoded.reply_to,
            Some(ReplyReference {
                external_message_id: "message-1".to_string(),
            })
        );
        assert_eq!(decoded.attachments.len(), 1);
        assert!(decoded.command_hint.is_some());
        assert!(decoded.approval_payload.is_some());
        assert_eq!(
            decoded.raw_payload_ref.as_deref(),
            Some("fixtures/update.json")
        );
    }

    #[test]
    fn model_call_contract_round_trips_provider_agnostic_shape() {
        let request = sample_model_call_request();
        let json = serde_json::to_string(&request).expect("model call should serialize");
        let decoded: ModelCallRequest =
            serde_json::from_str(&json).expect("model call should deserialize");
        assert_eq!(decoded.loop_kind, LoopKind::Conscious);
        assert_eq!(decoded.purpose, ModelCallPurpose::ForegroundResponse);
        assert_eq!(decoded.output_mode, ModelOutputMode::PlainText);
        assert_eq!(decoded.tool_policy, ToolPolicy::NoTools);
        assert_eq!(
            decoded.provider_hint,
            Some(ModelProviderHint {
                preferred_provider: Some(ModelProviderKind::ZAi),
                preferred_model: Some("z-ai-foreground".to_string()),
            })
        );
    }

    #[test]
    fn conscious_worker_protocol_messages_round_trip() {
        let outbound =
            ConsciousWorkerOutboundMessage::ModelCallRequest(sample_model_call_request());
        let json = serde_json::to_string(&outbound).expect("outbound message should serialize");
        let decoded: ConsciousWorkerOutboundMessage =
            serde_json::from_str(&json).expect("outbound message should deserialize");
        assert_eq!(decoded, outbound);

        let inbound =
            ConsciousWorkerInboundMessage::ModelCallResponse(sample_model_call_response());
        let json = serde_json::to_string(&inbound).expect("inbound message should serialize");
        let decoded: ConsciousWorkerInboundMessage =
            serde_json::from_str(&json).expect("inbound message should deserialize");
        assert_eq!(decoded, inbound);
    }

    #[test]
    fn canonical_proposal_contract_round_trips() {
        let proposal = sample_memory_proposal();
        let json = serde_json::to_string(&proposal).expect("proposal should serialize");
        let decoded: CanonicalProposal =
            serde_json::from_str(&json).expect("proposal should deserialize");
        assert_eq!(decoded, proposal);
        assert_eq!(decoded.proposal_kind, CanonicalProposalKind::MemoryArtifact);
        assert_eq!(
            decoded.canonical_target,
            CanonicalTargetKind::MemoryArtifacts
        );
    }

    #[test]
    fn retrieval_and_recovery_contracts_round_trip() {
        let context = sample_context();
        let json = serde_json::to_string(&context).expect("context should serialize");
        let decoded: ConsciousContext =
            serde_json::from_str(&json).expect("context should deserialize");
        assert_eq!(
            decoded.recovery_context.mode,
            ForegroundExecutionMode::BacklogRecovery
        );
        assert_eq!(decoded.retrieved_context.items.len(), 2);
    }

    #[test]
    fn unconscious_contracts_round_trip() {
        let context = sample_unconscious_context();
        let outputs = sample_unconscious_outputs();
        let json = serde_json::to_string(&context).expect("context should serialize");
        let decoded: UnconsciousContext =
            serde_json::from_str(&json).expect("context should deserialize");
        assert_eq!(decoded.job_kind, UnconsciousJobKind::MemoryConsolidation);
        assert_eq!(
            decoded.trigger.trigger_kind,
            BackgroundTriggerKind::ForegroundDelegation
        );
        assert_eq!(decoded.scope.episode_ids.len(), 2);

        let json = serde_json::to_string(&outputs).expect("outputs should serialize");
        let decoded: UnconsciousMaintenanceOutputs =
            serde_json::from_str(&json).expect("outputs should deserialize");
        assert_eq!(decoded.canonical_proposals.len(), 1);
        assert_eq!(decoded.retrieval_updates.len(), 1);
        assert_eq!(decoded.diagnostics.len(), 1);
        assert_eq!(decoded.wake_signals.len(), 1);
    }

    #[test]
    fn governed_action_contracts_round_trip() {
        let proposal = sample_governed_action_proposal();
        let json = serde_json::to_string(&proposal).expect("proposal should serialize");
        let decoded: GovernedActionProposal =
            serde_json::from_str(&json).expect("proposal should deserialize");
        assert_eq!(decoded, proposal);
        assert_eq!(decoded.action_kind, GovernedActionKind::RunSubprocess);
        assert_eq!(
            decoded.requested_risk_tier,
            Some(GovernedActionRiskTier::Tier2)
        );
    }

    #[test]
    fn approval_request_contracts_round_trip() {
        let request = sample_approval_request();
        let json = serde_json::to_string(&request).expect("request should serialize");
        let decoded: ApprovalRequest =
            serde_json::from_str(&json).expect("request should deserialize");
        assert_eq!(decoded, request);
        assert_eq!(decoded.status, ApprovalRequestStatus::Pending);
        assert_eq!(decoded.risk_tier, GovernedActionRiskTier::Tier2);
    }

    fn sample_context() -> ConsciousContext {
        ConsciousContext {
            context_id: Uuid::now_v7(),
            assembled_at: Utc::now(),
            trigger: ForegroundTrigger {
                trigger_id: Uuid::now_v7(),
                trace_id: Uuid::now_v7(),
                execution_id: Uuid::now_v7(),
                trigger_kind: ForegroundTriggerKind::UserIngress,
                ingress: sample_ingress(),
                received_at: Utc::now(),
                deduplication_key: "telegram:update-42".to_string(),
                budget: ForegroundBudget {
                    iteration_budget: 1,
                    wall_clock_budget_ms: 30_000,
                    token_budget: 4_000,
                },
            },
            self_model: SelfModelSnapshot {
                stable_identity: "blue-lagoon".to_string(),
                role: "personal_assistant".to_string(),
                communication_style: "direct".to_string(),
                capabilities: vec!["conversation".to_string(), "planning".to_string()],
                constraints: vec!["respect_harness_policy".to_string()],
                preferences: vec!["concise".to_string()],
                current_goals: vec!["support_the_user".to_string()],
                current_subgoals: vec!["reply_to_current_message".to_string()],
            },
            internal_state: InternalStateSnapshot {
                load_pct: 20,
                health_pct: 100,
                reliability_pct: 100,
                resource_pressure_pct: 10,
                confidence_pct: 75,
                connection_quality_pct: 90,
                active_conditions: vec![],
            },
            recent_history: vec![EpisodeExcerpt {
                episode_id: Uuid::now_v7(),
                trace_id: Uuid::now_v7(),
                started_at: Utc::now(),
                user_message: Some("hello".to_string()),
                assistant_message: Some("hi".to_string()),
                outcome: "completed".to_string(),
            }],
            retrieved_context: RetrievedContext {
                items: vec![
                    RetrievedContextItem::Episode(RetrievedEpisodeContext {
                        episode_id: Uuid::now_v7(),
                        internal_conversation_ref: "telegram-primary".to_string(),
                        started_at: Utc::now(),
                        summary: "Discussed travel preferences.".to_string(),
                        outcome: "completed".to_string(),
                        relevance_reason: "same_conversation_recent".to_string(),
                    }),
                    RetrievedContextItem::MemoryArtifact(RetrievedMemoryArtifactContext {
                        memory_artifact_id: Uuid::now_v7(),
                        artifact_kind: "preference".to_string(),
                        subject_ref: "user:primary".to_string(),
                        content_text: "Prefers direct answers.".to_string(),
                        validity_status: "active".to_string(),
                        relevance_reason: "lexical_match".to_string(),
                    }),
                ],
            },
            recovery_context: ForegroundRecoveryContext {
                mode: ForegroundExecutionMode::BacklogRecovery,
                ordered_ingress: vec![
                    OrderedIngressReference {
                        ingress_id: Uuid::now_v7(),
                        external_message_id: Some("message-40".to_string()),
                        occurred_at: Utc::now(),
                        text_body: Some("First delayed message".to_string()),
                    },
                    OrderedIngressReference {
                        ingress_id: Uuid::now_v7(),
                        external_message_id: Some("message-41".to_string()),
                        occurred_at: Utc::now(),
                        text_body: Some("Second delayed message".to_string()),
                    },
                ],
            },
        }
    }

    fn sample_unconscious_context() -> UnconsciousContext {
        UnconsciousContext {
            context_id: Uuid::now_v7(),
            assembled_at: Utc::now(),
            job_id: Uuid::now_v7(),
            job_kind: UnconsciousJobKind::MemoryConsolidation,
            trigger: BackgroundTrigger {
                trigger_id: Uuid::now_v7(),
                trigger_kind: BackgroundTriggerKind::ForegroundDelegation,
                requested_at: Utc::now(),
                reason_summary: "foreground requested memory consolidation".to_string(),
                payload_ref: Some("execution:latest".to_string()),
            },
            scope: UnconsciousScope {
                episode_ids: vec![Uuid::now_v7(), Uuid::now_v7()],
                memory_artifact_ids: vec![Uuid::now_v7()],
                retrieval_artifact_ids: vec![Uuid::now_v7()],
                self_model_artifact_id: Some(Uuid::now_v7()),
                internal_principal_ref: Some("primary-user".to_string()),
                internal_conversation_ref: Some("telegram-primary".to_string()),
                summary: "Consolidate recent episodes into stable memory.".to_string(),
            },
            budget: BackgroundExecutionBudget {
                iteration_budget: 2,
                wall_clock_budget_ms: 120_000,
                token_budget: 6_000,
            },
        }
    }

    fn sample_unconscious_outputs() -> UnconsciousMaintenanceOutputs {
        UnconsciousMaintenanceOutputs {
            canonical_proposals: vec![sample_memory_proposal()],
            retrieval_updates: vec![RetrievalUpdateProposal {
                update_id: Uuid::now_v7(),
                operation: RetrievalUpdateOperation::Upsert,
                source_ref: "memory_artifact:latest".to_string(),
                lexical_document: "Prefers direct answers".to_string(),
                relevance_timestamp: Utc::now(),
                internal_conversation_ref: Some("telegram-primary".to_string()),
                rationale: Some("maintain retrieval freshness".to_string()),
            }],
            diagnostics: vec![DiagnosticAlert {
                alert_id: Uuid::now_v7(),
                code: "contradiction_scan_clean".to_string(),
                severity: DiagnosticSeverity::Info,
                summary: "No contradictions detected in scoped memory.".to_string(),
                details: None,
            }],
            wake_signals: vec![WakeSignal {
                signal_id: Uuid::now_v7(),
                reason: WakeSignalReason::MaintenanceInsightReady,
                priority: WakeSignalPriority::Low,
                reason_code: "maintenance_insight_ready".to_string(),
                summary: "Background maintenance found a useful summary.".to_string(),
                payload_ref: Some("background_job_run:latest".to_string()),
            }],
        }
    }

    fn sample_memory_proposal() -> CanonicalProposal {
        CanonicalProposal {
            proposal_id: Uuid::now_v7(),
            proposal_kind: CanonicalProposalKind::MemoryArtifact,
            canonical_target: CanonicalTargetKind::MemoryArtifacts,
            confidence_pct: 92,
            conflict_posture: ProposalConflictPosture::Independent,
            subject_ref: "user:primary".to_string(),
            rationale: Some("Observed a stable preference during conversation.".to_string()),
            valid_from: Some(Utc::now()),
            valid_to: None,
            supersedes_artifact_id: None,
            provenance: ProposalProvenance {
                provenance_kind: ProposalProvenanceKind::EpisodeObservation,
                source_ingress_ids: vec![Uuid::now_v7()],
                source_episode_id: Some(Uuid::now_v7()),
            },
            payload: CanonicalProposalPayload::MemoryArtifact(MemoryArtifactProposal {
                artifact_kind: "preference".to_string(),
                content_text: "Prefers concise replies.".to_string(),
            }),
        }
    }

    fn sample_ingress() -> NormalizedIngress {
        NormalizedIngress {
            ingress_id: Uuid::now_v7(),
            channel_kind: ChannelKind::Telegram,
            external_user_id: "telegram-user-42".to_string(),
            external_conversation_id: "telegram-chat-42".to_string(),
            external_event_id: "update-42".to_string(),
            external_message_id: Some("message-42".to_string()),
            internal_principal_ref: "primary-user".to_string(),
            internal_conversation_ref: "telegram-primary".to_string(),
            event_kind: IngressEventKind::MessageCreated,
            occurred_at: Utc::now(),
            text_body: Some("hello".to_string()),
            reply_to: Some(ReplyReference {
                external_message_id: "message-1".to_string(),
            }),
            attachments: vec![AttachmentReference {
                attachment_id: "file-1".to_string(),
                media_type: Some("image/jpeg".to_string()),
                file_name: Some("photo.jpg".to_string()),
                size_bytes: Some(128),
            }],
            command_hint: Some(CommandHint {
                command: "/start".to_string(),
                args: vec!["foo".to_string()],
            }),
            approval_payload: Some(ApprovalPayload {
                token: "approval-token".to_string(),
                callback_data: Some("approve:123".to_string()),
            }),
            raw_payload_ref: Some("fixtures/update.json".to_string()),
        }
    }

    fn sample_model_call_request() -> ModelCallRequest {
        ModelCallRequest {
            request_id: Uuid::now_v7(),
            trace_id: Uuid::now_v7(),
            execution_id: Uuid::now_v7(),
            loop_kind: LoopKind::Conscious,
            purpose: ModelCallPurpose::ForegroundResponse,
            task_class: "telegram_foreground_reply".to_string(),
            budget: ModelBudget {
                max_input_tokens: 4_000,
                max_output_tokens: 800,
                timeout_ms: 30_000,
            },
            input: ModelInput {
                system_prompt: "You are Blue Lagoon.".to_string(),
                messages: vec![
                    ModelInputMessage {
                        role: ModelMessageRole::Developer,
                        content: "Stay concise.".to_string(),
                    },
                    ModelInputMessage {
                        role: ModelMessageRole::User,
                        content: "hello".to_string(),
                    },
                ],
            },
            output_mode: ModelOutputMode::PlainText,
            schema_name: None,
            schema_json: None,
            tool_policy: ToolPolicy::NoTools,
            provider_hint: Some(ModelProviderHint {
                preferred_provider: Some(ModelProviderKind::ZAi),
                preferred_model: Some("z-ai-foreground".to_string()),
            }),
        }
    }

    fn sample_model_call_response() -> ModelCallResponse {
        ModelCallResponse {
            request_id: Uuid::now_v7(),
            trace_id: Uuid::now_v7(),
            execution_id: Uuid::now_v7(),
            provider: ModelProviderKind::ZAi,
            model: "z-ai-foreground".to_string(),
            received_at: Utc::now(),
            output: ModelOutput {
                text: "hello from model".to_string(),
                json: None,
                finish_reason: "stop".to_string(),
            },
            usage: ModelUsage {
                input_tokens: 20,
                output_tokens: 5,
            },
        }
    }

    fn sample_governed_action_proposal() -> GovernedActionProposal {
        GovernedActionProposal {
            proposal_id: Uuid::now_v7(),
            title: "Run a bounded harness verification".to_string(),
            rationale: Some(
                "The current state should be checked through a scoped subprocess.".to_string(),
            ),
            action_kind: GovernedActionKind::RunSubprocess,
            requested_risk_tier: Some(GovernedActionRiskTier::Tier2),
            capability_scope: sample_capability_scope(),
            payload: GovernedActionPayload::RunSubprocess(SubprocessAction {
                command: "cargo".to_string(),
                args: vec!["test".to_string(), "-p".to_string(), "harness".to_string()],
                working_directory: Some("D:/Repos/blue-lagoon".to_string()),
            }),
        }
    }

    fn sample_approval_request() -> ApprovalRequest {
        ApprovalRequest {
            approval_request_id: Uuid::now_v7(),
            action_proposal_id: Uuid::now_v7(),
            action_fingerprint: GovernedActionFingerprint {
                value: "sha256:proposal-fingerprint".to_string(),
            },
            status: ApprovalRequestStatus::Pending,
            risk_tier: GovernedActionRiskTier::Tier2,
            title: "Run a scoped harness test".to_string(),
            consequence_summary:
                "Runs a bounded local subprocess inside the configured workspace scope.".to_string(),
            capability_scope: sample_capability_scope(),
            requested_at: Utc::now(),
            expires_at: Utc::now(),
        }
    }

    fn sample_capability_scope() -> CapabilityScope {
        CapabilityScope {
            filesystem: FilesystemCapabilityScope {
                read_roots: vec!["D:/Repos/blue-lagoon".to_string()],
                write_roots: vec!["D:/Repos/blue-lagoon/docs".to_string()],
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
}
