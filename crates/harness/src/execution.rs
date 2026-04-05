use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{Executor, PgPool, Postgres, Row};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct NewExecutionRecord {
    pub execution_id: Uuid,
    pub trace_id: Uuid,
    pub trigger_kind: String,
    pub synthetic_trigger: Option<String>,
    pub status: String,
    pub request_payload: Value,
}

#[derive(Debug, Clone)]
pub struct ExecutionRecord {
    pub execution_id: Uuid,
    pub trace_id: Uuid,
    pub status: String,
    pub worker_pid: Option<i32>,
    pub response_payload: Option<Value>,
    pub completed_at: Option<DateTime<Utc>>,
}

pub async fn insert<'e, E>(executor: E, record: &NewExecutionRecord) -> Result<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query(
        r#"
        INSERT INTO execution_records (
            execution_id,
            trace_id,
            trigger_kind,
            synthetic_trigger,
            status,
            worker_kind,
            worker_pid,
            request_payload,
            response_payload,
            created_at,
            updated_at,
            completed_at
        ) VALUES (
            $1,
            $2,
            $3,
            $4,
            $5,
            NULL,
            NULL,
            $6,
            NULL,
            NOW(),
            NOW(),
            NULL
        )
        "#,
    )
    .bind(record.execution_id)
    .bind(record.trace_id)
    .bind(&record.trigger_kind)
    .bind(&record.synthetic_trigger)
    .bind(&record.status)
    .bind(&record.request_payload)
    .execute(executor)
    .await
    .context("failed to insert execution record")?;
    Ok(())
}

pub async fn mark_succeeded(
    pool: &PgPool,
    execution_id: Uuid,
    worker_kind: &str,
    worker_pid: i32,
    response_payload: &Value,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE execution_records
        SET
            status = 'completed',
            worker_kind = $2,
            worker_pid = $3,
            response_payload = $4,
            updated_at = NOW(),
            completed_at = NOW()
        WHERE execution_id = $1
        "#,
    )
    .bind(execution_id)
    .bind(worker_kind)
    .bind(worker_pid)
    .bind(response_payload)
    .execute(pool)
    .await
    .context("failed to mark execution record as completed")?;
    Ok(())
}

pub async fn mark_failed(
    pool: &PgPool,
    execution_id: Uuid,
    response_payload: &Value,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE execution_records
        SET
            status = 'failed',
            response_payload = $2,
            updated_at = NOW(),
            completed_at = NOW()
        WHERE execution_id = $1
        "#,
    )
    .bind(execution_id)
    .bind(response_payload)
    .execute(pool)
    .await
    .context("failed to mark execution record as failed")?;
    Ok(())
}

pub async fn get(pool: &PgPool, execution_id: Uuid) -> Result<ExecutionRecord> {
    let row = sqlx::query(
        r#"
        SELECT execution_id, trace_id, status, worker_pid, response_payload, completed_at
        FROM execution_records
        WHERE execution_id = $1
        "#,
    )
    .bind(execution_id)
    .fetch_one(pool)
    .await
    .context("failed to fetch execution record")?;

    Ok(ExecutionRecord {
        execution_id: row.get("execution_id"),
        trace_id: row.get("trace_id"),
        status: row.get("status"),
        worker_pid: row.get("worker_pid"),
        response_payload: row.get("response_payload"),
        completed_at: row.get("completed_at"),
    })
}
