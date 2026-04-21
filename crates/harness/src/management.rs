use std::{env, path::PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use contracts::{BackgroundTrigger, BackgroundTriggerKind, UnconsciousJobKind};
use serde::Serialize;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::{
    background_execution,
    background_planning::{self, BackgroundPlanningDecision, BackgroundPlanningRequest},
    config::RuntimeConfig,
    db, migration, model_gateway,
    schema::{self, SchemaCompatibility, SchemaPolicy},
    worker,
};

const DEFAULT_LIST_LIMIT: u32 = 20;

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeStatusReport {
    pub schema: SchemaStatusReport,
    pub worker: WorkerStatusReport,
    pub telegram: TelegramStatusReport,
    pub model_gateway: ModelGatewayStatusReport,
    pub self_model: SelfModelStatusReport,
    pub pending_work: PendingWorkSummary,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaStatusReport {
    pub compatibility: String,
    pub current_version: Option<i64>,
    pub expected_version: i64,
    pub minimum_supported_version: i64,
    pub applied_migration_count: usize,
    pub history_valid: bool,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkerStatusReport {
    pub resolution_kind: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub timeout_ms: u64,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TelegramStatusReport {
    pub configured: bool,
    pub binding_present: bool,
    pub binding_internal_conversation_ref: Option<String>,
    pub binding_internal_principal_ref: Option<String>,
    pub bot_token_env: Option<String>,
    pub bot_token_present: bool,
    pub poll_limit: Option<u16>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelGatewayStatusReport {
    pub configured: bool,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_base_url: Option<String>,
    pub api_key_env: Option<String>,
    pub api_key_present: bool,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SelfModelStatusReport {
    pub configured: bool,
    pub seed_path: Option<String>,
    pub seed_exists: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PendingWorkSummary {
    pub pending_foreground_conversation_count: usize,
    pub pending_background_job_count: u32,
    pub due_background_job_count: u32,
    pub pending_wake_signal_count: u32,
}

#[derive(Debug, Clone, Serialize)]
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

#[derive(Debug, Clone, Serialize)]
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

#[derive(Debug, Clone, Serialize)]
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

    Ok(PendingWorkSummary {
        pending_foreground_conversation_count: pending_foreground_conversation_count as usize,
        pending_background_job_count: pending_background_job_count as u32,
        due_background_job_count: due_background_job_count as u32,
        pending_wake_signal_count: pending_wake_signal_count as u32,
    })
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
        AppConfig, BackgroundConfig, BackgroundExecutionConfig, BackgroundSchedulerConfig,
        BackgroundThresholdsConfig, ContinuityConfig, DatabaseConfig, HarnessConfig,
        ModelGatewayConfig, SelfModelConfig, TelegramConfig, TelegramForegroundBindingConfig,
        WakeSignalPolicyConfig, WorkerConfig,
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
}
