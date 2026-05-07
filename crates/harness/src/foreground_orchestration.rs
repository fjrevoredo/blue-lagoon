use anyhow::{Context, Error, Result, bail};
use chrono::{Duration, Utc};
use serde_json::json;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    approval,
    audit::{self, NewAuditEvent},
    causal_links::{self, NewCausalLink},
    config::{ResolvedModelGatewayConfig, ResolvedTelegramConfig, RuntimeConfig},
    context, execution,
    foreground::{self, ForegroundTriggerIntakeOutcome, NewEpisode, NewEpisodeMessage},
    governed_actions, identity,
    model_gateway::ModelProviderTransport,
    policy, proposal, recovery,
    telegram::{self, TelegramChatAction, TelegramDelivery, TelegramOutboundMessage},
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
pub struct TelegramForegroundPlanExecution {
    pub execution: ForegroundExecutionIds,
    pub trigger_kind_override: Option<contracts::ForegroundTriggerKind>,
    pub plan: foreground::PendingForegroundExecutionPlan,
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
    action_limit_reached: bool,
}

impl GovernedActionProcessingSummary {
    fn merge(&mut self, other: Self) {
        self.observations.extend(other.observations);
        self.proposed_count += other.proposed_count;
        self.executed_count += other.executed_count;
        self.blocked_count += other.blocked_count;
        self.pending_approval_count += other.pending_approval_count;
        self.action_limit_reached |= other.action_limit_reached;
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct ForegroundActionLoopTracker {
    executed_action_count: u32,
}

impl ForegroundActionLoopTracker {
    fn new(executed_action_count: u32) -> Self {
        Self {
            executed_action_count,
        }
    }

    fn max_actions_per_turn(self, config: &RuntimeConfig) -> u32 {
        config.governed_actions.max_actions_per_foreground_turn
    }

    fn remaining_actions_before_cap(self, config: &RuntimeConfig) -> u32 {
        self.max_actions_per_turn(config)
            .saturating_sub(self.executed_action_count)
    }

    fn is_at_cap(self, config: &RuntimeConfig) -> bool {
        self.remaining_actions_before_cap(config) == 0
    }

    fn record_executed_action(&mut self) {
        self.executed_action_count = self.executed_action_count.saturating_add(1);
    }

    fn as_contract_state(
        self,
        config: &RuntimeConfig,
    ) -> contracts::ForegroundGovernedActionLoopState {
        contracts::ForegroundGovernedActionLoopState {
            executed_action_count: self.executed_action_count,
            max_actions_per_turn: self.max_actions_per_turn(config),
            remaining_actions_before_cap: self.remaining_actions_before_cap(config),
            cap_exceeded_behavior: config.governed_actions.cap_exceeded_behavior,
        }
    }
}

#[derive(Debug, Clone)]
struct ConsciousTurnLoopOutcome {
    response: contracts::WorkerResponse,
    result: contracts::ConsciousWorkerResult,
    governed_action_summary: GovernedActionProcessingSummary,
}

#[derive(Debug)]
struct GovernedActionApprovalRoute<'a> {
    trigger: &'a contracts::ForegroundTrigger,
    proposal: &'a contracts::GovernedActionProposal,
    record: &'a governed_actions::GovernedActionExecutionRecord,
    approval_title: &'a str,
    approval_consequence_summary: &'a str,
}

#[derive(Debug, Clone)]
struct ConsciousTurnLoopRequest<'a, T> {
    pool: &'a sqlx::PgPool,
    config: &'a RuntimeConfig,
    model_gateway_config: &'a ResolvedModelGatewayConfig,
    trigger: &'a contracts::ForegroundTrigger,
    context: contracts::ConsciousContext,
    initial_executed_action_count: u32,
    transport: &'a T,
    chat_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ForegroundFailureContext {
    trace_id: Uuid,
    execution_id: Uuid,
    episode_id: Option<Uuid>,
    failure_kind: ForegroundFailureKind,
    terminal_ingress_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ForegroundFailureDeliveryTarget {
    chat_id: i64,
    reply_to_message_id: Option<i64>,
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
            model_gateway_config,
            trigger,
            parsed_resolution,
            delivery,
            transport,
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
    execution: TelegramForegroundPlanExecution,
    transport: &T,
    delivery: &mut D,
) -> Result<TelegramForegroundOrchestrationOutcome>
where
    T: ModelProviderTransport,
    D: TelegramDelivery,
{
    let primary_ingress =
        foreground::load_normalized_ingress(pool, execution.plan.primary_ingress.ingress_id)
            .await?;
    let trigger = match execution.trigger_kind_override {
        Some(trigger_kind) => foreground::build_foreground_trigger_with_kind(
            config,
            execution.execution.trace_id,
            execution.execution.execution_id,
            trigger_kind,
            primary_ingress,
        )?,
        None => foreground::build_foreground_trigger(
            config,
            execution.execution.trace_id,
            execution.execution.execution_id,
            primary_ingress,
        )?,
    };
    if let Some(parsed_resolution) = parse_approval_resolution_ingress(&trigger.ingress)? {
        return orchestrate_telegram_approval_resolution_trigger(
            pool,
            config,
            model_gateway_config,
            trigger,
            parsed_resolution,
            delivery,
            transport,
        )
        .await;
    }
    let user_messages = build_trigger_user_messages(
        trigger.trigger_kind,
        execution
            .plan
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
            .collect::<Vec<_>>(),
    );

    orchestrate_telegram_foreground_trigger(
        pool,
        config,
        model_gateway_config,
        ForegroundExecutionInput {
            trigger,
            recovery_context: contracts::ForegroundRecoveryContext {
                mode: execution.plan.mode,
                ordered_ingress: execution.plan.ordered_ingress,
            },
            user_messages,
        },
        transport,
        delivery,
    )
    .await
}

async fn orchestrate_telegram_approval_resolution_trigger<T, D>(
    pool: &sqlx::PgPool,
    config: &RuntimeConfig,
    model_gateway_config: &ResolvedModelGatewayConfig,
    trigger: contracts::ForegroundTrigger,
    parsed_resolution: ParsedApprovalResolutionIngress,
    delivery: &mut D,
    transport: &T,
) -> Result<TelegramForegroundOrchestrationOutcome>
where
    T: ModelProviderTransport,
    D: TelegramDelivery,
{
    info!(
        trace_id = %trigger.trace_id,
        execution_id = %trigger.execution_id,
        ingress_id = %trigger.ingress.ingress_id,
        approval_token = %parsed_resolution.approval_token,
        "resolving telegram approval trigger"
    );
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
    info!(
        trace_id = %trigger.trace_id,
        execution_id = %trigger.execution_id,
        approval_request_id = %resolution.request.approval_request_id,
        decision = ?resolution.event.decision,
        "approval request resolved"
    );

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
    emit_typing_chat_action(
        delivery,
        chat_id,
        trigger.trace_id,
        trigger.execution_id,
        "approval_resolution",
    )
    .await;

    let action_execution =
        match governed_actions::get_governed_action_execution_by_approval_request_id(
            pool,
            resolution.request.approval_request_id,
        )
        .await
        {
            Ok(Some(record)) => {
                info!(
                    trace_id = %trigger.trace_id,
                    execution_id = %trigger.execution_id,
                    approval_request_id = %resolution.request.approval_request_id,
                    governed_action_execution_id = %record.governed_action_execution_id,
                    "approval resolution matched governed action execution"
                );
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
                        Ok(executed) => {
                            info!(
                                trace_id = %trigger.trace_id,
                                execution_id = %trigger.execution_id,
                                governed_action_execution_id = %executed.record.governed_action_execution_id,
                                status = ?executed.outcome.status,
                                output_ref = ?executed.outcome.output_ref,
                                "approved governed action executed"
                            );
                            Some(executed)
                        }
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
            Ok(None) => {
                warn!(
                    trace_id = %trigger.trace_id,
                    execution_id = %trigger.execution_id,
                    approval_request_id = %resolution.request.approval_request_id,
                    "approval resolution had no linked governed action execution"
                );
                None
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

    let delivery_receipt = match delivery
        .send_message(&TelegramOutboundMessage {
            chat_id,
            text: approval_resolution_message(&resolution),
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

    if let Some(ref executed) = action_execution {
        emit_typing_chat_action(
            delivery,
            chat_id,
            trigger.trace_id,
            trigger.execution_id,
            "approval_follow_up",
        )
        .await;

        let follow_up_episode_id = Uuid::now_v7();
        let (episode_trigger_kind, episode_trigger_source) =
            episode_trigger_metadata(trigger.trigger_kind);

        let episode_inserted = match foreground::insert_episode(
            pool,
            &NewEpisode {
                episode_id: follow_up_episode_id,
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
            Ok(()) => true,
            Err(error) => {
                let _ = audit::insert(
                    pool,
                    &NewAuditEvent {
                        loop_kind: "conscious".to_string(),
                        subsystem: "foreground_orchestration".to_string(),
                        event_kind: "approval_follow_up_failed".to_string(),
                        severity: "warning".to_string(),
                        trace_id: trigger.trace_id,
                        execution_id: Some(trigger.execution_id),
                        worker_pid: None,
                        payload: json!({
                            "step": "insert_episode",
                            "error": format_error_chain(&error),
                        }),
                    },
                )
                .await;
                false
            }
        };

        if episode_inserted {
            let follow_up_result: Result<()> = async {
                let mut assembly = context::assemble_foreground_context(
                    pool,
                    config,
                    trigger.clone(),
                    context::ContextAssemblyOptions {
                        episode_id: Some(follow_up_episode_id),
                        ..context::ContextAssemblyOptions::default()
                    },
                )
                .await?;
                assembly.context.governed_action_observations = vec![executed.observation.clone()];
                let turn_outcome = execute_conscious_turn_with_governed_action_loop(
                    ConsciousTurnLoopRequest {
                        pool,
                        config,
                        model_gateway_config,
                        trigger: &trigger,
                        context: assembly.context.clone(),
                        initial_executed_action_count: 1,
                        transport,
                        chat_id,
                    },
                    delivery,
                )
                .await?;
                let persisted_follow_up_text = approval_follow_up_episode_text(
                    &turn_outcome.governed_action_summary.observations,
                    &turn_outcome.result.assistant_output.text,
                );
                let delivered_follow_up_text =
                    approval_follow_up_delivery_text(&turn_outcome.result.assistant_output.text);

                let follow_up_message_id = Uuid::now_v7();
                foreground::insert_episode_message(
                    pool,
                    &NewEpisodeMessage {
                        episode_message_id: follow_up_message_id,
                        episode_id: follow_up_episode_id,
                        trace_id: trigger.trace_id,
                        execution_id: trigger.execution_id,
                        message_order: 0,
                        message_role: "assistant".to_string(),
                        channel_kind: turn_outcome.result.assistant_output.channel_kind,
                        text_body: Some(persisted_follow_up_text.clone()),
                        external_message_id: None,
                    },
                )
                .await?;

                proposal::apply_candidate_proposals(
                    pool,
                    config,
                    &proposal::ProposalProcessingContext {
                        trace_id: trigger.trace_id,
                        execution_id: trigger.execution_id,
                        episode_id: Some(follow_up_episode_id),
                        source_ingress_id: Some(trigger.ingress.ingress_id),
                        source_loop_kind: "conscious".to_string(),
                    },
                    "foreground_orchestration",
                    None,
                    &turn_outcome.result.candidate_proposals,
                )
                .await?;

                let follow_up_receipt = delivery
                    .send_message(&TelegramOutboundMessage {
                        chat_id,
                        text: delivered_follow_up_text,
                        reply_to_message_id,
                        reply_markup: None,
                    })
                    .await?;

                foreground::update_episode_message_external_message_id(
                    pool,
                    follow_up_message_id,
                    &follow_up_receipt.message_id.to_string(),
                )
                .await?;

                foreground::mark_episode_completed(
                    pool,
                    follow_up_episode_id,
                    "completed",
                    &persisted_follow_up_text
                        .chars()
                        .take(120)
                        .collect::<String>(),
                )
                .await?;

                Ok(())
            }
            .await;

            if let Err(error) = follow_up_result {
                let error_message = format_error_chain(&error);
                warn!(
                    trace_id = %trigger.trace_id,
                    execution_id = %trigger.execution_id,
                    follow_up_episode_id = %follow_up_episode_id,
                    error = %error_message,
                    "approval follow-up worker failed after governed action execution"
                );
                let _ = foreground::mark_episode_failed(
                    pool,
                    follow_up_episode_id,
                    "follow_up_failed",
                    &error_message,
                )
                .await;
                let _ = audit::insert(
                    pool,
                    &NewAuditEvent {
                        loop_kind: "conscious".to_string(),
                        subsystem: "foreground_orchestration".to_string(),
                        event_kind: "approval_follow_up_failed".to_string(),
                        severity: "warning".to_string(),
                        trace_id: trigger.trace_id,
                        execution_id: Some(trigger.execution_id),
                        worker_pid: None,
                        payload: json!({
                            "follow_up_episode_id": follow_up_episode_id,
                            "error": error_message,
                        }),
                    },
                )
                .await;
            }
        }
    }

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
    if let Err(error) = causal_links::insert(
        pool,
        &NewCausalLink {
            trace_id: trigger.trace_id,
            source_kind: "execution_record".to_string(),
            source_id: trigger.execution_id,
            target_kind: "episode".to_string(),
            target_id: episode_id,
            edge_kind: "opened_episode".to_string(),
            payload: json!({
                "ingress_id": trigger.ingress.ingress_id,
                "trigger_kind": episode_trigger_kind,
                "trigger_source": episode_trigger_source,
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
    let failure_delivery_target = ForegroundFailureDeliveryTarget {
        chat_id,
        reply_to_message_id,
    };
    emit_typing_chat_action(
        delivery,
        chat_id,
        trigger.trace_id,
        trigger.execution_id,
        "foreground_start",
    )
    .await;

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
            return record_deliver_and_return_failure(
                pool,
                foreground_failure_context(
                    &trigger,
                    &recovery_context,
                    recorded_episode_id,
                    ForegroundFailureKind::ContextAssemblyFailure,
                ),
                error,
                delivery,
                failure_delivery_target,
            )
            .await;
        }
    };
    let metadata_payload = match serde_json::to_value(&assembly.metadata)
        .context("failed to serialize context assembly metadata")
    {
        Ok(payload) => payload,
        Err(error) => {
            return record_deliver_and_return_failure(
                pool,
                foreground_failure_context(
                    &trigger,
                    &recovery_context,
                    recorded_episode_id,
                    ForegroundFailureKind::PersistenceFailure,
                ),
                error,
                delivery,
                failure_delivery_target,
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
        return record_deliver_and_return_failure(
            pool,
            foreground_failure_context(
                &trigger,
                &recovery_context,
                recorded_episode_id,
                ForegroundFailureKind::PersistenceFailure,
            ),
            error,
            delivery,
            failure_delivery_target,
        )
        .await;
    }

    let turn_outcome = match execute_conscious_turn_with_governed_action_loop(
        ConsciousTurnLoopRequest {
            pool,
            config,
            model_gateway_config,
            trigger: &trigger,
            context: assembly.context.clone(),
            initial_executed_action_count: 0,
            transport,
            chat_id,
        },
        delivery,
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(error) => {
            record_and_deliver_foreground_failure(
                pool,
                foreground_failure_context(
                    &trigger,
                    &recovery_context,
                    recorded_episode_id,
                    classify_conscious_worker_failure(&error),
                ),
                &format_error_chain(&error),
                delivery,
                failure_delivery_target,
            )
            .await?;
            return Err(error);
        }
    };
    let response = turn_outcome.response;
    let result = turn_outcome.result;
    let governed_action_summary = turn_outcome.governed_action_summary;

    let candidate_proposals =
        foreground_candidate_proposals(&assembly.context, &trigger, &result.candidate_proposals);

    let assistant_text = foreground_assistant_delivery_text(
        &result.assistant_output.text,
        &governed_action_summary,
        &candidate_proposals,
        &assembly.context,
    );
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
            text_body: Some(assistant_text.clone()),
            external_message_id: None,
        },
    )
    .await
    {
        return record_deliver_and_return_failure(
            pool,
            foreground_failure_context(
                &trigger,
                &recovery_context,
                recorded_episode_id,
                ForegroundFailureKind::PersistenceFailure,
            ),
            error,
            delivery,
            failure_delivery_target,
        )
        .await;
    }

    let delivery_receipt = match delivery
        .send_message(&TelegramOutboundMessage {
            chat_id,
            text: assistant_text.clone(),
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
        return record_deliver_and_return_failure(
            pool,
            foreground_failure_context(
                &trigger,
                &recovery_context,
                recorded_episode_id,
                ForegroundFailureKind::PersistenceFailure,
            ),
            error,
            delivery,
            failure_delivery_target,
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
        &candidate_proposals,
    )
    .await
    {
        Ok(summary) => summary,
        Err(error) => {
            return record_deliver_and_return_failure(
                pool,
                foreground_failure_context(
                    &trigger,
                    &recovery_context,
                    recorded_episode_id,
                    ForegroundFailureKind::PersistenceFailure,
                ),
                error,
                delivery,
                failure_delivery_target,
            )
            .await;
        }
    };

    let response_payload = serde_json::to_value(&response)
        .context("failed to serialize conscious worker response payload");
    let response_payload = match response_payload {
        Ok(response_payload) => response_payload,
        Err(error) => {
            return record_deliver_and_return_failure(
                pool,
                foreground_failure_context(
                    &trigger,
                    &recovery_context,
                    recorded_episode_id,
                    ForegroundFailureKind::PersistenceFailure,
                ),
                error,
                delivery,
                failure_delivery_target,
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
        return record_deliver_and_return_failure(
            pool,
            foreground_failure_context(
                &trigger,
                &recovery_context,
                recorded_episode_id,
                ForegroundFailureKind::PersistenceFailure,
            ),
            error,
            delivery,
            failure_delivery_target,
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
        return record_deliver_and_return_failure(
            pool,
            foreground_failure_context(
                &trigger,
                &recovery_context,
                recorded_episode_id,
                ForegroundFailureKind::PersistenceFailure,
            ),
            error,
            delivery,
            failure_delivery_target,
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
        return record_deliver_and_return_failure(
            pool,
            foreground_failure_context(
                &trigger,
                &recovery_context,
                recorded_episode_id,
                ForegroundFailureKind::PersistenceFailure,
            ),
            error,
            delivery,
            failure_delivery_target,
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
                "candidate_proposal_count": candidate_proposals.len(),
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
        return record_deliver_and_return_failure(
            pool,
            foreground_failure_context(
                &trigger,
                &recovery_context,
                recorded_episode_id,
                ForegroundFailureKind::PersistenceFailure,
            ),
            error,
            delivery,
            failure_delivery_target,
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
    action_loop_tracker: &mut ForegroundActionLoopTracker,
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
                let mut approval_title = proposal.title.clone();
                let mut approval_consequence_summary = consequence_summary_for_proposal(proposal);
                let mut requires_approval = planned.requires_approval;

                if action_loop_tracker.is_at_cap(config) {
                    let limit_summary =
                        foreground_action_limit_summary(config, *action_loop_tracker);
                    match config.governed_actions.cap_exceeded_behavior {
                        contracts::GovernedActionCapExceededBehavior::AlwaysApprove => {
                            governed_actions::write_governed_action_audit_event(
                                pool,
                                &planned.record,
                                "governed_action_cap_auto_approved",
                                "info",
                                foreground_action_limit_payload(config, *action_loop_tracker),
                            )
                            .await?;
                        }
                        contracts::GovernedActionCapExceededBehavior::AlwaysDeny => {
                            let blocked_record =
                                governed_actions::update_governed_action_execution(
                                    pool,
                                    governed_actions::GovernedActionExecutionUpdate {
                                        governed_action_execution_id: planned
                                            .record
                                            .governed_action_execution_id,
                                        status: contracts::GovernedActionStatus::Blocked,
                                        execution_id: None,
                                        output_ref: None,
                                        blocked_reason: Some(&limit_summary),
                                        approval_request_id: None,
                                        started_at: None,
                                        completed_at: None,
                                    },
                                )
                                .await?;
                            governed_actions::write_governed_action_audit_event(
                                pool,
                                &blocked_record,
                                "governed_action_blocked",
                                "warn",
                                json!({
                                    "phase": "foreground_action_limit",
                                    "reason": limit_summary,
                                    "limit": foreground_action_limit_payload(
                                        config,
                                        *action_loop_tracker,
                                    ),
                                }),
                            )
                            .await?;
                            summary.blocked_count += 1;
                            summary.action_limit_reached = true;
                            summary
                                .observations
                                .push(contracts::GovernedActionObservation {
                                    observation_id: Uuid::now_v7(),
                                    action_kind: blocked_record.action_kind,
                                    outcome: contracts::GovernedActionExecutionOutcome {
                                        status: contracts::GovernedActionStatus::Blocked,
                                        summary: limit_summary,
                                        fingerprint: Some(
                                            blocked_record.action_fingerprint.clone(),
                                        ),
                                        output_ref: blocked_record.output_ref.clone(),
                                    },
                                });
                            continue;
                        }
                        contracts::GovernedActionCapExceededBehavior::Escalate => {
                            if !requires_approval {
                                let approval_record =
                                    governed_actions::update_governed_action_execution(
                                        pool,
                                        governed_actions::GovernedActionExecutionUpdate {
                                            governed_action_execution_id: planned
                                                .record
                                                .governed_action_execution_id,
                                            status:
                                                contracts::GovernedActionStatus::AwaitingApproval,
                                            execution_id: None,
                                            output_ref: None,
                                            blocked_reason: None,
                                            approval_request_id: None,
                                            started_at: None,
                                            completed_at: None,
                                        },
                                    )
                                    .await?;
                                governed_actions::write_governed_action_audit_event(
                                    pool,
                                    &approval_record,
                                    "governed_action_cap_escalated",
                                    "info",
                                    foreground_action_limit_payload(config, *action_loop_tracker),
                                )
                                .await?;
                            }
                            requires_approval = true;
                            approval_title =
                                format!("Continue after action limit: {}", proposal.title);
                            approval_consequence_summary =
                                format!("{}. {}", limit_summary, approval_consequence_summary);
                        }
                    }
                }

                if requires_approval {
                    route_governed_action_for_approval(
                        pool,
                        config,
                        GovernedActionApprovalRoute {
                            trigger,
                            proposal,
                            record: &planned.record,
                            approval_title: &approval_title,
                            approval_consequence_summary: &approval_consequence_summary,
                        },
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
                        action_loop_tracker.record_executed_action();
                    }
                    summary.observations.push(executed.observation);
                }
            }
        }
    }

    Ok(summary)
}

async fn route_governed_action_for_approval<D>(
    pool: &sqlx::PgPool,
    config: &RuntimeConfig,
    route: GovernedActionApprovalRoute<'_>,
    delivery: &mut D,
) -> Result<()>
where
    D: TelegramDelivery,
{
    let approval_request = match approval::get_pending_approval_request_by_fingerprint(
        pool,
        &route.record.action_fingerprint,
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
                    trace_id: route.trigger.trace_id,
                    execution_id: Some(route.trigger.execution_id),
                    action_proposal_id: route.record.action_proposal_id,
                    action_fingerprint: route.record.action_fingerprint.clone(),
                    action_kind: route.record.action_kind,
                    risk_tier: route.record.risk_tier,
                    title: route.approval_title.to_string(),
                    consequence_summary: route.approval_consequence_summary.to_string(),
                    capability_scope: route.proposal.capability_scope.clone(),
                    requested_by: format!(
                        "telegram:{}",
                        route.trigger.ingress.internal_principal_ref
                    ),
                    token: Uuid::now_v7().to_string(),
                    requested_at: route.trigger.received_at,
                    expires_at: route.trigger.received_at
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
        route.record.governed_action_execution_id,
        approval_request.approval_request_id,
    )
    .await?;
    recovery::recover_approval_request_transition(
        pool,
        &approval_request,
        recovery::RecoveryApprovalState::Pending,
        Utc::now(),
        "approval_transition_pending",
    )
    .await
    .context("failed to route pending approval transition through recovery")?;
    deliver_telegram_approval_prompt(
        config.approvals.prompt_mode,
        &route.trigger.ingress,
        &approval_request,
        delivery,
    )
    .await?;
    Ok(())
}

fn foreground_action_limit_summary(
    config: &RuntimeConfig,
    action_loop_tracker: ForegroundActionLoopTracker,
) -> String {
    format!(
        "foreground governed-action limit reached: {} action(s) already executed in this turn; configured maximum is {}",
        action_loop_tracker.executed_action_count,
        config.governed_actions.max_actions_per_foreground_turn
    )
}

fn foreground_action_limit_payload(
    config: &RuntimeConfig,
    action_loop_tracker: ForegroundActionLoopTracker,
) -> serde_json::Value {
    json!({
        "executed_action_count": action_loop_tracker.executed_action_count,
        "max_actions_per_turn": config.governed_actions.max_actions_per_foreground_turn,
        "remaining_actions_before_cap": action_loop_tracker.remaining_actions_before_cap(config),
        "cap_exceeded_behavior": match config.governed_actions.cap_exceeded_behavior {
            contracts::GovernedActionCapExceededBehavior::Escalate => "escalate",
            contracts::GovernedActionCapExceededBehavior::AlwaysApprove => "always_approve",
            contracts::GovernedActionCapExceededBehavior::AlwaysDeny => "always_deny",
        },
    })
}

async fn execute_conscious_turn_with_governed_action_loop<T, D>(
    request: ConsciousTurnLoopRequest<'_, T>,
    delivery: &mut D,
) -> Result<ConsciousTurnLoopOutcome>
where
    T: ModelProviderTransport,
    D: TelegramDelivery,
{
    let ConsciousTurnLoopRequest {
        pool,
        config,
        model_gateway_config,
        trigger,
        mut context,
        initial_executed_action_count,
        transport,
        chat_id,
    } = request;
    let foreground_timeout_ms = policy::effective_foreground_worker_timeout_ms(config);
    let max_worker_passes = config
        .governed_actions
        .max_actions_per_foreground_turn
        .saturating_add(2);
    let mut action_loop_tracker = ForegroundActionLoopTracker::new(initial_executed_action_count);
    let mut governed_action_summary = GovernedActionProcessingSummary::default();
    let mut limit_follow_up_consumed = false;
    let mut worker_pass_count = 0u32;

    loop {
        if worker_pass_count > 0 {
            emit_typing_chat_action(
                delivery,
                chat_id,
                trigger.trace_id,
                trigger.execution_id,
                "foreground_follow_up",
            )
            .await;
        }
        worker_pass_count = worker_pass_count.saturating_add(1);
        context.governed_action_loop_state = Some(action_loop_tracker.as_contract_state(config));
        let request_context = context.clone();
        let request = contracts::WorkerRequest::conscious(
            trigger.trace_id,
            trigger.execution_id,
            request_context.clone(),
        );
        let response = launch_leased_conscious_worker(
            pool,
            config,
            model_gateway_config,
            trigger,
            &request,
            transport,
            foreground_timeout_ms,
        )
        .await?;
        let contracts::WorkerResult::Conscious(result) = &response.result else {
            bail!("conscious follow-up worker returned a non-conscious result");
        };
        let round_summary = process_governed_action_proposals(
            pool,
            config,
            trigger,
            &mut action_loop_tracker,
            &result.governed_action_proposals,
            delivery,
        )
        .await?;
        let should_continue = !round_summary.observations.is_empty()
            && round_summary.pending_approval_count == 0
            && worker_pass_count < max_worker_passes
            && (!round_summary.action_limit_reached || !limit_follow_up_consumed);
        if round_summary.action_limit_reached {
            limit_follow_up_consumed = true;
        }
        governed_action_summary.merge(round_summary);
        let result = result.clone();
        if !should_continue {
            return Ok(ConsciousTurnLoopOutcome {
                response,
                result,
                governed_action_summary,
            });
        }
        context = request_context;
        context.governed_action_observations = governed_action_summary.observations.clone();
    }
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

fn foreground_failure_context(
    trigger: &contracts::ForegroundTrigger,
    recovery_context: &contracts::ForegroundRecoveryContext,
    episode_id: Option<Uuid>,
    failure_kind: ForegroundFailureKind,
) -> ForegroundFailureContext {
    ForegroundFailureContext {
        trace_id: trigger.trace_id,
        execution_id: trigger.execution_id,
        episode_id,
        failure_kind,
        terminal_ingress_ids: terminal_failure_ingress_ids(trigger, recovery_context),
    }
}

fn terminal_failure_ingress_ids(
    trigger: &contracts::ForegroundTrigger,
    recovery_context: &contracts::ForegroundRecoveryContext,
) -> Vec<Uuid> {
    if recovery_context.ordered_ingress.is_empty() {
        vec![trigger.ingress.ingress_id]
    } else {
        recovery_context
            .ordered_ingress
            .iter()
            .map(|ingress| ingress.ingress_id)
            .collect()
    }
}

async fn record_and_deliver_foreground_failure<D>(
    pool: &sqlx::PgPool,
    context: ForegroundFailureContext,
    error_message: &str,
    delivery: &mut D,
    target: ForegroundFailureDeliveryTarget,
) -> Result<()>
where
    D: TelegramDelivery,
{
    let notice_text = foreground_failure_notice_text(context.trace_id, context.failure_kind);
    let record_result = record_foreground_failure(
        pool,
        context.trace_id,
        context.execution_id,
        context.episode_id,
        context.failure_kind,
        error_message,
    )
    .await;
    let persist_result = if record_result.is_ok() {
        persist_foreground_failure_notice(pool, &context, &notice_text).await
    } else {
        Ok(None)
    };
    let delivery_receipt = deliver_foreground_failure_notice(
        delivery,
        target.chat_id,
        target.reply_to_message_id,
        context.trace_id,
        context.execution_id,
        context.failure_kind,
        notice_text,
    )
    .await;
    let close_ingress_result = if record_result.is_ok() {
        mark_terminal_failure_ingress_processed(pool, &context).await
    } else {
        Ok(())
    };
    record_result?;
    let persisted_message_id = persist_result?;
    if let (Some(episode_message_id), Some(outbound_message_id)) =
        (persisted_message_id, delivery_receipt)
    {
        foreground::update_episode_message_external_message_id(
            pool,
            episode_message_id,
            &outbound_message_id.to_string(),
        )
        .await
        .context("failed to attach delivered failure notice id to episode message")?;
    }
    close_ingress_result?;
    Ok(())
}

async fn persist_foreground_failure_notice(
    pool: &sqlx::PgPool,
    context: &ForegroundFailureContext,
    notice_text: &str,
) -> Result<Option<Uuid>> {
    let Some(episode_id) = context.episode_id else {
        return Ok(None);
    };
    let message_order: i32 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(MAX(message_order), -1) + 1
        FROM episode_messages
        WHERE episode_id = $1
        "#,
    )
    .bind(episode_id)
    .fetch_one(pool)
    .await
    .context("failed to allocate foreground failure notice message order")?;

    let episode_message_id = Uuid::now_v7();
    foreground::insert_episode_message(
        pool,
        &NewEpisodeMessage {
            episode_message_id,
            episode_id,
            trace_id: context.trace_id,
            execution_id: context.execution_id,
            message_order,
            message_role: "assistant".to_string(),
            channel_kind: contracts::ChannelKind::Telegram,
            text_body: Some(notice_text.to_string()),
            external_message_id: None,
        },
    )
    .await
    .context("failed to persist foreground failure notice episode message")?;
    Ok(Some(episode_message_id))
}

async fn mark_terminal_failure_ingress_processed(
    pool: &sqlx::PgPool,
    context: &ForegroundFailureContext,
) -> Result<()> {
    if context.terminal_ingress_ids.is_empty() {
        return Ok(());
    }

    foreground::mark_ingress_events_processed(
        pool,
        &context.terminal_ingress_ids,
        context.execution_id,
    )
    .await
    .context("failed to close terminally failed foreground ingress")
}

async fn launch_leased_conscious_worker<T>(
    pool: &sqlx::PgPool,
    config: &RuntimeConfig,
    model_gateway_config: &ResolvedModelGatewayConfig,
    trigger: &contracts::ForegroundTrigger,
    request: &contracts::WorkerRequest,
    transport: &T,
    timeout_ms: u64,
) -> Result<contracts::WorkerResponse>
where
    T: ModelProviderTransport,
{
    let started_at = Utc::now();
    let timeout_ms_i64 =
        i64::try_from(timeout_ms).context("foreground worker timeout exceeded chrono range")?;
    let worker_lease = recovery::create_worker_lease(
        pool,
        &recovery::NewWorkerLease {
            worker_lease_id: Uuid::now_v7(),
            trace_id: trigger.trace_id,
            execution_id: Some(trigger.execution_id),
            background_job_id: None,
            background_job_run_id: None,
            governed_action_execution_id: None,
            worker_kind: recovery::WorkerLeaseKind::Foreground,
            lease_token: Uuid::now_v7(),
            worker_pid: None,
            lease_acquired_at: started_at,
            lease_expires_at: started_at + Duration::milliseconds(timeout_ms_i64),
            last_heartbeat_at: started_at,
            metadata: json!({
                "source": "foreground_orchestration",
                "trigger_kind": trigger.trigger_kind,
                "ingress_id": trigger.ingress.ingress_id,
            }),
        },
    )
    .await?;

    let response = worker::launch_conscious_worker_with_timeout(
        config,
        model_gateway_config,
        request,
        transport,
        timeout_ms,
    )
    .await;

    match response {
        Ok(response) => {
            recovery::refresh_worker_lease_progress(pool, worker_lease.worker_lease_id, Utc::now())
                .await
                .context("failed to refresh foreground worker lease after worker response")?;
            recovery::release_worker_lease(pool, worker_lease.worker_lease_id, Utc::now())
                .await
                .context("failed to release foreground worker lease after success")?;
            Ok(response)
        }
        Err(error) => {
            let error_message = format_error_chain(&error);
            if error_message.contains("timed out") {
                recovery::recover_observed_worker_timeout(
                    pool,
                    worker_lease.worker_lease_id,
                    Utc::now(),
                    "foreground_worker_timeout",
                    &error_message,
                )
                .await
                .context(format!(
                    "failed to route timed-out foreground worker lease through recovery: {error_message}"
                ))?;
            } else {
                recovery::release_worker_lease(pool, worker_lease.worker_lease_id, Utc::now())
                    .await
                    .context(format!(
                        "failed to release foreground worker lease after worker failure: {error_message}"
                    ))?;
            }
            Err(error)
        }
    }
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

async fn record_deliver_and_return_failure<T, D>(
    pool: &sqlx::PgPool,
    context: ForegroundFailureContext,
    error: Error,
    delivery: &mut D,
    target: ForegroundFailureDeliveryTarget,
) -> Result<T>
where
    D: TelegramDelivery,
{
    let error_message = format_error_chain(&error);
    if let Err(record_error) =
        record_and_deliver_foreground_failure(pool, context, &error_message, delivery, target).await
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
        contracts::ForegroundTriggerKind::UserIngress
        | contracts::ForegroundTriggerKind::ScheduledTask
        | contracts::ForegroundTriggerKind::SupervisorRecoveryEvent => candidate_messages,
        contracts::ForegroundTriggerKind::ApprovedWakeSignal
        | contracts::ForegroundTriggerKind::ApprovalResolutionEvent => Vec::new(),
    }
}

fn episode_trigger_metadata(
    trigger_kind: contracts::ForegroundTriggerKind,
) -> (&'static str, &'static str) {
    match trigger_kind {
        contracts::ForegroundTriggerKind::UserIngress => ("user_ingress", "telegram"),
        contracts::ForegroundTriggerKind::ScheduledTask => {
            ("scheduled_task", "foreground_scheduler")
        }
        contracts::ForegroundTriggerKind::ApprovedWakeSignal => {
            ("approved_wake_signal", "wake_signal")
        }
        contracts::ForegroundTriggerKind::SupervisorRecoveryEvent => {
            ("supervisor_recovery_event", "foreground_recovery")
        }
        contracts::ForegroundTriggerKind::ApprovalResolutionEvent => {
            ("approval_resolution_event", "approval_resolution")
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

async fn deliver_foreground_failure_notice<D>(
    delivery: &mut D,
    chat_id: i64,
    reply_to_message_id: Option<i64>,
    trace_id: Uuid,
    execution_id: Uuid,
    failure_kind: ForegroundFailureKind,
    text: String,
) -> Option<i64>
where
    D: TelegramDelivery,
{
    match delivery
        .send_message(&TelegramOutboundMessage {
            chat_id,
            text,
            reply_to_message_id,
            reply_markup: None,
        })
        .await
    {
        Ok(receipt) => {
            info!(
                trace_id = %trace_id,
                execution_id = %execution_id,
                outbound_message_id = receipt.message_id,
                failure_kind = failure_kind.as_str(),
                "foreground failure notice delivered"
            );
            Some(receipt.message_id)
        }
        Err(error) => {
            warn!(
                trace_id = %trace_id,
                execution_id = %execution_id,
                failure_kind = failure_kind.as_str(),
                error = %format_error_chain(&error),
                "foreground failure notice delivery failed"
            );
            None
        }
    }
}

fn foreground_failure_notice_text(trace_id: Uuid, failure_kind: ForegroundFailureKind) -> String {
    match failure_kind {
        ForegroundFailureKind::MalformedActionProposal => format!(
            "I couldn't complete that because the assistant failed to produce a valid governed-action proposal for the required task. Trace: {trace_id}. Failure kind: {}. Send the request again; if it repeats, inspect the trace with `admin trace explain --trace-id {trace_id}`.",
            failure_kind.as_str()
        ),
        _ => format!(
            "I hit an internal runtime error while processing that message. Trace: {trace_id}. Failure kind: {}. Send another message to continue.",
            failure_kind.as_str()
        ),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ForegroundFailureKind {
    ApprovalResolutionFailure,
    ContextAssemblyFailure,
    MalformedActionProposal,
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
            Self::MalformedActionProposal => "malformed_action_proposal",
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
    if message.contains("worker_error_code=invalid_model_output")
        && (message.contains("invalid governed-action proposal block")
            || message.contains(
                "attempted a governed action without the required governed-action block",
            )
            || message.contains(
                "returned a likely governed-action payload outside the required governed-action block",
            )
            || message.contains(
                "governed-action control block marker was present but the block was malformed or incomplete",
            ))
    {
        return ForegroundFailureKind::MalformedActionProposal;
    }
    if message.contains("invalid governed-action proposal block")
        || message.contains("attempted a governed action without the required governed-action block")
        || message.contains("returned a likely governed-action payload outside the required governed-action block")
        || message.contains("governed-action control block marker was present but the block was malformed or incomplete")
    {
        return ForegroundFailureKind::MalformedActionProposal;
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

fn approval_resolution_message(resolution: &approval::ApprovalResolutionResult) -> String {
    match resolution.event.decision {
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
    }
}

fn approval_follow_up_episode_text(
    observations: &[contracts::GovernedActionObservation],
    model_text: &str,
) -> String {
    let observation_text = if observations.is_empty() {
        "Harness governed-action observation: approved action completed.".to_string()
    } else {
        let joined = observations
            .iter()
            .map(|observation| {
                format!(
                    "{}:{}",
                    governed_action_kind_label(observation.action_kind),
                    observation.outcome.summary
                )
            })
            .collect::<Vec<_>>()
            .join(" | ");
        format!("Harness governed-action observations: {joined}")
    };
    let trimmed_model_text = model_text.trim();
    if trimmed_model_text.is_empty() {
        observation_text
    } else {
        format!("{trimmed_model_text}\n\n{observation_text}")
    }
}

fn approval_follow_up_delivery_text(model_text: &str) -> String {
    let trimmed = model_text.trim();
    if trimmed.is_empty() {
        "Approved action completed.".to_string()
    } else {
        trimmed.to_string()
    }
}

fn foreground_candidate_proposals(
    context: &contracts::ConsciousContext,
    trigger: &contracts::ForegroundTrigger,
    worker_proposals: &[contracts::CanonicalProposal],
) -> Vec<contracts::CanonicalProposal> {
    let mut proposals = worker_proposals.to_vec();
    proposals.extend(inferred_identity_kickstart_proposals(
        context,
        trigger,
        worker_proposals,
    ));
    proposals
}

fn foreground_assistant_delivery_text(
    model_text: &str,
    governed_action_summary: &GovernedActionProcessingSummary,
    candidate_proposals: &[contracts::CanonicalProposal],
    context: &contracts::ConsciousContext,
) -> String {
    let trimmed = model_text.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }

    if governed_action_summary.pending_approval_count > 0 {
        return if governed_action_summary.pending_approval_count == 1 {
            "Approval requested. Use the approval prompt above to continue.".to_string()
        } else {
            format!(
                "{} approvals requested. Use the approval prompts above to continue.",
                governed_action_summary.pending_approval_count
            )
        };
    }

    if let Some(blocked_observation) = governed_action_summary
        .observations
        .iter()
        .rev()
        .find(|observation| observation.outcome.status == contracts::GovernedActionStatus::Blocked)
    {
        return blocked_observation.outcome.summary.clone();
    }

    if let Some(identity_fallback) =
        identity_kickstart_delivery_fallback(candidate_proposals, context)
    {
        return identity_fallback;
    }

    "No assistant response was generated.".to_string()
}

fn identity_kickstart_delivery_fallback(
    candidate_proposals: &[contracts::CanonicalProposal],
    context: &contracts::ConsciousContext,
) -> Option<String> {
    candidate_proposals.iter().find_map(|proposal| {
        let contracts::CanonicalProposalPayload::IdentityDelta(payload) = &proposal.payload else {
            return None;
        };

        match payload.interview_action.as_ref() {
            Some(contracts::IdentityKickstartAction::StartCustomInterview) => Some(format!(
                "Starting the custom identity interview. {}",
                identity::custom_identity_step_user_prompt("name")
            )),
            Some(contracts::IdentityKickstartAction::AnswerCustomInterview(_)) => {
                Some(identity_answer_delivery_fallback(context, payload))
            }
            Some(contracts::IdentityKickstartAction::SelectPredefinedTemplate { .. }) => {
                Some("Identity template selected.".to_string())
            }
            Some(contracts::IdentityKickstartAction::Cancel { .. }) => {
                Some("Identity kickstart cancelled.".to_string())
            }
            None if payload.lifecycle_state
                == contracts::IdentityLifecycleState::CompleteIdentityActive =>
            {
                Some("Identity update prepared.".to_string())
            }
            None => None,
        }
    })
}

fn identity_answer_delivery_fallback(
    context: &contracts::ConsciousContext,
    payload: &contracts::IdentityDeltaProposal,
) -> String {
    if payload.lifecycle_state == contracts::IdentityLifecycleState::CompleteIdentityActive {
        return "Identity interview completed.".to_string();
    }

    let Some(current_step) = context
        .self_model
        .identity_lifecycle
        .kickstart
        .as_ref()
        .and_then(|kickstart| kickstart.next_step.as_deref())
    else {
        return "Saved that identity interview answer. Continue with the next identity detail when ready.".to_string();
    };

    let Some(next_step) = next_custom_identity_step(current_step) else {
        return "Identity interview completed.".to_string();
    };

    format!(
        "Saved that identity interview answer. {}",
        identity::custom_identity_step_user_prompt(next_step)
    )
}

fn inferred_identity_kickstart_proposals(
    context: &contracts::ConsciousContext,
    trigger: &contracts::ForegroundTrigger,
    existing_proposals: &[contracts::CanonicalProposal],
) -> Vec<contracts::CanonicalProposal> {
    if !context.self_model.identity_lifecycle.kickstart_available
        || existing_proposals.iter().any(|proposal| {
            matches!(
                proposal.payload,
                contracts::CanonicalProposalPayload::IdentityDelta(_)
            )
        })
    {
        return Vec::new();
    }

    let Some(user_text) = trigger.ingress.text_body.as_deref().map(str::trim) else {
        return Vec::new();
    };
    if user_text.is_empty() {
        return Vec::new();
    }

    match context.self_model.identity_lifecycle.state {
        contracts::IdentityLifecycleState::BootstrapSeedOnly => {
            infer_bootstrap_identity_proposal(context, trigger, user_text)
                .into_iter()
                .collect()
        }
        contracts::IdentityLifecycleState::IdentityKickstartInProgress => {
            infer_in_progress_identity_proposal(context, trigger, user_text)
                .into_iter()
                .collect()
        }
        contracts::IdentityLifecycleState::CompleteIdentityActive
        | contracts::IdentityLifecycleState::IdentityResetPending => Vec::new(),
    }
}

fn infer_bootstrap_identity_proposal(
    context: &contracts::ConsciousContext,
    trigger: &contracts::ForegroundTrigger,
    user_text: &str,
) -> Option<contracts::CanonicalProposal> {
    let normalized = normalize_identity_intent_text(user_text);
    if is_custom_identity_start_intent(&normalized) {
        return Some(identity_interview_action_proposal(
            context,
            trigger,
            contracts::IdentityKickstartAction::StartCustomInterview,
            contracts::IdentityLifecycleState::IdentityKickstartInProgress,
            "User started a custom identity interview.",
        ));
    }

    let template_key = predefined_identity_template_intent(&normalized)?;
    let payload = contracts::predefined_identity_delta(&template_key, trigger.ingress.occurred_at)?;
    Some(identity_delta_proposal(
        context,
        trigger,
        payload,
        format!("User selected predefined identity template '{template_key}'."),
    ))
}

fn infer_in_progress_identity_proposal(
    context: &contracts::ConsciousContext,
    trigger: &contracts::ForegroundTrigger,
    user_text: &str,
) -> Option<contracts::CanonicalProposal> {
    let normalized = normalize_identity_intent_text(user_text);
    if is_identity_cancel_intent(&normalized) {
        return Some(identity_interview_action_proposal(
            context,
            trigger,
            contracts::IdentityKickstartAction::Cancel {
                reason: Some(user_text.to_string()),
            },
            contracts::IdentityLifecycleState::BootstrapSeedOnly,
            "User cancelled identity formation.",
        ));
    }
    if is_ambiguous_identity_answer(&normalized) {
        return None;
    }
    let step_key = context
        .self_model
        .identity_lifecycle
        .kickstart
        .as_ref()
        .and_then(|kickstart| kickstart.next_step.as_deref())?;
    if !CUSTOM_IDENTITY_STEPS.contains(&step_key) {
        return None;
    }
    Some(identity_interview_action_proposal(
        context,
        trigger,
        contracts::IdentityKickstartAction::AnswerCustomInterview(
            contracts::IdentityInterviewAnswer {
                step_key: step_key.to_string(),
                answer_text: user_text.to_string(),
            },
        ),
        contracts::IdentityLifecycleState::IdentityKickstartInProgress,
        "User answered a custom identity interview step.",
    ))
}

fn identity_interview_action_proposal(
    context: &contracts::ConsciousContext,
    trigger: &contracts::ForegroundTrigger,
    action: contracts::IdentityKickstartAction,
    lifecycle_state: contracts::IdentityLifecycleState,
    rationale: &str,
) -> contracts::CanonicalProposal {
    identity_delta_proposal(
        context,
        trigger,
        contracts::IdentityDeltaProposal {
            lifecycle_state,
            item_deltas: Vec::new(),
            self_description_delta: None,
            interview_action: Some(action),
            rationale: rationale.to_string(),
        },
        rationale.to_string(),
    )
}

fn identity_delta_proposal(
    _context: &contracts::ConsciousContext,
    trigger: &contracts::ForegroundTrigger,
    payload: contracts::IdentityDeltaProposal,
    rationale: String,
) -> contracts::CanonicalProposal {
    contracts::CanonicalProposal {
        proposal_id: Uuid::now_v7(),
        proposal_kind: contracts::CanonicalProposalKind::IdentityDelta,
        canonical_target: contracts::CanonicalTargetKind::IdentityItems,
        confidence_pct: 100,
        conflict_posture: contracts::ProposalConflictPosture::Independent,
        subject_ref: "self:blue-lagoon".to_string(),
        rationale: Some(rationale),
        valid_from: Some(trigger.ingress.occurred_at),
        valid_to: None,
        supersedes_artifact_id: None,
        provenance: contracts::ProposalProvenance {
            provenance_kind: contracts::ProposalProvenanceKind::EpisodeObservation,
            source_ingress_ids: vec![trigger.ingress.ingress_id],
            source_episode_id: None,
        },
        payload: contracts::CanonicalProposalPayload::IdentityDelta(payload),
    }
}

const CUSTOM_IDENTITY_STEPS: &[&str] = &[
    "name",
    "identity_form",
    "archetype_role",
    "temperament",
    "communication_style",
    "backstory",
    "age_framing",
    "likes",
    "dislikes",
    "values",
    "boundaries",
    "tendencies",
    "goals",
    "relationship_to_user",
];

fn next_custom_identity_step(current_step: &str) -> Option<&'static str> {
    let current_index = CUSTOM_IDENTITY_STEPS
        .iter()
        .position(|step| *step == current_step)?;
    CUSTOM_IDENTITY_STEPS.get(current_index + 1).copied()
}

fn normalize_identity_intent_text(text: &str) -> String {
    text.to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character.is_ascii_whitespace() {
                character
            } else {
                ' '
            }
        })
        .collect::<String>()
}

fn is_custom_identity_start_intent(normalized: &str) -> bool {
    let has_custom = normalized.contains("custom")
        || normalized.contains("scratch")
        || normalized.contains("from zero")
        || normalized.contains("from the ground");
    let has_identity = normalized.contains("identity")
        || normalized.contains("one")
        || normalized.contains("path")
        || normalized.contains("create")
        || normalized.contains("build");
    has_custom && has_identity
}

fn predefined_identity_template_intent(normalized: &str) -> Option<String> {
    let trimmed = normalized.trim();
    if matches!(trimmed, "1" | "one" | "option 1" | "first") {
        return Some("continuity_operator".to_string());
    }
    if matches!(trimmed, "2" | "two" | "option 2" | "second") {
        return Some("reflective_companion".to_string());
    }
    if matches!(trimmed, "3" | "three" | "option 3" | "third") {
        return Some("pragmatic_copilot".to_string());
    }

    let options: [(&str, &[&str]); 3] = [
        ("continuity_operator", &["continuity operator"]),
        ("reflective_companion", &["reflective companion"]),
        (
            "pragmatic_copilot",
            &["pragmatic copilot", "pragmatic co pilot"],
        ),
    ];
    options
        .iter()
        .find(|(_, needles)| needles.iter().any(|needle| normalized.contains(needle)))
        .map(|(template_key, _)| (*template_key).to_string())
}

fn is_identity_cancel_intent(normalized: &str) -> bool {
    if normalized.contains("never mind") {
        return true;
    }
    normalized
        .split_whitespace()
        .any(|word| matches!(word, "cancel" | "stop" | "abort" | "nevermind" | "quit"))
}

fn is_ambiguous_identity_answer(normalized: &str) -> bool {
    let trimmed = normalized.trim();
    trimmed.is_empty()
        || matches!(
            trimmed,
            "ok" | "okay" | "hello" | "hi" | "hey" | "hmm" | "yes" | "no"
        )
}

async fn emit_typing_chat_action<D>(
    delivery: &mut D,
    chat_id: i64,
    trace_id: Uuid,
    execution_id: Uuid,
    phase: &'static str,
) where
    D: TelegramDelivery,
{
    match delivery
        .send_chat_action(chat_id, TelegramChatAction::Typing)
        .await
    {
        Ok(()) => {
            info!(
                trace_id = %trace_id,
                execution_id = %execution_id,
                chat_id,
                phase,
                "telegram typing chat action sent"
            );
        }
        Err(error) => {
            warn!(
                trace_id = %trace_id,
                execution_id = %execution_id,
                chat_id,
                phase,
                error = %format_error_chain(&error),
                "telegram typing chat action failed"
            );
        }
    }
}

fn governed_action_kind_label(kind: contracts::GovernedActionKind) -> &'static str {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_follow_up_delivery_uses_model_text_only() {
        let delivered = approval_follow_up_delivery_text(
            "  The fetch completed and the current rate is available.  ",
        );

        assert_eq!(
            delivered,
            "The fetch completed and the current rate is available."
        );
    }

    #[test]
    fn approval_follow_up_episode_text_keeps_model_text_first_for_history() {
        let observation = contracts::GovernedActionObservation {
            observation_id: Uuid::now_v7(),
            action_kind: contracts::GovernedActionKind::WebFetch,
            outcome: contracts::GovernedActionExecutionOutcome {
                status: contracts::GovernedActionStatus::Executed,
                summary: "web fetch completed for https://example.com/; preview: very long"
                    .to_string(),
                fingerprint: None,
                output_ref: None,
            },
        };

        let stored = approval_follow_up_episode_text(
            &[observation],
            "I found the page. Let me summarize it.",
        );

        assert!(stored.starts_with("I found the page. Let me summarize it."));
        assert!(stored.contains("Harness governed-action observations: web_fetch:"));
    }

    #[test]
    fn approval_follow_up_delivery_falls_back_when_model_text_is_empty() {
        assert_eq!(
            approval_follow_up_delivery_text(" \n\t "),
            "Approved action completed."
        );
    }

    #[test]
    fn foreground_assistant_delivery_uses_model_text_when_present() {
        let summary = GovernedActionProcessingSummary {
            pending_approval_count: 1,
            ..GovernedActionProcessingSummary::default()
        };
        let context = test_conscious_context(
            contracts::IdentityLifecycleState::BootstrapSeedOnly,
            Some("choose_predefined_identity_or_start_custom_interview"),
            "waiting",
        );

        assert_eq!(
            foreground_assistant_delivery_text("  Waiting on approval.  ", &summary, &[], &context),
            "Waiting on approval."
        );
    }

    #[test]
    fn foreground_assistant_delivery_falls_back_for_single_pending_approval() {
        let summary = GovernedActionProcessingSummary {
            pending_approval_count: 1,
            ..GovernedActionProcessingSummary::default()
        };
        let context = test_conscious_context(
            contracts::IdentityLifecycleState::BootstrapSeedOnly,
            Some("choose_predefined_identity_or_start_custom_interview"),
            "waiting",
        );

        assert_eq!(
            foreground_assistant_delivery_text("", &summary, &[], &context),
            "Approval requested. Use the approval prompt above to continue."
        );
    }

    #[test]
    fn foreground_assistant_delivery_falls_back_for_multiple_pending_approvals() {
        let summary = GovernedActionProcessingSummary {
            pending_approval_count: 2,
            ..GovernedActionProcessingSummary::default()
        };
        let context = test_conscious_context(
            contracts::IdentityLifecycleState::BootstrapSeedOnly,
            Some("choose_predefined_identity_or_start_custom_interview"),
            "waiting",
        );

        assert_eq!(
            foreground_assistant_delivery_text(" \n", &summary, &[], &context),
            "2 approvals requested. Use the approval prompts above to continue."
        );
    }

    #[test]
    fn foreground_assistant_delivery_falls_back_for_identity_kickstart_control_only() {
        let summary = GovernedActionProcessingSummary::default();
        let proposal = identity_action_proposal(
            contracts::IdentityKickstartAction::StartCustomInterview,
            contracts::IdentityLifecycleState::IdentityKickstartInProgress,
        );
        let context = test_conscious_context(
            contracts::IdentityLifecycleState::BootstrapSeedOnly,
            Some("choose_predefined_identity_or_start_custom_interview"),
            "let's create a custom one",
        );

        assert_eq!(
            foreground_assistant_delivery_text(" \n", &summary, &[proposal], &context),
            "Starting the custom identity interview. What name should this assistant identity use?"
        );
    }

    #[test]
    fn foreground_candidate_proposals_infers_custom_identity_start_when_worker_omits_block() {
        let context = test_conscious_context(
            contracts::IdentityLifecycleState::BootstrapSeedOnly,
            Some("choose_predefined_identity_or_start_custom_interview"),
            "let's create a custom one",
        );

        let proposals = foreground_candidate_proposals(&context, &context.trigger, &[]);

        assert_eq!(proposals.len(), 1);
        let contracts::CanonicalProposalPayload::IdentityDelta(delta) = &proposals[0].payload
        else {
            panic!("expected identity delta");
        };
        assert_eq!(
            delta.interview_action,
            Some(contracts::IdentityKickstartAction::StartCustomInterview)
        );
    }

    #[test]
    fn foreground_candidate_proposals_ignores_ambiguous_identity_answer() {
        let context = test_conscious_context(
            contracts::IdentityLifecycleState::IdentityKickstartInProgress,
            Some("name"),
            "ok",
        );

        let proposals = foreground_candidate_proposals(&context, &context.trigger, &[]);

        assert!(proposals.is_empty());
    }

    #[test]
    fn foreground_candidate_proposals_infers_identity_interview_answer() {
        let context = test_conscious_context(
            contracts::IdentityLifecycleState::IdentityKickstartInProgress,
            Some("name"),
            "Lagoon Forge",
        );

        let proposals = foreground_candidate_proposals(&context, &context.trigger, &[]);

        assert_eq!(proposals.len(), 1);
        let contracts::CanonicalProposalPayload::IdentityDelta(delta) = &proposals[0].payload
        else {
            panic!("expected identity delta");
        };
        assert_eq!(
            delta.interview_action,
            Some(contracts::IdentityKickstartAction::AnswerCustomInterview(
                contracts::IdentityInterviewAnswer {
                    step_key: "name".to_string(),
                    answer_text: "Lagoon Forge".to_string(),
                },
            ))
        );
        assert_eq!(
            foreground_assistant_delivery_text(
                " \n",
                &GovernedActionProcessingSummary::default(),
                &proposals,
                &context
            ),
            "Saved that identity interview answer. What kind of identity form should this assistant have?"
        );
    }

    #[test]
    fn foreground_failure_notice_includes_trace_and_kind_without_internal_error() {
        let trace_id = Uuid::now_v7();
        let notice =
            foreground_failure_notice_text(trace_id, ForegroundFailureKind::ContextAssemblyFailure);

        assert!(notice.contains(&trace_id.to_string()));
        assert!(notice.contains("context_assembly_failure"));
        assert!(!notice.contains("missing foreground self-model seed configuration"));
    }

    #[test]
    fn foreground_failure_notice_explains_malformed_action_proposal() {
        let trace_id = Uuid::now_v7();
        let notice = foreground_failure_notice_text(
            trace_id,
            ForegroundFailureKind::MalformedActionProposal,
        );

        assert!(notice.contains("valid governed-action proposal"));
        assert!(notice.contains("malformed_action_proposal"));
        assert!(notice.contains("admin trace explain --trace-id"));
        assert!(notice.contains(&trace_id.to_string()));
    }

    #[test]
    fn classify_conscious_worker_failure_detects_malformed_action_proposal() {
        let error = anyhow::anyhow!(
            "conscious worker returned an error response: model attempted a governed action without the required governed-action block; returned bare action token 'list_workspace_artifacts'"
        );

        assert_eq!(
            classify_conscious_worker_failure(&error),
            ForegroundFailureKind::MalformedActionProposal
        );
    }

    fn identity_action_proposal(
        action: contracts::IdentityKickstartAction,
        lifecycle_state: contracts::IdentityLifecycleState,
    ) -> contracts::CanonicalProposal {
        contracts::CanonicalProposal {
            proposal_id: Uuid::now_v7(),
            proposal_kind: contracts::CanonicalProposalKind::IdentityDelta,
            canonical_target: contracts::CanonicalTargetKind::IdentityItems,
            confidence_pct: 100,
            conflict_posture: contracts::ProposalConflictPosture::Independent,
            subject_ref: "self:blue-lagoon".to_string(),
            rationale: Some("Identity kickstart action.".to_string()),
            valid_from: Some(Utc::now()),
            valid_to: None,
            supersedes_artifact_id: None,
            provenance: contracts::ProposalProvenance {
                provenance_kind: contracts::ProposalProvenanceKind::EpisodeObservation,
                source_ingress_ids: vec![Uuid::now_v7()],
                source_episode_id: Some(Uuid::now_v7()),
            },
            payload: contracts::CanonicalProposalPayload::IdentityDelta(
                contracts::IdentityDeltaProposal {
                    lifecycle_state,
                    item_deltas: Vec::new(),
                    self_description_delta: None,
                    interview_action: Some(action),
                    rationale: "Identity kickstart action.".to_string(),
                },
            ),
        }
    }

    fn test_conscious_context(
        state: contracts::IdentityLifecycleState,
        next_step: Option<&str>,
        user_text: &str,
    ) -> contracts::ConsciousContext {
        let trigger = test_foreground_trigger(user_text);
        contracts::ConsciousContext {
            context_id: Uuid::now_v7(),
            assembled_at: Utc::now(),
            trigger,
            self_model: contracts::SelfModelSnapshot {
                stable_identity: "blue-lagoon".to_string(),
                role: "Personal AI assistant".to_string(),
                communication_style: "Direct".to_string(),
                capabilities: Vec::new(),
                constraints: Vec::new(),
                preferences: Vec::new(),
                current_goals: Vec::new(),
                current_subgoals: Vec::new(),
                identity: None,
                identity_lifecycle: contracts::IdentityLifecycleContext {
                    state,
                    kickstart_available: true,
                    kickstart: Some(contracts::IdentityKickstartContext {
                        available_actions: vec![
                            contracts::IdentityKickstartActionKind::SelectPredefinedTemplate,
                            contracts::IdentityKickstartActionKind::StartCustomInterview,
                            contracts::IdentityKickstartActionKind::AnswerCustomInterview,
                            contracts::IdentityKickstartActionKind::Cancel,
                        ],
                        next_step: next_step.map(str::to_string),
                        resume_summary: None,
                        predefined_templates: contracts::predefined_identity_templates(),
                    }),
                },
            },
            internal_state: contracts::InternalStateSnapshot {
                load_pct: 0,
                health_pct: 100,
                reliability_pct: 100,
                resource_pressure_pct: 0,
                confidence_pct: 100,
                connection_quality_pct: 100,
                active_conditions: Vec::new(),
            },
            recent_history: Vec::new(),
            retrieved_context: contracts::RetrievedContext::default(),
            governed_action_observations: Vec::new(),
            governed_action_loop_state: None,
            recovery_context: contracts::ForegroundRecoveryContext::default(),
        }
    }

    fn test_foreground_trigger(user_text: &str) -> contracts::ForegroundTrigger {
        let ingress_id = Uuid::now_v7();
        contracts::ForegroundTrigger {
            trigger_id: Uuid::now_v7(),
            trace_id: Uuid::now_v7(),
            execution_id: Uuid::now_v7(),
            trigger_kind: contracts::ForegroundTriggerKind::UserIngress,
            ingress: contracts::NormalizedIngress {
                ingress_id,
                channel_kind: contracts::ChannelKind::Telegram,
                external_user_id: "42".to_string(),
                external_conversation_id: "42".to_string(),
                external_event_id: format!("event-{ingress_id}"),
                external_message_id: Some(format!("message-{ingress_id}")),
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
                event_kind: contracts::IngressEventKind::MessageCreated,
                occurred_at: Utc::now(),
                text_body: Some(user_text.to_string()),
                reply_to: None,
                attachments: Vec::new(),
                command_hint: None,
                approval_payload: None,
                raw_payload_ref: None,
            },
            received_at: Utc::now(),
            deduplication_key: format!("test:{ingress_id}"),
            budget: contracts::ForegroundBudget {
                iteration_budget: 1,
                wall_clock_budget_ms: 30_000,
                token_budget: 4_000,
            },
        }
    }
}
