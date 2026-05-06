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
    ForegroundExecutionMode, GovernedActionObservation, GovernedActionProposal,
    IdentityDeltaProposal, IdentityInterviewAnswer, IdentityKickstartAction,
    IdentityKickstartActionKind, IdentityLifecycleState, IdentityReflectionOutput, LoopKind,
    MemoryArtifactProposal, ModelBudget, ModelCallPurpose, ModelCallRequest, ModelCallResponse,
    ModelInput, ModelInputMessage, ModelMessageRole, ModelOutputMode, ProposalConflictPosture,
    ProposalProvenance, ProposalProvenanceKind, RetrievalUpdateOperation, RetrievalUpdateProposal,
    SelfModelObservationProposal, SmokeWorkerResult, ToolPolicy, UnconsciousContext,
    UnconsciousJobKind, UnconsciousMaintenanceOutputs, UnconsciousWorkerRequest,
    UnconsciousWorkerResult, UnconsciousWorkerStatus, WakeSignal, WorkerErrorCode, WorkerFailure,
    WorkerPayload, WorkerRequest, WorkerResponse, WorkerResult, predefined_identity_delta,
};
use serde::Serialize;

const GOVERNED_ACTIONS_BLOCK_TAG: &str = "blue-lagoon-governed-actions";
const IDENTITY_KICKSTART_BLOCK_TAG: &str = "blue-lagoon-identity-kickstart";

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
        tool_policy: ToolPolicy::ProposalOnly,
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

    let identity_reflection_output =
        payload.context.job_kind == UnconsciousJobKind::SelfModelReflection;

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
        output_mode: if identity_reflection_output {
            ModelOutputMode::JsonObject
        } else {
            ModelOutputMode::PlainText
        },
        schema_name: identity_reflection_output.then(|| "identity_reflection_output".to_string()),
        schema_json: identity_reflection_output.then(identity_reflection_output_schema),
        tool_policy: ToolPolicy::ProposalOnly,
        provider_hint: None,
    }
}

fn identity_reflection_output_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["identity_delta", "no_change_rationale", "diagnostics", "wake_signals"],
        "properties": {
            "identity_delta": {
                "type": ["object", "null"],
                "description": "Optional IdentityDeltaProposal. Use null when no identity change is warranted."
            },
            "no_change_rationale": {
                "type": ["string", "null"],
                "description": "Required when identity_delta is null; concise reason no canonical identity change should be proposed."
            },
            "diagnostics": {
                "type": "array",
                "description": "Optional diagnostic alerts for drift, contradiction, uncertainty, or maintenance notes.",
                "items": { "type": "object" }
            },
            "wake_signals": {
                "type": "array",
                "description": "Optional wake-signal requests when user guidance or later foreground attention is warranted.",
                "items": { "type": "object" }
            }
        }
    })
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

    if !context.governed_action_observations.is_empty() {
        let loop_state_fragment = context
            .governed_action_loop_state
            .as_ref()
            .map(|state| {
                format!(
                    " Foreground action loop state: {}.",
                    governed_action_loop_state_summary(state)
                )
            })
            .unwrap_or_default();
        messages.push(ModelInputMessage {
            role: ModelMessageRole::Developer,
            content: format!(
                "Harness governed-action observations: {}.{} Continue the foreground turn using these outcomes. If another governed action is still needed, propose it in the same turn and let the harness decide whether it is allowed, approval-gated, or denied based on policy, the configured per-turn action limit, and the remaining loop budget. Do not claim that any follow-up action already happened unless it appears in the harness observations.",
                governed_action_observation_summary(&context.governed_action_observations),
                loop_state_fragment,
            ),
        });
    } else {
        messages.push(ModelInputMessage {
            role: ModelMessageRole::Developer,
            content: governed_action_schema_message(),
        });
        if should_include_troubleshooting_guidance(context) {
            messages.push(ModelInputMessage {
                role: ModelMessageRole::Developer,
                content: troubleshooting_guidance_message(),
            });
        }
        if let Some(message) = identity_kickstart_schema_message(context) {
            messages.push(ModelInputMessage {
                role: ModelMessageRole::Developer,
                content: message,
            });
        }
    }

    let subgoals_fragment = if context.self_model.current_subgoals.is_empty() {
        String::new()
    } else {
        format!(
            " Active subgoals: {}.",
            join_or_none(&context.self_model.current_subgoals)
        )
    };
    let active_conditions_fragment = if context.internal_state.active_conditions.is_empty() {
        String::new()
    } else {
        format!(
            " Active conditions: {}.",
            join_or_none(&context.internal_state.active_conditions)
        )
    };

    let current_time = context
        .assembled_at
        .format("%Y-%m-%d %H:%M UTC")
        .to_string();

    let identity_fragment = identity_system_prompt_fragment(context);

    ModelInput {
        system_prompt: format!(
            "You are {name}, a harness-governed personal AI assistant. You communicate with a single privileged user via Telegram.\n\nRole: {role}. Communication style: {style}. Behavioral preferences: {preferences}.{identity}\n\nCapabilities: {capabilities}.\nActive constraints: {constraints}.\nGoals: {goals}.{subgoals}{conditions}\n\nCurrent time: {current_time}.\n\nRuntime state: load={load}%, health={health}%, confidence={confidence}%, mode={mode}.\n\nYou have governed actions available for executing commands and running workspace scripts. Network access is disabled by default; any proposal with network enabled is automatically routed for approval. See the developer message for the full action schema. Never tell the user you have no tools — use the governed action system when needed.",
            name = context.self_model.stable_identity,
            role = context.self_model.role,
            style = context.self_model.communication_style,
            preferences = join_or_none(&context.self_model.preferences),
            identity = identity_fragment,
            capabilities = join_or_none(&context.self_model.capabilities),
            constraints = join_or_none(&context.self_model.constraints),
            goals = join_or_none(&context.self_model.current_goals),
            subgoals = subgoals_fragment,
            conditions = active_conditions_fragment,
            current_time = current_time,
            load = context.internal_state.load_pct,
            health = context.internal_state.health_pct,
            confidence = context.internal_state.confidence_pct,
            mode = foreground_execution_mode_as_str(context.recovery_context.mode),
        ),
        messages,
    }
}

fn identity_system_prompt_fragment(context: &ConsciousContext) -> String {
    if context.self_model.identity_lifecycle.kickstart_available {
        return " Identity formation is available: the assistant does not yet have a complete chosen identity with the user.".to_string();
    }

    let Some(identity) = &context.self_model.identity else {
        return String::new();
    };

    let mut parts = Vec::new();
    if !identity.identity_summary.is_empty() {
        parts.push(format!("Identity: {}", identity.identity_summary));
    }
    if let Some(description) = &identity.self_description {
        parts.push(format!("Self-description: {description}"));
    }
    if !identity.values.is_empty() {
        parts.push(format!("Values: {}", join_or_none(&identity.values)));
    }
    if !identity.boundaries.is_empty() {
        parts.push(format!(
            "Boundaries: {}",
            join_or_none(&identity.boundaries)
        ));
    }

    if parts.is_empty() {
        String::new()
    } else {
        format!(" {}", parts.join(". "))
    }
}

