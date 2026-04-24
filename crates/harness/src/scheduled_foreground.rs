use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use contracts::{ChannelKind, ScheduledForegroundLastOutcome, ScheduledForegroundTaskStatus};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::config::RuntimeConfig;

#[derive(Debug, Clone)]
pub struct UpsertScheduledForegroundTask {
    pub task_key: String,
    pub internal_principal_ref: String,
    pub internal_conversation_ref: String,
    pub message_text: String,
    pub cadence_seconds: u64,
    pub cooldown_seconds: Option<u64>,
    pub next_due_at: Option<DateTime<Utc>>,
    pub status: ScheduledForegroundTaskStatus,
    pub actor_ref: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduledForegroundTaskWriteAction {
    Created,
    Updated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledForegroundTaskRecord {
    pub scheduled_foreground_task_id: Uuid,
    pub task_key: String,
    pub channel_kind: ChannelKind,
    pub status: ScheduledForegroundTaskStatus,
    pub internal_principal_ref: String,
    pub internal_conversation_ref: String,
    pub message_text: String,
    pub cadence_seconds: u64,
    pub cooldown_seconds: u64,
    pub next_due_at: DateTime<Utc>,
    pub current_execution_id: Option<Uuid>,
    pub current_run_started_at: Option<DateTime<Utc>>,
    pub last_execution_id: Option<Uuid>,
    pub last_run_started_at: Option<DateTime<Utc>>,
    pub last_run_completed_at: Option<DateTime<Utc>>,
    pub last_outcome: Option<ScheduledForegroundLastOutcome>,
    pub last_outcome_reason: Option<String>,
    pub last_outcome_summary: Option<String>,
    pub created_by: String,
    pub updated_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledForegroundTaskWriteResult {
    pub action: ScheduledForegroundTaskWriteAction,
    pub record: ScheduledForegroundTaskRecord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduledForegroundTaskListFilter {
    pub status: Option<ScheduledForegroundTaskStatus>,
    pub due_only: bool,
    pub limit: i64,
}

pub async fn upsert_task(
    pool: &PgPool,
    config: &RuntimeConfig,
    request: &UpsertScheduledForegroundTask,
) -> Result<ScheduledForegroundTaskWriteResult> {
    validate_upsert_request(config, request)?;

    if let Some(existing) = get_task_by_key(pool, &request.task_key).await? {
        let record = update_task(pool, &existing, request).await?;
        Ok(ScheduledForegroundTaskWriteResult {
            action: ScheduledForegroundTaskWriteAction::Updated,
            record,
        })
    } else {
        let record = insert_task(pool, config, request).await?;
        Ok(ScheduledForegroundTaskWriteResult {
            action: ScheduledForegroundTaskWriteAction::Created,
            record,
        })
    }
}

pub async fn get_task_by_key(
    pool: &PgPool,
    task_key: &str,
) -> Result<Option<ScheduledForegroundTaskRecord>> {
    let row = sqlx::query(
        r#"
        SELECT
            scheduled_foreground_task_id,
            task_key,
            channel_kind,
            status,
            internal_principal_ref,
            internal_conversation_ref,
            message_text,
            cadence_seconds,
            cooldown_seconds,
            next_due_at,
            current_execution_id,
            current_run_started_at,
            last_execution_id,
            last_run_started_at,
            last_run_completed_at,
            last_outcome,
            last_outcome_reason,
            last_outcome_summary,
            created_by,
            updated_by,
            created_at,
            updated_at
        FROM scheduled_foreground_tasks
        WHERE task_key = $1
        "#,
    )
    .bind(task_key)
    .fetch_optional(pool)
    .await
    .with_context(|| format!("failed to load scheduled foreground task '{task_key}'"))?;

    row.map(decode_task_row).transpose()
}

pub async fn list_tasks(
    pool: &PgPool,
    filter: ScheduledForegroundTaskListFilter,
) -> Result<Vec<ScheduledForegroundTaskRecord>> {
    let rows = match (filter.status, filter.due_only) {
        (Some(status), true) => sqlx::query(
            r#"
                SELECT
                    scheduled_foreground_task_id,
                    task_key,
                    channel_kind,
                    status,
                    internal_principal_ref,
                    internal_conversation_ref,
                    message_text,
                    cadence_seconds,
                    cooldown_seconds,
                    next_due_at,
                    current_execution_id,
                    current_run_started_at,
                    last_execution_id,
                    last_run_started_at,
                    last_run_completed_at,
                    last_outcome,
                    last_outcome_reason,
                    last_outcome_summary,
                    created_by,
                    updated_by,
                    created_at,
                    updated_at
                FROM scheduled_foreground_tasks
                WHERE status = $1
                  AND next_due_at <= NOW()
                ORDER BY next_due_at ASC, task_key ASC
                LIMIT $2
                "#,
        )
        .bind(scheduled_task_status_as_str(status))
        .bind(filter.limit)
        .fetch_all(pool)
        .await
        .context("failed to list due scheduled foreground tasks")?,
        (Some(status), false) => sqlx::query(
            r#"
                SELECT
                    scheduled_foreground_task_id,
                    task_key,
                    channel_kind,
                    status,
                    internal_principal_ref,
                    internal_conversation_ref,
                    message_text,
                    cadence_seconds,
                    cooldown_seconds,
                    next_due_at,
                    current_execution_id,
                    current_run_started_at,
                    last_execution_id,
                    last_run_started_at,
                    last_run_completed_at,
                    last_outcome,
                    last_outcome_reason,
                    last_outcome_summary,
                    created_by,
                    updated_by,
                    created_at,
                    updated_at
                FROM scheduled_foreground_tasks
                WHERE status = $1
                ORDER BY next_due_at ASC, task_key ASC
                LIMIT $2
                "#,
        )
        .bind(scheduled_task_status_as_str(status))
        .bind(filter.limit)
        .fetch_all(pool)
        .await
        .context("failed to list scheduled foreground tasks by status")?,
        (None, true) => sqlx::query(
            r#"
                SELECT
                    scheduled_foreground_task_id,
                    task_key,
                    channel_kind,
                    status,
                    internal_principal_ref,
                    internal_conversation_ref,
                    message_text,
                    cadence_seconds,
                    cooldown_seconds,
                    next_due_at,
                    current_execution_id,
                    current_run_started_at,
                    last_execution_id,
                    last_run_started_at,
                    last_run_completed_at,
                    last_outcome,
                    last_outcome_reason,
                    last_outcome_summary,
                    created_by,
                    updated_by,
                    created_at,
                    updated_at
                FROM scheduled_foreground_tasks
                WHERE next_due_at <= NOW()
                ORDER BY next_due_at ASC, task_key ASC
                LIMIT $1
                "#,
        )
        .bind(filter.limit)
        .fetch_all(pool)
        .await
        .context("failed to list all due scheduled foreground tasks")?,
        (None, false) => sqlx::query(
            r#"
                SELECT
                    scheduled_foreground_task_id,
                    task_key,
                    channel_kind,
                    status,
                    internal_principal_ref,
                    internal_conversation_ref,
                    message_text,
                    cadence_seconds,
                    cooldown_seconds,
                    next_due_at,
                    current_execution_id,
                    current_run_started_at,
                    last_execution_id,
                    last_run_started_at,
                    last_run_completed_at,
                    last_outcome,
                    last_outcome_reason,
                    last_outcome_summary,
                    created_by,
                    updated_by,
                    created_at,
                    updated_at
                FROM scheduled_foreground_tasks
                ORDER BY next_due_at ASC, task_key ASC
                LIMIT $1
                "#,
        )
        .bind(filter.limit)
        .fetch_all(pool)
        .await
        .context("failed to list scheduled foreground tasks")?,
    };

    rows.into_iter().map(decode_task_row).collect()
}

pub async fn conversation_binding_present(
    pool: &PgPool,
    internal_conversation_ref: &str,
) -> Result<bool> {
    let exists: Option<i32> = sqlx::query_scalar(
        r#"
        SELECT 1
        FROM conversation_bindings
        WHERE internal_conversation_ref = $1
        LIMIT 1
        "#,
    )
    .bind(internal_conversation_ref)
    .fetch_optional(pool)
    .await
    .with_context(|| {
        format!(
            "failed to inspect conversation binding presence for '{}'",
            internal_conversation_ref
        )
    })?;
    Ok(exists.is_some())
}

fn validate_upsert_request(
    config: &RuntimeConfig,
    request: &UpsertScheduledForegroundTask,
) -> Result<()> {
    if request.task_key.trim().is_empty() {
        bail!("scheduled foreground task key must not be empty");
    }
    if request.actor_ref.trim().is_empty() {
        bail!("scheduled foreground actor_ref must not be empty");
    }
    if request.internal_principal_ref.trim().is_empty() {
        bail!("scheduled foreground internal_principal_ref must not be empty");
    }
    if request.internal_conversation_ref.trim().is_empty() {
        bail!("scheduled foreground internal_conversation_ref must not be empty");
    }
    if request.message_text.trim().is_empty() {
        bail!("scheduled foreground message_text must not be empty");
    }
    if request.cadence_seconds < config.scheduled_foreground.min_cadence_seconds {
        bail!(
            "scheduled foreground cadence_seconds must be at least {}",
            config.scheduled_foreground.min_cadence_seconds
        );
    }
    if let Some(cooldown_seconds) = request.cooldown_seconds {
        if cooldown_seconds == 0 {
            bail!("scheduled foreground cooldown_seconds must be greater than zero");
        }
    }
    Ok(())
}

async fn insert_task(
    pool: &PgPool,
    config: &RuntimeConfig,
    request: &UpsertScheduledForegroundTask,
) -> Result<ScheduledForegroundTaskRecord> {
    let cooldown_seconds = request
        .cooldown_seconds
        .unwrap_or(config.scheduled_foreground.default_cooldown_seconds);
    let next_due_at = request.next_due_at.unwrap_or_else(Utc::now);
    let row = sqlx::query(
        r#"
        INSERT INTO scheduled_foreground_tasks (
            scheduled_foreground_task_id,
            task_key,
            channel_kind,
            status,
            internal_principal_ref,
            internal_conversation_ref,
            message_text,
            cadence_seconds,
            cooldown_seconds,
            next_due_at,
            created_by,
            updated_by,
            created_at,
            updated_at
        ) VALUES (
            $1,
            $2,
            $3,
            $4,
            $5,
            $6,
            $7,
            $8,
            $9,
            $10,
            $11,
            $12,
            NOW(),
            NOW()
        )
        RETURNING
            scheduled_foreground_task_id,
            task_key,
            channel_kind,
            status,
            internal_principal_ref,
            internal_conversation_ref,
            message_text,
            cadence_seconds,
            cooldown_seconds,
            next_due_at,
            current_execution_id,
            current_run_started_at,
            last_execution_id,
            last_run_started_at,
            last_run_completed_at,
            last_outcome,
            last_outcome_reason,
            last_outcome_summary,
            created_by,
            updated_by,
            created_at,
            updated_at
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(request.task_key.trim())
    .bind(channel_kind_as_str(ChannelKind::Telegram))
    .bind(scheduled_task_status_as_str(request.status))
    .bind(request.internal_principal_ref.trim())
    .bind(request.internal_conversation_ref.trim())
    .bind(request.message_text.trim())
    .bind(i64::try_from(request.cadence_seconds).context("cadence_seconds exceeds i64")?)
    .bind(i64::try_from(cooldown_seconds).context("cooldown_seconds exceeds i64")?)
    .bind(next_due_at)
    .bind(request.actor_ref.trim())
    .bind(request.actor_ref.trim())
    .fetch_one(pool)
    .await
    .context("failed to insert scheduled foreground task")?;

    decode_task_row(row)
}

async fn update_task(
    pool: &PgPool,
    existing: &ScheduledForegroundTaskRecord,
    request: &UpsertScheduledForegroundTask,
) -> Result<ScheduledForegroundTaskRecord> {
    let row = sqlx::query(
        r#"
        UPDATE scheduled_foreground_tasks
        SET
            status = $2,
            internal_principal_ref = $3,
            internal_conversation_ref = $4,
            message_text = $5,
            cadence_seconds = $6,
            cooldown_seconds = $7,
            next_due_at = $8,
            updated_by = $9,
            updated_at = NOW()
        WHERE scheduled_foreground_task_id = $1
        RETURNING
            scheduled_foreground_task_id,
            task_key,
            channel_kind,
            status,
            internal_principal_ref,
            internal_conversation_ref,
            message_text,
            cadence_seconds,
            cooldown_seconds,
            next_due_at,
            current_execution_id,
            current_run_started_at,
            last_execution_id,
            last_run_started_at,
            last_run_completed_at,
            last_outcome,
            last_outcome_reason,
            last_outcome_summary,
            created_by,
            updated_by,
            created_at,
            updated_at
        "#,
    )
    .bind(existing.scheduled_foreground_task_id)
    .bind(scheduled_task_status_as_str(request.status))
    .bind(request.internal_principal_ref.trim())
    .bind(request.internal_conversation_ref.trim())
    .bind(request.message_text.trim())
    .bind(i64::try_from(request.cadence_seconds).context("cadence_seconds exceeds i64")?)
    .bind(
        i64::try_from(
            request
                .cooldown_seconds
                .unwrap_or(existing.cooldown_seconds),
        )
        .context("cooldown_seconds exceeds i64")?,
    )
    .bind(request.next_due_at.unwrap_or(existing.next_due_at))
    .bind(request.actor_ref.trim())
    .fetch_one(pool)
    .await
    .with_context(|| {
        format!(
            "failed to update scheduled foreground task '{}'",
            existing.task_key
        )
    })?;

    decode_task_row(row)
}

fn decode_task_row(row: sqlx::postgres::PgRow) -> Result<ScheduledForegroundTaskRecord> {
    Ok(ScheduledForegroundTaskRecord {
        scheduled_foreground_task_id: row.get("scheduled_foreground_task_id"),
        task_key: row.get("task_key"),
        channel_kind: parse_channel_kind(row.get("channel_kind"))?,
        status: parse_scheduled_task_status(row.get("status"))?,
        internal_principal_ref: row.get("internal_principal_ref"),
        internal_conversation_ref: row.get("internal_conversation_ref"),
        message_text: row.get("message_text"),
        cadence_seconds: i64_to_u64(row.get::<i64, _>("cadence_seconds"), "cadence_seconds")?,
        cooldown_seconds: i64_to_u64(row.get::<i64, _>("cooldown_seconds"), "cooldown_seconds")?,
        next_due_at: row.get("next_due_at"),
        current_execution_id: row.get("current_execution_id"),
        current_run_started_at: row.get("current_run_started_at"),
        last_execution_id: row.get("last_execution_id"),
        last_run_started_at: row.get("last_run_started_at"),
        last_run_completed_at: row.get("last_run_completed_at"),
        last_outcome: row
            .get::<Option<String>, _>("last_outcome")
            .as_deref()
            .map(parse_scheduled_last_outcome)
            .transpose()?,
        last_outcome_reason: row.get("last_outcome_reason"),
        last_outcome_summary: row.get("last_outcome_summary"),
        created_by: row.get("created_by"),
        updated_by: row.get("updated_by"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

fn parse_channel_kind(raw: String) -> Result<ChannelKind> {
    match raw.as_str() {
        "telegram" => Ok(ChannelKind::Telegram),
        other => bail!("unsupported channel kind '{other}' in scheduled foreground task"),
    }
}

fn channel_kind_as_str(value: ChannelKind) -> &'static str {
    match value {
        ChannelKind::Telegram => "telegram",
    }
}

fn scheduled_task_status_as_str(value: ScheduledForegroundTaskStatus) -> &'static str {
    match value {
        ScheduledForegroundTaskStatus::Active => "active",
        ScheduledForegroundTaskStatus::Paused => "paused",
        ScheduledForegroundTaskStatus::Disabled => "disabled",
    }
}

fn parse_scheduled_task_status(raw: String) -> Result<ScheduledForegroundTaskStatus> {
    match raw.as_str() {
        "active" => Ok(ScheduledForegroundTaskStatus::Active),
        "paused" => Ok(ScheduledForegroundTaskStatus::Paused),
        "disabled" => Ok(ScheduledForegroundTaskStatus::Disabled),
        other => bail!("unsupported scheduled foreground task status '{other}'"),
    }
}

fn parse_scheduled_last_outcome(raw: &str) -> Result<ScheduledForegroundLastOutcome> {
    match raw {
        "completed" => Ok(ScheduledForegroundLastOutcome::Completed),
        "suppressed" => Ok(ScheduledForegroundLastOutcome::Suppressed),
        "failed" => Ok(ScheduledForegroundLastOutcome::Failed),
        other => bail!("unsupported scheduled foreground last outcome '{other}'"),
    }
}

fn i64_to_u64(value: i64, field_name: &str) -> Result<u64> {
    u64::try_from(value).with_context(|| format!("{field_name} must not be negative"))
}
