use std::cmp::{max, min};
use std::io::{BufRead, Read, Write};
use std::{path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use contracts::{
    AssistantOutput, CanonicalProposal, CanonicalProposalKind, CanonicalProposalPayload,
    CanonicalTargetKind, ConsciousContext, ConsciousWorkerInboundMessage,
    ConsciousWorkerOutboundMessage, ConsciousWorkerRequest, ConsciousWorkerResult,
    ConsciousWorkerStatus, EpisodeSummary, ForegroundExecutionMode, LoopKind,
    MemoryArtifactProposal, ModelBudget, ModelCallPurpose, ModelCallRequest, ModelCallResponse,
    ModelInput, ModelInputMessage, ModelMessageRole, ModelOutputMode, ProposalConflictPosture,
    ProposalProvenance, ProposalProvenanceKind, SelfModelObservationProposal, SmokeWorkerResult,
    ToolPolicy, WorkerErrorCode, WorkerFailure, WorkerPayload, WorkerRequest, WorkerResponse,
    WorkerResult,
};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(name = "workers", about = "Blue Lagoon worker runtime")]
struct Cli {
    #[command(subcommand)]
    command: WorkerCommand,
}

#[derive(Debug, Subcommand)]
enum WorkerCommand {
    #[command(name = "smoke-worker")]
    Smoke,
    #[command(name = "conscious-worker")]
    Conscious,
    #[command(name = "stall-worker", hide = true)]
    Stall {
        #[arg(long)]
        sleep_ms: u64,
        #[arg(long)]
        pid_file: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        WorkerCommand::Smoke => run_smoke_worker(),
        WorkerCommand::Conscious => run_conscious_worker(),
        WorkerCommand::Stall { sleep_ms, pid_file } => run_stall_worker(sleep_ms, pid_file),
    }
}

fn run_smoke_worker() -> Result<()> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .context("failed to read worker request from stdin")?;

    let response = match serde_json::from_str::<WorkerRequest>(&input) {
        Ok(request) => handle_request(request),
        Err(error) => error_response(
            WorkerErrorCode::InvalidRequest,
            format!("invalid worker request: {error}"),
        ),
    };

    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer(&mut handle, &response).context("failed to serialize worker response")?;
    handle
        .write_all(b"\n")
        .context("failed to terminate worker response line")?;
    Ok(())
}

fn run_stall_worker(sleep_ms: u64, pid_file: Option<PathBuf>) -> Result<()> {
    if let Some(path) = pid_file {
        std::fs::write(path, std::process::id().to_string())
            .context("failed to write stall-worker pid file")?;
    }
    std::thread::sleep(Duration::from_millis(sleep_ms));
    Ok(())
}

