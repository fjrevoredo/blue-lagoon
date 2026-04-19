use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use contracts::{WorkerRequest, WorkerResult};
use serde_json::json;
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

use crate::{
    audit::{self, NewAuditEvent},
    config::RuntimeConfig,
    db,
    execution::{self, NewExecutionRecord},
    foreground,
    foreground_orchestration::{self, TelegramForegroundOrchestrationOutcome},
    ingress::{self, TelegramNormalizationOutcome},
    migration,
    model_gateway::{self, ModelProviderTransport},
    policy::{self, PolicyDecision},
    schema::{self, SchemaPolicy},
    telegram::{self, TelegramDelivery, TelegramUpdate},
    trace::TraceContext,
    worker,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntheticTrigger {
    Smoke,
}

impl SyntheticTrigger {
    pub fn as_str(self) -> &'static str {
        match self {
            SyntheticTrigger::Smoke => "smoke",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessOptions {
    pub once: bool,
    pub idle: bool,
    pub synthetic_trigger: Option<SyntheticTrigger>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HarnessOutcome {
    IdleVerified,
    SyntheticCompleted { execution_id: Uuid, trace_id: Uuid },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelegramOptions {
    pub fixture_path: Option<PathBuf>,
    pub poll_once: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TelegramOutcome {
    FixtureProcessed(TelegramProcessingSummary),
    PollProcessed(TelegramProcessingSummary),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TelegramProcessingSummary {
    pub fetched_updates: usize,
    pub completed_count: usize,
    pub backlog_recovery_count: usize,
    pub duplicate_count: usize,
    pub trigger_rejected_count: usize,
    pub normalization_rejected_count: usize,
    pub ignored_count: usize,
}

pub async fn run_migrate(config: &RuntimeConfig) -> Result<migration::MigrationSummary> {
    let pool = db::connect(config).await?;
    migration::apply_pending_migrations(&pool, env!("CARGO_PKG_VERSION")).await
}

pub async fn run_harness_once(
    config: &RuntimeConfig,
    options: HarnessOptions,
) -> Result<HarnessOutcome> {
    if !options.once {
        bail!("current harness mode supports one-shot execution only");
    }
    if options.idle == options.synthetic_trigger.is_some() {
        bail!("choose exactly one of --idle or --synthetic-trigger");
    }

    let pool = db::connect(config).await?;
    verify_schema(&pool, config).await?;

    if options.idle {
        info!("harness boot verified and returned to idle");
        return Ok(HarnessOutcome::IdleVerified);
    }

    match options.synthetic_trigger {
        Some(SyntheticTrigger::Smoke) => run_smoke_trigger(&pool, config).await,
        None => bail!("missing harness mode"),
    }
}

pub async fn run_telegram_once(
    config: &RuntimeConfig,
    options: TelegramOptions,
) -> Result<TelegramOutcome> {
    if options.fixture_path.is_some() == options.poll_once {
        bail!("choose exactly one of --fixture or --poll-once");
    }

    let pool = db::connect(config).await?;
    verify_schema(&pool, config).await?;

    let telegram_config = config.require_telegram_config()?;
    let model_gateway_config = config.require_model_gateway_config()?;

    if let Some(fixture_path) = options.fixture_path {
        let transport = model_gateway::ReqwestModelProviderTransport::new();
        let mut delivery = telegram::ReqwestTelegramDelivery::new(telegram_config.clone());
        let summary = run_telegram_fixture_with(
            &pool,
            config,
            &telegram_config,
            &model_gateway_config,
            &fixture_path,
            &transport,
            &mut delivery,
        )
        .await?;
        return Ok(TelegramOutcome::FixtureProcessed(summary));
    }

    let transport = model_gateway::ReqwestModelProviderTransport::new();
    let updates = match telegram::fetch_updates_once(telegram_config.clone()).await {
        Ok(updates) => updates,
        Err(error) => {
            if let Err(record_error) =
                record_telegram_fetch_failed(&pool, &error, Some("telegram:getUpdates")).await
            {
                return Err(error.context(format!(
                    "failed to record telegram fetch failure: {record_error}"
                )));
            }
            return Err(error);
        }
    };
    let mut delivery = telegram::ReqwestTelegramDelivery::new(telegram_config.clone());
    let summary = process_telegram_updates(
        TelegramProcessingContext {
            pool: &pool,
            config,
            telegram_config: &telegram_config,
            model_gateway_config: &model_gateway_config,
            transport: &transport,
            raw_payload_ref: Some("telegram:getUpdates".to_string()),
        },
        updates,
        &mut delivery,
    )
    .await?;
    Ok(TelegramOutcome::PollProcessed(summary))
}

pub async fn run_telegram_fixture_with<T, D>(
    pool: &PgPool,
    config: &RuntimeConfig,
    telegram_config: &crate::config::ResolvedTelegramConfig,
    model_gateway_config: &crate::config::ResolvedModelGatewayConfig,
    fixture_path: &Path,
    transport: &T,
    delivery: &mut D,
) -> Result<TelegramProcessingSummary>
where
    T: ModelProviderTransport,
    D: TelegramDelivery,
{
    let updates = telegram::load_fixture_updates(fixture_path)?;
    process_telegram_updates(
        TelegramProcessingContext {
            pool,
            config,
            telegram_config,
            model_gateway_config,
            transport,
            raw_payload_ref: Some(fixture_path.display().to_string()),
        },
        updates,
        delivery,
    )
    .await
}

async fn verify_schema(pool: &PgPool, config: &RuntimeConfig) -> Result<i64> {
    let migrations = migration::load_migrations()?;
    let policy = SchemaPolicy {
        minimum_supported_version: config.database.minimum_supported_schema_version,
        expected_version: migration::latest_version(&migrations),
    };
    schema::verify(pool, policy).await
}

async fn run_smoke_trigger(pool: &PgPool, config: &RuntimeConfig) -> Result<HarnessOutcome> {
    match policy::evaluate_synthetic_smoke(config) {
        PolicyDecision::Allowed => {}
        PolicyDecision::Denied { reason } => bail!(reason),
    }

    let budget = policy::default_budget(config);
    policy::validate_budget(budget)?;

    let trace = TraceContext::root();
    let execution_id = Uuid::now_v7();
    let request = WorkerRequest::smoke(
        trace.trace_id,
        execution_id,
        SyntheticTrigger::Smoke.as_str(),
    );

    execution::insert(
        pool,
        &NewExecutionRecord {
            execution_id,
            trace_id: trace.trace_id,
            trigger_kind: "synthetic".to_string(),
            synthetic_trigger: Some(SyntheticTrigger::Smoke.as_str().to_string()),
            status: "started".to_string(),
            request_payload: serde_json::to_value(&request)
                .context("failed to serialize execution request payload")?,
        },
    )
    .await?;

    audit::insert(
        pool,
        &NewAuditEvent {
            loop_kind: "conscious".to_string(),
            subsystem: "harness".to_string(),
            event_kind: "synthetic_trigger_received".to_string(),
            severity: "info".to_string(),
            trace_id: trace.trace_id,
            execution_id: Some(execution_id),
            worker_pid: None,
            payload: json!({
                "synthetic_trigger": SyntheticTrigger::Smoke.as_str(),
                "budget_ms": budget.wall_clock_budget_ms,
            }),
        },
    )
    .await?;

    let response = match worker::launch_smoke_worker(config, &request).await {
        Ok(response) => response,
        Err(error) => {
            record_smoke_failure(pool, trace.trace_id, execution_id, &error.to_string()).await?;
            return Err(error);
        }
    };
    let response_payload = match serde_json::to_value(&response)
        .context("failed to serialize worker response payload")
    {
        Ok(payload) => payload,
        Err(error) => {
            record_smoke_failure(pool, trace.trace_id, execution_id, &error.to_string()).await?;
            return Err(error);
        }
    };

    execution::mark_succeeded(
        pool,
        execution_id,
        "smoke",
        response.worker_pid as i32,
        &response_payload,
    )
    .await?;

    let result_summary = match &response.result {
        WorkerResult::Smoke(result) => result.summary.clone(),
        WorkerResult::Conscious(result) => result.episode_summary.summary.clone(),
        WorkerResult::Unconscious(result) => result.summary.clone(),
        WorkerResult::Error(error) => error.message.clone(),
    };

    audit::insert(
        pool,
        &NewAuditEvent {
            loop_kind: "conscious".to_string(),
            subsystem: "harness".to_string(),
            event_kind: "synthetic_trigger_completed".to_string(),
            severity: "info".to_string(),
            trace_id: trace.trace_id,
            execution_id: Some(execution_id),
            worker_pid: Some(response.worker_pid as i32),
            payload: json!({
                "synthetic_trigger": SyntheticTrigger::Smoke.as_str(),
                "worker_pid": response.worker_pid,
                "summary": result_summary,
            }),
        },
    )
    .await?;

    Ok(HarnessOutcome::SyntheticCompleted {
        execution_id,
        trace_id: trace.trace_id,
    })
}

struct TelegramProcessingContext<'a, T> {
    pool: &'a PgPool,
    config: &'a RuntimeConfig,
    telegram_config: &'a crate::config::ResolvedTelegramConfig,
    model_gateway_config: &'a crate::config::ResolvedModelGatewayConfig,
    transport: &'a T,
    raw_payload_ref: Option<String>,
}

async fn process_telegram_updates<T, D>(
    context: TelegramProcessingContext<'_, T>,
    updates: Vec<TelegramUpdate>,
    delivery: &mut D,
) -> Result<TelegramProcessingSummary>
where
    T: ModelProviderTransport,
    D: TelegramDelivery,
{
    let mut summary = TelegramProcessingSummary {
        fetched_updates: updates.len(),
        ..TelegramProcessingSummary::default()
    };
    let mut staged_conversations = BTreeSet::new();

    for update in updates {
        let normalization = ingress::normalize_telegram_update(
            context.telegram_config,
            &update,
            context.raw_payload_ref.clone(),
        )?;

        match normalization {
            TelegramNormalizationOutcome::Accepted(ingress) => {
                match foreground::stage_telegram_foreground_ingress(
                    context.pool,
                    context.telegram_config,
                    *ingress,
                )
                .await?
                {
                    foreground::StagedForegroundIngressOutcome::Accepted(staged) => {
                        staged_conversations.insert(staged.internal_conversation_ref);
                    }
                    foreground::StagedForegroundIngressOutcome::Duplicate(_) => {
                        summary.duplicate_count += 1;
                    }
                    foreground::StagedForegroundIngressOutcome::Rejected(_) => {
                        summary.trigger_rejected_count += 1;
                    }
                }
            }
            TelegramNormalizationOutcome::Rejected(rejected) => {
                summary.normalization_rejected_count += 1;
                record_telegram_normalization_rejected(
                    context.pool,
                    &rejected.external_event_id,
                    &rejected.detail,
                    context.raw_payload_ref.as_deref(),
                )
                .await?;
                info!(
                    external_event_id = rejected.external_event_id,
                    detail = rejected.detail,
                    "telegram update rejected during normalization",
                );
            }
            TelegramNormalizationOutcome::Ignored(ignored) => {
                summary.ignored_count += 1;
                record_telegram_update_ignored(
                    context.pool,
                    &ignored.external_event_id,
                    context.raw_payload_ref.as_deref(),
                )
                .await?;
                info!(
                    external_event_id = ignored.external_event_id,
                    "telegram update ignored during normalization",
                );
            }
        }
    }

    for internal_conversation_ref in
        foreground::list_recoverable_foreground_conversations(context.pool, context.config).await?
    {
        staged_conversations.insert(internal_conversation_ref);
    }

    for internal_conversation_ref in staged_conversations {
        let trace_id = Uuid::now_v7();
        let execution_id = Uuid::now_v7();
        execution::insert(
            context.pool,
            &NewExecutionRecord {
                execution_id,
                trace_id,
                trigger_kind: "telegram_pending_ingress".to_string(),
                synthetic_trigger: None,
                status: "started".to_string(),
                request_payload: json!({
                    "internal_conversation_ref": internal_conversation_ref,
                    "kind": "telegram_pending_ingress",
                }),
            },
        )
        .await?;

        let Some(plan) = foreground::plan_pending_foreground_execution(
            context.pool,
            context.config,
            trace_id,
            execution_id,
            &internal_conversation_ref,
            foreground::PendingForegroundExecutionOptions::default(),
        )
        .await?
        else {
            continue;
        };
        let is_backlog_recovery = plan.mode == contracts::ForegroundExecutionMode::BacklogRecovery;

        match foreground_orchestration::orchestrate_telegram_foreground_plan(
            context.pool,
            context.config,
            context.model_gateway_config,
            foreground_orchestration::ForegroundExecutionIds {
                trace_id,
                execution_id,
            },
            plan,
            context.transport,
            delivery,
        )
        .await?
        {
            TelegramForegroundOrchestrationOutcome::Completed(_) => {
                summary.completed_count += 1;
                if is_backlog_recovery {
                    summary.backlog_recovery_count += 1;
                }
            }
            TelegramForegroundOrchestrationOutcome::Duplicate(_) => {
                summary.duplicate_count += 1;
            }
            TelegramForegroundOrchestrationOutcome::Rejected(_) => {
                summary.trigger_rejected_count += 1;
            }
        }
    }

    Ok(summary)
}

async fn record_telegram_normalization_rejected(
    pool: &PgPool,
    external_event_id: &str,
    detail: &str,
    raw_payload_ref: Option<&str>,
) -> Result<()> {
    let trace = TraceContext::root();
    audit::insert(
        pool,
        &NewAuditEvent {
            loop_kind: "conscious".to_string(),
            subsystem: "telegram_ingress".to_string(),
            event_kind: "telegram_ingress_normalization_rejected".to_string(),
            severity: "warn".to_string(),
            trace_id: trace.trace_id,
            execution_id: None,
            worker_pid: None,
            payload: json!({
                "external_event_id": external_event_id,
                "detail": detail,
                "raw_payload_ref": raw_payload_ref,
            }),
        },
    )
    .await?;
    Ok(())
}

async fn record_telegram_update_ignored(
    pool: &PgPool,
    external_event_id: &str,
    raw_payload_ref: Option<&str>,
) -> Result<()> {
    let trace = TraceContext::root();
    audit::insert(
        pool,
        &NewAuditEvent {
            loop_kind: "conscious".to_string(),
            subsystem: "telegram_ingress".to_string(),
            event_kind: "telegram_ingress_ignored".to_string(),
            severity: "info".to_string(),
            trace_id: trace.trace_id,
            execution_id: None,
            worker_pid: None,
            payload: json!({
                "external_event_id": external_event_id,
                "raw_payload_ref": raw_payload_ref,
            }),
        },
    )
    .await?;
    Ok(())
}

async fn record_telegram_fetch_failed(
    pool: &PgPool,
    error: &anyhow::Error,
    raw_payload_ref: Option<&str>,
) -> Result<()> {
    let trace = TraceContext::root();
    audit::insert(
        pool,
        &NewAuditEvent {
            loop_kind: "conscious".to_string(),
            subsystem: "telegram_ingress".to_string(),
            event_kind: "telegram_fetch_failed".to_string(),
            severity: "error".to_string(),
            trace_id: trace.trace_id,
            execution_id: None,
            worker_pid: None,
            payload: json!({
                "error": format_error_chain(error),
                "raw_payload_ref": raw_payload_ref,
            }),
        },
    )
    .await?;
    Ok(())
}

fn format_error_chain(error: &anyhow::Error) -> String {
    error
        .chain()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(": ")
}

async fn record_smoke_failure(
    pool: &PgPool,
    trace_id: Uuid,
    execution_id: Uuid,
    error_message: &str,
) -> Result<()> {
    let failure_payload = json!({
        "kind": "worker_failure",
        "message": error_message,
    });

    execution::mark_failed(pool, execution_id, &failure_payload).await?;
    audit::insert(
        pool,
        &NewAuditEvent {
            loop_kind: "conscious".to_string(),
            subsystem: "harness".to_string(),
            event_kind: "synthetic_trigger_failed".to_string(),
            severity: "error".to_string(),
            trace_id,
            execution_id: Some(execution_id),
            worker_pid: None,
            payload: json!({
                "synthetic_trigger": SyntheticTrigger::Smoke.as_str(),
                "error": error_message,
            }),
        },
    )
    .await?;
    Ok(())
}
