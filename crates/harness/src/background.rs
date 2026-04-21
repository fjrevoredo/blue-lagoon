use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use contracts::{
    BackgroundExecutionBudget, BackgroundTrigger, BackgroundTriggerKind, UnconsciousJobKind,
    UnconsciousScope, WakeSignal, WakeSignalDecision, WakeSignalDecisionKind, WakeSignalPriority,
    WakeSignalReason,
};
use serde_json::Value;
use sqlx::{Executor, PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundJobStatus {
    Planned,
    Leased,
    Running,
    Completed,
    Failed,
    Suppressed,
    Cancelled,
}

impl BackgroundJobStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Leased => "leased",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Suppressed => "suppressed",
            Self::Cancelled => "cancelled",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "planned" => Ok(Self::Planned),
            "leased" => Ok(Self::Leased),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "suppressed" => Ok(Self::Suppressed),
            "cancelled" => Ok(Self::Cancelled),
            other => bail!("unrecognized background job status '{other}'"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundJobRunStatus {
    Leased,
    Running,
    Completed,
    Failed,
    TimedOut,
}

impl BackgroundJobRunStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Leased => "leased",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "leased" => Ok(Self::Leased),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "timed_out" => Ok(Self::TimedOut),
            other => bail!("unrecognized background job run status '{other}'"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeSignalStatus {
    PendingReview,
    Accepted,
    Rejected,
    Suppressed,
    Deferred,
}

impl WakeSignalStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::PendingReview => "pending_review",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Suppressed => "suppressed",
            Self::Deferred => "deferred",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "pending_review" => Ok(Self::PendingReview),
            "accepted" => Ok(Self::Accepted),
            "rejected" => Ok(Self::Rejected),
            "suppressed" => Ok(Self::Suppressed),
            "deferred" => Ok(Self::Deferred),
            other => bail!("unrecognized wake signal status '{other}'"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NewBackgroundJob {
    pub background_job_id: Uuid,
    pub trace_id: Uuid,
    pub job_kind: UnconsciousJobKind,
    pub trigger: BackgroundTrigger,
    pub deduplication_key: String,
    pub scope: UnconsciousScope,
    pub budget: BackgroundExecutionBudget,
    pub status: BackgroundJobStatus,
    pub available_at: DateTime<Utc>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub last_started_at: Option<DateTime<Utc>>,
    pub last_completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct BackgroundJobRecord {
    pub background_job_id: Uuid,
    pub trace_id: Uuid,
    pub job_kind: UnconsciousJobKind,
    pub trigger: BackgroundTrigger,
    pub deduplication_key: String,
    pub scope: UnconsciousScope,
    pub budget: BackgroundExecutionBudget,
    pub status: BackgroundJobStatus,
    pub available_at: DateTime<Utc>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub last_started_at: Option<DateTime<Utc>>,
    pub last_completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct UpdateBackgroundJobStatus {
    pub status: BackgroundJobStatus,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub last_started_at: Option<DateTime<Utc>>,
    pub last_completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct NewBackgroundJobRun {
    pub background_job_run_id: Uuid,
    pub background_job_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub lease_token: Uuid,
    pub status: BackgroundJobRunStatus,
    pub worker_pid: Option<i32>,
    pub lease_acquired_at: DateTime<Utc>,
    pub lease_expires_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub result_payload: Option<Value>,
    pub failure_payload: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct BackgroundJobRunRecord {
    pub background_job_run_id: Uuid,
    pub background_job_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub lease_token: Uuid,
    pub status: BackgroundJobRunStatus,
    pub worker_pid: Option<i32>,
    pub lease_acquired_at: DateTime<Utc>,
    pub lease_expires_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub result_payload: Option<Value>,
    pub failure_payload: Option<Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct UpdateBackgroundJobRunStatus {
    pub status: BackgroundJobRunStatus,
    pub worker_pid: Option<i32>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub result_payload: Option<Value>,
    pub failure_payload: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct NewWakeSignalRecord {
    pub background_job_id: Uuid,
    pub background_job_run_id: Option<Uuid>,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub signal: WakeSignal,
    pub status: WakeSignalStatus,
    pub requested_at: DateTime<Utc>,
    pub cooldown_until: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct WakeSignalRecord {
    pub wake_signal_id: Uuid,
    pub background_job_id: Uuid,
    pub background_job_run_id: Option<Uuid>,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub signal: WakeSignal,
    pub status: WakeSignalStatus,
    pub decision: Option<WakeSignalDecision>,
    pub requested_at: DateTime<Utc>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub cooldown_until: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub async fn insert_job<'e, E>(executor: E, job: &NewBackgroundJob) -> Result<()>
where
    E: Executor<'e, Database = Postgres>,
{
    let scope_json =
        serde_json::to_value(&job.scope).context("failed to encode background job scope")?;

    sqlx::query(
        r#"
        INSERT INTO background_jobs (
            background_job_id,
            trace_id,
            job_kind,
            trigger_id,
            trigger_kind,
            trigger_requested_at,
            trigger_reason_summary,
            trigger_payload_ref,
            deduplication_key,
            scope_summary,
            scope_json,
            iteration_budget,
            wall_clock_budget_ms,
            token_budget,
            status,
            available_at,
            lease_expires_at,
            last_started_at,
            last_completed_at,
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
            $13,
            $14,
            $15,
            $16,
            $17,
            $18,
            $19,
            NOW(),
            NOW()
        )
        "#,
    )
    .bind(job.background_job_id)
    .bind(job.trace_id)
    .bind(job_kind_as_str(job.job_kind))
    .bind(job.trigger.trigger_id)
    .bind(background_trigger_kind_as_str(job.trigger.trigger_kind))
    .bind(job.trigger.requested_at)
    .bind(&job.trigger.reason_summary)
    .bind(&job.trigger.payload_ref)
    .bind(&job.deduplication_key)
    .bind(&job.scope.summary)
    .bind(&scope_json)
    .bind(i32::try_from(job.budget.iteration_budget).context("iteration budget exceeds i32")?)
    .bind(i64::try_from(job.budget.wall_clock_budget_ms).context("wall-clock budget exceeds i64")?)
    .bind(i32::try_from(job.budget.token_budget).context("token budget exceeds i32")?)
    .bind(job.status.as_str())
    .bind(job.available_at)
    .bind(job.lease_expires_at)
    .bind(job.last_started_at)
    .bind(job.last_completed_at)
    .execute(executor)
    .await
    .context("failed to insert background job")?;
    Ok(())
}

pub async fn get_job(pool: &PgPool, background_job_id: Uuid) -> Result<BackgroundJobRecord> {
    let row = sqlx::query(
        r#"
        SELECT
            background_job_id,
            trace_id,
            job_kind,
            trigger_id,
            trigger_kind,
            trigger_requested_at,
            trigger_reason_summary,
            trigger_payload_ref,
            deduplication_key,
            scope_json,
            iteration_budget,
            wall_clock_budget_ms,
            token_budget,
            status,
            available_at,
            lease_expires_at,
            last_started_at,
            last_completed_at,
            created_at,
            updated_at
        FROM background_jobs
        WHERE background_job_id = $1
        "#,
    )
    .bind(background_job_id)
    .fetch_one(pool)
    .await
    .context("failed to fetch background job")?;

    decode_background_job_row(row)
}

pub async fn list_due_jobs(
    pool: &PgPool,
    due_before_or_at: DateTime<Utc>,
    limit: u32,
) -> Result<Vec<BackgroundJobRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT
            background_job_id,
            trace_id,
            job_kind,
            trigger_id,
            trigger_kind,
            trigger_requested_at,
            trigger_reason_summary,
            trigger_payload_ref,
            deduplication_key,
            scope_json,
            iteration_budget,
            wall_clock_budget_ms,
            token_budget,
            status,
            available_at,
            lease_expires_at,
            last_started_at,
            last_completed_at,
            created_at,
            updated_at
        FROM background_jobs
        WHERE status = 'planned'
          AND available_at <= $1
        ORDER BY available_at ASC, created_at ASC
        LIMIT $2
        "#,
    )
    .bind(due_before_or_at)
    .bind(i64::from(limit))
    .fetch_all(pool)
    .await
    .context("failed to list due background jobs")?;

    rows.into_iter().map(decode_background_job_row).collect()
}

pub async fn lease_due_job(
    transaction: &mut Transaction<'_, Postgres>,
    due_before_or_at: DateTime<Utc>,
    lease_expires_at: DateTime<Utc>,
) -> Result<Option<BackgroundJobRecord>> {
    let row = sqlx::query(
        r#"
        SELECT
            background_job_id,
            trace_id,
            job_kind,
            trigger_id,
            trigger_kind,
            trigger_requested_at,
            trigger_reason_summary,
            trigger_payload_ref,
            deduplication_key,
            scope_json,
            iteration_budget,
            wall_clock_budget_ms,
            token_budget,
            status,
            available_at,
            lease_expires_at,
            last_started_at,
            last_completed_at,
            created_at,
            updated_at
        FROM background_jobs
        WHERE status = 'planned'
          AND available_at <= $1
        ORDER BY available_at ASC, created_at ASC
        FOR UPDATE SKIP LOCKED
        LIMIT 1
        "#,
    )
    .bind(due_before_or_at)
    .fetch_optional(&mut **transaction)
    .await
    .context("failed to select due background job for lease")?;

    let Some(row) = row else {
        return Ok(None);
    };

    let mut job = decode_background_job_row(row)?;
    sqlx::query(
        r#"
        UPDATE background_jobs
        SET
            status = 'leased',
            lease_expires_at = $2,
            updated_at = NOW()
        WHERE background_job_id = $1
        "#,
    )
    .bind(job.background_job_id)
    .bind(lease_expires_at)
    .execute(&mut **transaction)
    .await
    .context("failed to lease background job")?;

    job.status = BackgroundJobStatus::Leased;
    job.lease_expires_at = Some(lease_expires_at);
    Ok(Some(job))
}

pub async fn find_active_job_by_deduplication_key<'e, E>(
    executor: E,
    deduplication_key: &str,
) -> Result<Option<BackgroundJobRecord>>
where
    E: Executor<'e, Database = Postgres>,
{
    let row = sqlx::query(
        r#"
        SELECT
            background_job_id,
            trace_id,
            job_kind,
            trigger_id,
            trigger_kind,
            trigger_requested_at,
            trigger_reason_summary,
            trigger_payload_ref,
            deduplication_key,
            scope_json,
            iteration_budget,
            wall_clock_budget_ms,
            token_budget,
            status,
            available_at,
            lease_expires_at,
            last_started_at,
            last_completed_at,
            created_at,
            updated_at
        FROM background_jobs
        WHERE deduplication_key = $1
          AND status IN ('planned', 'leased', 'running')
        ORDER BY created_at DESC, background_job_id DESC
        LIMIT 1
        "#,
    )
    .bind(deduplication_key)
    .fetch_optional(executor)
    .await
    .context("failed to fetch active background job by deduplication key")?;

    row.map(decode_background_job_row).transpose()
}

pub async fn update_job_status<'e, E>(
    executor: E,
    background_job_id: Uuid,
    update: &UpdateBackgroundJobStatus,
) -> Result<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query(
        r#"
        UPDATE background_jobs
        SET
            status = $2,
            lease_expires_at = $3,
            last_started_at = $4,
            last_completed_at = $5,
            updated_at = NOW()
        WHERE background_job_id = $1
        "#,
    )
    .bind(background_job_id)
    .bind(update.status.as_str())
    .bind(update.lease_expires_at)
    .bind(update.last_started_at)
    .bind(update.last_completed_at)
    .execute(executor)
    .await
    .context("failed to update background job status")?;
    Ok(())
}

pub async fn insert_job_run<'e, E>(executor: E, run: &NewBackgroundJobRun) -> Result<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query(
        r#"
        INSERT INTO background_job_runs (
            background_job_run_id,
            background_job_id,
            trace_id,
            execution_id,
            lease_token,
            status,
            worker_pid,
            lease_acquired_at,
            lease_expires_at,
            started_at,
            completed_at,
            result_payload,
            failure_payload,
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
            $13,
            NOW(),
            NOW()
        )
        "#,
    )
    .bind(run.background_job_run_id)
    .bind(run.background_job_id)
    .bind(run.trace_id)
    .bind(run.execution_id)
    .bind(run.lease_token)
    .bind(run.status.as_str())
    .bind(run.worker_pid)
    .bind(run.lease_acquired_at)
    .bind(run.lease_expires_at)
    .bind(run.started_at)
    .bind(run.completed_at)
    .bind(&run.result_payload)
    .bind(&run.failure_payload)
    .execute(executor)
    .await
    .context("failed to insert background job run")?;
    Ok(())
}

pub async fn get_job_run(
    pool: &PgPool,
    background_job_run_id: Uuid,
) -> Result<BackgroundJobRunRecord> {
    let row = sqlx::query(
        r#"
        SELECT
            background_job_run_id,
            background_job_id,
            trace_id,
            execution_id,
            lease_token,
            status,
            worker_pid,
            lease_acquired_at,
            lease_expires_at,
            started_at,
            completed_at,
            result_payload,
            failure_payload,
            created_at,
            updated_at
        FROM background_job_runs
        WHERE background_job_run_id = $1
        "#,
    )
    .bind(background_job_run_id)
    .fetch_one(pool)
    .await
    .context("failed to fetch background job run")?;

    decode_background_job_run_row(row)
}

pub async fn list_active_job_runs(
    pool: &PgPool,
    background_job_id: Uuid,
) -> Result<Vec<BackgroundJobRunRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT
            background_job_run_id,
            background_job_id,
            trace_id,
            execution_id,
            lease_token,
            status,
            worker_pid,
            lease_acquired_at,
            lease_expires_at,
            started_at,
            completed_at,
            result_payload,
            failure_payload,
            created_at,
            updated_at
        FROM background_job_runs
        WHERE background_job_id = $1
          AND status IN ('leased', 'running')
        ORDER BY lease_acquired_at DESC, background_job_run_id DESC
        "#,
    )
    .bind(background_job_id)
    .fetch_all(pool)
    .await
    .context("failed to list active background job runs")?;

    rows.into_iter()
        .map(decode_background_job_run_row)
        .collect()
}

pub async fn list_completed_job_runs(
    pool: &PgPool,
    background_job_id: Uuid,
    limit: u32,
) -> Result<Vec<BackgroundJobRunRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT
            background_job_run_id,
            background_job_id,
            trace_id,
            execution_id,
            lease_token,
            status,
            worker_pid,
            lease_acquired_at,
            lease_expires_at,
            started_at,
            completed_at,
            result_payload,
            failure_payload,
            created_at,
            updated_at
        FROM background_job_runs
        WHERE background_job_id = $1
          AND status IN ('completed', 'failed', 'timed_out')
        ORDER BY completed_at DESC, background_job_run_id DESC
        LIMIT $2
        "#,
    )
    .bind(background_job_id)
    .bind(i64::from(limit))
    .fetch_all(pool)
    .await
    .context("failed to list completed background job runs")?;

    rows.into_iter()
        .map(decode_background_job_run_row)
        .collect()
}

pub async fn update_job_run_status<'e, E>(
    executor: E,
    background_job_run_id: Uuid,
    update: &UpdateBackgroundJobRunStatus,
) -> Result<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query(
        r#"
        UPDATE background_job_runs
        SET
            status = $2,
            worker_pid = $3,
            started_at = $4,
            completed_at = $5,
            result_payload = $6,
            failure_payload = $7,
            updated_at = NOW()
        WHERE background_job_run_id = $1
        "#,
    )
    .bind(background_job_run_id)
    .bind(update.status.as_str())
    .bind(update.worker_pid)
    .bind(update.started_at)
    .bind(update.completed_at)
    .bind(&update.result_payload)
    .bind(&update.failure_payload)
    .execute(executor)
    .await
    .context("failed to update background job run status")?;
    Ok(())
}

