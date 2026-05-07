use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use contracts::{
    LoopKind, ModelCallPurpose, ModelCallRequest, ModelCallResponse, ModelProviderKind,
};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::{
    causal_links::{self, NewCausalLink},
    config::ResolvedModelGatewayConfig,
};

#[derive(Debug, Clone)]
pub struct ModelCallRecord {
    pub model_call_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub loop_kind: String,
    pub purpose: String,
    pub task_class: Option<String>,
    pub provider: String,
    pub model: String,
    pub request_payload_json: Option<Value>,
    pub response_payload_json: Option<Value>,
    pub system_prompt_text: Option<String>,
    pub messages_json: Option<Value>,
    pub input_tokens: Option<i32>,
    pub output_tokens: Option<i32>,
    pub finish_reason: Option<String>,
    pub status: String,
    pub error_summary: Option<String>,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub payload_retention_expires_at: Option<DateTime<Utc>>,
    pub payload_cleared_at: Option<DateTime<Utc>>,
    pub payload_retention_reason: Option<String>,
}

pub async fn insert_pending_model_call_record(
    pool: &PgPool,
    gateway: &ResolvedModelGatewayConfig,
    request: &ModelCallRequest,
    started_at: DateTime<Utc>,
    payload_retention_days: u32,
) -> Result<Uuid> {
    let model_call_id = Uuid::now_v7();
    let request_payload = serde_json::to_value(request)
        .context("failed to serialize model call request for persistence")?;
    let messages_json = serde_json::to_value(&request.input.messages)
        .context("failed to serialize model input messages for persistence")?;
    let payload_retention_expires_at =
        started_at + Duration::days(i64::from(payload_retention_days));

    sqlx::query(
        r#"
        INSERT INTO model_call_records (
            model_call_id,
            trace_id,
            execution_id,
            loop_kind,
            purpose,
            task_class,
            provider,
            model,
            request_payload_json,
            system_prompt_text,
            messages_json,
            status,
            started_at,
            payload_retention_expires_at
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8,
            $9, $10, $11, 'pending', $12, $13
        )
        "#,
    )
    .bind(model_call_id)
    .bind(request.trace_id)
    .bind(request.execution_id)
    .bind(loop_kind_label(request.loop_kind))
    .bind(model_call_purpose_label(request.purpose))
    .bind(&request.task_class)
    .bind(model_provider_label(gateway.foreground.provider))
    .bind(&gateway.foreground.model)
    .bind(request_payload)
    .bind(&request.input.system_prompt)
    .bind(messages_json)
    .bind(started_at)
    .bind(payload_retention_expires_at)
    .execute(pool)
    .await
    .context("failed to insert pending model call record")?;

    causal_links::insert(
        pool,
        &NewCausalLink {
            trace_id: request.trace_id,
            source_kind: "execution_record".to_string(),
            source_id: request.execution_id,
            target_kind: "model_call_record".to_string(),
            target_id: model_call_id,
            edge_kind: "invoked_model".to_string(),
            payload: model_call_request_summary(request),
        },
    )
    .await?;
    if let Some(background_job_run_id) =
        background_job_run_for_execution(pool, request.execution_id).await?
    {
        causal_links::insert(
            pool,
            &NewCausalLink {
                trace_id: request.trace_id,
                source_kind: "background_job_run".to_string(),
                source_id: background_job_run_id,
                target_kind: "model_call_record".to_string(),
                target_id: model_call_id,
                edge_kind: "invoked_model".to_string(),
                payload: model_call_request_summary(request),
            },
        )
        .await?;
    }

    Ok(model_call_id)
}

pub async fn mark_model_call_succeeded(
    pool: &PgPool,
    model_call_id: Uuid,
    response: &ModelCallResponse,
    completed_at: DateTime<Utc>,
) -> Result<()> {
    let response_payload = serde_json::to_value(response)
        .context("failed to serialize model call response for persistence")?;
    sqlx::query(
        r#"
        UPDATE model_call_records
        SET
            response_payload_json = $2,
            input_tokens = $3,
            output_tokens = $4,
            finish_reason = $5,
            status = 'succeeded',
            completed_at = $6,
            updated_at = NOW()
        WHERE model_call_id = $1
        "#,
    )
    .bind(model_call_id)
    .bind(response_payload)
    .bind(response.usage.input_tokens as i32)
    .bind(response.usage.output_tokens as i32)
    .bind(&response.output.finish_reason)
    .bind(completed_at)
    .execute(pool)
    .await
    .context("failed to mark model call record succeeded")?;
    Ok(())
}

pub async fn mark_model_call_failed(
    pool: &PgPool,
    model_call_id: Uuid,
    error_summary: &str,
    response_payload_json: Option<&Value>,
    completed_at: DateTime<Utc>,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE model_call_records
        SET
            status = 'failed',
            error_summary = $2,
            response_payload_json = $3,
            completed_at = $4,
            updated_at = NOW()
        WHERE model_call_id = $1
        "#,
    )
    .bind(model_call_id)
    .bind(error_summary)
    .bind(response_payload_json)
    .bind(completed_at)
    .execute(pool)
    .await
    .context("failed to mark model call record failed")?;
    Ok(())
}

