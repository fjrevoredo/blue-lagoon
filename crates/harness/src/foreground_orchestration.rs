use anyhow::{Context, Error, Result, bail};
use serde_json::json;
use uuid::Uuid;

use crate::{
    audit::{self, NewAuditEvent},
    config::{ResolvedModelGatewayConfig, ResolvedTelegramConfig, RuntimeConfig},
    context, execution,
    foreground::{self, ForegroundTriggerIntakeOutcome, NewEpisode, NewEpisodeMessage},
    model_gateway::ModelProviderTransport,
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
            error,
        )
        .await;
    }
    recorded_episode_id = Some(episode_id);

    if let Some(text_body) = &trigger.ingress.text_body {
        if let Err(error) = foreground::insert_episode_message(
            pool,
            &NewEpisodeMessage {
                episode_message_id: Uuid::now_v7(),
                episode_id,
                trace_id: trigger.trace_id,
                execution_id: trigger.execution_id,
                message_order: 0,
                message_role: "user".to_string(),
                channel_kind: trigger.ingress.channel_kind,
                text_body: Some(text_body.clone()),
                external_message_id: trigger.ingress.external_message_id.clone(),
            },
        )
        .await
        {
            return record_and_return_failure(
                pool,
                trigger.trace_id,
                trigger.execution_id,
                recorded_episode_id,
                error,
            )
            .await;
        }
    }

    let assembly = match context::assemble_foreground_context(
        pool,
        config,
        trigger.clone(),
        context::ContextAssemblyOptions::default(),
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
            error,
        )
        .await;
    }

    let request = contracts::WorkerRequest::conscious(
        trigger.trace_id,
        trigger.execution_id,
        assembly.context,
    );
    let response =
        match worker::launch_conscious_worker(config, model_gateway_config, &request, transport)
            .await
        {
            Ok(response) => response,
            Err(error) => {
                record_foreground_failure(
                    pool,
                    trigger.trace_id,
                    trigger.execution_id,
                    recorded_episode_id,
                    &error.to_string(),
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
            message_order: 1,
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
                error,
            )
            .await;
        }
    };

    let delivery_receipt = match delivery.send_message(&TelegramOutboundMessage {
        chat_id,
        text: result.assistant_output.text.clone(),
        reply_to_message_id,
    }) {
        Ok(receipt) => receipt,
        Err(error) => {
            record_foreground_failure(
                pool,
                trigger.trace_id,
                trigger.execution_id,
                recorded_episode_id,
                &error.to_string(),
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
            error,
        )
        .await;
    }

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
            error,
        )
        .await;
    }
    if let Err(error) = foreground::mark_episode_completed(
        pool,
        episode_id,
        &result.episode_summary.outcome,
        &result.episode_summary.summary,
    )
    .await
    {
        return record_and_return_failure(
            pool,
            trigger.trace_id,
            trigger.execution_id,
            recorded_episode_id,
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
                "outbound_message_id": delivery_receipt.message_id,
                "assistant_summary": result.episode_summary.summary,
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
    error_message: &str,
) -> Result<()> {
    execution::mark_failed(
        pool,
        execution_id,
        &json!({
            "kind": "worker_failure",
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
    error: Error,
) -> Result<T> {
    if let Err(record_error) =
        record_foreground_failure(pool, trace_id, execution_id, episode_id, &error.to_string())
            .await
    {
        return Err(error.context(format!(
            "failed to record foreground execution failure: {record_error}"
        )));
    }

    Err(error)
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