pub async fn insert_wake_signal<'e, E>(executor: E, signal: &NewWakeSignalRecord) -> Result<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query(
        r#"
        INSERT INTO wake_signals (
            wake_signal_id,
            background_job_id,
            background_job_run_id,
            trace_id,
            execution_id,
            reason,
            priority,
            reason_code,
            summary,
            payload_ref,
            status,
            decision_kind,
            decision_reason,
            requested_at,
            reviewed_at,
            cooldown_until,
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
            NULL,
            NULL,
            $12,
            NULL,
            $13,
            NOW(),
            NOW()
        )
        "#,
    )
    .bind(signal.signal.signal_id)
    .bind(signal.background_job_id)
    .bind(signal.background_job_run_id)
    .bind(signal.trace_id)
    .bind(signal.execution_id)
    .bind(wake_signal_reason_as_str(signal.signal.reason))
    .bind(wake_signal_priority_as_str(signal.signal.priority))
    .bind(&signal.signal.reason_code)
    .bind(&signal.signal.summary)
    .bind(&signal.signal.payload_ref)
    .bind(signal.status.as_str())
    .bind(signal.requested_at)
    .bind(signal.cooldown_until)
    .execute(executor)
    .await
    .context("failed to insert wake signal")?;
    Ok(())
}

pub async fn get_wake_signal(pool: &PgPool, wake_signal_id: Uuid) -> Result<WakeSignalRecord> {
    let row = sqlx::query(
        r#"
        SELECT
            wake_signal_id,
            background_job_id,
            background_job_run_id,
            trace_id,
            execution_id,
            reason,
            priority,
            reason_code,
            summary,
            payload_ref,
            status,
            decision_kind,
            decision_reason,
            requested_at,
            reviewed_at,
            cooldown_until,
            created_at,
            updated_at
        FROM wake_signals
        WHERE wake_signal_id = $1
        "#,
    )
    .bind(wake_signal_id)
    .fetch_one(pool)
    .await
    .context("failed to fetch wake signal")?;

    decode_wake_signal_row(row)
}