fn run_conscious_worker() -> Result<()> {
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();

    let Some(request_line) = lines.next() else {
        write_json_line(
            &mut handle,
            &ConsciousWorkerOutboundMessage::FinalResponse(error_response(
                WorkerErrorCode::InvalidRequest,
                "missing conscious worker request on stdin".to_string(),
            )),
        )?;
        return Ok(());
    };

    let request = match serde_json::from_str::<WorkerRequest>(
        &request_line.context("failed to read conscious worker request line")?,
    ) {
        Ok(request) => request,
        Err(error) => {
            write_json_line(
                &mut handle,
                &ConsciousWorkerOutboundMessage::FinalResponse(error_response(
                    WorkerErrorCode::InvalidRequest,
                    format!("invalid worker request: {error}"),
                )),
            )?;
            return Ok(());
        }
    };

    if let Err(error) = request.validate() {
        write_json_line(
            &mut handle,
            &ConsciousWorkerOutboundMessage::FinalResponse(request_error_response(
                &request,
                WorkerErrorCode::InvalidRequest,
                error.to_string(),
            )),
        )?;
        return Ok(());
    }

    let payload = match &request.payload {
        WorkerPayload::Conscious(payload) => payload.as_ref(),
        WorkerPayload::Smoke(_) => {
            write_json_line(
                &mut handle,
                &ConsciousWorkerOutboundMessage::FinalResponse(request_error_response(
                    &request,
                    WorkerErrorCode::UnsupportedWorker,
                    "conscious worker entrypoint requires a conscious worker request".to_string(),
                )),
            )?;
            return Ok(());
        }
    };

    let model_request = build_model_call_request(&request, payload);
    write_json_line(
        &mut handle,
        &ConsciousWorkerOutboundMessage::ModelCallRequest(model_request.clone()),
    )?;

    let Some(inbound_line) = lines.next() else {
        write_json_line(
            &mut handle,
            &ConsciousWorkerOutboundMessage::FinalResponse(request_error_response(
                &request,
                WorkerErrorCode::InvalidRequest,
                "missing harness model-call response for conscious worker".to_string(),
            )),
        )?;
        return Ok(());
    };

    let inbound = match serde_json::from_str::<ConsciousWorkerInboundMessage>(
        &inbound_line.context("failed to read conscious worker inbound line")?,
    ) {
        Ok(message) => message,
        Err(error) => {
            write_json_line(
                &mut handle,
                &ConsciousWorkerOutboundMessage::FinalResponse(request_error_response(
                    &request,
                    WorkerErrorCode::InvalidRequest,
                    format!("invalid conscious worker inbound message: {error}"),
                )),
            )?;
            return Ok(());
        }
    };

    let ConsciousWorkerInboundMessage::ModelCallResponse(model_response) = inbound;
    if let Err(message) = validate_model_response(&model_request, &model_response) {
        write_json_line(
            &mut handle,
            &ConsciousWorkerOutboundMessage::FinalResponse(request_error_response(
                &request,
                WorkerErrorCode::InvalidRequest,
                message,
            )),
        )?;
        return Ok(());
    }

    let response = match build_conscious_worker_response(&request, payload, model_response) {
        Ok(response) => response,
        Err(message) => request_error_response(&request, WorkerErrorCode::InvalidRequest, message),
    };
    write_json_line(
        &mut handle,
        &ConsciousWorkerOutboundMessage::FinalResponse(response),
    )?;
    Ok(())
}

fn handle_request(request: WorkerRequest) -> WorkerResponse {
    if let Err(error) = request.validate() {
        return error_response(WorkerErrorCode::InvalidRequest, error.to_string());
    }

    match request.payload {
        WorkerPayload::Smoke(ref payload) => {
            smoke_worker_response(&request, payload.synthetic_trigger.clone())
        }
        WorkerPayload::Conscious(_) => request_error_response(
            &request,
            WorkerErrorCode::UnsupportedWorker,
            "conscious worker protocol is not implemented yet".to_string(),
        ),
    }
}

fn smoke_worker_response(request: &WorkerRequest, synthetic_trigger: String) -> WorkerResponse {
    WorkerResponse {
        request_id: request.request_id,
        trace_id: request.trace_id,
        execution_id: request.execution_id,
        finished_at: chrono::Utc::now(),
        worker_pid: std::process::id(),
        result: WorkerResult::Smoke(SmokeWorkerResult {
            status: "completed".to_string(),
            summary: format!(
                "synthetic trigger '{}' completed by smoke worker",
                synthetic_trigger
            ),
        }),
    }
}

fn build_model_call_request(
    request: &WorkerRequest,
    payload: &ConsciousWorkerRequest,
) -> ModelCallRequest {
    let token_budget = payload.context.trigger.budget.token_budget;
    let max_output_tokens = min(token_budget, 800);
    let max_input_tokens = max(1, token_budget.saturating_sub(max_output_tokens));

    ModelCallRequest {
        request_id: uuid::Uuid::now_v7(),
        trace_id: request.trace_id,
        execution_id: request.execution_id,
        loop_kind: LoopKind::Conscious,
        purpose: ModelCallPurpose::ForegroundResponse,
        task_class: "telegram_foreground_reply".to_string(),
        budget: ModelBudget {
            max_input_tokens,
            max_output_tokens,
            timeout_ms: payload.context.trigger.budget.wall_clock_budget_ms,
        },
        input: build_model_input(&payload.context),
        output_mode: ModelOutputMode::PlainText,
        schema_name: None,
        schema_json: None,
        tool_policy: ToolPolicy::NoTools,
        provider_hint: None,
    }
}