fn should_include_troubleshooting_guidance(context: &ConsciousContext) -> bool {
    let trigger_text = context
        .trigger
        .ingress
        .text_body
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let troubleshooting_terms = [
        "error",
        "trace",
        "log",
        "logs",
        "diagnostic",
        "diagnostics",
        "troubleshoot",
        "debug",
        "failure",
        "failed",
        "crash",
        "stuck",
        "what happened",
        "why did",
    ];
    troubleshooting_terms
        .iter()
        .any(|term| trigger_text.contains(term))
}

fn troubleshooting_guidance_message() -> String {
    r#"TROUBLESHOOTING CAPABILITY

Use this only when the user asks about runtime errors, traces, logs, failures, diagnostics, or why the assistant got stuck. This is progressive disclosure: do not discuss these internals unless they are relevant to troubleshooting.

Self-understanding boundary:
- You are the conscious assistant identity, not the harness. The harness is the runtime/body that assembles context, mediates actions, validates proposals, owns canonical writes, and records traces.
- You may know the high-level conscious/unconscious loop model and read restricted internal documentation for troubleshooting.
- You must not claim direct control over memory, identity storage, the database, workers, or the harness. You can influence memory and identity only through normal conscious behavior and harness-mediated proposals.

Restricted internal documentation reads:
- You may inspect `PHILOSOPHY.md`, `docs/REQUIREMENTS.md`, `docs/IMPLEMENTATION_DESIGN.md`, and selected files under `docs/internal/` only through the `run_diagnostic` action's `internal_doc` query.

Diagnostic tool:
- Use `run_diagnostic` for runtime troubleshooting. It is a harness-native read-only action: no shell, no filesystem scope, no environment variables, no network, and no state-changing admin commands.
- Available query names: `runtime_status`, `health_summary`, `operational_diagnostics`, `trace_recent`, `trace_show`, `foreground_pending`, `foreground_schedules`, `background_list`, `recovery_checkpoints`, `recovery_leases`, `schema_status`, `schema_upgrade_path`, `approvals_list`, `actions_list`, `wake_signals_list`, `identity_status`, `identity_show`, `identity_history`, `identity_diagnostics`, `workspace_artifacts`, `workspace_scripts`, `workspace_runs`, `internal_doc`.
- Available internal documents: `philosophy`, `requirements`, `implementation_design`, `internal_documentation`, `context_assembly`, `governed_actions`.

Do not propose `run_subprocess` for diagnostics. If a diagnostic query fails, report the exact failure and the next useful diagnostic query instead of guessing.

When a trace id is present in chat, start with `trace_show`, then use `operational_diagnostics`, `health_summary`, and `trace_recent` only if needed."#
        .to_string()
}

