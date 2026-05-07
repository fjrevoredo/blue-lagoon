use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use contracts::{
    ChannelKind, IngressEventKind, NormalizedIngress, ScheduledForegroundLastOutcome,
    ScheduledForegroundTaskStatus,
};
use serde_json::json;
use sqlx::{PgPool, Postgres, Row};
use uuid::Uuid;

use crate::{
    causal_links::{self, NewCausalLink},
    config::RuntimeConfig,
    execution::{self, NewExecutionRecord},
    foreground::{self, ConversationBindingRecord, NewIngressEvent},
};

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

struct TaskRunOutcomeData<'a> {
    outcome: ScheduledForegroundLastOutcome,
    reason: Option<&'a str>,
    summary: Option<&'a str>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedScheduledForegroundTask {
    pub task: ScheduledForegroundTaskRecord,
    pub ingress: Option<NormalizedIngress>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoverableScheduledForegroundTask {
    pub task: ScheduledForegroundTaskRecord,
    pub execution_status: String,
    pub ingress_id: Option<Uuid>,
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
        let record = update_task(pool, config, &existing, request).await?;
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
                  AND current_execution_id IS NULL
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
                WHERE current_execution_id IS NULL
                  AND next_due_at <= NOW()
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

pub async fn claim_next_due_task(
    pool: &PgPool,
    execution_id: Uuid,
    trace_id: Uuid,
    claimed_at: DateTime<Utc>,
) -> Result<Option<ClaimedScheduledForegroundTask>> {
    let mut tx = pool
        .begin()
        .await
        .context("failed to begin scheduled foreground claim transaction")?;

    let Some(mut task) = select_next_due_task_locked(&mut tx, claimed_at).await? else {
        tx.rollback()
            .await
            .context("failed to roll back empty scheduled foreground claim transaction")?;
        return Ok(None);
    };

    execution::insert(
        &mut *tx,
        &NewExecutionRecord {
            execution_id,
            trace_id,
            trigger_kind: "scheduled_foreground".to_string(),
            synthetic_trigger: None,
            status: "started".to_string(),
            request_payload: json!({
                "kind": "scheduled_foreground",
                "scheduled_foreground_task_id": task.scheduled_foreground_task_id,
                "task_key": task.task_key,
                "internal_principal_ref": task.internal_principal_ref,
                "internal_conversation_ref": task.internal_conversation_ref,
            }),
        },
    )
    .await
    .context("failed to insert execution record for scheduled foreground task claim")?;

    causal_links::insert(
        &mut *tx,
        &NewCausalLink {
            trace_id,
            source_kind: "scheduled_foreground_task".to_string(),
            source_id: task.scheduled_foreground_task_id,
            target_kind: "execution_record".to_string(),
            target_id: execution_id,
            edge_kind: "triggered_execution".to_string(),
            payload: json!({
                "task_key": task.task_key,
                "trigger_kind": "scheduled_foreground",
                "claimed_at": claimed_at,
            }),
        },
    )
    .await?;

    let binding = find_conversation_binding_locked(&mut tx, &task.internal_conversation_ref)
        .await
        .with_context(|| {
            format!(
                "failed to load conversation binding for scheduled foreground task '{}'",
                task.task_key
            )
        })?;
    let ingress = if let Some(binding) = binding.as_ref() {
        let ingress = build_scheduled_task_ingress(&task, binding, execution_id, claimed_at);
        foreground::insert_ingress_event(
            &mut *tx,
            &NewIngressEvent {
                ingress: ingress.clone(),
                conversation_binding_id: Some(binding.conversation_binding_id),
                trace_id,
                execution_id: Some(execution_id),
                status: "accepted".to_string(),
                rejection_reason: None,
            },
        )
        .await
        .with_context(|| {
            format!(
                "failed to insert synthetic ingress event for scheduled foreground task '{}'",
                task.task_key
            )
        })?;
        mark_ingress_processing(&mut tx, ingress.ingress_id, execution_id)
            .await
            .with_context(|| {
                format!(
                    "failed to mark scheduled foreground ingress as processing for task '{}'",
                    task.task_key
                )
            })?;
        causal_links::insert(
            &mut *tx,
            &NewCausalLink {
                trace_id,
                source_kind: "scheduled_foreground_task".to_string(),
                source_id: task.scheduled_foreground_task_id,
                target_kind: "ingress_event".to_string(),
                target_id: ingress.ingress_id,
                edge_kind: "staged_foreground_trigger".to_string(),
                payload: json!({
                    "task_key": task.task_key,
                    "external_event_id": ingress.external_event_id,
                }),
            },
        )
        .await?;
        causal_links::insert(
            &mut *tx,
            &NewCausalLink {
                trace_id,
                source_kind: "ingress_event".to_string(),
                source_id: ingress.ingress_id,
                target_kind: "execution_record".to_string(),
                target_id: execution_id,
                edge_kind: "triggered_execution".to_string(),
                payload: json!({
                    "link_role": "scheduled_task",
                    "task_key": task.task_key,
                }),
            },
        )
        .await?;
        Some(ingress)
    } else {
        None
    };

    let row = sqlx::query(
        r#"
        UPDATE scheduled_foreground_tasks
        SET
            current_execution_id = $2,
            current_run_started_at = $3,
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
    .bind(task.scheduled_foreground_task_id)
    .bind(execution_id)
    .bind(claimed_at)
    .fetch_one(&mut *tx)
    .await
    .with_context(|| {
        format!(
            "failed to mark scheduled foreground task '{}' as in-progress",
            task.task_key
        )
    })?;
    task = decode_task_row(row)?;

    tx.commit()
        .await
        .context("failed to commit scheduled foreground claim transaction")?;

    Ok(Some(ClaimedScheduledForegroundTask { task, ingress }))
}

pub async fn mark_task_completed(
    pool: &PgPool,
    task: &ScheduledForegroundTaskRecord,
    execution_id: Uuid,
    completed_at: DateTime<Utc>,
    summary: &str,
) -> Result<ScheduledForegroundTaskRecord> {
    update_task_run_outcome(
        pool,
        task,
        execution_id,
        completed_at,
        TaskRunOutcomeData {
            outcome: ScheduledForegroundLastOutcome::Completed,
            reason: None,
            summary: Some(summary),
        },
        completed_at + cadence_as_duration(task.cadence_seconds)?,
    )
    .await
}

pub async fn mark_task_suppressed(
    pool: &PgPool,
    task: &ScheduledForegroundTaskRecord,
    execution_id: Uuid,
    completed_at: DateTime<Utc>,
    reason: &str,
    summary: &str,
) -> Result<ScheduledForegroundTaskRecord> {
    update_task_run_outcome(
        pool,
        task,
        execution_id,
        completed_at,
        TaskRunOutcomeData {
            outcome: ScheduledForegroundLastOutcome::Suppressed,
            reason: Some(reason),
            summary: Some(summary),
        },
        completed_at + cadence_as_duration(task.cooldown_seconds)?,
    )
    .await
}

pub async fn mark_task_failed(
    pool: &PgPool,
    task: &ScheduledForegroundTaskRecord,
    execution_id: Uuid,
    completed_at: DateTime<Utc>,
    reason: &str,
    summary: &str,
) -> Result<ScheduledForegroundTaskRecord> {
    update_task_run_outcome(
        pool,
        task,
        execution_id,
        completed_at,
        TaskRunOutcomeData {
            outcome: ScheduledForegroundLastOutcome::Failed,
            reason: Some(reason),
            summary: Some(summary),
        },
        completed_at + cadence_as_duration(task.cooldown_seconds)?,
    )
    .await
}

pub async fn list_recoverable_in_progress_tasks(
    pool: &PgPool,
    stale_cutoff: DateTime<Utc>,
    limit: i64,
) -> Result<Vec<RecoverableScheduledForegroundTask>> {
    let rows = sqlx::query(
        r#"
        SELECT
            task.scheduled_foreground_task_id,
            task.task_key,
            task.channel_kind,
            task.status,
            task.internal_principal_ref,
            task.internal_conversation_ref,
            task.message_text,
            task.cadence_seconds,
            task.cooldown_seconds,
            task.next_due_at,
            task.current_execution_id,
            task.current_run_started_at,
            task.last_execution_id,
            task.last_run_started_at,
            task.last_run_completed_at,
            task.last_outcome,
            task.last_outcome_reason,
            task.last_outcome_summary,
            task.created_by,
            task.updated_by,
            task.created_at,
            task.updated_at,
            execution.status AS execution_status,
            (
                SELECT ingress_id
                FROM ingress_events
                WHERE execution_id = task.current_execution_id
                ORDER BY received_at ASC
                LIMIT 1
            ) AS ingress_id
        FROM scheduled_foreground_tasks task
        JOIN execution_records execution
          ON execution.execution_id = task.current_execution_id
        WHERE task.current_execution_id IS NOT NULL
          AND (
              execution.status IN ('completed', 'failed')
              OR (
                  execution.status = 'started'
                  AND task.current_run_started_at <= $1
                  AND NOT EXISTS (
                      SELECT 1
                      FROM worker_leases lease
                      WHERE lease.execution_id = task.current_execution_id
                        AND lease.status = 'active'
                  )
              )
          )
        ORDER BY task.current_run_started_at ASC, task.task_key ASC
        LIMIT $2
        "#,
    )
    .bind(stale_cutoff)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("failed to list recoverable in-progress scheduled foreground tasks")?;

    rows.into_iter()
        .map(|row| {
            let execution_status: String = row.get("execution_status");
            let ingress_id: Option<Uuid> = row.get("ingress_id");
            Ok(RecoverableScheduledForegroundTask {
                task: decode_task_row(row)?,
                execution_status,
                ingress_id,
            })
        })
        .collect()
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
    if request.cadence_seconds == 0 && !is_one_shot_task_key(&request.task_key) {
        bail!(
            "scheduled foreground cadence_seconds must be greater than zero unless task_key uses the one-shot prefix"
        );
    }
    if request.cadence_seconds > 0
        && request.cadence_seconds < config.scheduled_foreground.min_cadence_seconds
    {
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

fn stored_cadence_seconds(config: &RuntimeConfig, request: &UpsertScheduledForegroundTask) -> u64 {
    if request.cadence_seconds == 0 && is_one_shot_task_key(&request.task_key) {
        return config.scheduled_foreground.min_cadence_seconds;
    }
    request.cadence_seconds
}

fn is_one_shot_task_key(task_key: &str) -> bool {
    let task_key = task_key.trim().to_ascii_lowercase();
    task_key.starts_with("oneoff_") || task_key.starts_with("one_shot_")
}

async fn insert_task(
    pool: &PgPool,
    config: &RuntimeConfig,
    request: &UpsertScheduledForegroundTask,
) -> Result<ScheduledForegroundTaskRecord> {
    let cooldown_seconds = request
        .cooldown_seconds
        .unwrap_or(config.scheduled_foreground.default_cooldown_seconds);
    let cadence_seconds = stored_cadence_seconds(config, request);
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
    .bind(i64::try_from(cadence_seconds).context("cadence_seconds exceeds i64")?)
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
    config: &RuntimeConfig,
    existing: &ScheduledForegroundTaskRecord,
    request: &UpsertScheduledForegroundTask,
) -> Result<ScheduledForegroundTaskRecord> {
    let cadence_seconds = stored_cadence_seconds(config, request);
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
    .bind(i64::try_from(cadence_seconds).context("cadence_seconds exceeds i64")?)
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

async fn select_next_due_task_locked(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    claimed_at: DateTime<Utc>,
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
        WHERE status = 'active'
          AND current_execution_id IS NULL
          AND next_due_at <= $1
        ORDER BY next_due_at ASC, task_key ASC
        LIMIT 1
        FOR UPDATE SKIP LOCKED
        "#,
    )
    .bind(claimed_at)
    .fetch_optional(&mut **tx)
    .await
    .context("failed to select next due scheduled foreground task")?;

    row.map(decode_task_row).transpose()
}

async fn find_conversation_binding_locked(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    internal_conversation_ref: &str,
) -> Result<Option<ConversationBindingRecord>> {
    let row = sqlx::query(
        r#"
        SELECT
            conversation_binding_id,
            channel_kind,
            external_user_id,
            external_conversation_id,
            internal_principal_ref,
            internal_conversation_ref
        FROM conversation_bindings
        WHERE internal_conversation_ref = $1
        FOR SHARE
        "#,
    )
    .bind(internal_conversation_ref)
    .fetch_optional(&mut **tx)
    .await
    .context("failed to load conversation binding for scheduled foreground task")?;

    Ok(row.map(|row| ConversationBindingRecord {
        conversation_binding_id: row.get("conversation_binding_id"),
        channel_kind: row.get("channel_kind"),
        external_user_id: row.get("external_user_id"),
        external_conversation_id: row.get("external_conversation_id"),
        internal_principal_ref: row.get("internal_principal_ref"),
        internal_conversation_ref: row.get("internal_conversation_ref"),
    }))
}

fn build_scheduled_task_ingress(
    task: &ScheduledForegroundTaskRecord,
    binding: &ConversationBindingRecord,
    execution_id: Uuid,
    occurred_at: DateTime<Utc>,
) -> NormalizedIngress {
    NormalizedIngress {
        ingress_id: Uuid::now_v7(),
        channel_kind: ChannelKind::Telegram,
        external_user_id: binding.external_user_id.clone(),
        external_conversation_id: binding.external_conversation_id.clone(),
        external_event_id: format!("scheduled-task:{}:{execution_id}", task.task_key),
        external_message_id: None,
        internal_principal_ref: task.internal_principal_ref.clone(),
        internal_conversation_ref: task.internal_conversation_ref.clone(),
        event_kind: IngressEventKind::MessageCreated,
        occurred_at,
        text_body: Some(task.message_text.clone()),
        reply_to: None,
        attachments: Vec::new(),
        command_hint: None,
        approval_payload: None,
        raw_payload_ref: Some(format!(
            "scheduled_foreground_task:{}",
            task.scheduled_foreground_task_id
        )),
    }
}

async fn mark_ingress_processing(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    ingress_id: Uuid,
    execution_id: Uuid,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE ingress_events
        SET
            execution_id = $2,
            foreground_status = 'processing',
            last_processed_at = NOW()
        WHERE ingress_id = $1
        "#,
    )
    .bind(ingress_id)
    .bind(execution_id)
    .execute(&mut **tx)
    .await
    .context("failed to mark scheduled foreground ingress as processing")?;
    Ok(())
}

async fn update_task_run_outcome(
    pool: &PgPool,
    task: &ScheduledForegroundTaskRecord,
    execution_id: Uuid,
    completed_at: DateTime<Utc>,
    outcome_data: TaskRunOutcomeData<'_>,
    next_due_at: DateTime<Utc>,
) -> Result<ScheduledForegroundTaskRecord> {
    let row = sqlx::query(
        r#"
        UPDATE scheduled_foreground_tasks
        SET
            current_execution_id = NULL,
            current_run_started_at = NULL,
            status = CASE WHEN $8 THEN 'disabled' ELSE status END,
            last_execution_id = $3,
            last_run_started_at = COALESCE(current_run_started_at, $2),
            last_run_completed_at = $2,
            last_outcome = $4,
            last_outcome_reason = $5,
            last_outcome_summary = $6,
            next_due_at = $7,
            updated_at = NOW()
        WHERE scheduled_foreground_task_id = $1
          AND current_execution_id = $3
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
    .bind(task.scheduled_foreground_task_id)
    .bind(completed_at)
    .bind(execution_id)
    .bind(scheduled_last_outcome_as_str(outcome_data.outcome))
    .bind(outcome_data.reason)
    .bind(outcome_data.summary)
    .bind(next_due_at)
    .bind(is_one_shot_task_key(&task.task_key))
    .fetch_optional(pool)
    .await
    .with_context(|| {
        format!(
            "failed to update scheduled foreground task '{}' run outcome",
            task.task_key
        )
    })?
    .with_context(|| {
        format!(
            "scheduled foreground task '{}' was no longer owned by execution {}",
            task.task_key, execution_id
        )
    })?;

    decode_task_row(row)
}

fn cadence_as_duration(seconds: u64) -> Result<chrono::Duration> {
    let seconds = i64::try_from(seconds).context("scheduled foreground cadence exceeded i64")?;
    Ok(chrono::Duration::seconds(seconds))
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

fn scheduled_last_outcome_as_str(value: ScheduledForegroundLastOutcome) -> &'static str {
    match value {
        ScheduledForegroundLastOutcome::Completed => "completed",
        ScheduledForegroundLastOutcome::Suppressed => "suppressed",
        ScheduledForegroundLastOutcome::Failed => "failed",
    }
}

fn i64_to_u64(value: i64, field_name: &str) -> Result<u64> {
    u64::try_from(value).with_context(|| format!("{field_name} must not be negative"))
}