pub async fn list_pending_wake_signals(pool: &PgPool, limit: u32) -> Result<Vec<WakeSignalRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT
            wake_signal_id,
            background_job_id,
            background_job_run_id,
            trace_id,
            execution_id,
            reason,
            priority,
            reason_code,
            summary,
            payload_ref,
            status,
            decision_kind,
            decision_reason,
            requested_at,
            reviewed_at,
            cooldown_until,
            created_at,
            updated_at
        FROM wake_signals
        WHERE status = 'pending_review'
        ORDER BY requested_at ASC, wake_signal_id ASC
        LIMIT $1
        "#,
    )
    .bind(i64::from(limit))
    .fetch_all(pool)
    .await
    .context("failed to list pending wake signals")?;

    rows.into_iter().map(decode_wake_signal_row).collect()
}

pub async fn count_open_wake_signals(pool: &PgPool, now: DateTime<Utc>) -> Result<u32> {
    let count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM wake_signals
        WHERE status IN ('pending_review', 'accepted', 'deferred')
          AND (cooldown_until IS NULL OR cooldown_until > $1)
        "#,
    )
    .bind(now)
    .fetch_one(pool)
    .await
    .context("failed to count open wake signals")?;

    u32::try_from(count).map_err(|_| anyhow!("wake signal count exceeded u32 range"))
}

