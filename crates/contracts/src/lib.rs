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

    pub fn validate(&self) -> Result<(), ContractError> {
        match (&self.worker_kind, &self.payload) {
            (WorkerKind::Smoke, WorkerPayload::Smoke(_))
            | (WorkerKind::Conscious, WorkerPayload::Conscious(_)) => Ok(()),
            _ => Err(ContractError::WorkerPayloadMismatch),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum WorkerPayload {
    Smoke(SmokeWorkerRequest),
    Conscious(Box<ConsciousWorkerRequest>),
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsciousWorkerStatus {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsciousContext {
    pub context_id: Uuid,
    pub assembled_at: DateTime<Utc>,
    pub trigger: ForegroundTrigger,
    pub self_model: SelfModelSnapshot,
    pub internal_state: InternalStateSnapshot,
    pub recent_history: Vec<EpisodeExcerpt>,
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
}