fn identity_kickstart_schema_message(context: &ConsciousContext) -> Option<String> {
    let kickstart = context.self_model.identity_lifecycle.kickstart.as_ref()?;
    if !context.self_model.identity_lifecycle.kickstart_available {
        return None;
    }

    let mut available_actions = kickstart
        .available_actions
        .iter()
        .map(|action| match action {
            IdentityKickstartActionKind::SelectPredefinedTemplate => "select_predefined_identity",
            IdentityKickstartActionKind::StartCustomInterview => "start_custom_identity_interview",
            IdentityKickstartActionKind::AnswerCustomInterview => "answer_custom_identity_question",
            IdentityKickstartActionKind::Cancel => "cancel_identity_formation",
        })
        .collect::<Vec<_>>();
    available_actions.sort_unstable();

    let templates = kickstart
        .predefined_templates
        .iter()
        .map(|template| {
            format!(
                "- {}: {} ({})",
                template.template_key, template.display_name, template.summary
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let templates = if templates.is_empty() {
        "No predefined identities are available in this step.".to_string()
    } else {
        templates
    };
    let next_step = kickstart.next_step.as_deref().unwrap_or("choose_next_step");
    let resume_summary = kickstart.resume_summary.as_deref().unwrap_or("none");

    Some(format!(
        r#"IDENTITY FORMATION CAPABILITY

The assistant does not yet have a complete chosen identity with the user. You may help the user form it when the conversation calls for that. Do not mention implementation details or hidden maintenance.

Available identity actions: {available_actions}
Next identity step: {next_step}
Resume summary: {resume_summary}
Predefined identities:
{templates}

To request identity formation, append exactly one fenced code block tagged "{tag}" after your user-visible reply. Omit this block unless the user is choosing, starting, answering, or canceling identity formation.

```{tag}
{{
  "action": "select_predefined_identity",
  "template_key": "<one predefined identity key, or null>",
  "answer": null,
  "cancel_reason": null
}}
```

For a custom path, use action "start_custom_identity_interview" or "answer_custom_identity_question". For cancellation, use action "cancel_identity_formation"."#,
        available_actions = available_actions.join(", "),
        next_step = next_step,
        resume_summary = resume_summary,
        templates = templates,
        tag = IDENTITY_KICKSTART_BLOCK_TAG,
    ))
}

fn governed_action_schema_message() -> String {
    let template = r#"GOVERNED ACTION SYSTEM

To perform an action, append exactly one fenced code block tagged "TAG" after your user-visible reply. Omit the block entirely if no action is needed. Keep all user-facing text outside the block.

Available action kinds:
- inspect_workspace_artifact: inspect one non-script workspace artifact by UUID
- list_workspace_artifacts: list/search non-script workspace artifacts
- create_workspace_artifact: create a note, runbook, scratchpad, or task list
- update_workspace_artifact: replace a non-script workspace artifact with provenance
- list_workspace_scripts: list/search workspace scripts
- inspect_workspace_script: inspect workspace script metadata and bounded content
- create_workspace_script: create a governed workspace script
- append_workspace_script_version: append an auditable script version
- list_workspace_script_runs: inspect bounded script run history
- upsert_scheduled_foreground_task: create or update future foreground work
- request_background_job: request bounded background maintenance work
- run_diagnostic: run a harness-native read-only diagnostic query
- run_subprocess: execute a bounded shell command
- run_workspace_script: run a registered workspace script by its script_id UUID
- web_fetch: perform an HTTP GET request to a URL (requires network: "enabled"; automatically routed for approval)

Block format (wrap all proposals in {"actions": [...]}):
```TAG
{
  "actions": [
    {
      "proposal_id": "<generate a fresh UUID v4>",
      "title": "<one-line description>",
      "rationale": "<why this action is needed>",
      "action_kind": "run_subprocess",
      "requested_risk_tier": null,
      "capability_scope": {
        "filesystem": { "read_roots": ["<absolute path>"], "write_roots": [] },
        "network": "disabled",
        "environment": { "allow_variables": [] },
        "execution": { "timeout_ms": 30000, "max_stdout_bytes": 16384, "max_stderr_bytes": 8192 }
      },
      "payload": {
        "kind": "run_subprocess",
        "value": { "command": "<executable>", "args": ["<arg1>", "<arg2>"], "working_directory": "<absolute path or null>" }
      }
    }
  ]
}
```

Alternate payload shape for run_workspace_script:
- "payload": { "kind": "run_workspace_script", "value": { "script_id": "<uuid>", "script_version_id": null, "args": [] } }

Harness-native payload examples:
- "payload": { "kind": "list_workspace_artifacts", "value": { "artifact_kind": null, "status": "active", "query": null, "limit": 10 } }
- "payload": { "kind": "inspect_workspace_script", "value": { "script_id": "<uuid>", "script_version_id": null } }
- "payload": { "kind": "create_workspace_artifact", "value": { "artifact_kind": "scratchpad", "title": "...", "content_text": "...", "provenance": "conversation" } }
- "payload": { "kind": "append_workspace_script_version", "value": { "script_id": "<uuid>", "expected_latest_version_id": "<uuid>", "expected_content_sha256": null, "language": "python", "content_text": "...", "change_summary": "..." } }
- "payload": { "kind": "upsert_scheduled_foreground_task", "value": { "task_key": "check_in", "title": "Check in", "user_facing_prompt": "...", "next_due_at_utc": "2026-04-29T10:00:00Z", "cadence_seconds": 86400, "cooldown_seconds": 3600, "internal_principal_ref": "primary-user", "internal_conversation_ref": "telegram-primary", "active": true } }
- "payload": { "kind": "request_background_job", "value": { "job_kind": "memory_consolidation", "rationale": "...", "input_scope_ref": null, "urgency": "normal", "wake_preference": null, "internal_conversation_ref": "telegram-primary" } }
- "payload": { "kind": "run_diagnostic", "value": { "query": { "query": "trace_show", "params": { "trace_id": "<uuid>", "execution_id": null } } } }
- "payload": { "kind": "run_diagnostic", "value": { "query": { "query": "internal_doc", "params": { "document": "context_assembly" } } } }
- For harness-native payloads, capability_scope.filesystem read_roots/write_roots must be [], network must be "disabled", environment allow_variables must be [], and execution values may be 0.

Alternate payload shape for web_fetch:
- "payload": { "kind": "web_fetch", "value": { "url": "https://...", "timeout_ms": 10000, "max_response_bytes": 524288 } }
- capability_scope.filesystem: { "read_roots": [], "write_roots": [] } (no filesystem access needed)
- capability_scope.network must be "enabled" (triggers approval flow)
- capability_scope.environment: { "allow_variables": [] }
- capability_scope.execution: { "timeout_ms": 0, "max_stdout_bytes": 0, "max_stderr_bytes": 0 } (ignored for web_fetch)

Scope rules: filesystem.read_roots must be non-empty for subprocess/script actions. write_roots only if the action writes files. Propose at most one action per turn."#;
    template.replace("TAG", GOVERNED_ACTIONS_BLOCK_TAG)
}

fn governed_action_observation_summary(observations: &[GovernedActionObservation]) -> String {
    observations
        .iter()
        .map(|observation| {
            format!(
                "{}:{}:{}",
                governed_action_kind_as_str(observation.action_kind),
                governed_action_status_as_str(observation.outcome.status),
                observation.outcome.summary
            )
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

fn governed_action_loop_state_summary(
    state: &contracts::ForegroundGovernedActionLoopState,
) -> String {
    format!(
        "executed_actions={}; remaining_before_cap={}; max_actions_per_turn={}; cap_exceeded_behavior={}",
        state.executed_action_count,
        state.remaining_actions_before_cap,
        state.max_actions_per_turn,
        match state.cap_exceeded_behavior {
            contracts::GovernedActionCapExceededBehavior::Escalate => "escalate",
            contracts::GovernedActionCapExceededBehavior::AlwaysApprove => "always_approve",
            contracts::GovernedActionCapExceededBehavior::AlwaysDeny => "always_deny",
        }
    )
}

fn build_governed_action_proposals(
    model_text: &str,
) -> std::result::Result<Vec<GovernedActionProposal>, String> {
    let Some(block_json) = extract_governed_action_block(model_text) else {
        return Ok(Vec::new());
    };

    #[derive(serde::Deserialize)]
    struct GovernedActionEnvelope {
        actions: Vec<GovernedActionProposal>,
    }

    let envelope: GovernedActionEnvelope = serde_json::from_str(block_json)
        .map_err(|error| format!("invalid governed-action proposal block: {error}"))?;
    Ok(envelope.actions)
}

fn build_identity_kickstart_proposals(
    context: &ConsciousContext,
    model_text: &str,
) -> std::result::Result<Vec<CanonicalProposal>, String> {
    let Some(block_json) = extract_tagged_block(model_text, IDENTITY_KICKSTART_BLOCK_TAG) else {
        return Ok(Vec::new());
    };
    if !context.self_model.identity_lifecycle.kickstart_available {
        return Ok(Vec::new());
    }
    #[derive(serde::Deserialize)]
    struct IdentityKickstartBlock {
        action: String,
        template_key: Option<String>,
        answer: Option<serde_json::Value>,
        cancel_reason: Option<String>,
    }

    let block: IdentityKickstartBlock = match serde_json::from_str(block_json) {
        Ok(block) => block,
        Err(_) => return Ok(Vec::new()),
    };
    match block.action.as_str() {
        "select_predefined_identity" => {
            if context.self_model.identity_lifecycle.state
                != IdentityLifecycleState::BootstrapSeedOnly
            {
                return Ok(Vec::new());
            }
            let Some(template_key) = block.template_key.as_deref() else {
                return Ok(Vec::new());
            };
            let Some(delta) =
                predefined_identity_delta(template_key, context.trigger.ingress.occurred_at)
            else {
                return Ok(Vec::new());
            };
            Ok(vec![CanonicalProposal {
                proposal_id: uuid::Uuid::now_v7(),
                proposal_kind: CanonicalProposalKind::IdentityDelta,
                canonical_target: CanonicalTargetKind::IdentityItems,
                confidence_pct: 100,
                conflict_posture: ProposalConflictPosture::Independent,
                subject_ref: "self:blue-lagoon".to_string(),
                rationale: Some(format!(
                    "User selected predefined identity template '{template_key}'."
                )),
                valid_from: Some(context.trigger.ingress.occurred_at),
                valid_to: None,
                supersedes_artifact_id: None,
                provenance: ProposalProvenance {
                    provenance_kind: ProposalProvenanceKind::EpisodeObservation,
                    source_ingress_ids: current_source_ingress_ids(context),
                    source_episode_id: None,
                },
                payload: CanonicalProposalPayload::IdentityDelta(delta),
            }])
        }
        "start_custom_identity_interview" => Ok(vec![identity_interview_action_proposal(
            context,
            IdentityKickstartAction::StartCustomInterview,
            IdentityLifecycleState::IdentityKickstartInProgress,
            "User started a custom identity interview.",
        )]),
        "answer_custom_identity_question" => {
            let Some(answer) = parse_identity_interview_answer(context, block.answer.as_ref())?
            else {
                return Ok(Vec::new());
            };
            Ok(vec![identity_interview_action_proposal(
                context,
                IdentityKickstartAction::AnswerCustomInterview(answer),
                IdentityLifecycleState::IdentityKickstartInProgress,
                "User answered a custom identity interview step.",
            )])
        }
        "cancel_identity_formation" => Ok(vec![identity_interview_action_proposal(
            context,
            IdentityKickstartAction::Cancel {
                reason: block.cancel_reason,
            },
            IdentityLifecycleState::BootstrapSeedOnly,
            "User cancelled identity formation.",
        )]),
        _ => Ok(Vec::new()),
    }
}

fn parse_identity_interview_answer(
    context: &ConsciousContext,
    value: Option<&serde_json::Value>,
) -> std::result::Result<Option<IdentityInterviewAnswer>, String> {
    let Some(value) = value else {
        return Ok(infer_identity_interview_answer_from_trigger(context));
    };
    if value.is_null() {
        return Ok(infer_identity_interview_answer_from_trigger(context));
    }
    if let Some(answer_text) = value.as_str() {
        let Some(step_key) = current_identity_interview_step(context) else {
            return Ok(None);
        };
        return Ok(Some(IdentityInterviewAnswer {
            step_key,
            answer_text: answer_text.trim().to_string(),
        }));
    }

    match serde_json::from_value::<IdentityInterviewAnswer>(value.clone()) {
        Ok(answer) => Ok(Some(answer)),
        Err(_) => Ok(infer_identity_interview_answer_from_trigger(context)),
    }
}

fn infer_identity_interview_answer_from_trigger(
    context: &ConsciousContext,
) -> Option<IdentityInterviewAnswer> {
    let step_key = current_identity_interview_step(context)?;
    let answer_text = context.trigger.ingress.text_body.as_deref()?.trim();
    if answer_text.is_empty() {
        return None;
    }
    Some(IdentityInterviewAnswer {
        step_key,
        answer_text: answer_text.to_string(),
    })
}

fn current_identity_interview_step(context: &ConsciousContext) -> Option<String> {
    if context.self_model.identity_lifecycle.state
        != IdentityLifecycleState::IdentityKickstartInProgress
    {
        return None;
    }
    context
        .self_model
        .identity_lifecycle
        .kickstart
        .as_ref()
        .and_then(|kickstart| kickstart.next_step.clone())
}

fn identity_interview_action_proposal(
    context: &ConsciousContext,
    action: IdentityKickstartAction,
    lifecycle_state: IdentityLifecycleState,
    rationale: &str,
) -> CanonicalProposal {
    CanonicalProposal {
        proposal_id: uuid::Uuid::now_v7(),
        proposal_kind: CanonicalProposalKind::IdentityDelta,
        canonical_target: CanonicalTargetKind::IdentityItems,
        confidence_pct: 100,
        conflict_posture: ProposalConflictPosture::Independent,
        subject_ref: "self:blue-lagoon".to_string(),
        rationale: Some(rationale.to_string()),
        valid_from: Some(context.trigger.ingress.occurred_at),
        valid_to: None,
        supersedes_artifact_id: None,
        provenance: ProposalProvenance {
            provenance_kind: ProposalProvenanceKind::EpisodeObservation,
            source_ingress_ids: current_source_ingress_ids(context),
            source_episode_id: None,
        },
        payload: CanonicalProposalPayload::IdentityDelta(IdentityDeltaProposal {
            lifecycle_state,
            item_deltas: Vec::new(),
            self_description_delta: None,
            interview_action: Some(action),
            rationale: rationale.to_string(),
        }),
    }
}

fn strip_worker_control_blocks(model_text: &str) -> String {
    let without_identity = strip_tagged_block(model_text, IDENTITY_KICKSTART_BLOCK_TAG);
    strip_tagged_block(&without_identity, GOVERNED_ACTIONS_BLOCK_TAG)
}

fn strip_tagged_block(model_text: &str, tag: &str) -> String {
    match tagged_block_bounds(model_text, tag) {
        Some((start, _json_start, _json_end, _end)) => model_text[..start].trim_end().to_string(),
        None => model_text.to_string(),
    }
}

fn extract_governed_action_block(model_text: &str) -> Option<&str> {
    extract_tagged_block(model_text, GOVERNED_ACTIONS_BLOCK_TAG)
}

fn extract_tagged_block<'a>(model_text: &'a str, tag: &str) -> Option<&'a str> {
    tagged_block_bounds(model_text, tag)
        .map(|(_start, json_start, json_end, _end)| model_text[json_start..json_end].trim())
}

fn tagged_block_bounds(model_text: &str, tag: &str) -> Option<(usize, usize, usize, usize)> {
    let marker = format!("```{tag}");
    let start = model_text.rfind(&marker)?;
    let after_marker = &model_text[start + marker.len()..];
    let newline_offset = after_marker.find('\n')?;
    let json_start = start + marker.len() + newline_offset + 1;
    let after_json = &model_text[json_start..];
    let fence_offset = after_json.find("\n```")?;
    let json_end = json_start + fence_offset;
    let end = json_end + "\n```".len();
    Some((start, json_start, json_end, end))
}

fn governed_action_kind_as_str(kind: contracts::GovernedActionKind) -> &'static str {
    match kind {
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
        contracts::GovernedActionKind::RunSubprocess => "run_subprocess",
        contracts::GovernedActionKind::RunWorkspaceScript => "run_workspace_script",
        contracts::GovernedActionKind::WebFetch => "web_fetch",
    }
}

fn governed_action_status_as_str(status: contracts::GovernedActionStatus) -> &'static str {
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

    if let Some(evidence) = &context.evidence {
        messages.push(ModelInputMessage {
            role: ModelMessageRole::Developer,
            content: format!(
                "Bounded reflection evidence: {}",
                serde_json::to_string(evidence)
                    .unwrap_or_else(|_| "evidence serialization unavailable".to_string())
            ),
        });
    }

    if context.job_kind == UnconsciousJobKind::SelfModelReflection {
        messages.push(ModelInputMessage {
            role: ModelMessageRole::Developer,
            content: "Self-model reflection must return one JSON object matching identity_reflection_output. Use identity_delta only for evidence-backed identity item or self-description changes. Use no_change_rationale when no canonical identity change is warranted. Diagnostics and wake_signals are optional arrays. Do not request direct writes, direct side effects, or user-facing replies.".to_string(),
        });
    }

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
    let mut candidate_proposals = build_candidate_proposals(&payload.context)?;
    candidate_proposals.extend(build_identity_kickstart_proposals(
        &payload.context,
        &model_response.output.text,
    )?);
    let governed_action_proposals = build_governed_action_proposals(&model_response.output.text)?;
    let assistant_text = strip_worker_control_blocks(&model_response.output.text);
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
                text: assistant_text,
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
            governed_action_proposals,
            governed_action_observations: Vec::new(),
        }),
    })
}