pub async fn has_active_wake_signal_cooldown(
    pool: &PgPool,
    reason_code: &str,
    now: DateTime<Utc>,
    exclude_wake_signal_id: Uuid,
) -> Result<bool> {
    let exists = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM wake_signals
            WHERE reason_code = $1
              AND wake_signal_id <> $2
              AND cooldown_until IS NOT NULL
              AND cooldown_until > $3
        )
        "#,
    )
    .bind(reason_code)
    .bind(exclude_wake_signal_id)
    .bind(now)
    .fetch_one(pool)
    .await
    .context("failed to check active wake-signal cooldown")?;

    Ok(exists)
}

pub async fn record_wake_signal_decision(
    pool: &PgPool,
    wake_signal_id: Uuid,
    decision: &WakeSignalDecision,
    reviewed_at: DateTime<Utc>,
    cooldown_until: Option<DateTime<Utc>>,
) -> Result<()> {
    let status = wake_signal_status_for_decision(decision.decision);
    sqlx::query(
        r#"
        UPDATE wake_signals
        SET
            status = $2,
            decision_kind = $3,
            decision_reason = $4,
            reviewed_at = $5,
            cooldown_until = $6,
            updated_at = NOW()
        WHERE wake_signal_id = $1
        "#,
    )
    .bind(wake_signal_id)
    .bind(status.as_str())
    .bind(wake_signal_decision_kind_as_str(decision.decision))
    .bind(&decision.reason)
    .bind(reviewed_at)
    .bind(cooldown_until)
    .execute(pool)
    .await
    .context("failed to record wake signal decision")?;
    Ok(())
}