fn build_model_input(context: &ConsciousContext) -> ModelInput {
    let mut messages = Vec::new();
    for episode in context.recent_history.iter().rev() {
        if let Some(user_message) = &episode.user_message {
            messages.push(ModelInputMessage {
                role: ModelMessageRole::User,
                content: user_message.clone(),
            });
        }
        if let Some(assistant_message) = &episode.assistant_message {
            messages.push(ModelInputMessage {
                role: ModelMessageRole::Assistant,
                content: assistant_message.clone(),
            });
        }
    }

    if let Some(trigger_text) = &context.trigger.ingress.text_body {
        messages.push(ModelInputMessage {
            role: ModelMessageRole::User,
            content: trigger_text.clone(),
        });
    }

    if context.recovery_context.mode == ForegroundExecutionMode::BacklogRecovery
        && !context.recovery_context.ordered_ingress.is_empty()
    {
        messages.push(ModelInputMessage {
            role: ModelMessageRole::Developer,
            content: format!(
                "Recovery mode is backlog_recovery. Ordered delayed ingress batch: {}.",
                join_or_none(
                    &context
                        .recovery_context
                        .ordered_ingress
                        .iter()
                        .map(|ingress| {
                            ingress
                                .text_body
                                .clone()
                                .unwrap_or_else(|| "<empty>".to_string())
                        })
                        .collect::<Vec<_>>()
                )
            ),
        });
    }

    if !context.retrieved_context.items.is_empty() {
        messages.push(ModelInputMessage {
            role: ModelMessageRole::Developer,
            content: format!(
                "Retrieved canonical context: {}.",
                retrieved_context_summary(&context.retrieved_context.items)
            ),
        });
    }

    ModelInput {
        system_prompt: format!(
            "You are {}. Role: {}. Communication style: {}. Capabilities: {}. Constraints: {}. Preferences: {}. Current goals: {}. Current subgoals: {}. Internal state: load_pct={}, health_pct={}, reliability_pct={}, resource_pressure_pct={}, confidence_pct={}, connection_quality_pct={}, active_conditions={}. Execution mode: {}.",
            context.self_model.stable_identity,
            context.self_model.role,
            context.self_model.communication_style,
            join_or_none(&context.self_model.capabilities),
            join_or_none(&context.self_model.constraints),
            join_or_none(&context.self_model.preferences),
            join_or_none(&context.self_model.current_goals),
            join_or_none(&context.self_model.current_subgoals),
            context.internal_state.load_pct,
            context.internal_state.health_pct,
            context.internal_state.reliability_pct,
            context.internal_state.resource_pressure_pct,
            context.internal_state.confidence_pct,
            context.internal_state.connection_quality_pct,
            join_or_none(&context.internal_state.active_conditions),
            foreground_execution_mode_as_str(context.recovery_context.mode),
        ),
        messages,
    }
}

fn build_conscious_worker_response(
    request: &WorkerRequest,
    payload: &ConsciousWorkerRequest,
    model_response: ModelCallResponse,
) -> std::result::Result<WorkerResponse, String> {
    let candidate_proposals = build_candidate_proposals(&payload.context)?;
    Ok(WorkerResponse {
        request_id: request.request_id,
        trace_id: request.trace_id,
        execution_id: request.execution_id,
        finished_at: chrono::Utc::now(),
        worker_pid: std::process::id(),
        result: WorkerResult::Conscious(ConsciousWorkerResult {
            status: ConsciousWorkerStatus::Completed,
            assistant_output: AssistantOutput {
                channel_kind: payload.context.trigger.ingress.channel_kind,
                internal_conversation_ref: payload
                    .context
                    .trigger
                    .ingress
                    .internal_conversation_ref
                    .clone(),
                text: model_response.output.text,
            },
            episode_summary: EpisodeSummary {
                summary: format!(
                    "foreground response completed for {}",
                    payload.context.trigger.ingress.external_event_id
                ),
                outcome: "completed".to_string(),
                message_count: history_message_count(&payload.context) + 2,
            },
            candidate_proposals,
        }),
    })
}

