use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::{Executor, PgPool, Postgres, Row};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct NewCausalLink {
    pub trace_id: Uuid,
    pub source_kind: String,
    pub source_id: Uuid,
    pub target_kind: String,
    pub target_id: Uuid,
    pub edge_kind: String,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct CausalLinkRecord {
    pub causal_link_id: Uuid,
    pub trace_id: Uuid,
    pub source_kind: String,
    pub source_id: Uuid,
    pub target_kind: String,
    pub target_id: Uuid,
    pub edge_kind: String,
    pub created_at: DateTime<Utc>,
    pub payload: Value,
}

pub async fn insert<'e, E>(executor: E, link: &NewCausalLink) -> Result<Uuid>
where
    E: Executor<'e, Database = Postgres>,
{
    let causal_link_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO causal_links (
            causal_link_id,
            trace_id,
            source_kind,
            source_id,
            target_kind,
            target_id,
            edge_kind,
            payload_json
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8
        )
        ON CONFLICT (source_kind, source_id, target_kind, target_id, edge_kind)
        DO NOTHING
        "#,
    )
    .bind(causal_link_id)
    .bind(link.trace_id)
    .bind(&link.source_kind)
    .bind(link.source_id)
    .bind(&link.target_kind)
    .bind(link.target_id)
    .bind(&link.edge_kind)
    .bind(&link.payload)
    .execute(executor)
    .await
    .context("failed to insert causal link")?;

    Ok(causal_link_id)
}

pub async fn list_for_trace(pool: &PgPool, trace_id: Uuid) -> Result<Vec<CausalLinkRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT
            causal_link_id,
            trace_id,
            source_kind,
            source_id,
            target_kind,
            target_id,
            edge_kind,
            created_at,
            payload_json
        FROM causal_links
        WHERE trace_id = $1
        ORDER BY created_at ASC, causal_link_id ASC
        "#,
    )
    .bind(trace_id)
    .fetch_all(pool)
    .await
    .context("failed to list causal links for trace")?;

    Ok(rows
        .into_iter()
        .map(|row| CausalLinkRecord {
            causal_link_id: row.get("causal_link_id"),
            trace_id: row.get("trace_id"),
            source_kind: row.get("source_kind"),
            source_id: row.get("source_id"),
            target_kind: row.get("target_kind"),
            target_id: row.get("target_id"),
            edge_kind: row.get("edge_kind"),
            created_at: row.get("created_at"),
            payload: row.get("payload_json"),
        })
        .collect())
}

pub fn payload_with_reason(reason: &str) -> Value {
    json!({ "reason": reason })
}