fn decode_background_job_row(row: sqlx::postgres::PgRow) -> Result<BackgroundJobRecord> {
    let job_kind_value: String = row.get("job_kind");
    let trigger_kind_value: String = row.get("trigger_kind");
    let status_value: String = row.get("status");
    let job_kind = parse_job_kind(&job_kind_value)?;
    let trigger_kind = parse_background_trigger_kind(&trigger_kind_value)?;
    let scope: UnconsciousScope = serde_json::from_value(row.get("scope_json"))
        .context("failed to decode background job scope")?;
    let iteration_budget: i32 = row.get("iteration_budget");
    let wall_clock_budget_ms: i64 = row.get("wall_clock_budget_ms");
    let token_budget: i32 = row.get("token_budget");

    Ok(BackgroundJobRecord {
        background_job_id: row.get("background_job_id"),
        trace_id: row.get("trace_id"),
        job_kind,
        trigger: BackgroundTrigger {
            trigger_id: row.get("trigger_id"),
            trigger_kind,
            requested_at: row.get("trigger_requested_at"),
            reason_summary: row.get("trigger_reason_summary"),
            payload_ref: row.get("trigger_payload_ref"),
        },
        deduplication_key: row.get("deduplication_key"),
        scope,
        budget: BackgroundExecutionBudget {
            iteration_budget: u32::try_from(iteration_budget)
                .map_err(|_| anyhow!("negative iteration budget stored in background_jobs"))?,
            wall_clock_budget_ms: u64::try_from(wall_clock_budget_ms)
                .map_err(|_| anyhow!("negative wall-clock budget stored in background_jobs"))?,
            token_budget: u32::try_from(token_budget)
                .map_err(|_| anyhow!("negative token budget stored in background_jobs"))?,
        },
        status: BackgroundJobStatus::parse(&status_value)?,
        available_at: row.get("available_at"),
        lease_expires_at: row.get("lease_expires_at"),
        last_started_at: row.get("last_started_at"),
        last_completed_at: row.get("last_completed_at"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

fn decode_background_job_run_row(row: sqlx::postgres::PgRow) -> Result<BackgroundJobRunRecord> {
    let status_value: String = row.get("status");
    Ok(BackgroundJobRunRecord {
        background_job_run_id: row.get("background_job_run_id"),
        background_job_id: row.get("background_job_id"),
        trace_id: row.get("trace_id"),
        execution_id: row.get("execution_id"),
        lease_token: row.get("lease_token"),
        status: BackgroundJobRunStatus::parse(&status_value)?,
        worker_pid: row.get("worker_pid"),
        lease_acquired_at: row.get("lease_acquired_at"),
        lease_expires_at: row.get("lease_expires_at"),
        started_at: row.get("started_at"),
        completed_at: row.get("completed_at"),
        result_payload: row.get("result_payload"),
        failure_payload: row.get("failure_payload"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

fn decode_wake_signal_row(row: sqlx::postgres::PgRow) -> Result<WakeSignalRecord> {
    let wake_signal_id: Uuid = row.get("wake_signal_id");
    let reason_value: String = row.get("reason");
    let priority_value: String = row.get("priority");
    let status_value: String = row.get("status");
    let decision_kind = row.get::<Option<String>, _>("decision_kind");
    let decision = match decision_kind {
        Some(decision_kind) => Some(WakeSignalDecision {
            signal_id: wake_signal_id,
            decision: parse_wake_signal_decision_kind(&decision_kind)?,
            reason: row
                .get::<Option<String>, _>("decision_reason")
                .context("reviewed wake signal is missing decision reason")?,
        }),
        None => None,
    };

    Ok(WakeSignalRecord {
        wake_signal_id,
        background_job_id: row.get("background_job_id"),
        background_job_run_id: row.get("background_job_run_id"),
        trace_id: row.get("trace_id"),
        execution_id: row.get("execution_id"),
        signal: WakeSignal {
            signal_id: wake_signal_id,
            reason: parse_wake_signal_reason(&reason_value)?,
            priority: parse_wake_signal_priority(&priority_value)?,
            reason_code: row.get("reason_code"),
            summary: row.get("summary"),
            payload_ref: row.get("payload_ref"),
        },
        status: WakeSignalStatus::parse(&status_value)?,
        decision,
        requested_at: row.get("requested_at"),
        reviewed_at: row.get("reviewed_at"),
        cooldown_until: row.get("cooldown_until"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

fn job_kind_as_str(value: UnconsciousJobKind) -> &'static str {
    match value {
        UnconsciousJobKind::MemoryConsolidation => "memory_consolidation",
        UnconsciousJobKind::RetrievalMaintenance => "retrieval_maintenance",
        UnconsciousJobKind::ContradictionAndDriftScan => "contradiction_and_drift_scan",
        UnconsciousJobKind::SelfModelReflection => "self_model_reflection",
    }
}

fn parse_job_kind(value: &str) -> Result<UnconsciousJobKind> {
    match value {
        "memory_consolidation" => Ok(UnconsciousJobKind::MemoryConsolidation),
        "retrieval_maintenance" => Ok(UnconsciousJobKind::RetrievalMaintenance),
        "contradiction_and_drift_scan" => Ok(UnconsciousJobKind::ContradictionAndDriftScan),
        "self_model_reflection" => Ok(UnconsciousJobKind::SelfModelReflection),
        other => bail!("unrecognized unconscious job kind '{other}'"),
    }
}

fn background_trigger_kind_as_str(value: BackgroundTriggerKind) -> &'static str {
    match value {
        BackgroundTriggerKind::TimeSchedule => "time_schedule",
        BackgroundTriggerKind::VolumeThreshold => "volume_threshold",
        BackgroundTriggerKind::DriftOrAnomalySignal => "drift_or_anomaly_signal",
        BackgroundTriggerKind::ForegroundDelegation => "foreground_delegation",
        BackgroundTriggerKind::ExternalPassiveEvent => "external_passive_event",
        BackgroundTriggerKind::MaintenanceTrigger => "maintenance_trigger",
    }
}

fn parse_background_trigger_kind(value: &str) -> Result<BackgroundTriggerKind> {
    match value {
        "time_schedule" => Ok(BackgroundTriggerKind::TimeSchedule),
        "volume_threshold" => Ok(BackgroundTriggerKind::VolumeThreshold),
        "drift_or_anomaly_signal" => Ok(BackgroundTriggerKind::DriftOrAnomalySignal),
        "foreground_delegation" => Ok(BackgroundTriggerKind::ForegroundDelegation),
        "external_passive_event" => Ok(BackgroundTriggerKind::ExternalPassiveEvent),
        "maintenance_trigger" => Ok(BackgroundTriggerKind::MaintenanceTrigger),
        other => bail!("unrecognized background trigger kind '{other}'"),
    }
}

fn wake_signal_reason_as_str(value: WakeSignalReason) -> &'static str {
    match value {
        WakeSignalReason::CriticalConflict => "critical_conflict",
        WakeSignalReason::ProactiveBriefingReady => "proactive_briefing_ready",
        WakeSignalReason::SelfStateAnomaly => "self_state_anomaly",
        WakeSignalReason::MaintenanceInsightReady => "maintenance_insight_ready",
    }
}

fn parse_wake_signal_reason(value: &str) -> Result<WakeSignalReason> {
    match value {
        "critical_conflict" => Ok(WakeSignalReason::CriticalConflict),
        "proactive_briefing_ready" => Ok(WakeSignalReason::ProactiveBriefingReady),
        "self_state_anomaly" => Ok(WakeSignalReason::SelfStateAnomaly),
        "maintenance_insight_ready" => Ok(WakeSignalReason::MaintenanceInsightReady),
        other => bail!("unrecognized wake signal reason '{other}'"),
    }
}

fn wake_signal_priority_as_str(value: WakeSignalPriority) -> &'static str {
    match value {
        WakeSignalPriority::Low => "low",
        WakeSignalPriority::Normal => "normal",
        WakeSignalPriority::High => "high",
    }
}

fn parse_wake_signal_priority(value: &str) -> Result<WakeSignalPriority> {
    match value {
        "low" => Ok(WakeSignalPriority::Low),
        "normal" => Ok(WakeSignalPriority::Normal),
        "high" => Ok(WakeSignalPriority::High),
        other => bail!("unrecognized wake signal priority '{other}'"),
    }
}

fn wake_signal_decision_kind_as_str(value: WakeSignalDecisionKind) -> &'static str {
    match value {
        WakeSignalDecisionKind::Accepted => "accepted",
        WakeSignalDecisionKind::Rejected => "rejected",
        WakeSignalDecisionKind::Suppressed => "suppressed",
        WakeSignalDecisionKind::Deferred => "deferred",
    }
}

fn parse_wake_signal_decision_kind(value: &str) -> Result<WakeSignalDecisionKind> {
    match value {
        "accepted" => Ok(WakeSignalDecisionKind::Accepted),
        "rejected" => Ok(WakeSignalDecisionKind::Rejected),
        "suppressed" => Ok(WakeSignalDecisionKind::Suppressed),
        "deferred" => Ok(WakeSignalDecisionKind::Deferred),
        other => bail!("unrecognized wake signal decision kind '{other}'"),
    }
}

fn wake_signal_status_for_decision(decision: WakeSignalDecisionKind) -> WakeSignalStatus {
    match decision {
        WakeSignalDecisionKind::Accepted => WakeSignalStatus::Accepted,
        WakeSignalDecisionKind::Rejected => WakeSignalStatus::Rejected,
        WakeSignalDecisionKind::Suppressed => WakeSignalStatus::Suppressed,
        WakeSignalDecisionKind::Deferred => WakeSignalStatus::Deferred,
    }
}