pub async fn list_model_call_records_for_trace(
    pool: &PgPool,
    trace_id: Uuid,
) -> Result<Vec<ModelCallRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT
            model_call_id,
            trace_id,
            execution_id,
            loop_kind,
            purpose,
            task_class,
            provider,
            model,
            request_payload_json,
            response_payload_json,
            system_prompt_text,
            messages_json,
            input_tokens,
            output_tokens,
            finish_reason,
            status,
            error_summary,
            started_at,
            completed_at,
            payload_retention_expires_at,
            payload_cleared_at,
            payload_retention_reason
        FROM model_call_records
        WHERE trace_id = $1
        ORDER BY started_at ASC, model_call_id ASC
        "#,
    )
    .bind(trace_id)
    .fetch_all(pool)
    .await
    .context("failed to list model call records for trace")?;

    Ok(rows
        .into_iter()
        .map(|row| ModelCallRecord {
            model_call_id: row.get("model_call_id"),
            trace_id: row.get("trace_id"),
            execution_id: row.get("execution_id"),
            loop_kind: row.get("loop_kind"),
            purpose: row.get("purpose"),
            task_class: row.get("task_class"),
            provider: row.get("provider"),
            model: row.get("model"),
            request_payload_json: row.get("request_payload_json"),
            response_payload_json: row.get("response_payload_json"),
            system_prompt_text: row.get("system_prompt_text"),
            messages_json: row.get("messages_json"),
            input_tokens: row.get("input_tokens"),
            output_tokens: row.get("output_tokens"),
            finish_reason: row.get("finish_reason"),
            status: row.get("status"),
            error_summary: row.get("error_summary"),
            started_at: row.get("started_at"),
            completed_at: row.get("completed_at"),
            payload_retention_expires_at: row.get("payload_retention_expires_at"),
            payload_cleared_at: row.get("payload_cleared_at"),
            payload_retention_reason: row.get("payload_retention_reason"),
        })
        .collect())
}

pub async fn clear_expired_model_call_payloads(pool: &PgPool, now: DateTime<Utc>) -> Result<u64> {
    let result = sqlx::query(
        r#"
        UPDATE model_call_records
        SET
            request_payload_json = NULL,
            response_payload_json = NULL,
            system_prompt_text = NULL,
            messages_json = NULL,
            payload_cleared_at = $1,
            payload_retention_reason = 'retention_expired',
            updated_at = NOW()
        WHERE payload_retention_expires_at IS NOT NULL
          AND payload_retention_expires_at <= $1
          AND payload_cleared_at IS NULL
        "#,
    )
    .bind(now)
    .execute(pool)
    .await
    .context("failed to clear expired model call payloads")?;

    Ok(result.rows_affected())
}

pub fn model_call_request_summary(request: &ModelCallRequest) -> Value {
    json!({
        "request_id": request.request_id,
        "trace_id": request.trace_id,
        "execution_id": request.execution_id,
        "loop_kind": loop_kind_label(request.loop_kind),
        "purpose": model_call_purpose_label(request.purpose),
        "task_class": request.task_class,
        "budget": request.budget,
        "output_mode": request.output_mode,
        "schema_name": request.schema_name,
        "tool_policy": request.tool_policy,
        "provider_hint": request.provider_hint,
    })
}

pub fn is_missing_model_call_schema(error: &anyhow::Error) -> bool {
    error
        .chain()
        .filter_map(|cause| cause.downcast_ref::<sqlx::Error>())
        .any(|sqlx_error| {
            matches!(
                sqlx_error,
                sqlx::Error::Database(database_error)
                    if database_error.code().as_deref() == Some("42P01")
            )
        })
}

async fn background_job_run_for_execution(
    pool: &PgPool,
    execution_id: Uuid,
) -> Result<Option<Uuid>> {
    sqlx::query_scalar(
        r#"
        SELECT background_job_run_id
        FROM background_job_runs
        WHERE execution_id = $1
        ORDER BY created_at DESC, background_job_run_id DESC
        LIMIT 1
        "#,
    )
    .bind(execution_id)
    .fetch_optional(pool)
    .await
    .context("failed to load background job run for model call causal link")
}

fn loop_kind_label(loop_kind: LoopKind) -> &'static str {
    match loop_kind {
        LoopKind::Conscious => "foreground",
        LoopKind::Unconscious => "background",
    }
}

fn model_call_purpose_label(purpose: ModelCallPurpose) -> &'static str {
    match purpose {
        ModelCallPurpose::ForegroundResponse => "foreground_response",
        ModelCallPurpose::BackgroundAnalysis => "background_analysis",
    }
}

fn model_provider_label(provider: ModelProviderKind) -> &'static str {
    match provider {
        ModelProviderKind::ZAi => "z_ai",
        ModelProviderKind::OpenRouter => "openrouter",
    }
}
