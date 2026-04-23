use std::{collections::BTreeMap, env, path::PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Duration, Utc};
use contracts::{BackgroundTrigger, BackgroundTriggerKind, UnconsciousJobKind};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::{
    approval, background_execution,
    background_planning::{self, BackgroundPlanningDecision, BackgroundPlanningRequest},
    config::RuntimeConfig,
    db, governed_actions, migration, model_gateway, recovery,
    schema::{self, SchemaCompatibility, SchemaPolicy},
    worker, workspace,
};

const DEFAULT_LIST_LIMIT: u32 = 20;
const HEALTH_RECENT_WINDOW_MINUTES: i64 = 60;
const HEALTH_TOP_REASON_LIMIT: usize = 5;
const HEALTH_RECENT_BASE_DIAGNOSTIC_LIMIT: i64 = 200;
const ANOMALY_REPEAT_THRESHOLD: usize = 3;
const ANOMALY_FAILURE_PRESSURE_THRESHOLD: usize = 2;
const ANOMALY_DEDUPE_WINDOW_MINUTES: i64 = 15;
const LEASE_AT_RISK_WINDOW_SECONDS: i64 = 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStatusReport {
    pub schema: SchemaStatusReport,
    pub worker: WorkerStatusReport,
    pub telegram: TelegramStatusReport,
    pub model_gateway: ModelGatewayStatusReport,
    pub self_model: SelfModelStatusReport,
    pub pending_work: PendingWorkSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationalHealthSummary {
    pub evaluated_at: DateTime<Utc>,
    pub overall_status: String,
    pub pending_work: PendingWorkSummary,
    pub recovery: RecoveryHealthSummary,
    pub diagnostics: DiagnosticHealthSummary,
    pub anomalies: Vec<OperationalAnomalySummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryHealthSummary {
    pub open_checkpoint_count: u32,
    pub open_foreground_checkpoint_count: u32,
    pub open_background_checkpoint_count: u32,
    pub open_governed_action_checkpoint_count: u32,
    pub recent_resolved_checkpoint_count: u32,
    pub recent_abandoned_checkpoint_count: u32,
    pub active_worker_lease_count: u32,
    pub overdue_active_worker_lease_count: u32,
    pub at_risk_active_worker_lease_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticHealthSummary {
    pub recent_window_minutes: u32,
    pub observed_count: u32,
    pub info_count: u32,
    pub warn_count: u32,
    pub error_count: u32,
    pub critical_count: u32,
    pub top_reason_codes: Vec<OperationalReasonRollup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationalReasonRollup {
    pub reason_code: String,
    pub count: u32,
    pub latest_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationalAnomalySummary {
    pub anomaly_kind: String,
    pub severity: String,
    pub reason_code: String,
    pub summary: String,
    pub occurrence_count: u32,
    pub latest_trace_id: Option<Uuid>,
    pub latest_execution_id: Option<Uuid>,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationalDiagnosticSummary {
    pub operational_diagnostic_id: Uuid,
    pub trace_id: Option<Uuid>,
    pub execution_id: Option<Uuid>,
    pub subsystem: String,
    pub severity: String,
    pub reason_code: String,
    pub summary: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryCheckpointSummary {
    pub recovery_checkpoint_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub background_job_id: Option<Uuid>,
    pub background_job_run_id: Option<Uuid>,
    pub governed_action_execution_id: Option<Uuid>,
    pub approval_request_id: Option<Uuid>,
    pub checkpoint_kind: String,
    pub recovery_reason_code: String,
    pub status: String,
    pub recovery_decision: Option<String>,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaStatusReport {
    pub compatibility: String,
    pub current_version: Option<i64>,
    pub expected_version: i64,
    pub minimum_supported_version: i64,
    pub applied_migration_count: usize,
    pub history_valid: bool,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerStatusReport {
    pub resolution_kind: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub timeout_ms: u64,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramStatusReport {
    pub configured: bool,
    pub binding_present: bool,
    pub binding_internal_conversation_ref: Option<String>,
    pub binding_internal_principal_ref: Option<String>,
    pub bot_token_env: Option<String>,
    pub bot_token_present: bool,
    pub poll_limit: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelGatewayStatusReport {
    pub configured: bool,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_base_url: Option<String>,
    pub api_key_env: Option<String>,
    pub api_key_present: bool,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfModelStatusReport {
    pub configured: bool,
    pub seed_path: Option<String>,
    pub seed_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingWorkSummary {
    pub pending_foreground_conversation_count: usize,
    pub pending_background_job_count: u32,
    pub due_background_job_count: u32,
    pub pending_wake_signal_count: u32,
    pub pending_approval_request_count: u32,
    pub awaiting_approval_governed_action_count: u32,
    pub blocked_governed_action_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingForegroundConversationSummary {
    pub internal_conversation_ref: String,
    pub pending_count: u32,
    pub oldest_occurred_at: DateTime<Utc>,
    pub newest_occurred_at: DateTime<Utc>,
    pub oldest_touch_at: DateTime<Utc>,
    pub pending_span_seconds: u64,
    pub stale_pending_age_seconds: u64,
    pub includes_stale_processing: bool,
    pub suggested_mode: String,
    pub decision_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundJobSummary {
    pub background_job_id: Uuid,
    pub trace_id: Uuid,
    pub job_kind: String,
    pub trigger_kind: String,
    pub status: String,
    pub available_at: DateTime<Utc>,
    pub last_started_at: Option<DateTime<Utc>>,
    pub last_completed_at: Option<DateTime<Utc>>,
    pub internal_conversation_ref: Option<String>,
    pub scope_summary: String,
    pub latest_run_status: Option<String>,
    pub latest_run_completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakeSignalSummary {
    pub wake_signal_id: Uuid,
    pub background_job_id: Uuid,
    pub reason_code: String,
    pub reason: String,
    pub priority: String,
    pub status: String,
    pub decision_kind: Option<String>,
    pub requested_at: DateTime<Utc>,
    pub reviewed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequestSummary {
    pub approval_request_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub action_proposal_id: Uuid,
    pub action_fingerprint: String,
    pub action_kind: String,
    pub risk_tier: String,
    pub capability_scope: contracts::CapabilityScope,
    pub status: String,
    pub title: String,
    pub consequence_summary: String,
    pub requested_by: String,
    pub requested_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub resolution_kind: Option<String>,
    pub resolved_by: Option<String>,
    pub resolution_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernedActionSummary {
    pub governed_action_execution_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub approval_request_id: Option<Uuid>,
    pub action_proposal_id: Uuid,
    pub action_fingerprint: String,
    pub action_kind: String,
    pub risk_tier: String,
    pub status: String,
    pub workspace_script_id: Option<Uuid>,
    pub workspace_script_version_id: Option<Uuid>,
    pub blocked_reason: Option<String>,
    pub output_ref: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceScriptRunSummary {
    pub workspace_script_run_id: Uuid,
    pub workspace_script_id: Uuid,
    pub workspace_script_version_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub governed_action_execution_id: Option<Uuid>,
    pub approval_request_id: Option<Uuid>,
    pub status: String,
    pub risk_tier: String,
    pub args: Vec<String>,
    pub output_ref: Option<String>,
    pub failure_summary: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct ResolveApprovalRequest {
    pub approval_request_id: Uuid,
    pub decision: contracts::ApprovalResolutionDecision,
    pub actor_ref: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalResolutionSummary {
    pub approval_request: ApprovalRequestSummary,
    pub governed_action: Option<GovernedActionSummary>,
}

#[derive(Debug, Clone)]
pub struct EnqueueBackgroundJobRequest {
    pub job_kind: UnconsciousJobKind,
    pub trigger_kind: BackgroundTriggerKind,
    pub internal_conversation_ref: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum BackgroundEnqueueOutcome {
    Planned {
        background_job_id: Uuid,
        deduplication_key: String,
        scope_summary: String,
    },
    SuppressedDuplicate {
        existing_job_id: Uuid,
        deduplication_key: String,
        reason: String,
    },
    Rejected {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum BackgroundRunNextOutcome {
    NoDueJob,
    Completed {
        background_job_id: Uuid,
        execution_id: Uuid,
        trace_id: Uuid,
        summary: String,
    },
}

pub async fn load_runtime_status(config: &RuntimeConfig) -> Result<RuntimeStatusReport> {
    let pool = db::connect(config).await?;
    let schema = inspect_schema_status(&pool, config).await?;
    let pending_work = load_pending_work_summary(&pool, config).await?;
    Ok(RuntimeStatusReport {
        schema,
        worker: inspect_worker_status(config),
        telegram: inspect_telegram_status(config),
        model_gateway: inspect_model_gateway_status(config),
        self_model: inspect_self_model_status(config),
        pending_work,
    })
}

pub async fn load_operational_health_summary(
    config: &RuntimeConfig,
) -> Result<OperationalHealthSummary> {
    let pool = db::connect(config).await?;
    let now = Utc::now();
    let recent_window_start = now - Duration::minutes(HEALTH_RECENT_WINDOW_MINUTES);
    let pending_work = load_pending_work_summary(&pool, config).await?;
    let recovery = load_recovery_health_summary(&pool, now, recent_window_start).await?;
    let base_diagnostics = load_recent_base_diagnostics(&pool, recent_window_start).await?;
    record_operational_anomaly_rollups(&pool, now, &recovery, &base_diagnostics).await?;
    let diagnostics = summarize_diagnostic_health(&base_diagnostics);
    let anomalies = load_recent_operational_anomalies(&pool, recent_window_start).await?;

    Ok(OperationalHealthSummary {
        evaluated_at: now,
        overall_status: classify_overall_health(&pending_work, &recovery, &diagnostics),
        pending_work,
        recovery,
        diagnostics,
        anomalies,
    })
}

pub async fn list_recent_operational_diagnostics(
    config: &RuntimeConfig,
    limit: u32,
) -> Result<Vec<OperationalDiagnosticSummary>> {
    let pool = db::connect(config).await?;
    let rows = sqlx::query(
        r#"
        SELECT
            operational_diagnostic_id,
            trace_id,
            execution_id,
            subsystem,
            severity,
            reason_code,
            summary,
            created_at
        FROM operational_diagnostics
        ORDER BY created_at DESC, operational_diagnostic_id DESC
        LIMIT $1
        "#,
    )
    .bind(i64::from(limit))
    .fetch_all(&pool)
    .await
    .context("failed to list recent operational diagnostics for management")?;

    Ok(rows
        .into_iter()
        .map(|row| OperationalDiagnosticSummary {
            operational_diagnostic_id: row.get("operational_diagnostic_id"),
            trace_id: row.get("trace_id"),
            execution_id: row.get("execution_id"),
            subsystem: row.get("subsystem"),
            severity: row.get("severity"),
            reason_code: row.get("reason_code"),
            summary: row.get("summary"),
            created_at: row.get("created_at"),
        })
        .collect())
}

pub async fn list_recovery_checkpoints(
    config: &RuntimeConfig,
    open_only: bool,
    limit: u32,
) -> Result<Vec<RecoveryCheckpointSummary>> {
    let pool = db::connect(config).await?;
    let rows = if open_only {
        sqlx::query(
            r#"
            SELECT
                recovery_checkpoint_id,
                trace_id,
                execution_id,
                background_job_id,
                background_job_run_id,
                governed_action_execution_id,
                approval_request_id,
                checkpoint_kind,
                recovery_reason_code,
                status,
                recovery_decision,
                created_at,
                resolved_at
            FROM recovery_checkpoints
            WHERE status = 'open'
            ORDER BY created_at DESC, recovery_checkpoint_id DESC
            LIMIT $1
            "#,
        )
        .bind(i64::from(limit))
        .fetch_all(&pool)
        .await
        .context("failed to list open recovery checkpoints for management")?
    } else {
        sqlx::query(
            r#"
            SELECT
                recovery_checkpoint_id,
                trace_id,
                execution_id,
                background_job_id,
                background_job_run_id,
                governed_action_execution_id,
                approval_request_id,
                checkpoint_kind,
                recovery_reason_code,
                status,
                recovery_decision,
                created_at,
                resolved_at
            FROM recovery_checkpoints
            ORDER BY created_at DESC, recovery_checkpoint_id DESC
            LIMIT $1
            "#,
        )
        .bind(i64::from(limit))
        .fetch_all(&pool)
        .await
        .context("failed to list recovery checkpoints for management")?
    };

    Ok(rows
        .into_iter()
        .map(|row| RecoveryCheckpointSummary {
            recovery_checkpoint_id: row.get("recovery_checkpoint_id"),
            trace_id: row.get("trace_id"),
            execution_id: row.get("execution_id"),
            background_job_id: row.get("background_job_id"),
            background_job_run_id: row.get("background_job_run_id"),
            governed_action_execution_id: row.get("governed_action_execution_id"),
            approval_request_id: row.get("approval_request_id"),
            checkpoint_kind: row.get("checkpoint_kind"),
            recovery_reason_code: row.get("recovery_reason_code"),
            status: row.get("status"),
            recovery_decision: row.get("recovery_decision"),
            created_at: row.get("created_at"),
            resolved_at: row.get("resolved_at"),
        })
        .collect())
}

pub async fn list_pending_foreground_conversations(
    config: &RuntimeConfig,
    limit: u32,
) -> Result<Vec<PendingForegroundConversationSummary>> {
    let pool = db::connect(config).await?;
    let stale_cutoff = foreground_stale_cutoff(config);
    let rows = sqlx::query(
        r#"
        SELECT
            internal_conversation_ref,
            COUNT(*) AS pending_count,
            MIN(occurred_at) AS oldest_occurred_at,
            MAX(occurred_at) AS newest_occurred_at,
            MIN(COALESCE(last_processed_at, received_at)) AS oldest_touch_at,
            BOOL_OR(foreground_status = 'processing') AS includes_stale_processing
        FROM ingress_events
        WHERE internal_conversation_ref IS NOT NULL
          AND status = 'accepted'
          AND (
              foreground_status = 'pending'
              OR (
                  foreground_status = 'processing'
                  AND COALESCE(last_processed_at, received_at) <= $1
              )
          )
        GROUP BY internal_conversation_ref
        ORDER BY oldest_occurred_at ASC, internal_conversation_ref ASC
        LIMIT $2
        "#,
    )
    .bind(stale_cutoff)
    .bind(i64::from(limit))
    .fetch_all(&pool)
    .await
    .context("failed to list pending foreground conversations")?;

    let now = Utc::now();
    Ok(rows
        .into_iter()
        .map(|row| {
            let pending_count = row.get::<i64, _>("pending_count");
            let oldest_occurred_at: DateTime<Utc> = row.get("oldest_occurred_at");
            let newest_occurred_at: DateTime<Utc> = row.get("newest_occurred_at");
            let oldest_touch_at: DateTime<Utc> = row.get("oldest_touch_at");
            let pending_span_seconds = newest_occurred_at
                .signed_duration_since(oldest_occurred_at)
                .num_seconds()
                .max(0) as u64;
            let stale_pending_age_seconds = now
                .signed_duration_since(oldest_touch_at)
                .num_seconds()
                .max(0) as u64;
            let includes_stale_processing = row.get::<bool, _>("includes_stale_processing");
            let (suggested_mode, decision_reason) = classify_pending_foreground_summary(
                config,
                pending_count as usize,
                pending_span_seconds,
                stale_pending_age_seconds,
                includes_stale_processing,
            );

            PendingForegroundConversationSummary {
                internal_conversation_ref: row.get("internal_conversation_ref"),
                pending_count: pending_count as u32,
                oldest_occurred_at,
                newest_occurred_at,
                oldest_touch_at,
                pending_span_seconds,
                stale_pending_age_seconds,
                includes_stale_processing,
                suggested_mode: suggested_mode.to_string(),
                decision_reason: decision_reason.to_string(),
            }
        })
        .collect())
}

pub async fn list_background_jobs(
    config: &RuntimeConfig,
    limit: u32,
) -> Result<Vec<BackgroundJobSummary>> {
    let pool = db::connect(config).await?;
    let rows = sqlx::query(
        r#"
        SELECT
            job.background_job_id,
            job.trace_id,
            job.job_kind,
            job.trigger_kind,
            job.status,
            job.available_at,
            job.last_started_at,
            job.last_completed_at,
            job.scope_summary,
            job.scope_json,
            latest_run.status AS latest_run_status,
            latest_run.completed_at AS latest_run_completed_at
        FROM background_jobs job
        LEFT JOIN LATERAL (
            SELECT status, completed_at
            FROM background_job_runs
            WHERE background_job_id = job.background_job_id
            ORDER BY COALESCE(completed_at, started_at, lease_acquired_at) DESC,
                     background_job_run_id DESC
            LIMIT 1
        ) latest_run ON TRUE
        ORDER BY job.created_at DESC, job.background_job_id DESC
        LIMIT $1
        "#,
    )
    .bind(i64::from(limit))
    .fetch_all(&pool)
    .await
    .context("failed to list background jobs for management CLI")?;

    rows.into_iter()
        .map(|row| {
            let scope: contracts::UnconsciousScope = serde_json::from_value(row.get("scope_json"))
                .context("failed to decode background job scope for management CLI")?;

            Ok(BackgroundJobSummary {
                background_job_id: row.get("background_job_id"),
                trace_id: row.get("trace_id"),
                job_kind: row.get("job_kind"),
                trigger_kind: row.get("trigger_kind"),
                status: row.get("status"),
                available_at: row.get("available_at"),
                last_started_at: row.get("last_started_at"),
                last_completed_at: row.get("last_completed_at"),
                internal_conversation_ref: scope.internal_conversation_ref,
                scope_summary: row.get("scope_summary"),
                latest_run_status: row.get("latest_run_status"),
                latest_run_completed_at: row.get("latest_run_completed_at"),
            })
        })
        .collect()
}

pub async fn enqueue_background_job(
    config: &RuntimeConfig,
    request: EnqueueBackgroundJobRequest,
) -> Result<BackgroundEnqueueOutcome> {
    let pool = db::connect(config).await?;
    verify_schema(&pool, config).await?;
    let trigger = BackgroundTrigger {
        trigger_id: Uuid::now_v7(),
        trigger_kind: request.trigger_kind,
        requested_at: Utc::now(),
        reason_summary: request
            .reason
            .clone()
            .unwrap_or_else(|| "manual management CLI enqueue".to_string()),
        payload_ref: request.internal_conversation_ref.clone(),
    };
    let decision = background_planning::plan_background_job(
        &pool,
        config,
        BackgroundPlanningRequest {
            trace_id: Uuid::now_v7(),
            job_kind: request.job_kind,
            trigger,
            internal_conversation_ref: request.internal_conversation_ref,
            available_at: Utc::now(),
        },
    )
    .await?;

    Ok(match decision {
        BackgroundPlanningDecision::Planned(job) => BackgroundEnqueueOutcome::Planned {
            background_job_id: job.background_job_id,
            deduplication_key: job.deduplication_key,
            scope_summary: job.scope.summary,
        },
        BackgroundPlanningDecision::SuppressedDuplicate {
            existing_job_id,
            deduplication_key,
            reason,
        } => BackgroundEnqueueOutcome::SuppressedDuplicate {
            existing_job_id,
            deduplication_key,
            reason,
        },
        BackgroundPlanningDecision::Rejected { reason } => {
            BackgroundEnqueueOutcome::Rejected { reason }
        }
    })
}

pub async fn run_next_background_job(config: &RuntimeConfig) -> Result<BackgroundRunNextOutcome> {
    let transport = model_gateway::ReqwestModelProviderTransport::new();
    run_next_background_job_with_transport(config, &transport).await
}

pub async fn run_next_background_job_with_transport<T: model_gateway::ModelProviderTransport>(
    config: &RuntimeConfig,
    transport: &T,
) -> Result<BackgroundRunNextOutcome> {
    let pool = db::connect(config).await?;
    verify_schema(&pool, config).await?;
    let gateway = config.require_model_gateway_config()?;

    let outcome =
        background_execution::execute_next_due_job(&pool, config, &gateway, transport, Utc::now())
            .await?;

    Ok(match outcome {
        Some(result) => BackgroundRunNextOutcome::Completed {
            background_job_id: result.background_job_id,
            execution_id: result.execution_id,
            trace_id: result.trace_id,
            summary: result.summary,
        },
        None => BackgroundRunNextOutcome::NoDueJob,
    })
}

pub async fn list_wake_signals(
    config: &RuntimeConfig,
    limit: u32,
) -> Result<Vec<WakeSignalSummary>> {
    let pool = db::connect(config).await?;
    let rows = sqlx::query(
        r#"
        SELECT
            wake_signal_id,
            background_job_id,
            reason,
            priority,
            reason_code,
            status,
            decision_kind,
            requested_at,
            reviewed_at
        FROM wake_signals
        ORDER BY requested_at DESC, wake_signal_id DESC
        LIMIT $1
        "#,
    )
    .bind(i64::from(limit))
    .fetch_all(&pool)
    .await
    .context("failed to list wake signals for management CLI")?;

    Ok(rows
        .into_iter()
        .map(|row| WakeSignalSummary {
            wake_signal_id: row.get("wake_signal_id"),
            background_job_id: row.get("background_job_id"),
            reason_code: row.get("reason_code"),
            reason: row.get("reason"),
            priority: row.get("priority"),
            status: row.get("status"),
            decision_kind: row.get("decision_kind"),
            requested_at: row.get("requested_at"),
            reviewed_at: row.get("reviewed_at"),
        })
        .collect())
}

pub async fn list_approval_requests(
    config: &RuntimeConfig,
    status: Option<contracts::ApprovalRequestStatus>,
    limit: u32,
) -> Result<Vec<ApprovalRequestSummary>> {
    let pool = db::connect(config).await?;
    approval::list_approval_requests(&pool, status, i64::from(limit))
        .await?
        .into_iter()
        .map(|record| Ok(approval_request_summary(&record)))
        .collect()
}

pub async fn resolve_approval_request(
    config: &RuntimeConfig,
    request: ResolveApprovalRequest,
) -> Result<ApprovalResolutionSummary> {
    if !config.approvals.allow_cli_resolution {
        bail!("CLI approval resolution is disabled by configuration");
    }

    let pool = db::connect(config).await?;
    verify_schema(&pool, config).await?;

    let approval_request =
        approval::get_approval_request(&pool, request.approval_request_id).await?;
    let actor_ref = request
        .actor_ref
        .unwrap_or_else(|| default_cli_actor_ref(&approval_request.requested_by));
    let resolution = approval::resolve_approval_request(
        &pool,
        &approval::ApprovalResolutionAttempt {
            token: approval_request.token.clone(),
            actor_ref,
            expected_action_fingerprint: approval_request.action_fingerprint.clone(),
            decision: request.decision,
            reason: request.reason,
            resolved_at: Utc::now(),
        },
    )
    .await?;

    let governed_action =
        match governed_actions::get_governed_action_execution_by_approval_request_id(
            &pool,
            resolution.request.approval_request_id,
        )
        .await?
        {
            Some(record) => {
                let synced = governed_actions::sync_status_from_approval_resolution(
                    &pool,
                    record.governed_action_execution_id,
                    resolution.event.decision,
                    resolution.request.execution_id,
                    resolution.event.reason.as_deref(),
                )
                .await?;

                let record = if resolution.event.decision
                    == contracts::ApprovalResolutionDecision::Approved
                {
                    governed_actions::execute_governed_action(config, &pool, &synced)
                        .await?
                        .record
                } else {
                    synced
                };
                Some(governed_action_summary(&record))
            }
            None => None,
        };

    Ok(ApprovalResolutionSummary {
        approval_request: approval_request_summary(&resolution.request),
        governed_action,
    })
}

pub async fn list_governed_actions(
    config: &RuntimeConfig,
    status: Option<contracts::GovernedActionStatus>,
    limit: u32,
) -> Result<Vec<GovernedActionSummary>> {
    let pool = db::connect(config).await?;
    governed_actions::list_governed_action_executions(&pool, status, i64::from(limit))
        .await?
        .into_iter()
        .map(|record| Ok(governed_action_summary(&record)))
        .collect()
}

pub async fn list_workspace_artifact_summaries(
    config: &RuntimeConfig,
    limit: u32,
) -> Result<Vec<contracts::WorkspaceArtifactSummary>> {
    let pool = db::connect(config).await?;
    workspace::list_workspace_artifact_summaries(&pool, i64::from(limit)).await
}

pub async fn list_workspace_scripts(
    config: &RuntimeConfig,
    limit: u32,
) -> Result<Vec<contracts::WorkspaceScriptSummary>> {
    let pool = db::connect(config).await?;
    workspace::list_workspace_scripts(&pool, i64::from(limit)).await
}

pub async fn list_workspace_script_runs(
    config: &RuntimeConfig,
    workspace_script_id: Option<Uuid>,
    limit: u32,
) -> Result<Vec<WorkspaceScriptRunSummary>> {
    let pool = db::connect(config).await?;
    workspace::list_workspace_script_run_records(&pool, workspace_script_id, i64::from(limit))
        .await?
        .into_iter()
        .map(|record| Ok(workspace_script_run_summary(&record)))
        .collect()
}

pub fn default_list_limit() -> u32 {
    DEFAULT_LIST_LIMIT
}

fn inspect_worker_status(config: &RuntimeConfig) -> WorkerStatusReport {
    let resolution = worker::inspect_resolution(config);
    WorkerStatusReport {
        resolution_kind: resolution.resolution_kind.as_str().to_string(),
        command: resolution.command,
        args: resolution.args,
        timeout_ms: config.worker.timeout_ms,
        notes: resolution.notes,
    }
}

fn inspect_telegram_status(config: &RuntimeConfig) -> TelegramStatusReport {
    match &config.telegram {
        Some(telegram) => TelegramStatusReport {
            configured: true,
            binding_present: telegram.foreground_binding.is_some(),
            binding_internal_conversation_ref: telegram
                .foreground_binding
                .as_ref()
                .map(|binding| binding.internal_conversation_ref.clone()),
            binding_internal_principal_ref: telegram
                .foreground_binding
                .as_ref()
                .map(|binding| binding.internal_principal_ref.clone()),
            bot_token_env: Some(telegram.bot_token_env.clone()),
            bot_token_present: env_var_present(&telegram.bot_token_env),
            poll_limit: Some(telegram.poll_limit),
        },
        None => TelegramStatusReport {
            configured: false,
            binding_present: false,
            binding_internal_conversation_ref: None,
            binding_internal_principal_ref: None,
            bot_token_env: None,
            bot_token_present: false,
            poll_limit: None,
        },
    }
}

fn inspect_model_gateway_status(config: &RuntimeConfig) -> ModelGatewayStatusReport {
    match &config.model_gateway {
        Some(model_gateway) => ModelGatewayStatusReport {
            configured: true,
            provider: Some(model_gateway.foreground.provider.identifier().to_string()),
            model: Some(model_gateway.foreground.model.clone()),
            api_base_url: model_gateway.foreground.api_base_url.clone(),
            api_key_env: Some(model_gateway.foreground.api_key_env.clone()),
            api_key_present: env_var_present(&model_gateway.foreground.api_key_env),
            timeout_ms: Some(model_gateway.foreground.timeout_ms),
        },
        None => ModelGatewayStatusReport {
            configured: false,
            provider: None,
            model: None,
            api_base_url: None,
            api_key_env: None,
            api_key_present: false,
            timeout_ms: None,
        },
    }
}

fn inspect_self_model_status(config: &RuntimeConfig) -> SelfModelStatusReport {
    match &config.self_model {
        Some(self_model) => {
            let seed_path = resolve_seed_path(&self_model.seed_path);
            SelfModelStatusReport {
                configured: true,
                seed_path: Some(seed_path.display().to_string()),
                seed_exists: seed_path.is_file(),
            }
        }
        None => SelfModelStatusReport {
            configured: false,
            seed_path: None,
            seed_exists: false,
        },
    }
}

async fn inspect_schema_status(
    pool: &PgPool,
    config: &RuntimeConfig,
) -> Result<SchemaStatusReport> {
    let discovered = migration::load_migrations()?;
    let expected_version = migration::latest_version(&discovered);
    let applied = migration::load_applied_migrations(pool).await?;
    let current_version = applied.last().map(|migration| migration.version);
    let compatibility = match migration::validate_applied_history(&discovered, &applied) {
        Ok(()) => schema::evaluate(
            current_version,
            SchemaPolicy {
                minimum_supported_version: config.database.minimum_supported_schema_version,
                expected_version,
            },
        ),
        Err(error) => SchemaCompatibility::IncompatibleHistory {
            details: error.to_string(),
        },
    };

    let (compatibility_kind, details, history_valid) = match compatibility {
        SchemaCompatibility::Supported { .. } => ("supported".to_string(), None, true),
        SchemaCompatibility::Missing => (
            "missing".to_string(),
            Some("database schema is missing required migrations".to_string()),
            true,
        ),
        SchemaCompatibility::TooOld {
            current,
            minimum_supported,
        } => (
            "too_old".to_string(),
            Some(format!(
                "database schema version {current} is below minimum supported version {minimum_supported}"
            )),
            true,
        ),
        SchemaCompatibility::PendingMigrations { current, expected } => (
            "pending_migrations".to_string(),
            Some(format!(
                "database schema version {current} is behind expected version {expected}"
            )),
            true,
        ),
        SchemaCompatibility::TooNew { current, expected } => (
            "too_new".to_string(),
            Some(format!(
                "database schema version {current} is newer than runtime-supported version {expected}"
            )),
            true,
        ),
        SchemaCompatibility::IncompatibleHistory { details } => {
            ("incompatible_history".to_string(), Some(details), false)
        }
    };

    Ok(SchemaStatusReport {
        compatibility: compatibility_kind,
        current_version,
        expected_version,
        minimum_supported_version: config.database.minimum_supported_schema_version,
        applied_migration_count: applied.len(),
        history_valid,
        details,
    })
}

async fn load_pending_work_summary(
    pool: &PgPool,
    config: &RuntimeConfig,
) -> Result<PendingWorkSummary> {
    let pending_foreground_conversation_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(DISTINCT internal_conversation_ref)
        FROM ingress_events
        WHERE internal_conversation_ref IS NOT NULL
          AND status = 'accepted'
          AND (
              foreground_status = 'pending'
              OR (
                  foreground_status = 'processing'
                  AND COALESCE(last_processed_at, received_at) <= $1
              )
          )
        "#,
    )
    .bind(foreground_stale_cutoff(config))
    .fetch_one(pool)
    .await
    .context("failed to count pending foreground conversations")?;
    let due_background_job_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM background_jobs
        WHERE status = 'planned'
          AND available_at <= $1
        "#,
    )
    .bind(Utc::now())
    .fetch_one(pool)
    .await
    .context("failed to count due background jobs")?;
    let pending_background_job_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM background_jobs
        WHERE status = 'planned'
        "#,
    )
    .fetch_one(pool)
    .await
    .context("failed to count planned background jobs")?;
    let pending_wake_signal_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM wake_signals
        WHERE status = 'pending_review'
        "#,
    )
    .fetch_one(pool)
    .await
    .context("failed to count pending wake signals")?;
    let pending_approval_request_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM approval_requests
        WHERE status = 'pending'
        "#,
    )
    .fetch_one(pool)
    .await
    .context("failed to count pending approval requests")?;
    let awaiting_approval_governed_action_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM governed_action_executions
        WHERE status = 'awaiting_approval'
        "#,
    )
    .fetch_one(pool)
    .await
    .context("failed to count awaiting approval governed actions")?;
    let blocked_governed_action_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM governed_action_executions
        WHERE status = 'blocked'
        "#,
    )
    .fetch_one(pool)
    .await
    .context("failed to count blocked governed actions")?;

    Ok(PendingWorkSummary {
        pending_foreground_conversation_count: pending_foreground_conversation_count as usize,
        pending_background_job_count: pending_background_job_count as u32,
        due_background_job_count: due_background_job_count as u32,
        pending_wake_signal_count: pending_wake_signal_count as u32,
        pending_approval_request_count: pending_approval_request_count as u32,
        awaiting_approval_governed_action_count: awaiting_approval_governed_action_count as u32,
        blocked_governed_action_count: blocked_governed_action_count as u32,
    })
}

async fn load_recovery_health_summary(
    pool: &PgPool,
    now: DateTime<Utc>,
    recent_window_start: DateTime<Utc>,
) -> Result<RecoveryHealthSummary> {
    let checkpoint_row = sqlx::query(
        r#"
        SELECT
            COUNT(*) FILTER (WHERE status = 'open') AS open_checkpoint_count,
            COUNT(*) FILTER (WHERE status = 'open' AND checkpoint_kind = 'foreground')
                AS open_foreground_checkpoint_count,
            COUNT(*) FILTER (WHERE status = 'open' AND checkpoint_kind = 'background')
                AS open_background_checkpoint_count,
            COUNT(*) FILTER (WHERE status = 'open' AND checkpoint_kind = 'governed_action')
                AS open_governed_action_checkpoint_count,
            COUNT(*) FILTER (
                WHERE status = 'resolved'
                  AND resolved_at >= $1
            ) AS recent_resolved_checkpoint_count,
            COUNT(*) FILTER (
                WHERE status = 'abandoned'
                  AND resolved_at >= $1
            ) AS recent_abandoned_checkpoint_count
        FROM recovery_checkpoints
        "#,
    )
    .bind(recent_window_start)
    .fetch_one(pool)
    .await
    .context("failed to load recovery checkpoint health summary")?;

    let lease_row = sqlx::query(
        r#"
        SELECT
            COUNT(*) AS active_worker_lease_count,
            COUNT(*) FILTER (WHERE lease_expires_at <= $1) AS overdue_active_worker_lease_count,
            COUNT(*) FILTER (WHERE lease_expires_at <= $2) AS at_risk_active_worker_lease_count
        FROM worker_leases
        WHERE status = 'active'
        "#,
    )
    .bind(now)
    .bind(now + Duration::seconds(LEASE_AT_RISK_WINDOW_SECONDS))
    .fetch_one(pool)
    .await
    .context("failed to load worker lease health summary")?;

    Ok(RecoveryHealthSummary {
        open_checkpoint_count: count_from_row(&checkpoint_row, "open_checkpoint_count"),
        open_foreground_checkpoint_count: count_from_row(
            &checkpoint_row,
            "open_foreground_checkpoint_count",
        ),
        open_background_checkpoint_count: count_from_row(
            &checkpoint_row,
            "open_background_checkpoint_count",
        ),
        open_governed_action_checkpoint_count: count_from_row(
            &checkpoint_row,
            "open_governed_action_checkpoint_count",
        ),
        recent_resolved_checkpoint_count: count_from_row(
            &checkpoint_row,
            "recent_resolved_checkpoint_count",
        ),
        recent_abandoned_checkpoint_count: count_from_row(
            &checkpoint_row,
            "recent_abandoned_checkpoint_count",
        ),
        active_worker_lease_count: count_from_row(&lease_row, "active_worker_lease_count"),
        overdue_active_worker_lease_count: count_from_row(
            &lease_row,
            "overdue_active_worker_lease_count",
        ),
        at_risk_active_worker_lease_count: count_from_row(
            &lease_row,
            "at_risk_active_worker_lease_count",
        ),
    })
}

#[derive(Debug, Clone)]
struct BaseDiagnosticRecord {
    operational_diagnostic_id: Uuid,
    trace_id: Option<Uuid>,
    execution_id: Option<Uuid>,
    subsystem: String,
    severity: String,
    reason_code: String,
    created_at: DateTime<Utc>,
}

async fn load_recent_base_diagnostics(
    pool: &PgPool,
    recent_window_start: DateTime<Utc>,
) -> Result<Vec<BaseDiagnosticRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT
            operational_diagnostic_id,
            trace_id,
            execution_id,
            subsystem,
            severity,
            reason_code,
            created_at
        FROM operational_diagnostics
        WHERE created_at >= $1
          AND subsystem <> 'management_health'
        ORDER BY created_at DESC, operational_diagnostic_id DESC
        LIMIT $2
        "#,
    )
    .bind(recent_window_start)
    .bind(HEALTH_RECENT_BASE_DIAGNOSTIC_LIMIT)
    .fetch_all(pool)
    .await
    .context("failed to load recent operational diagnostics for health summary")?;

    Ok(rows
        .into_iter()
        .map(|row| BaseDiagnosticRecord {
            operational_diagnostic_id: row.get("operational_diagnostic_id"),
            trace_id: row.get("trace_id"),
                execution_id: row.get("execution_id"),
                subsystem: row.get("subsystem"),
                severity: row.get("severity"),
                reason_code: row.get("reason_code"),
                created_at: row.get("created_at"),
            })
        .collect())
}

fn summarize_diagnostic_health(
    diagnostics: &[BaseDiagnosticRecord],
) -> DiagnosticHealthSummary {
    let mut info_count = 0_u32;
    let mut warn_count = 0_u32;
    let mut error_count = 0_u32;
    let mut critical_count = 0_u32;
    let mut reasons: BTreeMap<String, OperationalReasonRollup> = BTreeMap::new();

    for diagnostic in diagnostics {
        match diagnostic.severity.as_str() {
            "info" => info_count += 1,
            "warn" => warn_count += 1,
            "error" => error_count += 1,
            "critical" => critical_count += 1,
            _ => {}
        }

        reasons
            .entry(diagnostic.reason_code.clone())
            .and_modify(|rollup| {
                rollup.count += 1;
                if diagnostic.created_at > rollup.latest_at {
                    rollup.latest_at = diagnostic.created_at;
                }
            })
            .or_insert_with(|| OperationalReasonRollup {
                reason_code: diagnostic.reason_code.clone(),
                count: 1,
                latest_at: diagnostic.created_at,
            });
    }

    let mut top_reason_codes = reasons.into_values().collect::<Vec<_>>();
    top_reason_codes.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| right.latest_at.cmp(&left.latest_at))
            .then_with(|| left.reason_code.cmp(&right.reason_code))
    });
    top_reason_codes.truncate(HEALTH_TOP_REASON_LIMIT);

    DiagnosticHealthSummary {
        recent_window_minutes: HEALTH_RECENT_WINDOW_MINUTES as u32,
        observed_count: diagnostics.len() as u32,
        info_count,
        warn_count,
        error_count,
        critical_count,
        top_reason_codes,
    }
}

async fn record_operational_anomaly_rollups(
    pool: &PgPool,
    now: DateTime<Utc>,
    recovery_summary: &RecoveryHealthSummary,
    diagnostics: &[BaseDiagnosticRecord],
) -> Result<()> {
    let recent_window_start = now - Duration::minutes(HEALTH_RECENT_WINDOW_MINUTES);
    let dedupe_window_start = now - Duration::minutes(ANOMALY_DEDUPE_WINDOW_MINUTES);

    let mut repeated_by_reason: BTreeMap<String, Vec<&BaseDiagnosticRecord>> = BTreeMap::new();
    for diagnostic in diagnostics {
        repeated_by_reason
            .entry(diagnostic.reason_code.clone())
            .or_default()
            .push(diagnostic);
    }
    for (reason_code, records) in repeated_by_reason {
        if records.len() < ANOMALY_REPEAT_THRESHOLD {
            continue;
        }
        let latest = records
            .iter()
            .max_by_key(|record| record.created_at)
            .expect("repeated anomaly records should not be empty");
        let first_seen_at = records
            .iter()
            .map(|record| record.created_at)
            .min()
            .expect("repeated anomaly records should not be empty");
        let last_seen_at = records
            .iter()
            .map(|record| record.created_at)
            .max()
            .expect("repeated anomaly records should not be empty");
        let severity = if records
            .iter()
            .any(|record| matches!(record.severity.as_str(), "critical" | "error"))
        {
            recovery::OperationalDiagnosticSeverity::Error
        } else {
            recovery::OperationalDiagnosticSeverity::Warn
        };
        let aggregate_key = format!("repeated_reason:{reason_code}");
        insert_management_anomaly_if_missing(
            pool,
            now,
            dedupe_window_start,
            recovery::NewOperationalDiagnostic {
                operational_diagnostic_id: Uuid::now_v7(),
                trace_id: latest.trace_id,
                execution_id: latest.execution_id,
                subsystem: "management_health".to_string(),
                severity,
                reason_code: "operational_repeated_condition_detected".to_string(),
                summary: format!(
                    "diagnostic reason '{reason_code}' repeated {} times in the last {} minutes",
                    records.len(),
                    HEALTH_RECENT_WINDOW_MINUTES
                ),
                diagnostic_payload: json!({
                    "aggregate_key": aggregate_key,
                    "anomaly_kind": "repeated_reason",
                    "source_reason_code": reason_code,
                    "occurrence_count": records.len(),
                    "first_seen_at": first_seen_at,
                    "last_seen_at": last_seen_at,
                    "recent_window_minutes": HEALTH_RECENT_WINDOW_MINUTES,
                    "source_operational_diagnostic_id": latest.operational_diagnostic_id,
                    "source_subsystem": latest.subsystem,
                }),
            },
        )
        .await?;
    }

    let failure_pressure_records = diagnostics
        .iter()
        .filter(|record| matches!(record.severity.as_str(), "error" | "critical"))
        .collect::<Vec<_>>();
    if failure_pressure_records.len() >= ANOMALY_FAILURE_PRESSURE_THRESHOLD {
        let latest = failure_pressure_records
            .iter()
            .max_by_key(|record| record.created_at)
            .expect("failure pressure records should not be empty");
        insert_management_anomaly_if_missing(
            pool,
            now,
            dedupe_window_start,
            recovery::NewOperationalDiagnostic {
                operational_diagnostic_id: Uuid::now_v7(),
                trace_id: latest.trace_id,
                execution_id: latest.execution_id,
                subsystem: "management_health".to_string(),
                severity: recovery::OperationalDiagnosticSeverity::Error,
                reason_code: "operational_failure_pressure_detected".to_string(),
                summary: format!(
                    "{} error or critical diagnostics observed in the last {} minutes",
                    failure_pressure_records.len(),
                    HEALTH_RECENT_WINDOW_MINUTES
                ),
                diagnostic_payload: json!({
                    "aggregate_key": "failure_pressure",
                    "anomaly_kind": "failure_pressure",
                    "occurrence_count": failure_pressure_records.len(),
                    "first_seen_at": failure_pressure_records
                        .iter()
                        .map(|record| record.created_at)
                        .min(),
                    "last_seen_at": failure_pressure_records
                        .iter()
                        .map(|record| record.created_at)
                        .max(),
                    "recent_window_minutes": HEALTH_RECENT_WINDOW_MINUTES,
                }),
            },
        )
        .await?;
    }

    if recovery_summary.open_checkpoint_count > 0
        || recovery_summary.overdue_active_worker_lease_count > 0
        || recovery_summary.at_risk_active_worker_lease_count > 0
    {
        let latest_checkpoint = sqlx::query(
            r#"
            SELECT trace_id, execution_id, created_at
            FROM recovery_checkpoints
            WHERE status = 'open'
            ORDER BY created_at DESC, recovery_checkpoint_id DESC
            LIMIT 1
            "#,
        )
        .fetch_optional(pool)
        .await
        .context("failed to load latest open recovery checkpoint for anomaly rollup")?;

        insert_management_anomaly_if_missing(
            pool,
            now,
            dedupe_window_start,
            recovery::NewOperationalDiagnostic {
                operational_diagnostic_id: Uuid::now_v7(),
                trace_id: latest_checkpoint.as_ref().map(|row| row.get("trace_id")),
                execution_id: latest_checkpoint.as_ref().and_then(|row| row.get("execution_id")),
                subsystem: "management_health".to_string(),
                severity: if recovery_summary.open_checkpoint_count > 0
                    || recovery_summary.overdue_active_worker_lease_count > 0
                {
                    recovery::OperationalDiagnosticSeverity::Error
                } else {
                    recovery::OperationalDiagnosticSeverity::Warn
                },
                reason_code: "operational_recovery_pressure_detected".to_string(),
                summary: format!(
                    "recovery pressure detected: {} open checkpoints, {} overdue active leases, {} at-risk active leases",
                    recovery_summary.open_checkpoint_count,
                    recovery_summary.overdue_active_worker_lease_count,
                    recovery_summary.at_risk_active_worker_lease_count
                ),
                diagnostic_payload: json!({
                    "aggregate_key": "recovery_pressure",
                    "anomaly_kind": "recovery_pressure",
                    "open_checkpoint_count": recovery_summary.open_checkpoint_count,
                    "open_foreground_checkpoint_count": recovery_summary.open_foreground_checkpoint_count,
                    "open_background_checkpoint_count": recovery_summary.open_background_checkpoint_count,
                    "open_governed_action_checkpoint_count": recovery_summary.open_governed_action_checkpoint_count,
                    "overdue_active_worker_lease_count": recovery_summary.overdue_active_worker_lease_count,
                    "at_risk_active_worker_lease_count": recovery_summary.at_risk_active_worker_lease_count,
                    "recent_window_minutes": HEALTH_RECENT_WINDOW_MINUTES,
                    "evaluated_at": now,
                    "recent_window_start": recent_window_start,
                }),
            },
        )
        .await?;
    }

    Ok(())
}

async fn insert_management_anomaly_if_missing(
    pool: &PgPool,
    now: DateTime<Utc>,
    dedupe_window_start: DateTime<Utc>,
    diagnostic: recovery::NewOperationalDiagnostic,
) -> Result<()> {
    let aggregate_key = diagnostic.diagnostic_payload["aggregate_key"]
        .as_str()
        .context("management anomaly diagnostic payload must include aggregate_key")?;
    let exists: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM operational_diagnostics
            WHERE subsystem = 'management_health'
              AND reason_code = $1
              AND diagnostic_payload_json ->> 'aggregate_key' = $2
              AND created_at >= $3
        )
        "#,
    )
    .bind(&diagnostic.reason_code)
    .bind(aggregate_key)
    .bind(dedupe_window_start)
    .fetch_one(pool)
    .await
    .context("failed to check management anomaly dedupe window")?;

    if exists {
        return Ok(());
    }

    recovery::insert_operational_diagnostic(pool, &diagnostic)
        .await
        .with_context(|| {
            format!(
                "failed to insert management anomaly diagnostic '{}' at {}",
                diagnostic.reason_code, now
            )
        })?;
    Ok(())
}

async fn load_recent_operational_anomalies(
    pool: &PgPool,
    recent_window_start: DateTime<Utc>,
) -> Result<Vec<OperationalAnomalySummary>> {
    let rows = sqlx::query(
        r#"
        SELECT
            operational_diagnostic_id,
            trace_id,
            execution_id,
            severity,
            reason_code,
            summary,
            diagnostic_payload_json,
            created_at
        FROM operational_diagnostics
        WHERE subsystem = 'management_health'
          AND created_at >= $1
        ORDER BY created_at DESC, operational_diagnostic_id DESC
        LIMIT $2
        "#,
    )
    .bind(recent_window_start)
    .bind(i64::from(DEFAULT_LIST_LIMIT))
    .fetch_all(pool)
    .await
    .context("failed to load recent operational anomalies")?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let payload: serde_json::Value = row.get("diagnostic_payload_json");
            OperationalAnomalySummary {
                anomaly_kind: payload["anomaly_kind"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string(),
                severity: row.get("severity"),
                reason_code: row.get("reason_code"),
                summary: row.get("summary"),
                occurrence_count: payload["occurrence_count"].as_u64().unwrap_or(1) as u32,
                latest_trace_id: row.get("trace_id"),
                latest_execution_id: row.get("execution_id"),
                first_seen_at: payload["first_seen_at"]
                    .as_str()
                    .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                    .map(|value| value.with_timezone(&Utc))
                    .unwrap_or_else(|| row.get("created_at")),
                last_seen_at: payload["last_seen_at"]
                    .as_str()
                    .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                    .map(|value| value.with_timezone(&Utc))
                    .unwrap_or_else(|| row.get("created_at")),
            }
        })
        .collect())
}

fn classify_overall_health(
    pending_work: &PendingWorkSummary,
    recovery: &RecoveryHealthSummary,
    diagnostics: &DiagnosticHealthSummary,
) -> String {
    if recovery.open_checkpoint_count > 0
        || recovery.overdue_active_worker_lease_count > 0
        || diagnostics.error_count > 0
        || diagnostics.critical_count > 0
    {
        return "unhealthy".to_string();
    }

    if recovery.at_risk_active_worker_lease_count > 0
        || diagnostics.warn_count > 0
        || pending_work.pending_wake_signal_count > 0
        || pending_work.pending_approval_request_count > 0
        || pending_work.blocked_governed_action_count > 0
    {
        return "degraded".to_string();
    }

    "healthy".to_string()
}

fn count_from_row(row: &sqlx::postgres::PgRow, field: &str) -> u32 {
    row.get::<i64, _>(field).max(0) as u32
}

fn approval_request_summary(record: &approval::ApprovalRequestRecord) -> ApprovalRequestSummary {
    ApprovalRequestSummary {
        approval_request_id: record.approval_request_id,
        trace_id: record.trace_id,
        execution_id: record.execution_id,
        action_proposal_id: record.action_proposal_id,
        action_fingerprint: record.action_fingerprint.value.clone(),
        action_kind: governed_action_kind_label(record.action_kind),
        risk_tier: governed_action_risk_tier_label(record.risk_tier),
        capability_scope: record.capability_scope.clone(),
        status: approval_request_status_label(record.status),
        title: record.title.clone(),
        consequence_summary: record.consequence_summary.clone(),
        requested_by: record.requested_by.clone(),
        requested_at: record.requested_at,
        expires_at: record.expires_at,
        resolved_at: record.resolved_at,
        resolution_kind: record
            .resolution_kind
            .map(approval_resolution_decision_label),
        resolved_by: record.resolved_by.clone(),
        resolution_reason: record.resolution_reason.clone(),
    }
}

fn governed_action_summary(
    record: &governed_actions::GovernedActionExecutionRecord,
) -> GovernedActionSummary {
    GovernedActionSummary {
        governed_action_execution_id: record.governed_action_execution_id,
        trace_id: record.trace_id,
        execution_id: record.execution_id,
        approval_request_id: record.approval_request_id,
        action_proposal_id: record.action_proposal_id,
        action_fingerprint: record.action_fingerprint.value.clone(),
        action_kind: governed_action_kind_label(record.action_kind),
        risk_tier: governed_action_risk_tier_label(record.risk_tier),
        status: governed_action_status_label(record.status),
        workspace_script_id: record.workspace_script_id,
        workspace_script_version_id: record.workspace_script_version_id,
        blocked_reason: record.blocked_reason.clone(),
        output_ref: record.output_ref.clone(),
        started_at: record.started_at,
        completed_at: record.completed_at,
    }
}

fn workspace_script_run_summary(
    record: &workspace::WorkspaceScriptRunRecord,
) -> WorkspaceScriptRunSummary {
    WorkspaceScriptRunSummary {
        workspace_script_run_id: record.workspace_script_run_id,
        workspace_script_id: record.workspace_script_id,
        workspace_script_version_id: record.workspace_script_version_id,
        trace_id: record.trace_id,
        execution_id: record.execution_id,
        governed_action_execution_id: record.governed_action_execution_id,
        approval_request_id: record.approval_request_id,
        status: workspace_script_run_status_label(record.status),
        risk_tier: governed_action_risk_tier_label(record.risk_tier),
        args: record.args.clone(),
        output_ref: record.output_ref.clone(),
        failure_summary: record.failure_summary.clone(),
        started_at: record.started_at,
        completed_at: record.completed_at,
    }
}

fn default_cli_actor_ref(requested_by: &str) -> String {
    match requested_by.split_once(':') {
        Some((_, principal)) if !principal.trim().is_empty() => format!("cli:{principal}"),
        _ => "cli:operator".to_string(),
    }
}

fn approval_request_status_label(status: contracts::ApprovalRequestStatus) -> String {
    match status {
        contracts::ApprovalRequestStatus::Pending => "pending",
        contracts::ApprovalRequestStatus::Approved => "approved",
        contracts::ApprovalRequestStatus::Rejected => "rejected",
        contracts::ApprovalRequestStatus::Expired => "expired",
        contracts::ApprovalRequestStatus::Invalidated => "invalidated",
    }
    .to_string()
}

fn approval_resolution_decision_label(decision: contracts::ApprovalResolutionDecision) -> String {
    match decision {
        contracts::ApprovalResolutionDecision::Approved => "approved",
        contracts::ApprovalResolutionDecision::Rejected => "rejected",
        contracts::ApprovalResolutionDecision::Expired => "expired",
        contracts::ApprovalResolutionDecision::Invalidated => "invalidated",
    }
    .to_string()
}

fn governed_action_kind_label(kind: contracts::GovernedActionKind) -> String {
    match kind {
        contracts::GovernedActionKind::RunSubprocess => "run_subprocess",
        contracts::GovernedActionKind::RunWorkspaceScript => "run_workspace_script",
        contracts::GovernedActionKind::InspectWorkspaceArtifact => "inspect_workspace_artifact",
    }
    .to_string()
}

fn governed_action_risk_tier_label(risk_tier: contracts::GovernedActionRiskTier) -> String {
    match risk_tier {
        contracts::GovernedActionRiskTier::Tier0 => "tier_0",
        contracts::GovernedActionRiskTier::Tier1 => "tier_1",
        contracts::GovernedActionRiskTier::Tier2 => "tier_2",
        contracts::GovernedActionRiskTier::Tier3 => "tier_3",
    }
    .to_string()
}

fn governed_action_status_label(status: contracts::GovernedActionStatus) -> String {
    match status {
        contracts::GovernedActionStatus::Proposed => "proposed",
        contracts::GovernedActionStatus::AwaitingApproval => "awaiting_approval",
        contracts::GovernedActionStatus::Approved => "approved",
        contracts::GovernedActionStatus::Rejected => "rejected",
        contracts::GovernedActionStatus::Expired => "expired",
        contracts::GovernedActionStatus::Invalidated => "invalidated",
        contracts::GovernedActionStatus::Blocked => "blocked",
        contracts::GovernedActionStatus::Executed => "executed",
        contracts::GovernedActionStatus::Failed => "failed",
    }
    .to_string()
}

fn workspace_script_run_status_label(status: contracts::WorkspaceScriptRunStatus) -> String {
    match status {
        contracts::WorkspaceScriptRunStatus::Pending => "pending",
        contracts::WorkspaceScriptRunStatus::Running => "running",
        contracts::WorkspaceScriptRunStatus::Completed => "completed",
        contracts::WorkspaceScriptRunStatus::Failed => "failed",
        contracts::WorkspaceScriptRunStatus::TimedOut => "timed_out",
        contracts::WorkspaceScriptRunStatus::Blocked => "blocked",
    }
    .to_string()
}

fn classify_pending_foreground_summary(
    config: &RuntimeConfig,
    pending_count: usize,
    pending_span_seconds: u64,
    stale_pending_age_seconds: u64,
    includes_stale_processing: bool,
) -> (&'static str, &'static str) {
    let backlog = &config.continuity.backlog_recovery;

    if pending_count < 2 {
        return ("single_ingress", "single_ingress");
    }
    if includes_stale_processing {
        return ("backlog_recovery", "stale_processing_resume");
    }
    if pending_count >= backlog.pending_message_count_threshold as usize
        && pending_span_seconds >= backlog.pending_message_span_seconds_threshold
    {
        return ("backlog_recovery", "pending_span_threshold");
    }
    if pending_count >= backlog.pending_message_count_threshold as usize
        && stale_pending_age_seconds >= backlog.stale_pending_ingress_age_seconds_threshold
    {
        return ("backlog_recovery", "stale_pending_batch");
    }
    ("single_ingress", "single_ingress")
}

fn foreground_stale_cutoff(config: &RuntimeConfig) -> DateTime<Utc> {
    Utc::now()
        - Duration::seconds(
            config
                .continuity
                .backlog_recovery
                .stale_pending_ingress_age_seconds_threshold as i64,
        )
}

fn env_var_present(name: &str) -> bool {
    env::var_os(name).is_some_and(|value| !value.is_empty())
}

fn resolve_seed_path(seed_path: &PathBuf) -> PathBuf {
    if seed_path.is_absolute() {
        return seed_path.clone();
    }
    migration::workspace_root().join(seed_path)
}

async fn verify_schema(pool: &PgPool, config: &RuntimeConfig) -> Result<i64> {
    let migrations = migration::load_migrations()?;
    let policy = SchemaPolicy {
        minimum_supported_version: config.database.minimum_supported_schema_version,
        expected_version: migration::latest_version(&migrations),
    };
    schema::verify(pool, policy).await
}

trait ModelProviderKindExt {
    fn identifier(self) -> &'static str;
}

impl ModelProviderKindExt for contracts::ModelProviderKind {
    fn identifier(self) -> &'static str {
        match self {
            contracts::ModelProviderKind::ZAi => "z_ai",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        AppConfig, ApprovalPromptMode, ApprovalsConfig, BackgroundConfig,
        BackgroundExecutionConfig, BackgroundSchedulerConfig, BackgroundThresholdsConfig,
        ContinuityConfig, DatabaseConfig, GovernedActionsConfig, HarnessConfig, ModelGatewayConfig,
        SelfModelConfig, TelegramConfig, TelegramForegroundBindingConfig, WakeSignalPolicyConfig,
        WorkerConfig, WorkspaceConfig,
    };
    use std::path::PathBuf;

    fn sample_config() -> RuntimeConfig {
        RuntimeConfig {
            app: AppConfig {
                name: "blue-lagoon".to_string(),
                log_filter: "info".to_string(),
            },
            database: DatabaseConfig {
                database_url: "postgres://example".to_string(),
                minimum_supported_schema_version: 1,
            },
            harness: HarnessConfig {
                allow_synthetic_smoke: true,
                default_foreground_iteration_budget: 1,
                default_wall_clock_budget_ms: 60_000,
                default_foreground_token_budget: 4_000,
            },
            background: BackgroundConfig {
                scheduler: BackgroundSchedulerConfig {
                    poll_interval_seconds: 300,
                    max_due_jobs_per_iteration: 4,
                    lease_timeout_ms: 300_000,
                },
                thresholds: BackgroundThresholdsConfig {
                    episode_backlog_threshold: 25,
                    candidate_memory_threshold: 10,
                    contradiction_alert_threshold: 3,
                },
                execution: BackgroundExecutionConfig {
                    default_iteration_budget: 2,
                    default_wall_clock_budget_ms: 120_000,
                    default_token_budget: 6_000,
                },
                wake_signals: WakeSignalPolicyConfig {
                    allow_foreground_conversion: true,
                    max_pending_signals: 8,
                    cooldown_seconds: 900,
                },
            },
            continuity: ContinuityConfig {
                retrieval: crate::config::RetrievalConfig {
                    max_recent_episode_candidates: 3,
                    max_memory_artifact_candidates: 5,
                    max_context_items: 6,
                },
                backlog_recovery: crate::config::BacklogRecoveryConfig {
                    pending_message_count_threshold: 3,
                    pending_message_span_seconds_threshold: 120,
                    stale_pending_ingress_age_seconds_threshold: 300,
                    max_recovery_batch_size: 8,
                },
            },
            workspace: WorkspaceConfig {
                root_dir: ".".into(),
                max_artifact_bytes: 1_048_576,
                max_script_bytes: 262_144,
            },
            approvals: ApprovalsConfig {
                default_ttl_seconds: 900,
                max_pending_requests: 32,
                allow_cli_resolution: true,
                prompt_mode: ApprovalPromptMode::InlineKeyboardWithFallback,
            },
            governed_actions: GovernedActionsConfig {
                approval_required_min_risk_tier: contracts::GovernedActionRiskTier::Tier2,
                default_subprocess_timeout_ms: 30_000,
                max_subprocess_timeout_ms: 120_000,
                max_filesystem_roots_per_action: 4,
                default_network_access: contracts::NetworkAccessPosture::Disabled,
                allowlisted_environment_variables: vec!["BLUE_LAGOON_DATABASE_URL".to_string()],
                max_environment_variables_per_action: 8,
                max_captured_output_bytes: 65_536,
            },
            worker: WorkerConfig {
                timeout_ms: 10_000,
                command: String::new(),
                args: Vec::new(),
            },
            telegram: Some(TelegramConfig {
                api_base_url: "https://api.telegram.org".to_string(),
                bot_token_env: "BLUE_LAGOON_TELEGRAM_BOT_TOKEN".to_string(),
                poll_limit: 10,
                foreground_binding: Some(TelegramForegroundBindingConfig {
                    allowed_user_id: 1,
                    allowed_chat_id: 2,
                    internal_principal_ref: "primary-user".to_string(),
                    internal_conversation_ref: "telegram-primary".to_string(),
                }),
            }),
            model_gateway: Some(ModelGatewayConfig {
                foreground: crate::config::ForegroundModelRouteConfig {
                    provider: contracts::ModelProviderKind::ZAi,
                    model: "foreground".to_string(),
                    api_base_url: None,
                    api_key_env: "BLUE_LAGOON_FOREGROUND_API_KEY".to_string(),
                    timeout_ms: 60_000,
                },
                z_ai: None,
            }),
            self_model: Some(SelfModelConfig {
                seed_path: PathBuf::from("config/self_model_seed.toml"),
            }),
        }
    }

    #[test]
    fn classify_pending_foreground_summary_prefers_backlog_for_stale_processing() {
        let config = sample_config();
        let (mode, reason) = classify_pending_foreground_summary(&config, 2, 10, 10, true);
        assert_eq!(mode, "backlog_recovery");
        assert_eq!(reason, "stale_processing_resume");
    }

    #[test]
    fn classify_pending_foreground_summary_defaults_to_single_ingress() {
        let config = sample_config();
        let (mode, reason) = classify_pending_foreground_summary(&config, 1, 0, 0, false);
        assert_eq!(mode, "single_ingress");
        assert_eq!(reason, "single_ingress");
    }

    #[test]
    fn default_cli_actor_ref_reuses_requested_principal() {
        assert_eq!(
            default_cli_actor_ref("telegram:primary-user"),
            "cli:primary-user"
        );
    }

    #[test]
    fn default_cli_actor_ref_falls_back_when_requested_by_is_malformed() {
        assert_eq!(default_cli_actor_ref("primary-user"), "cli:operator");
    }
}