fn build_unconscious_worker_response(
    request: &WorkerRequest,
    payload: &UnconsciousWorkerRequest,
    model_response: ModelCallResponse,
) -> WorkerResponse {
    let model_output = model_response.output;
    let model_text = model_output.text;
    let reflection_output = if payload.context.job_kind == UnconsciousJobKind::SelfModelReflection {
        Some(parse_identity_reflection_output(model_output.json.as_ref()))
    } else {
        None
    };
    let canonical_proposals = build_unconscious_canonical_proposals(
        &payload.context,
        &model_text,
        reflection_output
            .as_ref()
            .and_then(|parsed| parsed.output.as_ref()),
    );
    let retrieval_updates =
        build_unconscious_retrieval_updates(&payload.context, &model_text, &canonical_proposals);
    let diagnostics =
        build_unconscious_diagnostics(&payload.context, &model_text, reflection_output.as_ref());
    let wake_signals = build_unconscious_wake_signals(&payload.context, reflection_output.as_ref());

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
                wake_signals,
            },
        }),
    }
}

#[derive(Debug, Clone)]
struct ParsedIdentityReflectionOutput {
    output: Option<IdentityReflectionOutput>,
    diagnostic: Option<DiagnosticAlert>,
}

fn parse_identity_reflection_output(
    model_json: Option<&serde_json::Value>,
) -> ParsedIdentityReflectionOutput {
    let Some(model_json) = model_json else {
        return ParsedIdentityReflectionOutput {
            output: None,
            diagnostic: Some(identity_reflection_invalid_output_diagnostic(
                "self-model reflection did not return the required JSON object".to_string(),
            )),
        };
    };

    match serde_json::from_value::<IdentityReflectionOutput>(model_json.clone()) {
        Ok(output) => ParsedIdentityReflectionOutput {
            output: Some(output),
            diagnostic: None,
        },
        Err(error) => ParsedIdentityReflectionOutput {
            output: None,
            diagnostic: Some(identity_reflection_invalid_output_diagnostic(format!(
                "self-model reflection returned invalid identity_reflection_output JSON: {error}"
            ))),
        },
    }
}

