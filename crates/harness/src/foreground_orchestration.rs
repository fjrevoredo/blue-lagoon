use anyhow::{Context, Error, Result, bail};
use serde_json::json;
use uuid::Uuid;

use crate::{
    approval,
    audit::{self, NewAuditEvent},
    config::{ResolvedModelGatewayConfig, ResolvedTelegramConfig, RuntimeConfig},
    context, execution,
    foreground::{self, ForegroundTriggerIntakeOutcome, NewEpisode, NewEpisodeMessage},
    governed_actions,
    model_gateway::ModelProviderTransport,
    policy, proposal,
    telegram::{self, TelegramDelivery, TelegramOutboundMessage},
    worker,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TelegramForegroundOrchestrationOutcome {
    Completed(TelegramForegroundCompletion),
    ApprovalResolved(TelegramApprovalResolutionCompletion),
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelegramApprovalResolutionCompletion {
    pub trace_id: Uuid,
    pub execution_id: Uuid,
    pub ingress_id: Uuid,
    pub approval_request_id: Uuid,
    pub decision: contracts::ApprovalResolutionDecision,
    pub outbound_message_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelegramApprovalPromptDelivery {
    pub chat_id: i64,
    pub approval_request_id: Uuid,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedApprovalResolutionIngress {
    approval_token: String,
    decision: contracts::ApprovalResolutionDecision,
    expected_action_fingerprint: Option<contracts::GovernedActionFingerprint>,
    resolution_source: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct GovernedActionProcessingSummary {
    observations: Vec<contracts::GovernedActionObservation>,
    proposed_count: usize,
    executed_count: usize,
    blocked_count: usize,
    pending_approval_count: usize,
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

    if let Some(parsed_resolution) = parse_approval_resolution_ingress(&trigger.ingress)? {
        return orchestrate_telegram_approval_resolution_trigger(
            pool,
            config,
            trigger,
            parsed_resolution,
            delivery,
        )
        .await;
    }

    let user_messages = build_trigger_user_messages(
        trigger.trigger_kind,
        trigger
            .ingress
            .text_body
            .clone()
            .into_iter()
            .map(|text_body| UserEpisodeMessage {
                text_body,
                external_message_id: trigger.ingress.external_message_id.clone(),
            })
            .collect::<Vec<_>>(),
    );

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
    if let Some(parsed_resolution) = parse_approval_resolution_ingress(&trigger.ingress)? {
        return orchestrate_telegram_approval_resolution_trigger(
            pool,
            config,
            trigger,
            parsed_resolution,
            delivery,
        )
        .await;
    }
    let user_messages = build_trigger_user_messages(
        trigger.trigger_kind,
        plan.ordered_ingress
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
            .collect::<Vec<_>>(),
    );

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

async fn orchestrate_telegram_approval_resolution_trigger<D>(
    pool: &sqlx::PgPool,
    config: &RuntimeConfig,
    trigger: contracts::ForegroundTrigger,
    parsed_resolution: ParsedApprovalResolutionIngress,
    delivery: &mut D,
) -> Result<TelegramForegroundOrchestrationOutcome>
where
    D: TelegramDelivery,
{
    let approval_request = match approval::get_approval_request_by_token(
        pool,
        &parsed_resolution.approval_token,
    )
    .await
    {
        Ok(Some(request)) => request,
        Ok(None) => {
            return record_and_return_approval_resolution_failure(
                pool,
                &trigger,
                ForegroundFailureKind::ApprovalResolutionFailure,
                anyhow::anyhow!(
                    "approval callback referenced unknown approval token '{}'",
                    parsed_resolution.approval_token
                ),
            )
            .await;
        }
        Err(error) => {
            return record_and_return_approval_resolution_failure(
                pool,
                &trigger,
                ForegroundFailureKind::PersistenceFailure,
                error,
            )
            .await;
        }
    };

    let resolution = match approval::resolve_approval_request(
        pool,
        &approval::ApprovalResolutionAttempt {
            token: parsed_resolution.approval_token.clone(),
            actor_ref: format!("telegram:{}", trigger.ingress.internal_principal_ref),
            expected_action_fingerprint: parsed_resolution
                .expected_action_fingerprint
                .clone()
                .unwrap_or_else(|| approval_request.action_fingerprint.clone()),
            decision: parsed_resolution.decision,
            reason: Some(parsed_resolution.resolution_source.clone()),
            resolved_at: trigger.received_at,
        },
    )
    .await
    {
        Ok(resolution) => resolution,
        Err(error) => {
            return record_and_return_approval_resolution_failure(
                pool,
                &trigger,
                ForegroundFailureKind::ApprovalResolutionFailure,
                error,
            )
            .await;
        }
    };

    let action_execution =
        match governed_actions::get_governed_action_execution_by_approval_request_id(
            pool,
            resolution.request.approval_request_id,
        )
        .await
        {
            Ok(Some(record)) => {
                let synced = match governed_actions::sync_status_from_approval_resolution(
                    pool,
                    record.governed_action_execution_id,
                    resolution.event.decision,
                    Some(trigger.execution_id),
                    resolution.event.reason.as_deref(),
                )
                .await
                {
                    Ok(record) => record,
                    Err(error) => {
                        return record_and_return_approval_resolution_failure(
                            pool,
                            &trigger,
                            ForegroundFailureKind::PersistenceFailure,
                            error,
                        )
                        .await;
                    }
                };

                if resolution.event.decision == contracts::ApprovalResolutionDecision::Approved {
                    match governed_actions::execute_governed_action(config, pool, &synced).await {
                        Ok(executed) => Some(executed),
                        Err(error) => {
                            return record_and_return_approval_resolution_failure(
                                pool,
                                &trigger,
                                ForegroundFailureKind::WorkerProtocolFailure,
                                error,
                            )
                            .await;
                        }
                    }
                } else {
                    None
                }
            }
            Ok(None) => None,
            Err(error) => {
                return record_and_return_approval_resolution_failure(
                    pool,
                    &trigger,
                    ForegroundFailureKind::PersistenceFailure,
                    error,
                )
                .await;
            }
        };

    let chat_id = match parse_telegram_chat_id(&trigger.ingress) {
        Ok(chat_id) => chat_id,
        Err(error) => {
            return record_and_return_approval_resolution_failure(
                pool,
                &trigger,
                ForegroundFailureKind::TelegramDeliveryFailure,
                error,
            )
            .await;
        }
    };
    let reply_to_message_id = match parse_telegram_reply_target(&trigger.ingress) {
        Ok(reply_to_message_id) => reply_to_message_id,
        Err(error) => {
            return record_and_return_approval_resolution_failure(
                pool,
                &trigger,
                ForegroundFailureKind::TelegramDeliveryFailure,
                error,
            )
            .await;
        }
    };

    let delivery_receipt = match delivery
        .send_message(&TelegramOutboundMessage {
            chat_id,
            text: approval_resolution_message(&resolution, action_execution.as_ref()),
            reply_to_message_id,
            reply_markup: None,
        })
        .await
    {
        Ok(receipt) => receipt,
        Err(error) => {
            return record_and_return_approval_resolution_failure(
                pool,
                &trigger,
                ForegroundFailureKind::TelegramDeliveryFailure,
                error,
            )
            .await;
        }
    };

    let response_payload = json!({
        "kind": "approval_resolution",
        "approval_request_id": resolution.request.approval_request_id,
        "decision": resolution.event.decision,
        "resolved_by": resolution.event.resolved_by,
        "resolved_at": resolution.event.resolved_at,
        "governed_action_execution_id": action_execution
            .as_ref()
            .map(|result| result.record.governed_action_execution_id),
        "governed_action_status": action_execution
            .as_ref()
            .map(|result| result.outcome.status),
        "governed_action_summary": action_execution
            .as_ref()
            .map(|result| result.outcome.summary.clone()),
        "outbound_message_id": delivery_receipt.message_id,
        "resolution_source": parsed_resolution.resolution_source,
    });
    if let Err(error) = execution::mark_succeeded(
        pool,
        trigger.execution_id,
        "approval_resolution",
        0,
        &response_payload,
    )
    .await
    {
        return record_and_return_approval_resolution_failure(
            pool,
            &trigger,
            ForegroundFailureKind::PersistenceFailure,
            error,
        )
        .await;
    }
    if let Err(error) = foreground::mark_ingress_event_processed(
        pool,
        trigger.ingress.ingress_id,
        trigger.execution_id,
    )
    .await
    {
        return record_and_return_approval_resolution_failure(
            pool,
            &trigger,
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
            event_kind: "approval_resolution_resolved".to_string(),
            severity: "info".to_string(),
            trace_id: trigger.trace_id,
            execution_id: Some(trigger.execution_id),
            worker_pid: None,
            payload: json!({
                "ingress_id": trigger.ingress.ingress_id,
                "approval_request_id": resolution.request.approval_request_id,
                "decision": resolution.event.decision,
                "outbound_message_id": delivery_receipt.message_id,
                "resolution_source": parsed_resolution.resolution_source,
            }),
        },
    )
    .await
    {
        return record_and_return_approval_resolution_failure(
            pool,
            &trigger,
            ForegroundFailureKind::PersistenceFailure,
            error,
        )
        .await;
    }

    Ok(TelegramForegroundOrchestrationOutcome::ApprovalResolved(
        TelegramApprovalResolutionCompletion {
            trace_id: trigger.trace_id,
            execution_id: trigger.execution_id,
            ingress_id: trigger.ingress.ingress_id,
            approval_request_id: resolution.request.approval_request_id,
            decision: resolution.event.decision,
            outbound_message_id: delivery_receipt.message_id,
        },
    ))
}

pub async fn deliver_telegram_approval_prompt<D>(
    approvals_prompt_mode: crate::config::ApprovalPromptMode,
    ingress: &contracts::NormalizedIngress,
    approval_request: &approval::ApprovalRequestRecord,
    delivery: &mut D,
) -> Result<TelegramApprovalPromptDelivery>
where
    D: TelegramDelivery,
{
    let chat_id = parse_telegram_chat_id(ingress)?;
    let reply_to_message_id = parse_telegram_reply_target(ingress)?;
    let prompt = telegram::TelegramApprovalPrompt {
        token: approval_request.token.clone(),
        title: approval_request.title.clone(),
        consequence_summary: approval_request.consequence_summary.clone(),
        action_fingerprint: approval_request.action_fingerprint.value.clone(),
        risk_tier: approval_request.risk_tier,
        expires_at: approval_request.expires_at,
    };
    let message = telegram::build_approval_prompt_message(
        approvals_prompt_mode,
        chat_id,
        reply_to_message_id,
        &prompt,
    )?;
    let receipt = delivery.send_message(&message).await?;

    Ok(TelegramApprovalPromptDelivery {
        chat_id,
        approval_request_id: approval_request.approval_request_id,
        outbound_message_id: receipt.message_id,
    })
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
    let (episode_trigger_kind, episode_trigger_source) =
        episode_trigger_metadata(trigger.trigger_kind);
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
            trigger_kind: episode_trigger_kind.to_string(),
            trigger_source: episode_trigger_source.to_string(),
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
        assembly.context.clone(),
    );
    let initial_response = match worker::launch_conscious_worker_with_timeout(
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

    let contracts::WorkerResult::Conscious(initial_result) = &initial_response.result else {
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

    let governed_action_summary = match process_governed_action_proposals(
        pool,
        config,
        &trigger,
        &initial_result.governed_action_proposals,
        delivery,
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

    let (response, result) = if !governed_action_summary.observations.is_empty()
        && governed_action_summary.pending_approval_count == 0
    {
        let mut follow_up_context = assembly.context.clone();
        follow_up_context.governed_action_observations =
            governed_action_summary.observations.clone();
        let follow_up_request = contracts::WorkerRequest::conscious(
            trigger.trace_id,
            trigger.execution_id,
            follow_up_context,
        );
        let follow_up_response = match worker::launch_conscious_worker_with_timeout(
            config,
            model_gateway_config,
            &follow_up_request,
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
        let follow_up_result = match &follow_up_response.result {
            contracts::WorkerResult::Conscious(result) => result.clone(),
            _ => {
                let message =
                    "conscious follow-up worker returned a non-conscious result".to_string();
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
            }
        };
        (follow_up_response, follow_up_result)
    } else {
        (initial_response.clone(), initial_result.clone())
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
            reply_markup: None,
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

    let proposal_summary = match proposal::apply_candidate_proposals(
        pool,
        config,
        &proposal::ProposalProcessingContext {
            trace_id: trigger.trace_id,
            execution_id: trigger.execution_id,
            episode_id: Some(episode_id),
            source_ingress_id: Some(trigger.ingress.ingress_id),
            source_loop_kind: "conscious".to_string(),
        },
        "foreground_orchestration",
        None,
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
            "{} | proposals evaluated={}, accepted={}, rejected={}, canonical_writes={} | governed_actions proposed={}, executed={}, blocked={}, pending_approvals={}",
            result.episode_summary.summary,
            proposal_summary.evaluated_count,
            proposal_summary.accepted_count,
            proposal_summary.rejected_count,
            proposal_summary.canonical_write_count,
            governed_action_summary.proposed_count,
            governed_action_summary.executed_count,
            governed_action_summary.blocked_count,
            governed_action_summary.pending_approval_count
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
                "governed_action_summary": {
                    "proposed": governed_action_summary.proposed_count,
                    "executed": governed_action_summary.executed_count,
                    "blocked": governed_action_summary.blocked_count,
                    "pending_approvals": governed_action_summary.pending_approval_count,
                },
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

async fn process_governed_action_proposals<D>(
    pool: &sqlx::PgPool,
    config: &RuntimeConfig,
    trigger: &contracts::ForegroundTrigger,
    proposals: &[contracts::GovernedActionProposal],
    delivery: &mut D,
) -> Result<GovernedActionProcessingSummary>
where
    D: TelegramDelivery,
{
    let mut summary = GovernedActionProcessingSummary {
        proposed_count: proposals.len(),
        ..GovernedActionProcessingSummary::default()
    };

    for proposal in proposals {
        let planning = governed_actions::plan_governed_action(
            config,
            pool,
            &governed_actions::GovernedActionPlanningRequest {
                governed_action_execution_id: Uuid::now_v7(),
                trace_id: trigger.trace_id,
                execution_id: None,
                proposal: proposal.clone(),
            },
        )
        .await?;

        match planning {
            governed_actions::GovernedActionPlanningOutcome::Blocked(blocked) => {
                summary.blocked_count += 1;
                summary
                    .observations
                    .push(contracts::GovernedActionObservation {
                        observation_id: Uuid::now_v7(),
                        action_kind: blocked.record.action_kind,
                        outcome: blocked.outcome,
                    });
            }
            governed_actions::GovernedActionPlanningOutcome::Planned(planned) => {
                if planned.requires_approval {
                    let approval_request =
                        match approval::get_pending_approval_request_by_fingerprint(
                            pool,
                            &planned.record.action_fingerprint,
                        )
                        .await?
                        {
                            Some(existing) => existing,
                            None => {
                                approval::create_approval_request(
                                    config,
                                    pool,
                                    &approval::NewApprovalRequestRecord {
                                        approval_request_id: Uuid::now_v7(),
                                        trace_id: trigger.trace_id,
                                        execution_id: Some(trigger.execution_id),
                                        action_proposal_id: planned.record.action_proposal_id,
                                        action_fingerprint: planned
                                            .record
                                            .action_fingerprint
                                            .clone(),
                                        action_kind: planned.record.action_kind,
                                        risk_tier: planned.record.risk_tier,
                                        title: proposal.title.clone(),
                                        consequence_summary: consequence_summary_for_proposal(
                                            proposal,
                                        ),
                                        capability_scope: proposal.capability_scope.clone(),
                                        requested_by: format!(
                                            "telegram:{}",
                                            trigger.ingress.internal_principal_ref
                                        ),
                                        token: Uuid::now_v7().to_string(),
                                        requested_at: trigger.received_at,
                                        expires_at: trigger.received_at
                                            + chrono::Duration::seconds(
                                                i64::try_from(config.approvals.default_ttl_seconds)
                                                    .context("approval TTL exceeded i64 range")?,
                                            ),
                                    },
                                )
                                .await?
                            }
                        };
                    governed_actions::attach_approval_request(
                        pool,
                        planned.record.governed_action_execution_id,
                        approval_request.approval_request_id,
                    )
                    .await?;
                    deliver_telegram_approval_prompt(
                        config.approvals.prompt_mode,
                        &trigger.ingress,
                        &approval_request,
                        delivery,
                    )
                    .await?;
                    summary.pending_approval_count += 1;
                } else {
                    let executed =
                        governed_actions::execute_governed_action(config, pool, &planned.record)
                            .await?;
                    if executed.outcome.status == contracts::GovernedActionStatus::Blocked {
                        summary.blocked_count += 1;
                    } else {
                        summary.executed_count += 1;
                    }
                    summary.observations.push(executed.observation);
                }
            }
        }
    }

    Ok(summary)
}

fn consequence_summary_for_proposal(proposal: &contracts::GovernedActionProposal) -> String {
    let filesystem_reads = proposal.capability_scope.filesystem.read_roots.len();
    let filesystem_writes = proposal.capability_scope.filesystem.write_roots.len();
    let env_count = proposal.capability_scope.environment.allow_variables.len();
    format!(
        "{} with {} read root(s), {} write root(s), network={}, {} environment variable(s), timeout={} ms",
        proposal.title,
        filesystem_reads,
        filesystem_writes,
        match proposal.capability_scope.network {
            contracts::NetworkAccessPosture::Disabled => "disabled",
            contracts::NetworkAccessPosture::Enabled => "enabled",
            contracts::NetworkAccessPosture::Allowlisted => "allowlisted",
        },
        env_count,
        proposal.capability_scope.execution.timeout_ms
    )
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

async fn record_and_return_approval_resolution_failure<T>(
    pool: &sqlx::PgPool,
    trigger: &contracts::ForegroundTrigger,
    failure_kind: ForegroundFailureKind,
    error: Error,
) -> Result<T> {
    let error_message = format_error_chain(&error);
    if let Err(record_error) = record_foreground_failure(
        pool,
        trigger.trace_id,
        trigger.execution_id,
        None,
        failure_kind,
        &error_message,
    )
    .await
    {
        return Err(error.context(format!(
            "failed to record approval-resolution execution failure: {record_error}"
        )));
    }
    if let Err(process_error) = foreground::mark_ingress_event_processed(
        pool,
        trigger.ingress.ingress_id,
        trigger.execution_id,
    )
    .await
    {
        return Err(error.context(format!(
            "failed to mark approval resolution ingress as processed: {process_error}"
        )));
    }

    Err(error)
}

fn build_trigger_user_messages(
    trigger_kind: contracts::ForegroundTriggerKind,
    candidate_messages: Vec<UserEpisodeMessage>,
) -> Vec<UserEpisodeMessage> {
    match trigger_kind {
        contracts::ForegroundTriggerKind::UserIngress => candidate_messages,
        contracts::ForegroundTriggerKind::ApprovedWakeSignal => Vec::new(),
    }
}

fn episode_trigger_metadata(
    trigger_kind: contracts::ForegroundTriggerKind,
) -> (&'static str, &'static str) {
    match trigger_kind {
        contracts::ForegroundTriggerKind::UserIngress => ("user_ingress", "telegram"),
        contracts::ForegroundTriggerKind::ApprovedWakeSignal => {
            ("approved_wake_signal", "wake_signal")
        }
    }
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
    ApprovalResolutionFailure,
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
            Self::ApprovalResolutionFailure => "approval_resolution_failure",
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

fn parse_approval_resolution_ingress(
    ingress: &contracts::NormalizedIngress,
) -> Result<Option<ParsedApprovalResolutionIngress>> {
    if ingress.event_kind == contracts::IngressEventKind::ApprovalCallback {
        return parse_approval_callback_ingress(ingress).map(Some);
    }

    let Some(command_hint) = ingress.command_hint.as_ref() else {
        return Ok(None);
    };
    if !matches!(command_hint.command.as_str(), "approve" | "reject") {
        return Ok(None);
    }
    if command_hint.args.len() != 1 {
        bail!(
            "approval fallback command '/{}' requires exactly one token argument",
            command_hint.command
        );
    }
    let token = command_hint.args[0].trim();
    if token.is_empty() {
        bail!(
            "approval fallback command '/{}' requires a non-empty token argument",
            command_hint.command
        );
    }

    Ok(Some(ParsedApprovalResolutionIngress {
        approval_token: token.to_string(),
        decision: parse_approval_callback_decision(&command_hint.command)?,
        expected_action_fingerprint: None,
        resolution_source: format!("telegram command /{} {}", command_hint.command, token),
    }))
}

fn parse_approval_callback_ingress(
    ingress: &contracts::NormalizedIngress,
) -> Result<ParsedApprovalResolutionIngress> {
    let approval_payload = ingress
        .approval_payload
        .as_ref()
        .context("approval callback ingress is missing approval payload metadata")?;
    let callback_data = approval_payload
        .callback_data
        .as_deref()
        .context("approval callback ingress is missing callback data")?;

    if let Some((decision, token, fingerprint)) =
        callback_data
            .split_once('|')
            .and_then(|(decision, remainder)| {
                let (token, fingerprint) = remainder.split_once('|')?;
                Some((decision, token, fingerprint))
            })
    {
        return Ok(ParsedApprovalResolutionIngress {
            approval_token: token.trim().to_string(),
            decision: parse_approval_callback_decision(decision)?,
            expected_action_fingerprint: Some(contracts::GovernedActionFingerprint {
                value: fingerprint.trim().to_string(),
            }),
            resolution_source: format!("telegram callback {}", approval_payload.token),
        });
    }

    if let Some((decision, token)) = callback_data.split_once(':') {
        return Ok(ParsedApprovalResolutionIngress {
            approval_token: token.trim().to_string(),
            decision: parse_approval_callback_decision(decision)?,
            expected_action_fingerprint: None,
            resolution_source: format!("telegram callback {}", approval_payload.token),
        });
    }

    bail!("approval callback data '{callback_data}' is malformed");
}

fn parse_approval_callback_decision(
    decision: &str,
) -> Result<contracts::ApprovalResolutionDecision> {
    match decision.trim() {
        "approve" | "approved" => Ok(contracts::ApprovalResolutionDecision::Approved),
        "reject" | "rejected" => Ok(contracts::ApprovalResolutionDecision::Rejected),
        other => bail!("approval callback decision '{other}' is unsupported"),
    }
}

fn approval_resolution_message(
    resolution: &approval::ApprovalResolutionResult,
    action_execution: Option<&governed_actions::GovernedActionExecutionResult>,
) -> String {
    let base_message = match resolution.event.decision {
        contracts::ApprovalResolutionDecision::Approved => {
            format!("Approved: {}", resolution.request.title)
        }
        contracts::ApprovalResolutionDecision::Rejected => {
            format!("Rejected: {}", resolution.request.title)
        }
        contracts::ApprovalResolutionDecision::Expired => {
            "Approval request expired before it could be applied.".to_string()
        }
        contracts::ApprovalResolutionDecision::Invalidated => {
            "Approval request is no longer valid because the requested action changed.".to_string()
        }
    };
    match action_execution {
        Some(action_execution) => format!("{}. {}", base_message, action_execution.outcome.summary),
        None => base_message,
    }
}