fn history_message_count(context: &ConsciousContext) -> u32 {
    context
        .recent_history
        .iter()
        .map(|episode| {
            u32::from(episode.user_message.is_some())
                + u32::from(episode.assistant_message.is_some())
        })
        .sum()
}

fn validate_model_response(
    model_request: &ModelCallRequest,
    model_response: &ModelCallResponse,
) -> std::result::Result<(), String> {
    if model_response.request_id != model_request.request_id {
        return Err(
            "model-call response request_id did not match worker model request".to_string(),
        );
    }
    if model_response.trace_id != model_request.trace_id {
        return Err("model-call response trace_id did not match worker request".to_string());
    }
    if model_response.execution_id != model_request.execution_id {
        return Err("model-call response execution_id did not match worker request".to_string());
    }
    Ok(())
}

fn join_or_none(items: &[String]) -> String {
    if items.is_empty() {
        return "none".to_string();
    }

    items.join(", ")
}

fn foreground_execution_mode_as_str(mode: ForegroundExecutionMode) -> &'static str {
    match mode {
        ForegroundExecutionMode::SingleIngress => "single_ingress",
        ForegroundExecutionMode::BacklogRecovery => "backlog_recovery",
    }
}

fn retrieved_context_summary(items: &[contracts::RetrievedContextItem]) -> String {
    items
        .iter()
        .map(|item| match item {
            contracts::RetrievedContextItem::Episode(episode) => {
                format!("episode:{}:{}", episode.episode_id, episode.summary)
            }
            contracts::RetrievedContextItem::MemoryArtifact(artifact) => {
                format!(
                    "memory:{}:{}",
                    artifact.memory_artifact_id, artifact.content_text
                )
            }
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

fn build_candidate_proposals(
    context: &ConsciousContext,
) -> std::result::Result<Vec<CanonicalProposal>, String> {
    let mut proposals = Vec::new();
    let Some(trigger_text) = context.trigger.ingress.text_body.as_deref() else {
        return Ok(proposals);
    };
    let lowered = trigger_text.to_ascii_lowercase();

    if lowered.contains("remember that ") || lowered.contains("i prefer ") {
        proposals.push(CanonicalProposal {
            proposal_id: uuid::Uuid::now_v7(),
            proposal_kind: CanonicalProposalKind::MemoryArtifact,
            canonical_target: CanonicalTargetKind::MemoryArtifacts,
            confidence_pct: 85,
            conflict_posture: ProposalConflictPosture::Independent,
            subject_ref: format!(
                "principal:{}",
                context.trigger.ingress.internal_principal_ref
            ),
            rationale: Some(
                "Foreground trigger explicitly expressed a user preference or fact.".to_string(),
            ),
            valid_from: Some(context.trigger.ingress.occurred_at),
            valid_to: None,
            supersedes_artifact_id: None,
            provenance: ProposalProvenance {
                provenance_kind: match context.recovery_context.mode {
                    ForegroundExecutionMode::SingleIngress => {
                        ProposalProvenanceKind::EpisodeObservation
                    }
                    ForegroundExecutionMode::BacklogRecovery => {
                        ProposalProvenanceKind::BacklogRecovery
                    }
                },
                source_ingress_ids: current_source_ingress_ids(context),
                source_episode_id: None,
            },
            payload: CanonicalProposalPayload::MemoryArtifact(MemoryArtifactProposal {
                artifact_kind: "preference".to_string(),
                content_text: trigger_text.trim().to_string(),
            }),
        });
    }

    if lowered.contains("be concise")
        || lowered.contains("more concise")
        || lowered.contains("be direct")
        || lowered.contains("more direct")
    {
        proposals.push(CanonicalProposal {
            proposal_id: uuid::Uuid::now_v7(),
            proposal_kind: CanonicalProposalKind::SelfModelObservation,
            canonical_target: CanonicalTargetKind::SelfModelArtifacts,
            confidence_pct: 78,
            conflict_posture: ProposalConflictPosture::Independent,
            subject_ref: "self".to_string(),
            rationale: Some(
                "Foreground trigger contained an explicit instruction about assistant style."
                    .to_string(),
            ),
            valid_from: Some(context.trigger.ingress.occurred_at),
            valid_to: None,
            supersedes_artifact_id: None,
            provenance: ProposalProvenance {
                provenance_kind: ProposalProvenanceKind::EpisodeObservation,
                source_ingress_ids: current_source_ingress_ids(context),
                source_episode_id: None,
            },
            payload: CanonicalProposalPayload::SelfModelObservation(SelfModelObservationProposal {
                observation_kind: "interaction_style".to_string(),
                content_text: trigger_text.trim().to_string(),
            }),
        });
    }

    validate_candidate_proposals(context, &proposals)?;
    Ok(proposals)
}

fn validate_candidate_proposals(
    context: &ConsciousContext,
    proposals: &[CanonicalProposal],
) -> std::result::Result<(), String> {
    let allowed_ingress_ids = current_source_ingress_ids(context);

    for proposal in proposals {
        if proposal.confidence_pct == 0 {
            return Err("candidate proposal confidence_pct must be greater than zero".to_string());
        }
        if proposal.subject_ref.trim().is_empty() {
            return Err("candidate proposal subject_ref must not be empty".to_string());
        }
        if proposal.provenance.source_ingress_ids.is_empty() {
            return Err(
                "candidate proposal provenance must include source ingress ids".to_string(),
            );
        }
        if proposal
            .provenance
            .source_ingress_ids
            .iter()
            .any(|ingress_id| !allowed_ingress_ids.contains(ingress_id))
        {
            return Err(
                "candidate proposal provenance referenced an unknown ingress id".to_string(),
            );
        }
        match (
            &proposal.proposal_kind,
            &proposal.canonical_target,
            &proposal.payload,
        ) {
            (
                CanonicalProposalKind::MemoryArtifact,
                CanonicalTargetKind::MemoryArtifacts,
                CanonicalProposalPayload::MemoryArtifact(payload),
            ) if !payload.artifact_kind.trim().is_empty()
                && !payload.content_text.trim().is_empty() => {}
            (
                CanonicalProposalKind::SelfModelObservation,
                CanonicalTargetKind::SelfModelArtifacts,
                CanonicalProposalPayload::SelfModelObservation(payload),
            ) if !payload.observation_kind.trim().is_empty()
                && !payload.content_text.trim().is_empty() => {}
            _ => {
                return Err(
                    "candidate proposal payload did not match the declared proposal kind"
                        .to_string(),
                );
            }
        }
        match proposal.conflict_posture {
            ProposalConflictPosture::Independent | ProposalConflictPosture::Conflicts
                if proposal.supersedes_artifact_id.is_some() =>
            {
                return Err(
                    "candidate proposal conflict posture allows no supersedes_artifact_id"
                        .to_string(),
                );
            }
            ProposalConflictPosture::Revises | ProposalConflictPosture::Supersedes
                if proposal.supersedes_artifact_id.is_none() =>
            {
                return Err(
                    "candidate proposal conflict posture requires supersedes_artifact_id"
                        .to_string(),
                );
            }
            _ => {}
        }
    }

    Ok(())
}

fn current_source_ingress_ids(context: &ConsciousContext) -> Vec<uuid::Uuid> {
    if context.recovery_context.mode == ForegroundExecutionMode::BacklogRecovery
        && !context.recovery_context.ordered_ingress.is_empty()
    {
        return context
            .recovery_context
            .ordered_ingress
            .iter()
            .map(|ingress| ingress.ingress_id)
            .collect();
    }

    vec![context.trigger.ingress.ingress_id]
}

fn write_json_line<T: Serialize>(handle: &mut impl Write, value: &T) -> Result<()> {
    serde_json::to_writer(&mut *handle, value)
        .context("failed to serialize worker protocol message")?;
    handle
        .write_all(b"\n")
        .context("failed to terminate worker protocol line")?;
    handle
        .flush()
        .context("failed to flush worker protocol line")?;
    Ok(())
}

fn error_response(code: WorkerErrorCode, message: String) -> WorkerResponse {
    WorkerResponse {
        request_id: uuid::Uuid::nil(),
        trace_id: uuid::Uuid::nil(),
        execution_id: uuid::Uuid::nil(),
        finished_at: chrono::Utc::now(),
        worker_pid: std::process::id(),
        result: WorkerResult::Error(WorkerFailure { code, message }),
    }
}

fn request_error_response(
    request: &WorkerRequest,
    code: WorkerErrorCode,
    message: String,
) -> WorkerResponse {
    WorkerResponse {
        request_id: request.request_id,
        trace_id: request.trace_id,
        execution_id: request.execution_id,
        finished_at: chrono::Utc::now(),
        worker_pid: std::process::id(),
        result: WorkerResult::Error(WorkerFailure { code, message }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use contracts::{
        ChannelKind, ConsciousContext, ForegroundBudget, ForegroundTrigger, ForegroundTriggerKind,
        IngressEventKind, InternalStateSnapshot, ModelOutput, ModelUsage, NormalizedIngress,
        SelfModelSnapshot,
    };

    #[test]
    fn smoke_worker_returns_structured_result() {
        let request = WorkerRequest::smoke(uuid::Uuid::now_v7(), uuid::Uuid::now_v7(), "smoke");
        let response = handle_request(request.clone());
        assert_eq!(response.request_id, request.request_id);
        assert_eq!(response.trace_id, request.trace_id);
        assert_eq!(response.execution_id, request.execution_id);
        match response.result {
            WorkerResult::Smoke(result) => {
                assert_eq!(result.status, "completed");
                assert!(result.summary.contains("smoke"));
            }
            WorkerResult::Conscious(_) => {
                panic!("smoke worker should not return a conscious result")
            }
            WorkerResult::Error(_) => panic!("smoke worker should not return an error"),
        }
    }

    #[test]
    fn conscious_model_request_uses_context_and_budget() {
        let request =
            WorkerRequest::conscious(uuid::Uuid::now_v7(), uuid::Uuid::now_v7(), sample_context());
        let WorkerPayload::Conscious(payload) = &request.payload else {
            panic!("expected conscious payload");
        };

        let model_request = build_model_call_request(&request, payload.as_ref());
        assert_eq!(model_request.trace_id, request.trace_id);
        assert_eq!(model_request.execution_id, request.execution_id);
        assert_eq!(model_request.loop_kind, LoopKind::Conscious);
        assert_eq!(model_request.purpose, ModelCallPurpose::ForegroundResponse);
        assert_eq!(model_request.budget.timeout_ms, 30_000);
        assert_eq!(model_request.output_mode, ModelOutputMode::PlainText);
        assert_eq!(model_request.tool_policy, ToolPolicy::NoTools);
        assert!(model_request.input.system_prompt.contains("blue-lagoon"));
        assert!(model_request.input.system_prompt.contains("conversation"));
        assert!(
            model_request
                .input
                .system_prompt
                .contains("support_the_user")
        );
        assert!(
            model_request
                .input
                .system_prompt
                .contains("reply_to_current_message")
        );
        assert!(model_request.input.system_prompt.contains("load_pct=15"));
        assert!(
            model_request
                .input
                .system_prompt
                .contains("confidence_pct=80")
        );
        assert_eq!(
            model_request
                .input
                .messages
                .last()
                .map(|message| message.content.as_str()),
            Some("remember that I prefer concise replies and be direct")
        );
    }

    #[test]
    fn conscious_worker_response_wraps_model_output() {
        let request =
            WorkerRequest::conscious(uuid::Uuid::now_v7(), uuid::Uuid::now_v7(), sample_context());
        let WorkerPayload::Conscious(payload) = &request.payload else {
            panic!("expected conscious payload");
        };
        let model_request = build_model_call_request(&request, payload.as_ref());
        let model_response = ModelCallResponse {
            request_id: model_request.request_id,
            trace_id: request.trace_id,
            execution_id: request.execution_id,
            provider: contracts::ModelProviderKind::ZAi,
            model: "z-ai-foreground".to_string(),
            received_at: chrono::Utc::now(),
            output: ModelOutput {
                text: "hello back".to_string(),
                json: None,
                finish_reason: "stop".to_string(),
            },
            usage: ModelUsage {
                input_tokens: 12,
                output_tokens: 4,
            },
        };

        let response = build_conscious_worker_response(&request, payload.as_ref(), model_response)
            .expect("worker response should be valid");
        match response.result {
            WorkerResult::Conscious(result) => {
                assert_eq!(result.status, ConsciousWorkerStatus::Completed);
                assert_eq!(result.assistant_output.text, "hello back");
                assert_eq!(
                    result.assistant_output.internal_conversation_ref,
                    "telegram-primary"
                );
                assert_eq!(result.episode_summary.outcome, "completed");
                assert_eq!(result.candidate_proposals.len(), 2);
            }
            WorkerResult::Smoke(_) => panic!("conscious worker should not emit a smoke result"),
            WorkerResult::Error(error) => panic!("unexpected worker error: {}", error.message),
        }
    }

    fn sample_context() -> ConsciousContext {
        ConsciousContext {
            context_id: uuid::Uuid::now_v7(),
            assembled_at: chrono::Utc::now(),
            trigger: ForegroundTrigger {
                trigger_id: uuid::Uuid::now_v7(),
                trace_id: uuid::Uuid::now_v7(),
                execution_id: uuid::Uuid::now_v7(),
                trigger_kind: ForegroundTriggerKind::UserIngress,
                ingress: NormalizedIngress {
                    ingress_id: uuid::Uuid::now_v7(),
                    channel_kind: ChannelKind::Telegram,
                    external_user_id: "42".to_string(),
                    external_conversation_id: "42".to_string(),
                    external_event_id: "update-42".to_string(),
                    external_message_id: Some("message-42".to_string()),
                    internal_principal_ref: "primary-user".to_string(),
                    internal_conversation_ref: "telegram-primary".to_string(),
                    event_kind: IngressEventKind::MessageCreated,
                    occurred_at: chrono::Utc::now(),
                    text_body: Some(
                        "remember that I prefer concise replies and be direct".to_string(),
                    ),
                    reply_to: None,
                    attachments: Vec::new(),
                    command_hint: None,
                    approval_payload: None,
                    raw_payload_ref: None,
                },
                received_at: chrono::Utc::now(),
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
                capabilities: vec!["conversation".to_string()],
                constraints: vec!["respect_harness_policy".to_string()],
                preferences: vec!["concise".to_string()],
                current_goals: vec!["support_the_user".to_string()],
                current_subgoals: vec!["reply_to_current_message".to_string()],
            },
            internal_state: InternalStateSnapshot {
                load_pct: 15,
                health_pct: 100,
                reliability_pct: 100,
                resource_pressure_pct: 10,
                confidence_pct: 80,
                connection_quality_pct: 95,
                active_conditions: Vec::new(),
            },
            recent_history: vec![contracts::EpisodeExcerpt {
                episode_id: uuid::Uuid::now_v7(),
                trace_id: uuid::Uuid::now_v7(),
                started_at: chrono::Utc::now(),
                user_message: Some("older user".to_string()),
                assistant_message: Some("older assistant".to_string()),
                outcome: "completed".to_string(),
            }],
            retrieved_context: contracts::RetrievedContext::default(),
            recovery_context: contracts::ForegroundRecoveryContext::default(),
        }
    }
}
