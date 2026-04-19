use std::cmp::{max, min};
use std::io::{BufRead, Read, Write};
use std::{path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use contracts::{
    AssistantOutput, BackgroundTriggerKind, CanonicalProposal, CanonicalProposalKind,
    CanonicalProposalPayload, CanonicalTargetKind, ConsciousContext, ConsciousWorkerInboundMessage,
    ConsciousWorkerOutboundMessage, ConsciousWorkerRequest, ConsciousWorkerResult,
    ConsciousWorkerStatus, DiagnosticAlert, DiagnosticSeverity, EpisodeSummary,
    ForegroundExecutionMode, LoopKind, MemoryArtifactProposal, ModelBudget, ModelCallPurpose,
    ModelCallRequest, ModelCallResponse, ModelInput, ModelInputMessage, ModelMessageRole,
    ModelOutputMode, ProposalConflictPosture, ProposalProvenance, ProposalProvenanceKind,
    RetrievalUpdateOperation, RetrievalUpdateProposal, SelfModelObservationProposal,
    SmokeWorkerResult, ToolPolicy, UnconsciousContext, UnconsciousJobKind,
    UnconsciousMaintenanceOutputs, UnconsciousWorkerRequest, UnconsciousWorkerResult,
    UnconsciousWorkerStatus, WakeSignal, WakeSignalPriority, WakeSignalReason, WorkerErrorCode,
    WorkerFailure, WorkerPayload, WorkerRequest, WorkerResponse, WorkerResult,
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
    #[command(name = "unconscious-worker")]
    Unconscious,
    #[command(name = "wrong-result-worker", hide = true)]
    WrongResult,
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
        WorkerCommand::Unconscious => run_unconscious_worker(),
        WorkerCommand::WrongResult => run_wrong_result_worker(),
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

fn run_wrong_result_worker() -> Result<()> {
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();

    let Some(request_line) = lines.next() else {
        write_json_line(
            &mut handle,
            &ConsciousWorkerOutboundMessage::FinalResponse(error_response(
                WorkerErrorCode::InvalidRequest,
                "missing worker request on stdin".to_string(),
            )),
        )?;
        return Ok(());
    };

    let request = match serde_json::from_str::<WorkerRequest>(
        &request_line.context("failed to read wrong-result worker request line")?,
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

    write_json_line(
        &mut handle,
        &ConsciousWorkerOutboundMessage::FinalResponse(WorkerResponse {
            request_id: request.request_id,
            trace_id: request.trace_id,
            execution_id: request.execution_id,
            finished_at: chrono::Utc::now(),
            worker_pid: std::process::id(),
            result: WorkerResult::Smoke(SmokeWorkerResult {
                status: "completed".to_string(),
                summary: "wrong-result worker intentionally returned a mismatched payload"
                    .to_string(),
            }),
        }),
    )?;
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
        WorkerPayload::Smoke(_) | WorkerPayload::Unconscious(_) => {
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

fn run_unconscious_worker() -> Result<()> {
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();

    let Some(request_line) = lines.next() else {
        write_json_line(
            &mut handle,
            &ConsciousWorkerOutboundMessage::FinalResponse(error_response(
                WorkerErrorCode::InvalidRequest,
                "missing unconscious worker request on stdin".to_string(),
            )),
        )?;
        return Ok(());
    };

    let request = match serde_json::from_str::<WorkerRequest>(
        &request_line.context("failed to read unconscious worker request line")?,
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
        WorkerPayload::Unconscious(payload) => payload.as_ref(),
        WorkerPayload::Smoke(_) | WorkerPayload::Conscious(_) => {
            write_json_line(
                &mut handle,
                &ConsciousWorkerOutboundMessage::FinalResponse(request_error_response(
                    &request,
                    WorkerErrorCode::UnsupportedWorker,
                    "unconscious worker entrypoint requires an unconscious worker request"
                        .to_string(),
                )),
            )?;
            return Ok(());
        }
    };

    let model_request = build_unconscious_model_call_request(&request, payload);
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
                "missing harness model-call response for unconscious worker".to_string(),
            )),
        )?;
        return Ok(());
    };

    let inbound = match serde_json::from_str::<ConsciousWorkerInboundMessage>(
        &inbound_line.context("failed to read unconscious worker inbound line")?,
    ) {
        Ok(message) => message,
        Err(error) => {
            write_json_line(
                &mut handle,
                &ConsciousWorkerOutboundMessage::FinalResponse(request_error_response(
                    &request,
                    WorkerErrorCode::InvalidRequest,
                    format!("invalid unconscious worker inbound message: {error}"),
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

    let response = build_unconscious_worker_response(&request, payload, model_response);
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
        WorkerPayload::Conscious(_) | WorkerPayload::Unconscious(_) => request_error_response(
            &request,
            WorkerErrorCode::UnsupportedWorker,
            "interactive worker protocols are only supported through their dedicated entrypoints"
                .to_string(),
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

fn build_unconscious_model_call_request(
    request: &WorkerRequest,
    payload: &UnconsciousWorkerRequest,
) -> ModelCallRequest {
    let token_budget = payload.context.budget.token_budget;
    let max_output_tokens = min(token_budget, 1_200);
    let max_input_tokens = max(1, token_budget.saturating_sub(max_output_tokens));

    ModelCallRequest {
        request_id: uuid::Uuid::now_v7(),
        trace_id: request.trace_id,
        execution_id: request.execution_id,
        loop_kind: LoopKind::Unconscious,
        purpose: ModelCallPurpose::BackgroundAnalysis,
        task_class: unconscious_task_class(payload.context.job_kind).to_string(),
        budget: ModelBudget {
            max_input_tokens,
            max_output_tokens,
            timeout_ms: payload.context.budget.wall_clock_budget_ms,
        },
        input: build_unconscious_model_input(&payload.context),
        output_mode: ModelOutputMode::PlainText,
        schema_name: None,
        schema_json: None,
        tool_policy: ToolPolicy::ProposalOnly,
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

fn build_unconscious_model_input(context: &UnconsciousContext) -> ModelInput {
    let mut messages = vec![ModelInputMessage {
        role: ModelMessageRole::Developer,
        content: format!(
            "Scoped background maintenance input. Episodes: {}. Memory artifacts: {}. Retrieval artifacts: {}. Self-model artifact present: {}. Scope summary: {}.",
            context.scope.episode_ids.len(),
            context.scope.memory_artifact_ids.len(),
            context.scope.retrieval_artifact_ids.len(),
            context.scope.self_model_artifact_id.is_some(),
            context.scope.summary
        ),
    }];

    messages.push(ModelInputMessage {
        role: ModelMessageRole::User,
        content: format!(
            "Perform bounded {} for the provided scope. Return only structured maintenance outputs through the harness contract.",
            unconscious_task_class(context.job_kind)
        ),
    });

    ModelInput {
        system_prompt: format!(
            "You are Blue Lagoon's unconscious maintenance worker. Job kind: {}. Trigger kind: {}. Trigger summary: {}. Budget: iteration_budget={}, wall_clock_budget_ms={}, token_budget={}. Never produce user-facing output, direct canonical mutations, or side-effecting actions.",
            unconscious_task_class(context.job_kind),
            background_trigger_kind_as_str(context.trigger.trigger_kind),
            context.trigger.reason_summary,
            context.budget.iteration_budget,
            context.budget.wall_clock_budget_ms,
            context.budget.token_budget,
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

fn build_unconscious_worker_response(
    request: &WorkerRequest,
    payload: &UnconsciousWorkerRequest,
    model_response: ModelCallResponse,
) -> WorkerResponse {
    let model_text = model_response.output.text;
    let canonical_proposals = build_unconscious_canonical_proposals(&payload.context, &model_text);
    let retrieval_updates =
        build_unconscious_retrieval_updates(&payload.context, &model_text, &canonical_proposals);
    let diagnostics = build_unconscious_diagnostics(&payload.context, &model_text);

    WorkerResponse {
        request_id: request.request_id,
        trace_id: request.trace_id,
        execution_id: request.execution_id,
        finished_at: chrono::Utc::now(),
        worker_pid: std::process::id(),
        result: WorkerResult::Unconscious(UnconsciousWorkerResult {
            status: UnconsciousWorkerStatus::Completed,
            summary: format!(
                "{} completed for scoped background job",
                unconscious_task_class(payload.context.job_kind)
            ),
            maintenance_outputs: UnconsciousMaintenanceOutputs {
                canonical_proposals,
                retrieval_updates,
                diagnostics,
                wake_signals: build_default_wake_signals(&payload.context),
            },
        }),
    }
}

fn build_unconscious_canonical_proposals(
    context: &UnconsciousContext,
    model_text: &str,
) -> Vec<CanonicalProposal> {
    match context.job_kind {
        UnconsciousJobKind::MemoryConsolidation => {
            build_memory_consolidation_proposals(context, model_text)
        }
        UnconsciousJobKind::SelfModelReflection => {
            build_self_model_reflection_proposals(context, model_text)
        }
        UnconsciousJobKind::RetrievalMaintenance
        | UnconsciousJobKind::ContradictionAndDriftScan => Vec::new(),
    }
}

fn build_memory_consolidation_proposals(
    context: &UnconsciousContext,
    model_text: &str,
) -> Vec<CanonicalProposal> {
    let content_text = model_text.trim();
    if content_text.is_empty() || context.scope.episode_ids.is_empty() {
        return Vec::new();
    }

    let Some(subject_ref) = context
        .scope
        .internal_principal_ref
        .clone()
        .or_else(|| context.scope.internal_conversation_ref.clone())
    else {
        return Vec::new();
    };

    vec![CanonicalProposal {
        proposal_id: uuid::Uuid::now_v7(),
        proposal_kind: CanonicalProposalKind::MemoryArtifact,
        canonical_target: CanonicalTargetKind::MemoryArtifacts,
        confidence_pct: 72,
        conflict_posture: ProposalConflictPosture::Independent,
        subject_ref,
        rationale: Some(
            "Bounded background memory consolidation over the scoped recent episodes.".to_string(),
        ),
        valid_from: None,
        valid_to: None,
        supersedes_artifact_id: None,
        provenance: ProposalProvenance {
            provenance_kind: ProposalProvenanceKind::EpisodeObservation,
            source_ingress_ids: Vec::new(),
            source_episode_id: context.scope.episode_ids.first().copied(),
        },
        payload: CanonicalProposalPayload::MemoryArtifact(MemoryArtifactProposal {
            artifact_kind: "background_summary".to_string(),
            content_text: content_text.to_string(),
        }),
    }]
}

fn build_self_model_reflection_proposals(
    context: &UnconsciousContext,
    model_text: &str,
) -> Vec<CanonicalProposal> {
    let content_text = model_text.trim();
    if content_text.is_empty() {
        return Vec::new();
    }

    vec![CanonicalProposal {
        proposal_id: uuid::Uuid::now_v7(),
        proposal_kind: CanonicalProposalKind::SelfModelObservation,
        canonical_target: CanonicalTargetKind::SelfModelArtifacts,
        confidence_pct: 68,
        conflict_posture: ProposalConflictPosture::Independent,
        subject_ref: "self".to_string(),
        rationale: Some(
            "Bounded background self-model reflection over the canonical self-model state."
                .to_string(),
        ),
        valid_from: None,
        valid_to: None,
        supersedes_artifact_id: None,
        provenance: ProposalProvenance {
            provenance_kind: ProposalProvenanceKind::SelfModelReflection,
            source_ingress_ids: Vec::new(),
            source_episode_id: context.scope.episode_ids.first().copied(),
        },
        payload: CanonicalProposalPayload::SelfModelObservation(SelfModelObservationProposal {
            observation_kind: classify_self_model_observation_kind(content_text).to_string(),
            content_text: content_text.to_string(),
        }),
    }]
}

fn build_unconscious_retrieval_updates(
    context: &UnconsciousContext,
    model_text: &str,
    canonical_proposals: &[CanonicalProposal],
) -> Vec<RetrievalUpdateProposal> {
    match context.job_kind {
        UnconsciousJobKind::MemoryConsolidation | UnconsciousJobKind::RetrievalMaintenance => {
            let retrieval_rationale = if canonical_proposals.is_empty() {
                format!(
                    "scoped {} completed without canonical proposal changes",
                    unconscious_task_class(context.job_kind)
                )
            } else {
                format!(
                    "scoped {} produced {} canonical proposal(s)",
                    unconscious_task_class(context.job_kind),
                    canonical_proposals.len()
                )
            };

            vec![RetrievalUpdateProposal {
                update_id: uuid::Uuid::now_v7(),
                operation: RetrievalUpdateOperation::Upsert,
                source_ref: format!("background_job:{}", context.job_id),
                lexical_document: model_text.to_string(),
                relevance_timestamp: chrono::Utc::now(),
                internal_conversation_ref: context.scope.internal_conversation_ref.clone(),
                rationale: Some(retrieval_rationale),
            }]
        }
        UnconsciousJobKind::ContradictionAndDriftScan | UnconsciousJobKind::SelfModelReflection => {
            Vec::new()
        }
    }
}

fn build_unconscious_diagnostics(
    context: &UnconsciousContext,
    model_text: &str,
) -> Vec<DiagnosticAlert> {
    match context.job_kind {
        UnconsciousJobKind::ContradictionAndDriftScan => {
            vec![classify_contradiction_and_drift(context, model_text)]
        }
        UnconsciousJobKind::MemoryConsolidation
        | UnconsciousJobKind::RetrievalMaintenance
        | UnconsciousJobKind::SelfModelReflection => vec![DiagnosticAlert {
            alert_id: uuid::Uuid::now_v7(),
            code: format!("{}_completed", unconscious_task_class(context.job_kind)),
            severity: DiagnosticSeverity::Info,
            summary: format!(
                "{} completed under bounded background execution",
                unconscious_task_class(context.job_kind)
            ),
            details: None,
        }],
    }
}

fn classify_contradiction_and_drift(
    context: &UnconsciousContext,
    model_text: &str,
) -> DiagnosticAlert {
    let normalized = model_text.trim();
    let lowered = normalized.to_ascii_lowercase();

    let (code, severity, summary) =
        if lowered.contains("contradiction") || lowered.contains("conflict") {
            (
                "contradiction_detected",
                DiagnosticSeverity::Critical,
                format!(
                    "Potential contradiction detected in {}.",
                    contradiction_scope_label(context)
                ),
            )
        } else if lowered.contains("drift")
            || lowered.contains("inconsistent")
            || lowered.contains("divergence")
        {
            (
                "drift_signal_detected",
                DiagnosticSeverity::Warning,
                format!(
                    "Potential continuity drift detected in {}.",
                    contradiction_scope_label(context)
                ),
            )
        } else {
            (
                "drift_scan_clear",
                DiagnosticSeverity::Info,
                format!(
                    "No contradiction or drift indicators detected in {}.",
                    contradiction_scope_label(context)
                ),
            )
        };

    DiagnosticAlert {
        alert_id: uuid::Uuid::now_v7(),
        code: code.to_string(),
        severity,
        summary,
        details: (!normalized.is_empty()).then(|| normalized.to_string()),
    }
}

fn contradiction_scope_label(context: &UnconsciousContext) -> String {
    context
        .scope
        .internal_conversation_ref
        .clone()
        .or_else(|| context.scope.internal_principal_ref.clone())
        .unwrap_or_else(|| "the scoped continuity window".to_string())
}

fn classify_self_model_observation_kind(model_text: &str) -> &'static str {
    let lowered = model_text.to_ascii_lowercase();
    if lowered.contains("goal") || lowered.contains("subgoal") {
        return "subgoal";
    }
    if lowered.contains("style")
        || lowered.contains("tone")
        || lowered.contains("concise")
        || lowered.contains("direct")
        || lowered.contains("communication")
    {
        return "interaction_style";
    }
    "preference"
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

fn background_trigger_kind_as_str(kind: BackgroundTriggerKind) -> &'static str {
    match kind {
        BackgroundTriggerKind::TimeSchedule => "time_schedule",
        BackgroundTriggerKind::VolumeThreshold => "volume_threshold",
        BackgroundTriggerKind::DriftOrAnomalySignal => "drift_or_anomaly_signal",
        BackgroundTriggerKind::ForegroundDelegation => "foreground_delegation",
        BackgroundTriggerKind::ExternalPassiveEvent => "external_passive_event",
        BackgroundTriggerKind::MaintenanceTrigger => "maintenance_trigger",
    }
}

fn unconscious_task_class(kind: UnconsciousJobKind) -> &'static str {
    match kind {
        UnconsciousJobKind::MemoryConsolidation => "memory_consolidation",
        UnconsciousJobKind::RetrievalMaintenance => "retrieval_maintenance",
        UnconsciousJobKind::ContradictionAndDriftScan => "contradiction_and_drift_scan",
        UnconsciousJobKind::SelfModelReflection => "self_model_reflection",
    }
}

fn build_default_wake_signals(context: &UnconsciousContext) -> Vec<WakeSignal> {
    if context.job_kind != UnconsciousJobKind::SelfModelReflection {
        return Vec::new();
    }

    vec![WakeSignal {
        signal_id: uuid::Uuid::now_v7(),
        reason: WakeSignalReason::MaintenanceInsightReady,
        priority: WakeSignalPriority::Low,
        reason_code: "maintenance_insight_ready".to_string(),
        summary: "Background self-model reflection produced a maintenance insight.".to_string(),
        payload_ref: Some(format!("background_job:{}", context.job_id)),
    }]
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
        BackgroundExecutionBudget, BackgroundTrigger, BackgroundTriggerKind, ChannelKind,
        ConsciousContext, ForegroundBudget, ForegroundTrigger, ForegroundTriggerKind,
        IngressEventKind, InternalStateSnapshot, ModelOutput, ModelUsage, NormalizedIngress,
        SelfModelSnapshot, UnconsciousContext, UnconsciousJobKind, UnconsciousScope,
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
            WorkerResult::Unconscious(_) => {
                panic!("smoke worker should not return an unconscious result")
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
            WorkerResult::Unconscious(_) => {
                panic!("conscious worker should not emit an unconscious result")
            }
            WorkerResult::Error(error) => panic!("unexpected worker error: {}", error.message),
        }
    }

    #[test]
    fn unconscious_model_request_uses_scope_and_budget() {
        let request = WorkerRequest::unconscious(
            uuid::Uuid::now_v7(),
            uuid::Uuid::now_v7(),
            sample_unconscious_context(),
        );
        let WorkerPayload::Unconscious(payload) = &request.payload else {
            panic!("expected unconscious payload");
        };

        let model_request = build_unconscious_model_call_request(&request, payload.as_ref());
        assert_eq!(model_request.trace_id, request.trace_id);
        assert_eq!(model_request.execution_id, request.execution_id);
        assert_eq!(model_request.loop_kind, LoopKind::Unconscious);
        assert_eq!(model_request.purpose, ModelCallPurpose::BackgroundAnalysis);
        assert_eq!(model_request.budget.timeout_ms, 120_000);
        assert_eq!(model_request.output_mode, ModelOutputMode::PlainText);
        assert_eq!(model_request.tool_policy, ToolPolicy::ProposalOnly);
        assert!(
            model_request
                .input
                .system_prompt
                .contains("memory_consolidation")
        );
        assert!(
            model_request
                .input
                .messages
                .first()
                .is_some_and(|message| message.content.contains("Scoped background maintenance"))
        );
    }

    #[test]
    fn unconscious_worker_response_stays_structured_and_bounded() {
        let request = WorkerRequest::unconscious(
            uuid::Uuid::now_v7(),
            uuid::Uuid::now_v7(),
            sample_unconscious_context(),
        );
        let WorkerPayload::Unconscious(payload) = &request.payload else {
            panic!("expected unconscious payload");
        };
        let model_request = build_unconscious_model_call_request(&request, payload.as_ref());
        let model_response = ModelCallResponse {
            request_id: model_request.request_id,
            trace_id: request.trace_id,
            execution_id: request.execution_id,
            provider: contracts::ModelProviderKind::ZAi,
            model: "z-ai-background".to_string(),
            received_at: chrono::Utc::now(),
            output: ModelOutput {
                text: "maintenance summary".to_string(),
                json: None,
                finish_reason: "stop".to_string(),
            },
            usage: ModelUsage {
                input_tokens: 20,
                output_tokens: 6,
            },
        };

        let response =
            build_unconscious_worker_response(&request, payload.as_ref(), model_response);
        match response.result {
            WorkerResult::Unconscious(result) => {
                assert_eq!(result.status, UnconsciousWorkerStatus::Completed);
                assert!(result.summary.contains("memory_consolidation"));
                assert_eq!(result.maintenance_outputs.canonical_proposals.len(), 1);
                let proposal = &result.maintenance_outputs.canonical_proposals[0];
                assert_eq!(
                    proposal.proposal_kind,
                    CanonicalProposalKind::MemoryArtifact
                );
                assert_eq!(proposal.subject_ref, "primary-user");
                assert_eq!(
                    proposal.provenance.source_episode_id,
                    payload.context.scope.episode_ids.first().copied()
                );
                assert_eq!(result.maintenance_outputs.retrieval_updates.len(), 1);
                assert_eq!(result.maintenance_outputs.diagnostics.len(), 1);
                assert!(result.maintenance_outputs.wake_signals.is_empty());
            }
            WorkerResult::Smoke(_) => panic!("unexpected smoke response"),
            WorkerResult::Conscious(_) => panic!("unexpected conscious response"),
            WorkerResult::Error(error) => panic!("unexpected worker error: {}", error.message),
        }
    }

    #[test]
    fn contradiction_scan_classifies_conflict_as_critical_without_mutating_outputs() {
        let mut context = sample_unconscious_context();
        context.job_kind = UnconsciousJobKind::ContradictionAndDriftScan;
        context.scope.summary = "Scan recent continuity for contradictions.".to_string();

        let request =
            WorkerRequest::unconscious(uuid::Uuid::now_v7(), uuid::Uuid::now_v7(), context);
        let WorkerPayload::Unconscious(payload) = &request.payload else {
            panic!("expected unconscious payload");
        };
        let model_request = build_unconscious_model_call_request(&request, payload.as_ref());
        let model_response = ModelCallResponse {
            request_id: model_request.request_id,
            trace_id: request.trace_id,
            execution_id: request.execution_id,
            provider: contracts::ModelProviderKind::ZAi,
            model: "z-ai-background".to_string(),
            received_at: chrono::Utc::now(),
            output: ModelOutput {
                text: "Potential contradiction detected between recent memory snapshots."
                    .to_string(),
                json: None,
                finish_reason: "stop".to_string(),
            },
            usage: ModelUsage {
                input_tokens: 20,
                output_tokens: 8,
            },
        };

        let response =
            build_unconscious_worker_response(&request, payload.as_ref(), model_response);
        match response.result {
            WorkerResult::Unconscious(result) => {
                assert!(result.maintenance_outputs.canonical_proposals.is_empty());
                assert!(result.maintenance_outputs.retrieval_updates.is_empty());
                assert!(result.maintenance_outputs.wake_signals.is_empty());
                assert_eq!(result.maintenance_outputs.diagnostics.len(), 1);
                let diagnostic = &result.maintenance_outputs.diagnostics[0];
                assert_eq!(diagnostic.code, "contradiction_detected");
                assert_eq!(diagnostic.severity, DiagnosticSeverity::Critical);
            }
            WorkerResult::Smoke(_) => panic!("unexpected smoke response"),
            WorkerResult::Conscious(_) => panic!("unexpected conscious response"),
            WorkerResult::Error(error) => panic!("unexpected worker error: {}", error.message),
        }
    }

    #[test]
    fn contradiction_scan_classifies_clear_scope_as_info() {
        let mut context = sample_unconscious_context();
        context.job_kind = UnconsciousJobKind::ContradictionAndDriftScan;

        let diagnostics = build_unconscious_diagnostics(
            &context,
            "Continuity remains aligned and no notable drift was found.",
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "drift_signal_detected");
        assert_eq!(diagnostics[0].severity, DiagnosticSeverity::Warning);

        let diagnostics = build_unconscious_diagnostics(
            &context,
            "Continuity remains aligned and stable across the scoped review.",
        );
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "drift_scan_clear");
        assert_eq!(diagnostics[0].severity, DiagnosticSeverity::Info);
    }

    #[test]
    fn self_model_reflection_emits_a_self_model_observation_proposal() {
        let mut context = sample_unconscious_context();
        context.job_kind = UnconsciousJobKind::SelfModelReflection;

        let request =
            WorkerRequest::unconscious(uuid::Uuid::now_v7(), uuid::Uuid::now_v7(), context);
        let WorkerPayload::Unconscious(payload) = &request.payload else {
            panic!("expected unconscious payload");
        };
        let model_request = build_unconscious_model_call_request(&request, payload.as_ref());
        let model_response = ModelCallResponse {
            request_id: model_request.request_id,
            trace_id: request.trace_id,
            execution_id: request.execution_id,
            provider: contracts::ModelProviderKind::ZAi,
            model: "z-ai-background".to_string(),
            received_at: chrono::Utc::now(),
            output: ModelOutput {
                text: "Prefer concise progress updates during long maintenance runs.".to_string(),
                json: None,
                finish_reason: "stop".to_string(),
            },
            usage: ModelUsage {
                input_tokens: 18,
                output_tokens: 7,
            },
        };

        let response =
            build_unconscious_worker_response(&request, payload.as_ref(), model_response);
        match response.result {
            WorkerResult::Unconscious(result) => {
                assert_eq!(result.maintenance_outputs.canonical_proposals.len(), 1);
                assert!(result.maintenance_outputs.retrieval_updates.is_empty());
                assert_eq!(result.maintenance_outputs.diagnostics.len(), 1);
                assert_eq!(result.maintenance_outputs.wake_signals.len(), 1);
                let proposal = &result.maintenance_outputs.canonical_proposals[0];
                assert_eq!(
                    proposal.proposal_kind,
                    CanonicalProposalKind::SelfModelObservation
                );
                assert_eq!(
                    proposal.canonical_target,
                    CanonicalTargetKind::SelfModelArtifacts
                );
                assert_eq!(
                    proposal.provenance.provenance_kind,
                    ProposalProvenanceKind::SelfModelReflection
                );
                let CanonicalProposalPayload::SelfModelObservation(payload) = &proposal.payload
                else {
                    panic!("expected a self-model observation payload");
                };
                assert_eq!(payload.observation_kind, "interaction_style");
            }
            WorkerResult::Smoke(_) => panic!("unexpected smoke response"),
            WorkerResult::Conscious(_) => panic!("unexpected conscious response"),
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

    fn sample_unconscious_context() -> UnconsciousContext {
        UnconsciousContext {
            context_id: uuid::Uuid::now_v7(),
            assembled_at: chrono::Utc::now(),
            job_id: uuid::Uuid::now_v7(),
            job_kind: UnconsciousJobKind::MemoryConsolidation,
            trigger: BackgroundTrigger {
                trigger_id: uuid::Uuid::now_v7(),
                trigger_kind: BackgroundTriggerKind::ForegroundDelegation,
                requested_at: chrono::Utc::now(),
                reason_summary: "foreground requested consolidation".to_string(),
                payload_ref: Some("execution:latest".to_string()),
            },
            scope: UnconsciousScope {
                episode_ids: vec![uuid::Uuid::now_v7(), uuid::Uuid::now_v7()],
                memory_artifact_ids: vec![uuid::Uuid::now_v7()],
                retrieval_artifact_ids: vec![uuid::Uuid::now_v7()],
                self_model_artifact_id: None,
                internal_principal_ref: Some("primary-user".to_string()),
                internal_conversation_ref: Some("telegram-primary".to_string()),
                summary: "Consolidate recent episodes into long-term memory.".to_string(),
            },
            budget: BackgroundExecutionBudget {
                iteration_budget: 2,
                wall_clock_budget_ms: 120_000,
                token_budget: 6_000,
            },
        }
    }
}
