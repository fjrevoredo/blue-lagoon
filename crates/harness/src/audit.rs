use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct NewAuditEvent {
    pub loop_kind: String,
    pub subsystem: String,
    pub event_kind: String,
    pub severity: String,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub worker_pid: Option<i32>,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct AuditEvent {
    pub event_id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub event_kind: String,
    pub trace_id: Uuid,
}

pub async fn insert(pool: &PgPool, event: &NewAuditEvent) -> Result<Uuid> {
    let event_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO audit_events (
            event_id,
            occurred_at,
            loop_kind,
            subsystem,
            event_kind,
            severity,
            trace_id,
            span_id,
            parent_span_id,
            execution_id,
            worker_pid,
            model_tier,
            payload
        ) VALUES (
            $1,
            NOW(),
            $2,
            $3,
            $4,
            $5,
            $6,
            NULL,
            NULL,
            $7,
            $8,
            NULL,
            $9
        )
        "#,
    )
    .bind(event_id)
    .bind(&event.loop_kind)
    .bind(&event.subsystem)
    .bind(&event.event_kind)
    .bind(&event.severity)
    .bind(event.trace_id)
    .bind(event.execution_id)
    .bind(event.worker_pid)
    .bind(&event.payload)
    .execute(pool)
    .await
    .context("failed to insert audit event")?;
    Ok(event_id)
}

pub async fn list_for_execution(pool: &PgPool, execution_id: Uuid) -> Result<Vec<AuditEvent>> {
    let rows = sqlx::query(
        r#"
        SELECT event_id, occurred_at, event_kind, trace_id
        FROM audit_events
        WHERE execution_id = $1
        ORDER BY occurred_at, event_id
        "#,
    )
    .bind(execution_id)
    .fetch_all(pool)
    .await
    .context("failed to fetch audit events")?;

    Ok(rows
        .into_iter()
        .map(|row| AuditEvent {
            event_id: row.get("event_id"),
            occurred_at: row.get("occurred_at"),
            event_kind: row.get("event_kind"),
            trace_id: row.get("trace_id"),
        })
        .collect())
}

pub async fn list_for_trace(pool: &PgPool, trace_id: Uuid) -> Result<Vec<AuditEvent>> {
    let rows = sqlx::query(
        r#"
        SELECT event_id, occurred_at, event_kind, trace_id
        FROM audit_events
        WHERE trace_id = $1
        ORDER BY occurred_at, event_id
        "#,
    )
    .bind(trace_id)
    .fetch_all(pool)
    .await
    .context("failed to fetch audit events by trace")?;

    Ok(rows
        .into_iter()
        .map(|row| AuditEvent {
            event_id: row.get("event_id"),
            occurred_at: row.get("occurred_at"),
            event_kind: row.get("event_kind"),
            trace_id: row.get("trace_id"),
        })
        .collect())
}
