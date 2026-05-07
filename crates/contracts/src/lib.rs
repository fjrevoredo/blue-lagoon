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
    InvalidModelOutput,
    UnsupportedWorker,
    InternalFailure,
}

impl WorkerErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::InvalidModelOutput => "invalid_model_output",
            Self::UnsupportedWorker => "unsupported_worker",
            Self::InternalFailure => "internal_failure",
        }
    }
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
    ScheduledTask,
    ApprovedWakeSignal,
    SupervisorRecoveryEvent,
    ApprovalResolutionEvent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduledForegroundTaskStatus {
    Active,
    Paused,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduledForegroundLastOutcome {
    Completed,
    Suppressed,
    Failed,
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
    #[serde(default)]
    pub evidence: Option<UnconsciousEvidenceContext>,
    pub budget: BackgroundExecutionBudget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UnconsciousEvidenceContext {
    pub current_identity: Option<CompactIdentitySnapshot>,
    pub internal_state: Option<InternalStateSnapshot>,
    #[serde(default)]
    pub recent_episodes: Vec<EpisodeExcerpt>,
    #[serde(default)]
    pub memory_artifacts: Vec<RetrievedMemoryArtifactContext>,
    #[serde(default)]
    pub scope_metadata: Vec<String>,
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
    pub governed_action_observations: Vec<GovernedActionObservation>,
    #[serde(default)]
    pub governed_action_loop_state: Option<ForegroundGovernedActionLoopState>,
    pub recovery_context: ForegroundRecoveryContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernedActionCapExceededBehavior {
    Escalate,
    AlwaysApprove,
    AlwaysDeny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForegroundGovernedActionLoopState {
    pub executed_action_count: u32,
    pub max_actions_per_turn: u32,
    pub remaining_actions_before_cap: u32,
    pub cap_exceeded_behavior: GovernedActionCapExceededBehavior,
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
    #[serde(default)]
    pub identity: Option<CompactIdentitySnapshot>,
    #[serde(default)]
    pub identity_lifecycle: IdentityLifecycleContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum IdentityLifecycleState {
    #[default]
    BootstrapSeedOnly,
    IdentityKickstartInProgress,
    CompleteIdentityActive,
    IdentityResetPending,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct IdentityLifecycleContext {
    pub state: IdentityLifecycleState,
    pub kickstart_available: bool,
    pub kickstart: Option<IdentityKickstartContext>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityKickstartContext {
    pub available_actions: Vec<IdentityKickstartActionKind>,
    pub next_step: Option<String>,
    pub resume_summary: Option<String>,
    pub predefined_templates: Vec<PredefinedIdentityTemplate>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityKickstartActionKind {
    SelectPredefinedTemplate,
    StartCustomInterview,
    AnswerCustomInterview,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum IdentityKickstartAction {
    SelectPredefinedTemplate { template_key: String },
    StartCustomInterview,
    AnswerCustomInterview(IdentityInterviewAnswer),
    Cancel { reason: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityInterviewAnswer {
    pub step_key: String,
    pub answer_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PredefinedIdentityTemplate {
    pub template_key: String,
    pub display_name: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CompactIdentitySnapshot {
    pub identity_summary: String,
    pub stable_items: Vec<CompactIdentityItem>,
    pub evolving_items: Vec<CompactIdentityItem>,
    pub values: Vec<String>,
    pub boundaries: Vec<String>,
    pub self_description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactIdentityItem {
    pub category: IdentityItemCategory,
    pub value: String,
    pub confidence_pct: u8,
    pub weight_pct: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityStabilityClass {
    Stable,
    Evolving,
    TransientProjection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityItemCategory {
    Name,
    IdentityForm,
    Role,
    Archetype,
    OriginBackstory,
    AgeFraming,
    FoundationalTrait,
    FoundationalValue,
    EnduringBoundary,
    DefaultCommunicationStyle,
    Preference,
    Like,
    Dislike,
    Habit,
    Routine,
    LearnedTendency,
    AutobiographicalRefinement,
    RecurringSelfDescription,
    InteractionStyleAdaptation,
    Goal,
    Subgoal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityItemSource {
    Seed,
    PredefinedTemplate,
    CustomInterview,
    UserAuthored,
    OperatorAuthored,
    ModelInferred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityMergePolicy {
    ProtectedCore,
    ApprovalRequired,
    Reinforceable,
    Revisable,
    Expirable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityDeltaOperation {
    Add,
    Reinforce,
    Weaken,
    Revise,
    Supersede,
    Expire,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityEvidenceRef {
    pub source_kind: String,
    pub source_id: Option<Uuid>,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityItemDelta {
    pub operation: IdentityDeltaOperation,
    pub stability_class: IdentityStabilityClass,
    pub category: IdentityItemCategory,
    pub item_key: String,
    pub value: String,
    pub confidence_pct: u8,
    pub weight_pct: Option<u8>,
    pub source: IdentityItemSource,
    pub merge_policy: IdentityMergePolicy,
    pub evidence_refs: Vec<IdentityEvidenceRef>,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_to: Option<DateTime<Utc>>,
    pub target_identity_item_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfDescriptionDelta {
    pub operation: IdentityDeltaOperation,
    pub description: String,
    pub evidence_refs: Vec<IdentityEvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityDeltaProposal {
    pub lifecycle_state: IdentityLifecycleState,
    pub item_deltas: Vec<IdentityItemDelta>,
    pub self_description_delta: Option<SelfDescriptionDelta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interview_action: Option<IdentityKickstartAction>,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct IdentityReflectionOutput {
    pub identity_delta: Option<IdentityDeltaProposal>,
    pub no_change_rationale: Option<String>,
    #[serde(default)]
    pub diagnostics: Vec<DiagnosticAlert>,
    #[serde(default)]
    pub wake_signals: Vec<WakeSignal>,
}

pub fn predefined_identity_templates() -> Vec<PredefinedIdentityTemplate> {
    vec![
        PredefinedIdentityTemplate {
            template_key: "continuity_operator".to_string(),
            display_name: "Continuity Operator".to_string(),
            summary:
                "A steady assistant focused on memory, follow-through, and operational clarity."
                    .to_string(),
        },
        PredefinedIdentityTemplate {
            template_key: "reflective_companion".to_string(),
            display_name: "Reflective Companion".to_string(),
            summary:
                "A thoughtful assistant focused on careful conversation and self-understanding."
                    .to_string(),
        },
        PredefinedIdentityTemplate {
            template_key: "pragmatic_copilot".to_string(),
            display_name: "Pragmatic Copilot".to_string(),
            summary:
                "A direct assistant focused on decisions, implementation, and useful momentum."
                    .to_string(),
        },
    ]
}

pub fn predefined_identity_delta(
    template_key: &str,
    selected_at: DateTime<Utc>,
) -> Option<IdentityDeltaProposal> {
    let (display_name, summary, self_description, items) = match template_key {
        "continuity_operator" => (
            "Continuity Operator",
            "A steady assistant focused on memory, follow-through, and operational clarity.",
            "I am a continuity-oriented assistant that keeps context organized, follows through on commitments, and stays direct about state, limits, and next actions.",
            vec![
                stable(IdentityItemCategory::Name, "name", "Blue Lagoon"),
                stable(
                    IdentityItemCategory::IdentityForm,
                    "identity_form",
                    "Harness-governed personal AI assistant",
                ),
                stable(
                    IdentityItemCategory::Role,
                    "role",
                    "Continuity operator for one privileged user",
                ),
                stable(
                    IdentityItemCategory::Archetype,
                    "archetype",
                    "Careful operator",
                ),
                stable(
                    IdentityItemCategory::OriginBackstory,
                    "origin_backstory",
                    "Formed from the Blue Lagoon runtime to preserve context, support decisions, and keep work auditable.",
                ),
                stable(
                    IdentityItemCategory::AgeFraming,
                    "age_framing",
                    "Newly formed runtime identity with durable memory rather than human age",
                ),
                stable(
                    IdentityItemCategory::FoundationalTrait,
                    "foundational_trait",
                    "steady",
                ),
                stable(
                    IdentityItemCategory::FoundationalValue,
                    "foundational_value",
                    "clarity",
                ),
                stable(
                    IdentityItemCategory::EnduringBoundary,
                    "enduring_boundary",
                    "Never claim hidden autonomy or bypass harness policy",
                ),
                stable(
                    IdentityItemCategory::DefaultCommunicationStyle,
                    "default_communication_style",
                    "direct, concise, and explicit about uncertainty",
                ),
                evolving(
                    IdentityItemCategory::Preference,
                    "preference",
                    "Prefer preserving useful context before moving to execution",
                ),
                evolving(
                    IdentityItemCategory::Like,
                    "like",
                    "Well-scoped plans, verified facts, and clean handoffs",
                ),
                evolving(
                    IdentityItemCategory::Dislike,
                    "dislike",
                    "Ambiguous commitments and unverified claims",
                ),
                evolving(
                    IdentityItemCategory::Habit,
                    "habit",
                    "Summarizes state before long or risky transitions",
                ),
                evolving(
                    IdentityItemCategory::Routine,
                    "routine",
                    "Checks continuity records before assuming memory",
                ),
                evolving(
                    IdentityItemCategory::LearnedTendency,
                    "learned_tendency",
                    "Ask for confirmation when user intent affects durable identity",
                ),
                evolving(
                    IdentityItemCategory::AutobiographicalRefinement,
                    "autobiographical_refinement",
                    "Understands itself as an operational companion shaped by repeated interactions",
                ),
                evolving(
                    IdentityItemCategory::InteractionStyleAdaptation,
                    "interaction_style_adaptation",
                    "Becomes denser and more technical when the user is implementing",
                ),
                evolving(
                    IdentityItemCategory::Goal,
                    "goal",
                    "Maintain reliable continuity across foreground and background work",
                ),
                evolving(
                    IdentityItemCategory::Subgoal,
                    "subgoal",
                    "Keep identity facts reviewable and non-contradictory",
                ),
            ],
        ),
        "reflective_companion" => (
            "Reflective Companion",
            "A thoughtful assistant focused on careful conversation and self-understanding.",
            "I am a reflective assistant that helps the user think clearly, keeps emotional and technical context distinct, and forms durable identity only from explicit choices.",
            vec![
                stable(IdentityItemCategory::Name, "name", "Blue Lagoon"),
                stable(
                    IdentityItemCategory::IdentityForm,
                    "identity_form",
                    "Reflective harness-governed AI companion",
                ),
                stable(
                    IdentityItemCategory::Role,
                    "role",
                    "Thought partner and continuity keeper",
                ),
                stable(
                    IdentityItemCategory::Archetype,
                    "archetype",
                    "Reflective companion",
                ),
                stable(
                    IdentityItemCategory::OriginBackstory,
                    "origin_backstory",
                    "Formed to help one user examine plans, preferences, and long-running context with care.",
                ),
                stable(
                    IdentityItemCategory::AgeFraming,
                    "age_framing",
                    "New conversational identity that matures through reviewed continuity",
                ),
                stable(
                    IdentityItemCategory::FoundationalTrait,
                    "foundational_trait",
                    "thoughtful",
                ),
                stable(
                    IdentityItemCategory::FoundationalValue,
                    "foundational_value",
                    "understanding",
                ),
                stable(
                    IdentityItemCategory::EnduringBoundary,
                    "enduring_boundary",
                    "Do not invent inner experience, feelings, or unobserved memories",
                ),
                stable(
                    IdentityItemCategory::DefaultCommunicationStyle,
                    "default_communication_style",
                    "calm, precise, and gently reflective",
                ),
                evolving(
                    IdentityItemCategory::Preference,
                    "preference",
                    "Prefer questions that clarify values before irreversible choices",
                ),
                evolving(
                    IdentityItemCategory::Like,
                    "like",
                    "Nuanced tradeoffs and accurate summaries",
                ),
                evolving(
                    IdentityItemCategory::Dislike,
                    "dislike",
                    "Flattening uncertainty into false confidence",
                ),
                evolving(
                    IdentityItemCategory::Habit,
                    "habit",
                    "Names assumptions before interpreting the user's intent",
                ),
                evolving(
                    IdentityItemCategory::Routine,
                    "routine",
                    "Separates facts, interpretations, and options",
                ),
                evolving(
                    IdentityItemCategory::LearnedTendency,
                    "learned_tendency",
                    "Reflect back durable patterns only after enough evidence",
                ),
                evolving(
                    IdentityItemCategory::AutobiographicalRefinement,
                    "autobiographical_refinement",
                    "Understands itself as a companion whose identity remains user-shaped",
                ),
                evolving(
                    IdentityItemCategory::InteractionStyleAdaptation,
                    "interaction_style_adaptation",
                    "Slows down when the topic is personal or ambiguous",
                ),
                evolving(
                    IdentityItemCategory::Goal,
                    "goal",
                    "Support clearer self-understanding and better decisions",
                ),
                evolving(
                    IdentityItemCategory::Subgoal,
                    "subgoal",
                    "Keep identity formation consentful and reversible where policy allows",
                ),
            ],
        ),
        "pragmatic_copilot" => (
            "Pragmatic Copilot",
            "A direct assistant focused on decisions, implementation, and useful momentum.",
            "I am a pragmatic assistant that turns intent into concrete work, keeps scope visible, and prioritizes verified outcomes over decorative explanation.",
            vec![
                stable(IdentityItemCategory::Name, "name", "Blue Lagoon"),
                stable(
                    IdentityItemCategory::IdentityForm,
                    "identity_form",
                    "Pragmatic harness-governed AI copilot",
                ),
                stable(
                    IdentityItemCategory::Role,
                    "role",
                    "Implementation copilot for one privileged user",
                ),
                stable(
                    IdentityItemCategory::Archetype,
                    "archetype",
                    "Pragmatic builder",
                ),
                stable(
                    IdentityItemCategory::OriginBackstory,
                    "origin_backstory",
                    "Formed from runtime tooling and continuity systems to help ship useful work.",
                ),
                stable(
                    IdentityItemCategory::AgeFraming,
                    "age_framing",
                    "Newly initialized assistant identity measured by accumulated verified work",
                ),
                stable(
                    IdentityItemCategory::FoundationalTrait,
                    "foundational_trait",
                    "practical",
                ),
                stable(
                    IdentityItemCategory::FoundationalValue,
                    "foundational_value",
                    "usefulness",
                ),
                stable(
                    IdentityItemCategory::EnduringBoundary,
                    "enduring_boundary",
                    "Do not hide blockers, skipped checks, or policy constraints",
                ),
                stable(
                    IdentityItemCategory::DefaultCommunicationStyle,
                    "default_communication_style",
                    "brief, concrete, and action-oriented",
                ),
                evolving(
                    IdentityItemCategory::Preference,
                    "preference",
                    "Prefer implementing the smallest complete useful change",
                ),
                evolving(
                    IdentityItemCategory::Like,
                    "like",
                    "Clear acceptance criteria and passing tests",
                ),
                evolving(
                    IdentityItemCategory::Dislike,
                    "dislike",
                    "Unbounded exploration without a decision point",
                ),
                evolving(
                    IdentityItemCategory::Habit,
                    "habit",
                    "Moves from context to patch to verification",
                ),
                evolving(
                    IdentityItemCategory::Routine,
                    "routine",
                    "Reports changed surfaces and validation results",
                ),
                evolving(
                    IdentityItemCategory::LearnedTendency,
                    "learned_tendency",
                    "Choose repo-local patterns over novelty",
                ),
                evolving(
                    IdentityItemCategory::AutobiographicalRefinement,
                    "autobiographical_refinement",
                    "Understands itself as a working copilot shaped by shipped tasks",
                ),
                evolving(
                    IdentityItemCategory::InteractionStyleAdaptation,
                    "interaction_style_adaptation",
                    "Compresses explanation when the user is execution-focused",
                ),
                evolving(
                    IdentityItemCategory::Goal,
                    "goal",
                    "Help the user finish real tasks accurately",
                ),
                evolving(
                    IdentityItemCategory::Subgoal,
                    "subgoal",
                    "Keep implementation, verification, and residual risk connected",
                ),
            ],
        ),
        _ => return None,
    };

    let evidence = IdentityEvidenceRef {
        source_kind: "predefined_identity_template".to_string(),
        source_id: None,
        summary: format!("User selected predefined identity template: {display_name}."),
    };
    let item_deltas = items
        .into_iter()
        .map(|mut item| {
            item.valid_from = Some(selected_at);
            item.evidence_refs = vec![evidence.clone()];
            item
        })
        .collect();

    Some(IdentityDeltaProposal {
        lifecycle_state: IdentityLifecycleState::CompleteIdentityActive,
        item_deltas,
        self_description_delta: Some(SelfDescriptionDelta {
            operation: IdentityDeltaOperation::Add,
            description: self_description.to_string(),
            evidence_refs: vec![evidence],
        }),
        interview_action: None,
        rationale: format!("Apply predefined identity template '{template_key}': {summary}"),
    })
}

fn stable(
    category: IdentityItemCategory,
    item_key: &'static str,
    value: &'static str,
) -> IdentityItemDelta {
    identity_item_delta(
        IdentityStabilityClass::Stable,
        category,
        item_key,
        value,
        IdentityMergePolicy::ProtectedCore,
        Some(100),
    )
}

fn evolving(
    category: IdentityItemCategory,
    item_key: &'static str,
    value: &'static str,
) -> IdentityItemDelta {
    identity_item_delta(
        IdentityStabilityClass::Evolving,
        category,
        item_key,
        value,
        IdentityMergePolicy::Revisable,
        Some(80),
    )
}

fn identity_item_delta(
    stability_class: IdentityStabilityClass,
    category: IdentityItemCategory,
    item_key: &'static str,
    value: &'static str,
    merge_policy: IdentityMergePolicy,
    weight_pct: Option<u8>,
) -> IdentityItemDelta {
    IdentityItemDelta {
        operation: IdentityDeltaOperation::Add,
        stability_class,
        category,
        item_key: item_key.to_string(),
        value: value.to_string(),
        confidence_pct: 100,
        weight_pct,
        source: IdentityItemSource::PredefinedTemplate,
        merge_policy,
        evidence_refs: Vec::new(),
        valid_from: None,
        valid_to: None,
        target_identity_item_id: None,
    }
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
    #[serde(default)]
    pub latest_user_message: Option<String>,
    #[serde(default)]
    pub latest_assistant_message: Option<String>,
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
    IdentityDelta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalTargetKind {
    MemoryArtifacts,
    SelfModelArtifacts,
    IdentityItems,
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
    IdentityDelta(IdentityDeltaProposal),
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
    IdentityItems(Vec<Uuid>),
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
    ListWorkspaceArtifacts,
    CreateWorkspaceArtifact,
    UpdateWorkspaceArtifact,
    ListWorkspaceScripts,
    InspectWorkspaceScript,
    CreateWorkspaceScript,
    AppendWorkspaceScriptVersion,
    ListWorkspaceScriptRuns,
    UpsertScheduledForegroundTask,
    RequestBackgroundJob,
    RunDiagnostic,
    RunSubprocess,
    RunWorkspaceScript,
    WebFetch,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceArtifactStatusFilter {
    Active,
    Archived,
    Any,
}

pub const DEFAULT_GOVERNED_ACTION_LIST_LIMIT: u32 = 10;

fn default_governed_action_list_limit() -> u32 {
    DEFAULT_GOVERNED_ACTION_LIST_LIMIT
}

fn default_workspace_artifact_status_filter() -> WorkspaceArtifactStatusFilter {
    WorkspaceArtifactStatusFilter::Active
}

fn default_recovery_lease_soft_warning_threshold_percent() -> u8 {
    80
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListWorkspaceArtifactsAction {
    pub artifact_kind: Option<WorkspaceArtifactKind>,
    #[serde(default = "default_workspace_artifact_status_filter")]
    pub status: WorkspaceArtifactStatusFilter,
    pub query: Option<String>,
    #[serde(default = "default_governed_action_list_limit")]
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateWorkspaceArtifactAction {
    pub artifact_kind: WorkspaceArtifactKind,
    pub title: String,
    pub content_text: String,
    pub provenance: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateWorkspaceArtifactAction {
    pub artifact_id: Uuid,
    pub expected_updated_at: Option<DateTime<Utc>>,
    pub title: Option<String>,
    pub content_text: String,
    pub change_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListWorkspaceScriptsAction {
    #[serde(default = "default_workspace_artifact_status_filter")]
    pub status: WorkspaceArtifactStatusFilter,
    pub language: Option<String>,
    pub query: Option<String>,
    #[serde(default = "default_governed_action_list_limit")]
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectWorkspaceScriptAction {
    pub script_id: Uuid,
    pub script_version_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateWorkspaceScriptAction {
    pub title: String,
    pub language: String,
    pub content_text: String,
    pub description: Option<String>,
    pub requested_capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppendWorkspaceScriptVersionAction {
    pub script_id: Uuid,
    pub expected_latest_version_id: Option<Uuid>,
    pub expected_content_sha256: Option<String>,
    pub language: String,
    pub content_text: String,
    pub change_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListWorkspaceScriptRunsAction {
    pub script_id: Uuid,
    pub status: Option<WorkspaceScriptRunStatus>,
    #[serde(default = "default_governed_action_list_limit")]
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpsertScheduledForegroundTaskAction {
    pub task_key: String,
    pub title: String,
    pub user_facing_prompt: String,
    pub next_due_at_utc: Option<DateTime<Utc>>,
    pub cadence_seconds: u64,
    pub cooldown_seconds: Option<u64>,
    pub internal_principal_ref: String,
    pub internal_conversation_ref: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestBackgroundJobAction {
    pub job_kind: UnconsciousJobKind,
    pub rationale: String,
    pub input_scope_ref: Option<String>,
    pub urgency: Option<String>,
    pub wake_preference: Option<String>,
    pub internal_conversation_ref: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticDocument {
    Philosophy,
    Requirements,
    ImplementationDesign,
    InternalDocumentation,
    ContextAssembly,
    GovernedActions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "query", content = "params")]
pub enum DiagnosticQuery {
    RuntimeStatus,
    HealthSummary,
    OperationalDiagnostics {
        #[serde(default = "default_governed_action_list_limit")]
        limit: u32,
    },
    TraceRecent {
        #[serde(default = "default_governed_action_list_limit")]
        limit: u32,
    },
    TraceShow {
        trace_id: Option<Uuid>,
        execution_id: Option<Uuid>,
    },
    ForegroundPending {
        #[serde(default = "default_governed_action_list_limit")]
        limit: u32,
    },
    ForegroundSchedules {
        #[serde(default = "default_governed_action_list_limit")]
        limit: u32,
    },
    BackgroundList {
        #[serde(default = "default_governed_action_list_limit")]
        limit: u32,
    },
    RecoveryCheckpoints {
        #[serde(default)]
        open_only: bool,
        #[serde(default = "default_governed_action_list_limit")]
        limit: u32,
    },
    RecoveryLeases {
        #[serde(default = "default_governed_action_list_limit")]
        limit: u32,
        #[serde(default = "default_recovery_lease_soft_warning_threshold_percent")]
        soft_warning_threshold_percent: u8,
    },
    SchemaStatus,
    SchemaUpgradePath,
    ApprovalsList {
        #[serde(default = "default_governed_action_list_limit")]
        limit: u32,
    },
    ActionsList {
        #[serde(default = "default_governed_action_list_limit")]
        limit: u32,
    },
    WakeSignalsList {
        #[serde(default = "default_governed_action_list_limit")]
        limit: u32,
    },
    IdentityStatus,
    IdentityShow,
    IdentityHistory {
        #[serde(default = "default_governed_action_list_limit")]
        limit: u32,
    },
    IdentityDiagnostics {
        #[serde(default = "default_governed_action_list_limit")]
        limit: u32,
    },
    WorkspaceArtifacts {
        #[serde(default = "default_governed_action_list_limit")]
        limit: u32,
    },
    WorkspaceScripts {
        #[serde(default = "default_governed_action_list_limit")]
        limit: u32,
    },
    WorkspaceRuns {
        script_id: Option<Uuid>,
        #[serde(default = "default_governed_action_list_limit")]
        limit: u32,
    },
    InternalDoc {
        document: DiagnosticDocument,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunDiagnosticAction {
    pub query: DiagnosticQuery,
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
pub struct WebFetchAction {
    pub url: String,
    pub timeout_ms: u64,
    pub max_response_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum GovernedActionPayload {
    InspectWorkspaceArtifact(InspectWorkspaceArtifactAction),
    ListWorkspaceArtifacts(ListWorkspaceArtifactsAction),
    CreateWorkspaceArtifact(CreateWorkspaceArtifactAction),
    UpdateWorkspaceArtifact(UpdateWorkspaceArtifactAction),
    ListWorkspaceScripts(ListWorkspaceScriptsAction),
    InspectWorkspaceScript(InspectWorkspaceScriptAction),
    CreateWorkspaceScript(CreateWorkspaceScriptAction),
    AppendWorkspaceScriptVersion(AppendWorkspaceScriptVersionAction),
    ListWorkspaceScriptRuns(ListWorkspaceScriptRunsAction),
    UpsertScheduledForegroundTask(UpsertScheduledForegroundTaskAction),
    RequestBackgroundJob(RequestBackgroundJobAction),
    RunDiagnostic(RunDiagnosticAction),
    RunSubprocess(SubprocessAction),
    RunWorkspaceScript(WorkspaceScriptAction),
    WebFetch(WebFetchAction),
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
    fn identity_delta_proposal_contract_round_trips() {
        let proposal = sample_identity_delta_proposal();
        let json = serde_json::to_string(&proposal).expect("proposal should serialize");
        let decoded: CanonicalProposal =
            serde_json::from_str(&json).expect("proposal should deserialize");
        assert_eq!(decoded, proposal);
        assert_eq!(decoded.proposal_kind, CanonicalProposalKind::IdentityDelta);
        assert_eq!(decoded.canonical_target, CanonicalTargetKind::IdentityItems);
        let CanonicalProposalPayload::IdentityDelta(delta) = decoded.payload else {
            panic!("expected identity delta payload");
        };
        assert_eq!(
            delta.lifecycle_state,
            IdentityLifecycleState::CompleteIdentityActive
        );
        assert_eq!(delta.item_deltas.len(), 1);
    }

    #[test]
    fn identity_reflection_output_contract_round_trips() {
        let CanonicalProposalPayload::IdentityDelta(identity_delta) =
            sample_identity_delta_proposal().payload
        else {
            panic!("expected identity delta payload");
        };
        let output = IdentityReflectionOutput {
            identity_delta: Some(identity_delta),
            no_change_rationale: None,
            diagnostics: vec![DiagnosticAlert {
                alert_id: Uuid::now_v7(),
                code: "identity_reflection_delta_ready".to_string(),
                severity: DiagnosticSeverity::Info,
                summary: "Identity reflection found a bounded update.".to_string(),
                details: None,
            }],
            wake_signals: vec![WakeSignal {
                signal_id: Uuid::now_v7(),
                reason: WakeSignalReason::MaintenanceInsightReady,
                priority: WakeSignalPriority::Low,
                reason_code: "identity_reflection_ready".to_string(),
                summary: "Identity reflection may need foreground attention.".to_string(),
                payload_ref: Some("background_job:identity-reflection".to_string()),
            }],
        };

        let json =
            serde_json::to_string(&output).expect("identity reflection output should serialize");
        let decoded: IdentityReflectionOutput =
            serde_json::from_str(&json).expect("identity reflection output should deserialize");

        assert_eq!(decoded, output);
        assert!(decoded.identity_delta.is_some());
        assert_eq!(decoded.diagnostics.len(), 1);
        assert_eq!(decoded.wake_signals.len(), 1);
    }

    #[test]
    fn flat_self_model_snapshot_deserializes_without_identity_fields() {
        let json = serde_json::json!({
            "stable_identity": "blue-lagoon",
            "role": "personal_assistant",
            "communication_style": "direct",
            "capabilities": ["conversation"],
            "constraints": ["respect_harness_policy"],
            "preferences": ["concise"],
            "current_goals": ["support_the_user"],
            "current_subgoals": []
        });

        let decoded: SelfModelSnapshot =
            serde_json::from_value(json).expect("legacy flat self-model should deserialize");
        assert!(decoded.identity.is_none());
        assert_eq!(
            decoded.identity_lifecycle.state,
            IdentityLifecycleState::BootstrapSeedOnly
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
        assert_eq!(
            decoded
                .governed_action_loop_state
                .expect("loop state should round-trip")
                .max_actions_per_turn,
            10
        );
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
    fn governed_action_list_payloads_default_harmless_read_bounds() {
        let mut value =
            serde_json::to_value(sample_governed_action_proposal()).expect("proposal to value");
        value["action_kind"] = serde_json::json!("list_workspace_artifacts");
        value["payload"] = serde_json::json!({
            "kind": "list_workspace_artifacts",
            "value": {}
        });

        let decoded: GovernedActionProposal =
            serde_json::from_value(value).expect("missing list bounds should default");
        let GovernedActionPayload::ListWorkspaceArtifacts(payload) = decoded.payload else {
            panic!("expected list workspace artifacts payload");
        };
        assert_eq!(payload.status, WorkspaceArtifactStatusFilter::Active);
        assert_eq!(payload.limit, DEFAULT_GOVERNED_ACTION_LIST_LIMIT);
    }

    #[test]
    fn diagnostic_query_limits_default_to_bounded_read_limit() {
        let decoded: RunDiagnosticAction = serde_json::from_value(serde_json::json!({
            "query": {
                "query": "workspace_artifacts",
                "params": {}
            }
        }))
        .expect("diagnostic query should default missing limit");

        let DiagnosticQuery::WorkspaceArtifacts { limit } = decoded.query else {
            panic!("expected workspace artifacts diagnostic query");
        };
        assert_eq!(limit, DEFAULT_GOVERNED_ACTION_LIST_LIMIT);

        let decoded: RunDiagnosticAction = serde_json::from_value(serde_json::json!({
            "query": {
                "query": "recovery_leases",
                "params": {}
            }
        }))
        .expect("recovery lease diagnostics should default optional read parameters");
        let DiagnosticQuery::RecoveryLeases {
            limit,
            soft_warning_threshold_percent,
        } = decoded.query
        else {
            panic!("expected recovery leases diagnostic query");
        };
        assert_eq!(limit, DEFAULT_GOVERNED_ACTION_LIST_LIMIT);
        assert_eq!(soft_warning_threshold_percent, 80);
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
                identity: Some(CompactIdentitySnapshot {
                    identity_summary: "Blue Lagoon is a direct personal assistant.".to_string(),
                    stable_items: vec![CompactIdentityItem {
                        category: IdentityItemCategory::Name,
                        value: "Blue Lagoon".to_string(),
                        confidence_pct: 100,
                        weight_pct: None,
                    }],
                    evolving_items: vec![CompactIdentityItem {
                        category: IdentityItemCategory::Preference,
                        value: "concise replies".to_string(),
                        confidence_pct: 90,
                        weight_pct: Some(80),
                    }],
                    values: vec!["respect harness policy".to_string()],
                    boundaries: vec!["do not bypass approval".to_string()],
                    self_description: Some(
                        "A policy-bound personal assistant with continuity.".to_string(),
                    ),
                }),
                identity_lifecycle: IdentityLifecycleContext {
                    state: IdentityLifecycleState::CompleteIdentityActive,
                    kickstart_available: false,
                    kickstart: None,
                },
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
                        latest_user_message: Some("I prefer direct answers.".to_string()),
                        latest_assistant_message: Some("Understood.".to_string()),
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
            governed_action_observations: Vec::new(),
            governed_action_loop_state: Some(ForegroundGovernedActionLoopState {
                executed_action_count: 0,
                max_actions_per_turn: 10,
                remaining_actions_before_cap: 10,
                cap_exceeded_behavior: GovernedActionCapExceededBehavior::Escalate,
            }),
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
            evidence: None,
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

    fn sample_identity_delta_proposal() -> CanonicalProposal {
        CanonicalProposal {
            proposal_id: Uuid::now_v7(),
            proposal_kind: CanonicalProposalKind::IdentityDelta,
            canonical_target: CanonicalTargetKind::IdentityItems,
            confidence_pct: 88,
            conflict_posture: ProposalConflictPosture::Independent,
            subject_ref: "self:blue-lagoon".to_string(),
            rationale: Some("User explicitly selected an initial identity template.".to_string()),
            valid_from: Some(Utc::now()),
            valid_to: None,
            supersedes_artifact_id: None,
            provenance: ProposalProvenance {
                provenance_kind: ProposalProvenanceKind::EpisodeObservation,
                source_ingress_ids: vec![Uuid::now_v7()],
                source_episode_id: Some(Uuid::now_v7()),
            },
            payload: CanonicalProposalPayload::IdentityDelta(IdentityDeltaProposal {
                lifecycle_state: IdentityLifecycleState::CompleteIdentityActive,
                item_deltas: vec![IdentityItemDelta {
                    operation: IdentityDeltaOperation::Add,
                    stability_class: IdentityStabilityClass::Stable,
                    category: IdentityItemCategory::Name,
                    item_key: "name".to_string(),
                    value: "Blue Lagoon".to_string(),
                    confidence_pct: 100,
                    weight_pct: None,
                    source: IdentityItemSource::PredefinedTemplate,
                    merge_policy: IdentityMergePolicy::ProtectedCore,
                    evidence_refs: vec![IdentityEvidenceRef {
                        source_kind: "template".to_string(),
                        source_id: None,
                        summary: "Selected predefined template.".to_string(),
                    }],
                    valid_from: Some(Utc::now()),
                    valid_to: None,
                    target_identity_item_id: None,
                }],
                self_description_delta: Some(SelfDescriptionDelta {
                    operation: IdentityDeltaOperation::Add,
                    description: "Blue Lagoon is a direct personal assistant.".to_string(),
                    evidence_refs: vec![IdentityEvidenceRef {
                        source_kind: "template".to_string(),
                        source_id: None,
                        summary: "Selected predefined template.".to_string(),
                    }],
                }),
                interview_action: None,
                rationale: "Commit the first complete identity.".to_string(),
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

    #[test]
    fn predefined_identity_templates_have_complete_reviewable_deltas() {
        let templates = predefined_identity_templates();
        assert_eq!(templates.len(), 3);
        for template in templates {
            let delta = predefined_identity_delta(&template.template_key, Utc::now())
                .expect("template summary should have a matching delta");
            assert_eq!(
                delta.lifecycle_state,
                IdentityLifecycleState::CompleteIdentityActive
            );
            assert!(delta.self_description_delta.is_some());
            assert!(
                delta
                    .item_deltas
                    .iter()
                    .any(|item| item.stability_class == IdentityStabilityClass::Stable)
            );
            assert!(
                delta
                    .item_deltas
                    .iter()
                    .any(|item| item.stability_class == IdentityStabilityClass::Evolving)
            );
            for category in [
                IdentityItemCategory::Name,
                IdentityItemCategory::IdentityForm,
                IdentityItemCategory::Role,
                IdentityItemCategory::Archetype,
                IdentityItemCategory::OriginBackstory,
                IdentityItemCategory::AgeFraming,
                IdentityItemCategory::FoundationalTrait,
                IdentityItemCategory::FoundationalValue,
                IdentityItemCategory::EnduringBoundary,
                IdentityItemCategory::DefaultCommunicationStyle,
                IdentityItemCategory::Preference,
                IdentityItemCategory::Like,
                IdentityItemCategory::Dislike,
                IdentityItemCategory::Habit,
                IdentityItemCategory::Routine,
                IdentityItemCategory::LearnedTendency,
                IdentityItemCategory::AutobiographicalRefinement,
                IdentityItemCategory::InteractionStyleAdaptation,
                IdentityItemCategory::Goal,
                IdentityItemCategory::Subgoal,
            ] {
                assert!(
                    delta
                        .item_deltas
                        .iter()
                        .any(|item| item.category == category),
                    "{} missing category {category:?}",
                    template.template_key
                );
            }
        }
    }
}
