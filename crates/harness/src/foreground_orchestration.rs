use anyhow::{Context, Error, Result, bail};
use serde_json::json;
use uuid::Uuid;

use crate::{
    audit::{self, NewAuditEvent},
    config::{ResolvedModelGatewayConfig, ResolvedTelegramConfig, RuntimeConfig},
    context, execution,
    foreground::{self, ForegroundTriggerIntakeOutcome, NewEpisode, NewEpisodeMessage},
    memory,
    model_gateway::ModelProviderTransport,
    policy, proposal, self_model,
    telegram::{TelegramDelivery, TelegramOutboundMessage},
    worker,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TelegramForegroundOrchestrationOutcome {
    Completed(TelegramForegroundCompletion),
    Duplicate(foreground::DuplicateForegroundTrigger),
    Rejected(foreground::RejectedForegroundTrigger),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelegramForegroundCompletion {
    pub trace_id: Uuid,
    pub execution_id: Uuid,
    pub episode_id: Uuid,
    pub ingress_id: Uuid,
    pub outbound_message_id: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForegroundExecutionIds {
    pub trace_id: Uuid,
    pub execution_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UserEpisodeMessage {
    text_body: String,
    external_message_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ForegroundExecutionInput {
    trigger: contracts::ForegroundTrigger,
    recovery_context: contracts::ForegroundRecoveryContext,
    user_messages: Vec<UserEpisodeMessage>,
}

pub async fn orchestrate_telegram_foreground_ingress<T, D>(
    pool: &sqlx::PgPool,
    config: &RuntimeConfig,
    telegram_config: &ResolvedTelegramConfig,
    model_gateway_config: &ResolvedModelGatewayConfig,
    ingress: contracts::NormalizedIngress,
    transport: &T,
    delivery: &mut D,
) -> Result<TelegramForegroundOrchestrationOutcome>
where
    T: ModelProviderTransport,
    D: TelegramDelivery,
{
    let intake =
        foreground::intake_telegram_foreground_trigger(pool, config, telegram_config, ingress)
            .await?;

    let trigger = match intake {
        ForegroundTriggerIntakeOutcome::Accepted(trigger) => *trigger,
        ForegroundTriggerIntakeOutcome::Duplicate(duplicate) => {
            return Ok(TelegramForegroundOrchestrationOutcome::Duplicate(duplicate));
        }
        ForegroundTriggerIntakeOutcome::Rejected(rejected) => {
            return Ok(TelegramForegroundOrchestrationOutcome::Rejected(rejected));
        }
    };

    let user_messages = trigger
        .ingress
        .text_body
        .clone()
        .into_iter()
        .map(|text_body| UserEpisodeMessage {
            text_body,
            external_message_id: trigger.ingress.external_message_id.clone(),
        })
        .collect::<Vec<_>>();

    orchestrate_telegram_foreground_trigger(
        pool,
        config,
        model_gateway_config,
        ForegroundExecutionInput {
            trigger,
            recovery_context: contracts::ForegroundRecoveryContext::default(),
            user_messages,
        },
        transport,
        delivery,
    )
    .await
}

pub async fn orchestrate_telegram_foreground_plan<T, D>(
    pool: &sqlx::PgPool,
    config: &RuntimeConfig,
    model_gateway_config: &ResolvedModelGatewayConfig,
    execution: ForegroundExecutionIds,
    plan: foreground::PendingForegroundExecutionPlan,
    transport: &T,
    delivery: &mut D,
) -> Result<TelegramForegroundOrchestrationOutcome>
where
    T: ModelProviderTransport,
    D: TelegramDelivery,
{
    let primary_ingress =
        foreground::load_normalized_ingress(pool, plan.primary_ingress.ingress_id).await?;
    let trigger = foreground::build_foreground_trigger(
        config,
        execution.trace_id,
        execution.execution_id,
        primary_ingress,
    )?;
    let user_messages = plan
        .ordered_ingress
        .iter()
        .filter_map(|ingress| {
            ingress
                .text_body
                .clone()
                .map(|text_body| UserEpisodeMessage {
                    text_body,
                    external_message_id: ingress.external_message_id.clone(),
                })
        })
        .collect::<Vec<_>>();

    orchestrate_telegram_foreground_trigger(
        pool,
        config,
        model_gateway_config,
        ForegroundExecutionInput {
            trigger,
            recovery_context: contracts::ForegroundRecoveryContext {
                mode: plan.mode,
                ordered_ingress: plan.ordered_ingress,
            },
            user_messages,
        },
        transport,
        delivery,
    )
    .await
}

async fn orchestrate_telegram_foreground_trigger<T, D>(
    pool: &sqlx::PgPool,
    config: &RuntimeConfig,
    model_gateway_config: &ResolvedModelGatewayConfig,
    execution_input: ForegroundExecutionInput,
    transport: &T,
    delivery: &mut D,
) -> Result<TelegramForegroundOrchestrationOutcome>
where
    T: ModelProviderTransport,
    D: TelegramDelivery,
{
    let ForegroundExecutionInput {
        trigger,
        recovery_context,
        user_messages,
    } = execution_input;

    let episode_id = Uuid::now_v7();
    let mut recorded_episode_id = None;
    if let Err(error) = foreground::insert_episode(
        pool,
        &NewEpisode {
            episode_id,
            trace_id: trigger.trace_id,
            execution_id: trigger.execution_id,
            ingress_id: Some(trigger.ingress.ingress_id),
            internal_principal_ref: trigger.ingress.internal_principal_ref.clone(),
            internal_conversation_ref: trigger.ingress.internal_conversation_ref.clone(),
            trigger_kind: "user_ingress".to_string(),
            trigger_source: "telegram".to_string(),
            status: "started".to_string(),
            started_at: trigger.received_at,
        },
    )
    .await
    {
        return record_and_return_failure(
            pool,
            trigger.trace_id,
            trigger.execution_id,
            recorded_episode_id,
            ForegroundFailureKind::PersistenceFailure,
            error,
        )
        .await;
    }
    recorded_episode_id = Some(episode_id);

    for (index, user_message) in user_messages.iter().enumerate() {
        if let Err(error) = foreground::insert_episode_message(
            pool,
            &NewEpisodeMessage {
                episode_message_id: Uuid::now_v7(),
                episode_id,
                trace_id: trigger.trace_id,
                execution_id: trigger.execution_id,
                message_order: index as i32,
                message_role: "user".to_string(),
                channel_kind: trigger.ingress.channel_kind,
                text_body: Some(user_message.text_body.clone()),
                external_message_id: user_message.external_message_id.clone(),
            },
        )
        .await
        {
            return record_and_return_failure(
                pool,
                trigger.trace_id,
                trigger.execution_id,
                recorded_episode_id,
                ForegroundFailureKind::PersistenceFailure,
                error,
            )
            .await;
        }
    }

    let assembly = match context::assemble_foreground_context(
        pool,
        config,
        trigger.clone(),
        context::ContextAssemblyOptions {
            episode_id: Some(episode_id),
            recovery_context: recovery_context.clone(),
            ..context::ContextAssemblyOptions::default()
        },
    )
    .await
    {
        Ok(assembly) => assembly,
        Err(error) => {
            return record_and_return_failure(
                pool,
                trigger.trace_id,
                trigger.execution_id,
                recorded_episode_id,
                ForegroundFailureKind::ContextAssemblyFailure,
                error,
            )
            .await;
        }
    };
    let metadata_payload = match serde_json::to_value(&assembly.metadata)
        .context("failed to serialize context assembly metadata")
    {
        Ok(payload) => payload,
        Err(error) => {
            return record_and_return_failure(
                pool,
                trigger.trace_id,
                trigger.execution_id,
                recorded_episode_id,
                ForegroundFailureKind::PersistenceFailure,
                error,
            )
            .await;
        }
    };
    if let Err(error) = audit::insert(
        pool,
        &NewAuditEvent {
            loop_kind: "conscious".to_string(),
            subsystem: "foreground_orchestration".to_string(),
            event_kind: "foreground_context_assembled".to_string(),
            severity: "info".to_string(),
            trace_id: trigger.trace_id,
            execution_id: Some(trigger.execution_id),
            worker_pid: None,
            payload: metadata_payload,
        },
    )
    .await
    {
        return record_and_return_failure(
            pool,
            trigger.trace_id,
            trigger.execution_id,
            recorded_episode_id,
            ForegroundFailureKind::PersistenceFailure,
            error,
        )
        .await;
    }

    let request = contracts::WorkerRequest::conscious(
        trigger.trace_id,
        trigger.execution_id,
        assembly.context,
    );
    let response = match worker::launch_conscious_worker_with_timeout(
        config,
        model_gateway_config,
        &request,
        transport,
        policy::effective_foreground_worker_timeout_ms(config),
    )
    .await
    {
        Ok(response) => response,
        Err(error) => {
            record_foreground_failure(
                pool,
                trigger.trace_id,
                trigger.execution_id,
                recorded_episode_id,
                classify_conscious_worker_failure(&error),
                &format_error_chain(&error),
            )
            .await?;
            return Err(error);
        }
    };

    let contracts::WorkerResult::Conscious(result) = &response.result else {
        let message = "conscious worker returned a non-conscious result".to_string();
        record_foreground_failure(
            pool,
            trigger.trace_id,
            trigger.execution_id,
            recorded_episode_id,
            ForegroundFailureKind::WorkerProtocolFailure,
            &message,
        )
        .await?;
        bail!(message);
    };

    let assistant_episode_message_id = Uuid::now_v7();
    if let Err(error) = foreground::insert_episode_message(
        pool,
        &NewEpisodeMessage {
            episode_message_id: assistant_episode_message_id,
            episode_id,
            trace_id: trigger.trace_id,
            execution_id: trigger.execution_id,
            message_order: user_messages.len() as i32,
            message_role: "assistant".to_string(),
            channel_kind: result.assistant_output.channel_kind,
            text_body: Some(result.assistant_output.text.clone()),
            external_message_id: None,
        },
    )
    .await
    {
        return record_and_return_failure(
            pool,
            trigger.trace_id,
            trigger.execution_id,
            recorded_episode_id,
            ForegroundFailureKind::PersistenceFailure,
            error,
        )
        .await;
    }

    let chat_id = match parse_telegram_chat_id(&trigger.ingress) {
        Ok(chat_id) => chat_id,
        Err(error) => {
            return record_and_return_failure(
                pool,
                trigger.trace_id,
                trigger.execution_id,
                recorded_episode_id,
                ForegroundFailureKind::TelegramDeliveryFailure,
                error,
            )
            .await;
        }
    };
    let reply_to_message_id = match parse_telegram_reply_target(&trigger.ingress) {
        Ok(reply_to_message_id) => reply_to_message_id,
        Err(error) => {
            return record_and_return_failure(
                pool,
                trigger.trace_id,
                trigger.execution_id,
                recorded_episode_id,
                ForegroundFailureKind::TelegramDeliveryFailure,
                error,
            )
            .await;
        }
    };

    let delivery_receipt = match delivery
        .send_message(&TelegramOutboundMessage {
            chat_id,
            text: result.assistant_output.text.clone(),
            reply_to_message_id,
        })
        .await
    {
        Ok(receipt) => receipt,
        Err(error) => {
            record_foreground_failure(
                pool,
                trigger.trace_id,
                trigger.execution_id,
                recorded_episode_id,
                ForegroundFailureKind::TelegramDeliveryFailure,
                &format_error_chain(&error),
            )
            .await?;
            return Err(error);
        }
    };
    if let Err(error) = foreground::update_episode_message_external_message_id(
        pool,
        assistant_episode_message_id,
        &delivery_receipt.message_id.to_string(),
    )
    .await
    {
        return record_and_return_failure(
            pool,
            trigger.trace_id,
            trigger.execution_id,
            recorded_episode_id,
            ForegroundFailureKind::PersistenceFailure,
            error,
        )
        .await;
    }

    let proposal_summary = match apply_foreground_candidate_proposals(
        pool,
        config,
        &proposal::ProposalProcessingContext {
            trace_id: trigger.trace_id,
            execution_id: trigger.execution_id,
            episode_id: Some(episode_id),
            source_ingress_id: Some(trigger.ingress.ingress_id),
            source_loop_kind: "conscious".to_string(),
        },
        &result.candidate_proposals,
    )
    .await
    {
        Ok(summary) => summary,
        Err(error) => {
            return record_and_return_failure(
                pool,
                trigger.trace_id,
                trigger.execution_id,
                recorded_episode_id,
                ForegroundFailureKind::PersistenceFailure,
                error,
            )
            .await;
        }
    };

    let response_payload = serde_json::to_value(&response)
        .context("failed to serialize conscious worker response payload");
    let response_payload = match response_payload {
        Ok(response_payload) => response_payload,
        Err(error) => {
            return record_and_return_failure(
                pool,
                trigger.trace_id,
                trigger.execution_id,
                recorded_episode_id,
                ForegroundFailureKind::PersistenceFailure,
                error,
            )
            .await;
        }
    };
    if let Err(error) = execution::mark_succeeded(
        pool,
        trigger.execution_id,
        "conscious",
        response.worker_pid as i32,
        &response_payload,
    )
    .await
    {
        return record_and_return_failure(
            pool,
            trigger.trace_id,
            trigger.execution_id,
            recorded_episode_id,
            ForegroundFailureKind::PersistenceFailure,
            error,
        )
        .await;
    }
    if let Err(error) = foreground::mark_episode_completed(
        pool,
        episode_id,
        &result.episode_summary.outcome,
        &format!(
            "{} | proposals evaluated={}, accepted={}, rejected={}, canonical_writes={}",
            result.episode_summary.summary,
            proposal_summary.evaluated_count,
            proposal_summary.accepted_count,
            proposal_summary.rejected_count,
            proposal_summary.canonical_write_count
        ),
    )
    .await
    {
        return record_and_return_failure(
            pool,
            trigger.trace_id,
            trigger.execution_id,
            recorded_episode_id,
            ForegroundFailureKind::PersistenceFailure,
            error,
        )
        .await;
    }
    let processed_ingress_ids = if recovery_context.ordered_ingress.is_empty() {
        vec![trigger.ingress.ingress_id]
    } else {
        recovery_context
            .ordered_ingress
            .iter()
            .map(|ingress| ingress.ingress_id)
            .collect::<Vec<_>>()
    };
    if let Err(error) = foreground::mark_ingress_events_processed(
        pool,
        &processed_ingress_ids,
        trigger.execution_id,
    )
    .await
    {
        return record_and_return_failure(
            pool,
            trigger.trace_id,
            trigger.execution_id,
            recorded_episode_id,
            ForegroundFailureKind::PersistenceFailure,
            error,
        )
        .await;
    }
    if let Err(error) = audit::insert(
        pool,
        &NewAuditEvent {
            loop_kind: "conscious".to_string(),
            subsystem: "foreground_orchestration".to_string(),
            event_kind: "foreground_execution_completed".to_string(),
            severity: "info".to_string(),
            trace_id: trigger.trace_id,
            execution_id: Some(trigger.execution_id),
            worker_pid: Some(response.worker_pid as i32),
            payload: json!({
                "episode_id": episode_id,
                "ingress_id": trigger.ingress.ingress_id,
                "processed_ingress_ids": processed_ingress_ids,
                "outbound_message_id": delivery_receipt.message_id,
                "assistant_summary": result.episode_summary.summary,
                "candidate_proposal_count": result.candidate_proposals.len(),
                "proposal_summary": {
                    "evaluated": proposal_summary.evaluated_count,
                    "accepted": proposal_summary.accepted_count,
                    "rejected": proposal_summary.rejected_count,
                    "canonical_writes": proposal_summary.canonical_write_count,
                },
            }),
        },
    )
    .await
    {
        return record_and_return_failure(
            pool,
            trigger.trace_id,
            trigger.execution_id,
            recorded_episode_id,
            ForegroundFailureKind::PersistenceFailure,
            error,
        )
        .await;
    }

    Ok(TelegramForegroundOrchestrationOutcome::Completed(
        TelegramForegroundCompletion {
            trace_id: trigger.trace_id,
            execution_id: trigger.execution_id,
            episode_id,
            ingress_id: trigger.ingress.ingress_id,
            outbound_message_id: delivery_receipt.message_id,
        },
    ))
}

async fn record_foreground_failure(
    pool: &sqlx::PgPool,
    trace_id: Uuid,
    execution_id: Uuid,
    episode_id: Option<Uuid>,
    failure_kind: ForegroundFailureKind,
    error_message: &str,
) -> Result<()> {
    execution::mark_failed(
        pool,
        execution_id,
        &json!({
            "kind": failure_kind.as_str(),
            "message": error_message,
        }),
    )
    .await?;
    if let Some(episode_id) = episode_id {
        foreground::mark_episode_failed(pool, episode_id, "failed", error_message).await?;
    }
    audit::insert(
        pool,
        &NewAuditEvent {
            loop_kind: "conscious".to_string(),
            subsystem: "foreground_orchestration".to_string(),
            event_kind: "foreground_execution_failed".to_string(),
            severity: "error".to_string(),
            trace_id,
            execution_id: Some(execution_id),
            worker_pid: None,
            payload: json!({
                "episode_id": episode_id,
                "failure_kind": failure_kind.as_str(),
                "error": error_message,
            }),
        },
    )
    .await?;
    Ok(())
}

async fn record_and_return_failure<T>(
    pool: &sqlx::PgPool,
    trace_id: Uuid,
    execution_id: Uuid,
    episode_id: Option<Uuid>,
    failure_kind: ForegroundFailureKind,
    error: Error,
) -> Result<T> {
    let error_message = format_error_chain(&error);
    if let Err(record_error) = record_foreground_failure(
        pool,
        trace_id,
        execution_id,
        episode_id,
        failure_kind,
        &error_message,
    )
    .await
    {
        return Err(error.context(format!(
            "failed to record foreground execution failure: {record_error}"
        )));
    }

    Err(error)
}

fn format_error_chain(error: &Error) -> String {
    error
        .chain()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(": ")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ForegroundFailureKind {
    ContextAssemblyFailure,
    WorkerProtocolFailure,
    ModelGatewayTransportFailure,
    ProviderRejected,
    TelegramDeliveryFailure,
    PersistenceFailure,
}

impl ForegroundFailureKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::ContextAssemblyFailure => "context_assembly_failure",
            Self::WorkerProtocolFailure => "worker_protocol_failure",
            Self::ModelGatewayTransportFailure => "model_gateway_transport_failure",
            Self::ProviderRejected => "provider_rejected",
            Self::TelegramDeliveryFailure => "telegram_delivery_failure",
            Self::PersistenceFailure => "persistence_failure",
        }
    }
}

fn classify_conscious_worker_failure(error: &Error) -> ForegroundFailureKind {
    let message = format_error_chain(error);
    if message.contains("provider returned status") {
        return ForegroundFailureKind::ProviderRejected;
    }
    if message.contains("model gateway transport failed")
        || message.contains("error sending request for url")
        || message.contains("failed to decode provider HTTP response body")
    {
        return ForegroundFailureKind::ModelGatewayTransportFailure;
    }

    ForegroundFailureKind::WorkerProtocolFailure
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct ProposalApplicationSummary {
    evaluated_count: usize,
    accepted_count: usize,
    rejected_count: usize,
    canonical_write_count: usize,
}

async fn apply_foreground_candidate_proposals(
    pool: &sqlx::PgPool,
    config: &RuntimeConfig,
    context: &proposal::ProposalProcessingContext,
    proposals: &[contracts::CanonicalProposal],
) -> Result<ProposalApplicationSummary> {
    let mut summary = ProposalApplicationSummary::default();

    for candidate in proposals {
        summary.evaluated_count += 1;
        let validation = proposal::validate_and_record_proposal(pool, context, candidate).await?;
        match validation.outcome {
            contracts::ProposalEvaluationOutcome::Rejected => {
                summary.rejected_count += 1;
            }
            contracts::ProposalEvaluationOutcome::Accepted => {
                let merge_outcome = match candidate.proposal_kind {
                    contracts::CanonicalProposalKind::MemoryArtifact => {
                        memory::apply_memory_proposal_merge(pool, context, candidate).await?
                    }
                    contracts::CanonicalProposalKind::SelfModelObservation => {
                        self_model::apply_self_model_proposal_merge(
                            pool, config, context, candidate,
                        )
                        .await?
                    }
                };

                match merge_outcome.outcome {
                    contracts::ProposalEvaluationOutcome::Accepted => {
                        summary.accepted_count += 1;
                        if merge_outcome.target.is_some() {
                            summary.canonical_write_count += 1;
                            audit::insert(
                                pool,
                                &NewAuditEvent {
                                    loop_kind: context.source_loop_kind.clone(),
                                    subsystem: "foreground_orchestration".to_string(),
                                    event_kind: "canonical_write_applied".to_string(),
                                    severity: "info".to_string(),
                                    trace_id: context.trace_id,
                                    execution_id: Some(context.execution_id),
                                    worker_pid: None,
                                    payload: json!({
                                        "proposal_id": candidate.proposal_id,
                                        "outcome": "accepted",
                                        "target": merge_outcome.target,
                                    }),
                                },
                            )
                            .await?;
                        }
                    }
                    contracts::ProposalEvaluationOutcome::Rejected => {
                        summary.rejected_count += 1;
                    }
                }
            }
        }
    }

    Ok(summary)
}

fn parse_telegram_chat_id(ingress: &contracts::NormalizedIngress) -> Result<i64> {
    ingress
        .external_conversation_id
        .parse::<i64>()
        .with_context(|| {
            format!(
                "failed to parse Telegram conversation id '{}'",
                ingress.external_conversation_id
            )
        })
}

fn parse_telegram_reply_target(ingress: &contracts::NormalizedIngress) -> Result<Option<i64>> {
    ingress
        .external_message_id
        .as_deref()
        .map(|message_id| {
            message_id
                .parse::<i64>()
                .with_context(|| format!("failed to parse Telegram message id '{message_id}'"))
        })
        .transpose()
}