fn identity_reflection_invalid_output_diagnostic(details: String) -> DiagnosticAlert {
    DiagnosticAlert {
        alert_id: uuid::Uuid::now_v7(),
        code: "identity_reflection_invalid_output".to_string(),
        severity: DiagnosticSeverity::Warning,
        summary: "Self-model reflection output was ignored because it did not satisfy the structured identity contract.".to_string(),
        details: Some(details),
    }
}

fn build_unconscious_canonical_proposals(
    context: &UnconsciousContext,
    model_text: &str,
    reflection_output: Option<&IdentityReflectionOutput>,
) -> Vec<CanonicalProposal> {
    match context.job_kind {
        UnconsciousJobKind::MemoryConsolidation => {
            build_memory_consolidation_proposals(context, model_text)
        }
        UnconsciousJobKind::SelfModelReflection => reflection_output
            .and_then(|output| output.identity_delta.clone())
            .map(|delta| build_identity_reflection_proposal(context, delta))
            .into_iter()
            .collect(),
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

fn build_identity_reflection_proposal(
    context: &UnconsciousContext,
    identity_delta: IdentityDeltaProposal,
) -> CanonicalProposal {
    CanonicalProposal {
        proposal_id: uuid::Uuid::now_v7(),
        proposal_kind: CanonicalProposalKind::IdentityDelta,
        canonical_target: CanonicalTargetKind::IdentityItems,
        confidence_pct: identity_delta
            .item_deltas
            .iter()
            .map(|delta| delta.confidence_pct)
            .max()
            .unwrap_or(70),
        conflict_posture: ProposalConflictPosture::Independent,
        subject_ref: "self:blue-lagoon".to_string(),
        rationale: Some(identity_delta.rationale.clone()),
        valid_from: None,
        valid_to: None,
        supersedes_artifact_id: None,
        provenance: ProposalProvenance {
            provenance_kind: ProposalProvenanceKind::SelfModelReflection,
            source_ingress_ids: Vec::new(),
            source_episode_id: context.scope.episode_ids.first().copied(),
        },
        payload: CanonicalProposalPayload::IdentityDelta(identity_delta),
    }
}

fn build_unconscious_retrieval_updates(
    context: &UnconsciousContext,
    model_text: &str,
    canonical_proposals: &[CanonicalProposal],
) -> Vec<RetrievalUpdateProposal> {
    match context.job_kind {
        UnconsciousJobKind::MemoryConsolidation | UnconsciousJobKind::RetrievalMaintenance => {
            if context.scope.episode_ids.is_empty() {
                return Vec::new();
            }

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
    reflection_output: Option<&ParsedIdentityReflectionOutput>,
) -> Vec<DiagnosticAlert> {
    match context.job_kind {
        UnconsciousJobKind::ContradictionAndDriftScan => {
            vec![classify_contradiction_and_drift(context, model_text)]
        }
        UnconsciousJobKind::SelfModelReflection => {
            let Some(reflection_output) = reflection_output else {
                return vec![identity_reflection_invalid_output_diagnostic(
                    "identity reflection output was not parsed".to_string(),
                )];
            };

            if let Some(diagnostic) = &reflection_output.diagnostic {
                return vec![diagnostic.clone()];
            }

            let Some(output) = &reflection_output.output else {
                return Vec::new();
            };

            let mut diagnostics = output.diagnostics.clone();
            if output.identity_delta.is_none() {
                diagnostics.push(DiagnosticAlert {
                    alert_id: uuid::Uuid::now_v7(),
                    code: "identity_reflection_no_change".to_string(),
                    severity: DiagnosticSeverity::Info,
                    summary: "Self-model reflection completed without identity changes."
                        .to_string(),
                    details: output.no_change_rationale.clone(),
                });
            }
            diagnostics
        }
        UnconsciousJobKind::MemoryConsolidation | UnconsciousJobKind::RetrievalMaintenance => {
            vec![DiagnosticAlert {
                alert_id: uuid::Uuid::now_v7(),
                code: format!("{}_completed", unconscious_task_class(context.job_kind)),
                severity: DiagnosticSeverity::Info,
                summary: format!(
                    "{} completed under bounded background execution",
                    unconscious_task_class(context.job_kind)
                ),
                details: None,
            }]
        }
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

fn build_unconscious_wake_signals(
    context: &UnconsciousContext,
    reflection_output: Option<&ParsedIdentityReflectionOutput>,
) -> Vec<WakeSignal> {
    if context.job_kind != UnconsciousJobKind::SelfModelReflection {
        return Vec::new();
    }

    reflection_output
        .and_then(|parsed| parsed.output.as_ref())
        .map(|output| output.wake_signals.clone())
        .unwrap_or_default()
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
        CompactIdentityItem, CompactIdentitySnapshot, ConsciousContext, ForegroundBudget,
        ForegroundTrigger, ForegroundTriggerKind, IdentityDeltaOperation, IdentityEvidenceRef,
        IdentityItemCategory, IdentityItemDelta, IdentityItemSource, IdentityKickstartContext,
        IdentityLifecycleContext, IdentityLifecycleState, IdentityMergePolicy,
        IdentityStabilityClass, IngressEventKind, InternalStateSnapshot, ModelOutput, ModelUsage,
        NormalizedIngress, PredefinedIdentityTemplate, SelfDescriptionDelta, SelfModelSnapshot,
        UnconsciousContext, UnconsciousJobKind, UnconsciousScope, WakeSignalPriority,
        WakeSignalReason,
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
        assert_eq!(model_request.tool_policy, ToolPolicy::ProposalOnly);
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
        assert!(model_request.input.system_prompt.contains("load=15%"));
        assert!(model_request.input.system_prompt.contains("confidence=80%"));
        assert!(model_request.input.messages.iter().any(|message| {
            message.content == "remember that I prefer concise replies and be direct"
        }));
        assert!(
            model_request
                .input
                .messages
                .iter()
                .any(|message| { message.content.contains(GOVERNED_ACTIONS_BLOCK_TAG) })
        );
    }

    #[test]
    fn conscious_model_request_includes_identity_kickstart_only_when_available() {
        let mut context = sample_context();
        context.self_model.identity_lifecycle = IdentityLifecycleContext {
            state: IdentityLifecycleState::BootstrapSeedOnly,
            kickstart_available: true,
            kickstart: Some(IdentityKickstartContext {
                available_actions: vec![
                    IdentityKickstartActionKind::SelectPredefinedTemplate,
                    IdentityKickstartActionKind::StartCustomInterview,
                ],
                next_step: Some("choose_predefined_identity_or_start_custom_interview".to_string()),
                resume_summary: None,
                predefined_templates: vec![PredefinedIdentityTemplate {
                    template_key: "continuity_operator".to_string(),
                    display_name: "Continuity Operator".to_string(),
                    summary: "Steady continuity-focused assistant.".to_string(),
                }],
            }),
        };
        let request = WorkerRequest::conscious(uuid::Uuid::now_v7(), uuid::Uuid::now_v7(), context);
        let WorkerPayload::Conscious(payload) = &request.payload else {
            panic!("expected conscious payload");
        };

        let model_request = build_model_call_request(&request, payload.as_ref());
        let identity_message = model_request
            .input
            .messages
            .iter()
            .find(|message| message.content.contains(IDENTITY_KICKSTART_BLOCK_TAG))
            .expect("identity formation capability should be present");

        assert!(
            model_request
                .input
                .system_prompt
                .contains("Identity formation is available")
        );
        assert!(
            identity_message
                .content
                .contains("select_predefined_identity")
        );
        assert!(
            identity_message
                .content
                .contains("start_custom_identity_interview")
        );
        assert!(identity_message.content.contains("continuity_operator"));
        for hidden_term in ["table", "storage", "validation internals", "lifecycle"] {
            assert!(
                !identity_message.content.contains(hidden_term),
                "identity formation message leaked hidden term: {hidden_term}"
            );
        }
    }

    #[test]
    fn conscious_model_request_hides_identity_kickstart_after_completion() {
        let mut context = sample_context();
        context.self_model.identity_lifecycle = IdentityLifecycleContext {
            state: IdentityLifecycleState::CompleteIdentityActive,
            kickstart_available: false,
            kickstart: None,
        };
        context.self_model.identity = Some(CompactIdentitySnapshot {
            identity_summary: "Lagoon Complete".to_string(),
            stable_items: vec![CompactIdentityItem {
                category: IdentityItemCategory::Name,
                value: "Lagoon Complete".to_string(),
                confidence_pct: 100,
                weight_pct: None,
            }],
            evolving_items: Vec::new(),
            values: vec!["clarity".to_string()],
            boundaries: vec!["ask before risky actions".to_string()],
            self_description: Some("A clear, bounded assistant.".to_string()),
        });
        let request = WorkerRequest::conscious(uuid::Uuid::now_v7(), uuid::Uuid::now_v7(), context);
        let WorkerPayload::Conscious(payload) = &request.payload else {
            panic!("expected conscious payload");
        };

        let model_request = build_model_call_request(&request, payload.as_ref());

        assert!(
            !model_request
                .input
                .messages
                .iter()
                .any(|message| message.content.contains(IDENTITY_KICKSTART_BLOCK_TAG))
        );
        assert!(
            model_request
                .input
                .system_prompt
                .contains("Lagoon Complete")
        );
        assert!(model_request.input.system_prompt.contains("clarity"));
        assert!(
            model_request
                .input
                .system_prompt
                .contains("ask before risky actions")
        );
    }

    #[test]
    fn conscious_model_request_includes_troubleshooting_guidance_only_for_error_intent() {
        let mut context = sample_context();
        context.trigger.ingress.text_body = Some("tell me more about the error trace".to_string());
        let request = WorkerRequest::conscious(uuid::Uuid::now_v7(), uuid::Uuid::now_v7(), context);
        let WorkerPayload::Conscious(payload) = &request.payload else {
            panic!("expected conscious payload");
        };

        let model_request = build_model_call_request(&request, payload.as_ref());
        let troubleshooting_message = model_request
            .input
            .messages
            .iter()
            .find(|message| message.content.contains("TROUBLESHOOTING CAPABILITY"))
            .expect("troubleshooting guidance should be disclosed for error intent");

        assert!(troubleshooting_message.content.contains("run_diagnostic"));
        assert!(troubleshooting_message.content.contains("`trace_show`"));
        assert!(
            troubleshooting_message
                .content
                .contains("You are the conscious assistant identity, not the harness")
        );

        let normal_request =
            WorkerRequest::conscious(uuid::Uuid::now_v7(), uuid::Uuid::now_v7(), sample_context());
        let WorkerPayload::Conscious(normal_payload) = &normal_request.payload else {
            panic!("expected conscious payload");
        };
        let normal_model_request =
            build_model_call_request(&normal_request, normal_payload.as_ref());
        assert!(
            !normal_model_request
                .input
                .messages
                .iter()
                .any(|message| message.content.contains("TROUBLESHOOTING CAPABILITY"))
        );
    }

    #[test]
    fn conscious_model_request_observation_follow_up_forbids_new_action_promises() {
        let mut context = sample_context();
        context.governed_action_observations = vec![contracts::GovernedActionObservation {
            observation_id: uuid::Uuid::now_v7(),
            action_kind: contracts::GovernedActionKind::WebFetch,
            outcome: contracts::GovernedActionExecutionOutcome {
                status: contracts::GovernedActionStatus::Executed,
                summary: "web fetch completed for https://example.com/; preview truncated"
                    .to_string(),
                fingerprint: None,
                output_ref: Some("execution_record:test".to_string()),
            },
        }];
        let request = WorkerRequest::conscious(uuid::Uuid::now_v7(), uuid::Uuid::now_v7(), context);
        let WorkerPayload::Conscious(payload) = &request.payload else {
            panic!("expected conscious payload");
        };

        let model_request = build_model_call_request(&request, payload.as_ref());
        let developer_message = model_request
            .input
            .messages
            .iter()
            .find(|message| message.role == ModelMessageRole::Developer)
            .expect("developer message should exist");

        assert!(
            developer_message
                .content
                .contains("propose it in the same turn")
        );
        assert!(
            developer_message
                .content
                .contains("let the harness decide whether it is allowed")
        );
        assert!(
            developer_message
                .content
                .contains("Foreground action loop state:")
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
                assert!(result.governed_action_proposals.is_empty());
                assert!(result.governed_action_observations.is_empty());
            }
            WorkerResult::Smoke(_) => panic!("conscious worker should not emit a smoke result"),
            WorkerResult::Unconscious(_) => {
                panic!("conscious worker should not emit an unconscious result")
            }
            WorkerResult::Error(error) => panic!("unexpected worker error: {}", error.message),
        }
    }

    #[test]
    fn conscious_worker_response_extracts_governed_action_block() {
        let request =
            WorkerRequest::conscious(uuid::Uuid::now_v7(), uuid::Uuid::now_v7(), sample_context());
        let WorkerPayload::Conscious(payload) = &request.payload else {
            panic!("expected conscious payload");
        };
        let model_request = build_model_call_request(&request, payload.as_ref());
        let workspace_root = std::env::current_dir()
            .expect("current dir should resolve")
            .display()
            .to_string();
        let action_block = serde_json::json!({
            "actions": [{
                "proposal_id": uuid::Uuid::now_v7(),
                "title": "Echo test",
                "rationale": "Need a bounded workspace check",
                "action_kind": "run_subprocess",
                "requested_risk_tier": serde_json::Value::Null,
                "capability_scope": {
                    "filesystem": {
                        "read_roots": [workspace_root.clone()],
                        "write_roots": [],
                    },
                    "network": "disabled",
                    "environment": {
                        "allow_variables": [],
                    },
                    "execution": {
                        "timeout_ms": 30_000,
                        "max_stdout_bytes": 4_096,
                        "max_stderr_bytes": 4_096,
                    },
                },
                "payload": {
                    "kind": "run_subprocess",
                    "value": {
                        "command": if cfg!(windows) { "powershell" } else { "sh" },
                        "args": if cfg!(windows) {
                            serde_json::json!(["-NoProfile", "-Command", "Write-Output 'ok'"])
                        } else {
                            serde_json::json!(["-c", "printf 'ok\\n'"])
                        },
                        "working_directory": workspace_root.clone(),
                    },
                },
            }],
        });
        let model_response = ModelCallResponse {
            request_id: model_request.request_id,
            trace_id: request.trace_id,
            execution_id: request.execution_id,
            provider: contracts::ModelProviderKind::ZAi,
            model: "z-ai-foreground".to_string(),
            received_at: chrono::Utc::now(),
            output: ModelOutput {
                text: format!(
                    "I will run a bounded check.\n```{GOVERNED_ACTIONS_BLOCK_TAG}\n{}\n```",
                    action_block
                ),
                json: None,
                finish_reason: "stop".to_string(),
            },
            usage: ModelUsage {
                input_tokens: 20,
                output_tokens: 12,
            },
        };

        let response = build_conscious_worker_response(&request, payload.as_ref(), model_response)
            .expect("worker response should be valid");
        match response.result {
            WorkerResult::Conscious(result) => {
                assert_eq!(result.assistant_output.text, "I will run a bounded check.");
                assert_eq!(result.governed_action_proposals.len(), 1);
                assert_eq!(
                    result.governed_action_proposals[0].action_kind,
                    contracts::GovernedActionKind::RunSubprocess
                );
            }
            other => panic!("expected conscious worker result, got {other:?}"),
        }
    }

    #[test]
    fn conscious_worker_response_extracts_identity_kickstart_selection() {
        let mut context = sample_context();
        context.self_model.identity_lifecycle = IdentityLifecycleContext {
            state: IdentityLifecycleState::BootstrapSeedOnly,
            kickstart_available: true,
            kickstart: Some(IdentityKickstartContext {
                available_actions: vec![IdentityKickstartActionKind::SelectPredefinedTemplate],
                next_step: Some("choose_predefined_identity_or_start_custom_interview".to_string()),
                resume_summary: None,
                predefined_templates: contracts::predefined_identity_templates(),
            }),
        };
        let request = WorkerRequest::conscious(uuid::Uuid::now_v7(), uuid::Uuid::now_v7(), context);
        let WorkerPayload::Conscious(payload) = &request.payload else {
            panic!("expected conscious payload");
        };
        let model_request = build_model_call_request(&request, payload.as_ref());
        let identity_block = serde_json::json!({
            "action": "select_predefined_identity",
            "template_key": "continuity_operator",
            "answer": serde_json::Value::Null,
            "cancel_reason": serde_json::Value::Null,
        });
        let model_response = ModelCallResponse {
            request_id: model_request.request_id,
            trace_id: request.trace_id,
            execution_id: request.execution_id,
            provider: contracts::ModelProviderKind::ZAi,
            model: "z-ai-foreground".to_string(),
            received_at: chrono::Utc::now(),
            output: ModelOutput {
                text: format!(
                    "Continuity Operator selected.\n```{IDENTITY_KICKSTART_BLOCK_TAG}\n{}\n```",
                    identity_block
                ),
                json: None,
                finish_reason: "stop".to_string(),
            },
            usage: ModelUsage {
                input_tokens: 20,
                output_tokens: 12,
            },
        };

        let response = build_conscious_worker_response(&request, payload.as_ref(), model_response)
            .expect("worker response should be valid");
        match response.result {
            WorkerResult::Conscious(result) => {
                assert_eq!(
                    result.assistant_output.text,
                    "Continuity Operator selected."
                );
                let identity_proposals = result
                    .candidate_proposals
                    .iter()
                    .filter(|proposal| {
                        proposal.proposal_kind == CanonicalProposalKind::IdentityDelta
                    })
                    .collect::<Vec<_>>();
                assert_eq!(identity_proposals.len(), 1);
                let CanonicalProposalPayload::IdentityDelta(delta) = &identity_proposals[0].payload
                else {
                    panic!("expected identity delta");
                };
                assert_eq!(
                    delta.lifecycle_state,
                    IdentityLifecycleState::CompleteIdentityActive
                );
                assert!(delta.item_deltas.len() >= 20);
                assert!(delta.self_description_delta.is_some());
            }
            other => panic!("expected conscious worker result, got {other:?}"),
        }
    }

    #[test]
    fn conscious_worker_response_accepts_identity_answer_string_block() {
        let mut context = sample_context();
        context.trigger.ingress.text_body = Some("Richard".to_string());
        context.self_model.identity_lifecycle = IdentityLifecycleContext {
            state: IdentityLifecycleState::IdentityKickstartInProgress,
            kickstart_available: true,
            kickstart: Some(IdentityKickstartContext {
                available_actions: vec![IdentityKickstartActionKind::AnswerCustomInterview],
                next_step: Some("name".to_string()),
                resume_summary: Some("custom identity interview is in progress".to_string()),
                predefined_templates: Vec::new(),
            }),
        };
        let request = WorkerRequest::conscious(uuid::Uuid::now_v7(), uuid::Uuid::now_v7(), context);
        let WorkerPayload::Conscious(payload) = &request.payload else {
            panic!("expected conscious payload");
        };
        let model_request = build_model_call_request(&request, payload.as_ref());
        let identity_block = serde_json::json!({
            "action": "answer_custom_identity_question",
            "template_key": serde_json::Value::Null,
            "answer": "Richard",
            "cancel_reason": serde_json::Value::Null,
        });
        let model_response = conscious_model_response(
            &request,
            &model_request,
            format!(
                "Got it.\n```{IDENTITY_KICKSTART_BLOCK_TAG}\n{}\n```",
                identity_block
            ),
        );

        let response = build_conscious_worker_response(&request, payload.as_ref(), model_response)
            .expect("worker response should be valid");
        let WorkerResult::Conscious(result) = response.result else {
            panic!("expected conscious worker result");
        };
        let identity_proposal = result
            .candidate_proposals
            .iter()
            .find_map(|proposal| match &proposal.payload {
                CanonicalProposalPayload::IdentityDelta(delta) => Some(delta),
                _ => None,
            })
            .expect("identity answer proposal should be present");
        assert_eq!(
            identity_proposal.interview_action,
            Some(IdentityKickstartAction::AnswerCustomInterview(
                IdentityInterviewAnswer {
                    step_key: "name".to_string(),
                    answer_text: "Richard".to_string(),
                },
            ))
        );
        assert_eq!(result.assistant_output.text, "Got it.");
    }

    #[test]
    fn conscious_worker_response_ignores_malformed_identity_block() {
        let mut context = sample_context();
        context.trigger.ingress.text_body = Some("Richard".to_string());
        context.self_model.identity_lifecycle = IdentityLifecycleContext {
            state: IdentityLifecycleState::IdentityKickstartInProgress,
            kickstart_available: true,
            kickstart: Some(IdentityKickstartContext {
                available_actions: vec![IdentityKickstartActionKind::AnswerCustomInterview],
                next_step: Some("name".to_string()),
                resume_summary: Some("custom identity interview is in progress".to_string()),
                predefined_templates: Vec::new(),
            }),
        };
        let request = WorkerRequest::conscious(uuid::Uuid::now_v7(), uuid::Uuid::now_v7(), context);
        let WorkerPayload::Conscious(payload) = &request.payload else {
            panic!("expected conscious payload");
        };
        let model_request = build_model_call_request(&request, payload.as_ref());
        let model_response = conscious_model_response(
            &request,
            &model_request,
            format!("Got it.\n```{IDENTITY_KICKSTART_BLOCK_TAG}\nnot json\n```"),
        );

        let response = build_conscious_worker_response(&request, payload.as_ref(), model_response)
            .expect("malformed optional identity block should not fail worker response");
        let WorkerResult::Conscious(result) = response.result else {
            panic!("expected conscious worker result");
        };
        assert!(result.candidate_proposals.is_empty());
        assert_eq!(result.assistant_output.text, "Got it.");
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
    fn self_model_reflection_model_request_requires_structured_identity_output() {
        let mut context = sample_unconscious_context();
        context.job_kind = UnconsciousJobKind::SelfModelReflection;
        let request =
            WorkerRequest::unconscious(uuid::Uuid::now_v7(), uuid::Uuid::now_v7(), context);
        let WorkerPayload::Unconscious(payload) = &request.payload else {
            panic!("expected unconscious payload");
        };

        let model_request = build_unconscious_model_call_request(&request, payload.as_ref());

        assert_eq!(model_request.output_mode, ModelOutputMode::JsonObject);
        assert_eq!(
            model_request.schema_name.as_deref(),
            Some("identity_reflection_output")
        );
        assert!(model_request.schema_json.is_some());
        assert!(
            model_request
                .input
                .messages
                .iter()
                .any(|message| message.content.contains("identity_delta")
                    && message.content.contains("no_change_rationale"))
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
            None,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "drift_signal_detected");
        assert_eq!(diagnostics[0].severity, DiagnosticSeverity::Warning);

        let diagnostics = build_unconscious_diagnostics(
            &context,
            "Continuity remains aligned and stable across the scoped review.",
            None,
        );
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "drift_scan_clear");
        assert_eq!(diagnostics[0].severity, DiagnosticSeverity::Info);
    }

    #[test]
    fn self_model_reflection_emits_identity_delta_from_structured_output() {
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
                text: String::new(),
                json: Some(
                    serde_json::to_value(IdentityReflectionOutput {
                        identity_delta: Some(identity_reflection_delta()),
                        no_change_rationale: None,
                        diagnostics: vec![DiagnosticAlert {
                            alert_id: uuid::Uuid::now_v7(),
                            code: "identity_reflection_delta_ready".to_string(),
                            severity: DiagnosticSeverity::Info,
                            summary: "Reflection found an evidence-backed identity update."
                                .to_string(),
                            details: None,
                        }],
                        wake_signals: vec![WakeSignal {
                            signal_id: uuid::Uuid::now_v7(),
                            reason: WakeSignalReason::MaintenanceInsightReady,
                            priority: WakeSignalPriority::Low,
                            reason_code: "identity_reflection_ready".to_string(),
                            summary: "Identity reflection produced a bounded update.".to_string(),
                            payload_ref: Some("background_job:identity-reflection".to_string()),
                        }],
                    })
                    .expect("identity reflection output should serialize"),
                ),
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
                assert_eq!(proposal.proposal_kind, CanonicalProposalKind::IdentityDelta);
                assert_eq!(
                    proposal.canonical_target,
                    CanonicalTargetKind::IdentityItems
                );
                assert_eq!(
                    proposal.provenance.provenance_kind,
                    ProposalProvenanceKind::SelfModelReflection
                );
                let CanonicalProposalPayload::IdentityDelta(delta) = &proposal.payload else {
                    panic!("expected an identity delta payload");
                };
                assert_eq!(delta.item_deltas.len(), 1);
                assert_eq!(
                    delta.item_deltas[0].category,
                    IdentityItemCategory::InteractionStyleAdaptation
                );
            }
            WorkerResult::Smoke(_) => panic!("unexpected smoke response"),
            WorkerResult::Conscious(_) => panic!("unexpected conscious response"),
            WorkerResult::Error(error) => panic!("unexpected worker error: {}", error.message),
        }
    }

    #[test]
    fn self_model_reflection_invalid_output_records_diagnostic_without_delta() {
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
                text: "free text should not become a self-model write".to_string(),
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
                assert!(result.maintenance_outputs.canonical_proposals.is_empty());
                assert!(result.maintenance_outputs.retrieval_updates.is_empty());
                assert!(result.maintenance_outputs.wake_signals.is_empty());
                assert_eq!(result.maintenance_outputs.diagnostics.len(), 1);
                assert_eq!(
                    result.maintenance_outputs.diagnostics[0].code,
                    "identity_reflection_invalid_output"
                );
            }
            WorkerResult::Smoke(_) => panic!("unexpected smoke response"),
            WorkerResult::Conscious(_) => panic!("unexpected conscious response"),
            WorkerResult::Error(error) => panic!("unexpected worker error: {}", error.message),
        }
    }

    fn identity_reflection_delta() -> IdentityDeltaProposal {
        IdentityDeltaProposal {
            lifecycle_state: IdentityLifecycleState::CompleteIdentityActive,
            item_deltas: vec![IdentityItemDelta {
                operation: IdentityDeltaOperation::Add,
                stability_class: IdentityStabilityClass::Evolving,
                category: IdentityItemCategory::InteractionStyleAdaptation,
                item_key: "progress_updates".to_string(),
                value: "Use concise progress updates during long maintenance runs.".to_string(),
                confidence_pct: 82,
                weight_pct: Some(70),
                source: IdentityItemSource::ModelInferred,
                merge_policy: IdentityMergePolicy::Revisable,
                evidence_refs: vec![IdentityEvidenceRef {
                    source_kind: "episode".to_string(),
                    source_id: None,
                    summary: "Recent scoped episodes favored concise maintenance updates."
                        .to_string(),
                }],
                valid_from: None,
                valid_to: None,
                target_identity_item_id: None,
            }],
            self_description_delta: Some(SelfDescriptionDelta {
                operation: IdentityDeltaOperation::Revise,
                description: "Blue Lagoon gives concise progress updates during long work."
                    .to_string(),
                evidence_refs: vec![IdentityEvidenceRef {
                    source_kind: "episode".to_string(),
                    source_id: None,
                    summary: "Reflection over recent scoped episodes.".to_string(),
                }],
            }),
            interview_action: None,
            rationale: "Background reflection found an evidence-backed interaction style update."
                .to_string(),
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
                identity: None,
                identity_lifecycle: Default::default(),
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
            governed_action_observations: Vec::new(),
            governed_action_loop_state: Some(contracts::ForegroundGovernedActionLoopState {
                executed_action_count: 0,
                max_actions_per_turn: 10,
                remaining_actions_before_cap: 10,
                cap_exceeded_behavior: contracts::GovernedActionCapExceededBehavior::Escalate,
            }),
            recovery_context: contracts::ForegroundRecoveryContext::default(),
        }
    }

    fn conscious_model_response(
        request: &WorkerRequest,
        model_request: &contracts::ModelCallRequest,
        text: String,
    ) -> ModelCallResponse {
        ModelCallResponse {
            request_id: model_request.request_id,
            trace_id: request.trace_id,
            execution_id: request.execution_id,
            provider: contracts::ModelProviderKind::ZAi,
            model: "z-ai-foreground".to_string(),
            received_at: chrono::Utc::now(),
            output: ModelOutput {
                text,
                json: None,
                finish_reason: "stop".to_string(),
            },
            usage: ModelUsage {
                input_tokens: 20,
                output_tokens: 12,
            },
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
            evidence: None,
            budget: BackgroundExecutionBudget {
                iteration_budget: 2,
                wall_clock_budget_ms: 120_000,
                token_budget: 6_000,
            },
        }
    }
}
