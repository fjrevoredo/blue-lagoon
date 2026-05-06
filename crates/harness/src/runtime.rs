use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use contracts::{WorkerRequest, WorkerResult};
use serde_json::json;
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    audit::{self, NewAuditEvent},
    background, background_execution,
    config::RuntimeConfig,
    db,
    execution::{self, NewExecutionRecord},
    foreground,
    foreground_orchestration::{self, TelegramForegroundOrchestrationOutcome},
    ingress::{self, TelegramNormalizationOutcome},
    migration,
    model_gateway::{self, ModelProviderTransport},
    policy::{self, PolicyDecision},
    recovery, scheduled_foreground,
    schema::{self, SchemaPolicy},
    telegram::{self, TelegramDelivery, TelegramUpdate, TelegramUpdateSource},
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
    pub background_once: bool,
    pub synthetic_trigger: Option<SyntheticTrigger>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HarnessOutcome {
    IdleVerified,
    SyntheticCompleted {
        execution_id: Uuid,
        trace_id: Uuid,
    },
    BackgroundNoDueJob,
    BackgroundCompleted {
        background_job_id: Uuid,
        execution_id: Uuid,
        trace_id: Uuid,
        summary: String,
    },
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

#[derive(Debug)]
struct BlockedTelegramForegroundRecovery<'a> {
    chat_id: i64,
    trace_id: Uuid,
    execution_id: Uuid,
    primary_ingress: &'a foreground::IngressEventRecord,
    selected_ingress_ids: &'a [Uuid],
    outcome: &'a recovery::RecoveryTriggerOutcome,
}

pub async fn run_migrate(config: &RuntimeConfig) -> Result<migration::MigrationSummary> {
    let pool = db::connect(config).await?;
    migration::apply_pending_migrations(&pool, env!("CARGO_PKG_VERSION")).await
}

pub async fn run_harness_once(
    config: &RuntimeConfig,
    options: HarnessOptions,
) -> Result<HarnessOutcome> {
    let transport = model_gateway::ReqwestModelProviderTransport::new();
    run_harness_once_with_transport(config, options, &transport).await
}

pub async fn run_harness_service(config: &RuntimeConfig) -> Result<()> {
    let pool = db::connect(config).await?;
    verify_schema(&pool, config).await?;
    let transport = model_gateway::ReqwestModelProviderTransport::new();
    let resolved_gateway = if config.scheduled_foreground.enabled {
        Some(config.require_model_gateway_config()?)
    } else {
        None
    };
    let mut delivery = if config.scheduled_foreground.enabled {
        Some(telegram::ReqwestTelegramDelivery::new(
            config.require_telegram_config()?,
        ))
    } else {
        None
    };
    let poll_interval =
        Duration::from_secs(config.background.scheduler.poll_interval_seconds.max(1));

    loop {
        supervise_expired_worker_leases(&pool).await?;
        supervise_background_supervisor_restart_recovery(&pool).await?;
        recover_interrupted_scheduled_foreground_tasks(&pool, config, 40).await?;
        if let (Some(model_gateway_config), Some(delivery)) =
            (resolved_gateway.as_ref(), delivery.as_mut())
        {
            run_scheduled_foreground_iteration_with(
                &pool,
                config,
                model_gateway_config,
                &transport,
                delivery,
            )
            .await?;
        }
        run_background_scheduler_iteration(&pool, config, &transport).await?;
        tokio::time::sleep(poll_interval).await;
    }
}

pub async fn run_harness_once_with_transport<T: ModelProviderTransport>(
    config: &RuntimeConfig,
    options: HarnessOptions,
    transport: &T,
) -> Result<HarnessOutcome> {
    if !options.once {
        bail!("current harness mode supports one-shot execution only");
    }
    let selected_mode_count = u8::from(options.idle)
        + u8::from(options.background_once)
        + u8::from(options.synthetic_trigger.is_some());
    if selected_mode_count != 1 {
        bail!("choose exactly one of --idle, --background-once, or --synthetic-trigger");
    }

    let pool = db::connect(config).await?;
    verify_schema(&pool, config).await?;
    supervise_expired_worker_leases(&pool).await?;
    supervise_background_supervisor_restart_recovery(&pool).await?;

    if options.idle {
        info!("harness boot verified and returned to idle");
        return Ok(HarnessOutcome::IdleVerified);
    }
    if options.background_once {
        return run_background_once_with(&pool, config, transport).await;
    }

    match options.synthetic_trigger {
        Some(SyntheticTrigger::Smoke) => run_smoke_trigger(&pool, config).await,
        None => bail!("missing harness mode"),
    }
}

pub async fn run_background_once_with<T: ModelProviderTransport>(
    pool: &PgPool,
    config: &RuntimeConfig,
    transport: &T,
) -> Result<HarnessOutcome> {
    let now = Utc::now();
    if background::list_due_jobs(pool, now, 1).await?.is_empty() {
        record_background_no_due_job(pool, now).await?;
        return Ok(HarnessOutcome::BackgroundNoDueJob);
    }

    let gateway = config.require_model_gateway_config()?;
    match background_execution::execute_next_due_job(pool, config, &gateway, transport, now).await?
    {
        Some(outcome) => Ok(HarnessOutcome::BackgroundCompleted {
            background_job_id: outcome.background_job_id,
            execution_id: outcome.execution_id,
            trace_id: outcome.trace_id,
            summary: outcome.summary,
        }),
        None => {
            record_background_no_due_job(pool, now).await?;
            Ok(HarnessOutcome::BackgroundNoDueJob)
        }
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
    supervise_expired_worker_leases(&pool).await?;

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

pub async fn run_telegram_service(config: &RuntimeConfig) -> Result<()> {
    const TELEGRAM_SERVICE_POLL_INTERVAL: Duration = Duration::from_secs(1);

    let pool = db::connect(config).await?;
    verify_schema(&pool, config).await?;
    let telegram_config = config.require_telegram_config()?;
    let model_gateway_config = config.require_model_gateway_config()?;
    let transport = model_gateway::ReqwestModelProviderTransport::new();
    let mut source = telegram::ReqwestTelegramSource::new(telegram_config.clone());
    let mut delivery = telegram::ReqwestTelegramDelivery::new(telegram_config.clone());

    loop {
        supervise_expired_worker_leases(&pool).await?;
        let updates = match source.fetch_updates(telegram_config.poll_limit).await {
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
        process_telegram_updates(
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
        tokio::time::sleep(TELEGRAM_SERVICE_POLL_INTERVAL).await;
    }
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

async fn supervise_expired_worker_leases(pool: &PgPool) -> Result<()> {
    let summary = recovery::supervise_worker_leases(pool, Utc::now(), 80).await?;
    if !summary.recovered_expired_leases.is_empty() {
        info!(
            expired_worker_lease_count = summary.recovered_expired_leases.len(),
            "expired worker leases were routed through recovery"
        );
    }
    if !summary.soft_warning_diagnostics.is_empty() {
        info!(
            soft_warning_count = summary.soft_warning_diagnostics.len(),
            "worker lease soft warnings were recorded"
        );
    }
    Ok(())
}

async fn supervise_background_supervisor_restart_recovery(pool: &PgPool) -> Result<()> {
    let recovered = recovery::recover_background_supervisor_restart(pool, Utc::now(), 40).await?;
    if !recovered.is_empty() {
        info!(
            stranded_background_job_count = recovered.len(),
            "stranded background runs were routed through supervisor restart recovery"
        );
    }
    Ok(())
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

async fn record_background_no_due_job(
    pool: &PgPool,
    checked_at: chrono::DateTime<Utc>,
) -> Result<()> {
    let trace = TraceContext::root();
    audit::insert(
        pool,
        &NewAuditEvent {
            loop_kind: "unconscious".to_string(),
            subsystem: "harness".to_string(),
            event_kind: "background_maintenance_no_due_job".to_string(),
            severity: "info".to_string(),
            trace_id: trace.trace_id,
            execution_id: None,
            worker_pid: None,
            payload: json!({
                "checked_at": checked_at,
            }),
        },
    )
    .await?;
    Ok(())
}

async fn run_background_scheduler_iteration<T: ModelProviderTransport>(
    pool: &PgPool,
    config: &RuntimeConfig,
    transport: &T,
) -> Result<u32> {
    let due_limit = config
        .background
        .scheduler
        .max_due_jobs_per_iteration
        .max(1);
    if background::list_due_jobs(pool, Utc::now(), due_limit)
        .await?
        .is_empty()
    {
        return Ok(0);
    }

    let gateway = config.require_model_gateway_config()?;
    let mut completed = 0_u32;
    for _ in 0..due_limit {
        match background_execution::execute_next_due_job(
            pool,
            config,
            &gateway,
            transport,
            Utc::now(),
        )
        .await?
        {
            Some(_) => completed += 1,
            None => break,
        }
    }
    Ok(completed)
}

pub async fn run_scheduled_foreground_iteration_with<T, D>(
    pool: &PgPool,
    config: &RuntimeConfig,
    model_gateway_config: &crate::config::ResolvedModelGatewayConfig,
    transport: &T,
    delivery: &mut D,
) -> Result<u32>
where
    T: ModelProviderTransport,
    D: TelegramDelivery,
{
    if !config.scheduled_foreground.enabled {
        return Ok(0);
    }

    recover_interrupted_scheduled_foreground_tasks(pool, config, 40).await?;

    let due_limit = config
        .scheduled_foreground
        .max_due_tasks_per_iteration
        .max(1);
    let mut handled = 0_u32;
    for _ in 0..due_limit {
        let trace_id = Uuid::now_v7();
        let execution_id = Uuid::now_v7();
        let Some(claim) =
            scheduled_foreground::claim_next_due_task(pool, execution_id, trace_id, Utc::now())
                .await?
        else {
            break;
        };

        handle_claimed_scheduled_foreground_task(
            pool,
            config,
            model_gateway_config,
            claim,
            foreground_orchestration::ForegroundExecutionIds {
                trace_id,
                execution_id,
            },
            transport,
            delivery,
        )
        .await?;
        handled += 1;
    }

    Ok(handled)
}

pub async fn recover_interrupted_scheduled_foreground_tasks(
    pool: &PgPool,
    config: &RuntimeConfig,
    limit: i64,
) -> Result<u32> {
    if !config.scheduled_foreground.enabled {
        return Ok(0);
    }

    let timeout_ms = i64::try_from(policy::effective_foreground_worker_timeout_ms(config))
        .context("scheduled foreground worker timeout exceeded i64 range")?;
    let stale_cutoff = Utc::now() - chrono::Duration::milliseconds(timeout_ms);
    let recoverable =
        scheduled_foreground::list_recoverable_in_progress_tasks(pool, stale_cutoff, limit).await?;
    let mut recovered = 0_u32;
    for task in recoverable {
        recover_scheduled_foreground_task(pool, &task).await?;
        recovered += 1;
    }
    Ok(recovered)
}

async fn recover_scheduled_foreground_task(
    pool: &PgPool,
    recoverable: &scheduled_foreground::RecoverableScheduledForegroundTask,
) -> Result<()> {
    let execution_id = recoverable
        .task
        .current_execution_id
        .context("recoverable scheduled foreground task is missing current_execution_id")?;
    let trace_id = execution::get(pool, execution_id).await?.trace_id;
    let now = Utc::now();
    if let Some(ingress_id) = recoverable.ingress_id {
        let selected_ingress_ids = [ingress_id];
        recovery::recover_foreground_restart_trigger(
            pool,
            recovery::ForegroundRestartRecoveryRequest {
                trace_id,
                execution_id,
                interrupted_execution_id: Some(execution_id),
                internal_conversation_ref: &recoverable.task.internal_conversation_ref,
                recovery_reason_code: recovery::RecoveryReasonCode::SupervisorRestart,
                trigger_source: "scheduled_foreground_supervisor_recovery",
                decision_reason: "scheduled_task_supervisor_restart_recovery",
                selected_ingress_ids: &selected_ingress_ids,
                primary_ingress_id: ingress_id,
                recovery_mode: "single_ingress",
            },
            now,
        )
        .await
        .context("failed to route interrupted scheduled foreground task through recovery")?;
    }

    match recoverable.execution_status.as_str() {
        "completed" => {
            let completed = scheduled_foreground::mark_task_completed(
                pool,
                &recoverable.task,
                execution_id,
                now,
                "scheduled foreground completion was finalized during supervisor recovery",
            )
            .await?;
            record_scheduled_foreground_terminal_audit(
                pool,
                "scheduled_foreground_task_recovered_completed",
                "warn",
                trace_id,
                execution_id,
                &completed,
                json!({
                    "recovery": "supervisor_restart",
                    "execution_status": recoverable.execution_status,
                    "ingress_id": recoverable.ingress_id,
                }),
            )
            .await
        }
        "failed" => {
            let failed = scheduled_foreground::mark_task_failed(
                pool,
                &recoverable.task,
                execution_id,
                now,
                "supervisor_restart_recovery",
                "scheduled foreground failure was finalized during supervisor recovery",
            )
            .await?;
            record_scheduled_foreground_terminal_audit(
                pool,
                "scheduled_foreground_task_recovered_failed",
                "warn",
                trace_id,
                execution_id,
                &failed,
                json!({
                    "recovery": "supervisor_restart",
                    "execution_status": recoverable.execution_status,
                    "ingress_id": recoverable.ingress_id,
                }),
            )
            .await
        }
        _ => {
            execution::mark_failed(
                pool,
                execution_id,
                &json!({
                    "kind": "scheduled_foreground_recovery",
                    "message": "scheduled foreground execution was interrupted and failed closed during supervisor recovery",
                }),
            )
            .await?;
            let failed = scheduled_foreground::mark_task_failed(
                pool,
                &recoverable.task,
                execution_id,
                now,
                "supervisor_restart_recovery",
                "scheduled foreground execution was interrupted and failed closed during supervisor recovery",
            )
            .await?;
            record_scheduled_foreground_terminal_audit(
                pool,
                "scheduled_foreground_task_recovered_failed",
                "warn",
                trace_id,
                execution_id,
                &failed,
                json!({
                    "recovery": "supervisor_restart",
                    "execution_status": recoverable.execution_status,
                    "ingress_id": recoverable.ingress_id,
                }),
            )
            .await
        }
    }
}

async fn handle_claimed_scheduled_foreground_task<T, D>(
    pool: &PgPool,
    config: &RuntimeConfig,
    model_gateway_config: &crate::config::ResolvedModelGatewayConfig,
    claim: scheduled_foreground::ClaimedScheduledForegroundTask,
    ids: foreground_orchestration::ForegroundExecutionIds,
    transport: &T,
    delivery: &mut D,
) -> Result<()>
where
    T: ModelProviderTransport,
    D: TelegramDelivery,
{
    audit::insert(
        pool,
        &NewAuditEvent {
            loop_kind: "conscious".to_string(),
            subsystem: "scheduled_foreground".to_string(),
            event_kind: "scheduled_foreground_task_started".to_string(),
            severity: "info".to_string(),
            trace_id: ids.trace_id,
            execution_id: Some(ids.execution_id),
            worker_pid: None,
            payload: json!({
                "scheduled_foreground_task_id": claim.task.scheduled_foreground_task_id,
                "task_key": claim.task.task_key,
                "internal_principal_ref": claim.task.internal_principal_ref,
                "internal_conversation_ref": claim.task.internal_conversation_ref,
            }),
        },
    )
    .await?;

    let Some(ingress) = claim.ingress.clone() else {
        return suppress_scheduled_foreground_task(
            pool,
            &claim.task,
            ids.trace_id,
            ids.execution_id,
            "conversation_binding_missing",
            "scheduled foreground task has no matching conversation binding",
        )
        .await;
    };

    let ingress_record = foreground::get_ingress_event(pool, ingress.ingress_id).await?;
    let plan = foreground::PendingForegroundExecutionPlan {
        mode: contracts::ForegroundExecutionMode::SingleIngress,
        primary_ingress: ingress_record,
        interrupted_execution_id: None,
        ordered_ingress: vec![contracts::OrderedIngressReference {
            ingress_id: ingress.ingress_id,
            occurred_at: ingress.occurred_at,
            external_message_id: ingress.external_message_id.clone(),
            text_body: ingress.text_body.clone(),
        }],
        decision_reason: foreground::ForegroundExecutionDecisionReason::SingleIngress,
    };

    match foreground_orchestration::orchestrate_telegram_foreground_plan(
        pool,
        config,
        model_gateway_config,
        foreground_orchestration::TelegramForegroundPlanExecution {
            execution: ids,
            trigger_kind_override: Some(contracts::ForegroundTriggerKind::ScheduledTask),
            plan,
        },
        transport,
        delivery,
    )
    .await
    {
        Ok(TelegramForegroundOrchestrationOutcome::Completed(completion)) => {
            let completed = scheduled_foreground::mark_task_completed(
                pool,
                &claim.task,
                ids.execution_id,
                Utc::now(),
                &format!(
                    "scheduled foreground task '{}' delivered Telegram message {}",
                    claim.task.task_key, completion.outbound_message_id
                ),
            )
            .await?;
            record_scheduled_foreground_terminal_audit(
                pool,
                "scheduled_foreground_task_completed",
                "info",
                ids.trace_id,
                ids.execution_id,
                &completed,
                json!({
                    "outbound_message_id": completion.outbound_message_id,
                    "ingress_id": completion.ingress_id,
                    "episode_id": completion.episode_id,
                }),
            )
            .await
        }
        Ok(other) => {
            let message = format!(
                "scheduled foreground task '{}' returned an unexpected orchestration outcome: {other:?}",
                claim.task.task_key
            );
            fail_scheduled_foreground_task(
                pool,
                &claim.task,
                ids.trace_id,
                ids.execution_id,
                "unexpected_orchestration_outcome",
                &message,
            )
            .await
        }
        Err(error) => {
            let error_message = format_error_chain(&error);
            fail_scheduled_foreground_task(
                pool,
                &claim.task,
                ids.trace_id,
                ids.execution_id,
                "execution_failed",
                &error_message,
            )
            .await
        }
    }
}

async fn suppress_scheduled_foreground_task(
    pool: &PgPool,
    task: &scheduled_foreground::ScheduledForegroundTaskRecord,
    trace_id: Uuid,
    execution_id: Uuid,
    reason: &str,
    summary: &str,
) -> Result<()> {
    execution::mark_succeeded(
        pool,
        execution_id,
        "scheduled_foreground",
        0,
        &json!({
            "kind": "scheduled_foreground_suppressed",
            "scheduled_foreground_task_id": task.scheduled_foreground_task_id,
            "task_key": task.task_key,
            "reason": reason,
            "summary": summary,
        }),
    )
    .await?;
    let suppressed = scheduled_foreground::mark_task_suppressed(
        pool,
        task,
        execution_id,
        Utc::now(),
        reason,
        summary,
    )
    .await?;
    record_scheduled_foreground_terminal_audit(
        pool,
        "scheduled_foreground_task_suppressed",
        "warn",
        trace_id,
        execution_id,
        &suppressed,
        json!({
            "reason": reason,
            "summary": summary,
        }),
    )
    .await
}

async fn fail_scheduled_foreground_task(
    pool: &PgPool,
    task: &scheduled_foreground::ScheduledForegroundTaskRecord,
    trace_id: Uuid,
    execution_id: Uuid,
    reason: &str,
    summary: &str,
) -> Result<()> {
    let failed = scheduled_foreground::mark_task_failed(
        pool,
        task,
        execution_id,
        Utc::now(),
        reason,
        summary,
    )
    .await?;
    record_scheduled_foreground_terminal_audit(
        pool,
        "scheduled_foreground_task_failed",
        "error",
        trace_id,
        execution_id,
        &failed,
        json!({
            "reason": reason,
            "summary": summary,
        }),
    )
    .await
}

async fn record_scheduled_foreground_terminal_audit(
    pool: &PgPool,
    event_kind: &str,
    severity: &str,
    trace_id: Uuid,
    execution_id: Uuid,
    task: &scheduled_foreground::ScheduledForegroundTaskRecord,
    detail: serde_json::Value,
) -> Result<()> {
    audit::insert(
        pool,
        &NewAuditEvent {
            loop_kind: "conscious".to_string(),
            subsystem: "scheduled_foreground".to_string(),
            event_kind: event_kind.to_string(),
            severity: severity.to_string(),
            trace_id,
            execution_id: Some(execution_id),
            worker_pid: None,
            payload: json!({
                "scheduled_foreground_task_id": task.scheduled_foreground_task_id,
                "task_key": task.task_key,
                "status": format!("{:?}", task.status),
                "last_outcome": task.last_outcome.map(|value| format!("{value:?}")),
                "next_due_at": task.next_due_at,
                "detail": detail,
            }),
        },
    )
    .await?;
    Ok(())
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
    let mut newly_staged_conversations = BTreeSet::new();
    let mut recovery_scan_conversations = BTreeSet::new();

    for update in updates {
        let normalization = ingress::normalize_telegram_update(
            context.telegram_config,
            &update,
            context.raw_payload_ref.clone(),
        )?;

        match normalization {
            TelegramNormalizationOutcome::Accepted(ingress) => {
                if ingress.event_kind == contracts::IngressEventKind::ApprovalCallback {
                    match foreground_orchestration::orchestrate_telegram_foreground_ingress(
                        context.pool,
                        context.config,
                        context.telegram_config,
                        context.model_gateway_config,
                        *ingress,
                        context.transport,
                        delivery,
                    )
                    .await?
                    {
                        TelegramForegroundOrchestrationOutcome::Completed(_)
                        | TelegramForegroundOrchestrationOutcome::ApprovalResolved(_) => {
                            summary.completed_count += 1;
                        }
                        TelegramForegroundOrchestrationOutcome::Duplicate(_) => {
                            summary.duplicate_count += 1;
                        }
                        TelegramForegroundOrchestrationOutcome::Rejected(_) => {
                            summary.trigger_rejected_count += 1;
                        }
                    }
                    continue;
                }

                match foreground::stage_telegram_foreground_ingress(
                    context.pool,
                    context.telegram_config,
                    *ingress,
                )
                .await?
                {
                    foreground::StagedForegroundIngressOutcome::Accepted(staged) => {
                        staged_conversations.insert(staged.internal_conversation_ref.clone());
                        newly_staged_conversations.insert(staged.internal_conversation_ref);
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
        staged_conversations.insert(internal_conversation_ref.clone());
        recovery_scan_conversations.insert(internal_conversation_ref);
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
        let selected_ingress_ids = plan
            .ordered_ingress
            .iter()
            .map(|ingress| ingress.ingress_id)
            .collect::<Vec<_>>();
        let recovered_only_from_scan = recovery_scan_conversations
            .contains(&internal_conversation_ref)
            && !newly_staged_conversations.contains(&internal_conversation_ref);

        let interrupted_execution_id = plan.interrupted_execution_id;
        if plan.decision_reason
            == foreground::ForegroundExecutionDecisionReason::StaleProcessingResume
        {
            let outcome = recovery::recover_foreground_restart_trigger(
                context.pool,
                recovery::ForegroundRestartRecoveryRequest {
                    trace_id,
                    execution_id,
                    interrupted_execution_id,
                    internal_conversation_ref: &internal_conversation_ref,
                    recovery_reason_code: recovery::RecoveryReasonCode::Crash,
                    trigger_source: "telegram_foreground_processing_loop",
                    decision_reason: plan.decision_reason.as_str(),
                    selected_ingress_ids: &selected_ingress_ids,
                    primary_ingress_id: plan.primary_ingress.ingress_id,
                    recovery_mode: "backlog_recovery",
                },
                Utc::now(),
            )
            .await
            .context("failed to route stale foreground processing through recovery")?;
            if outcome.decision.decision != recovery::RecoveryDecision::Continue {
                handle_blocked_telegram_foreground_recovery(
                    context.pool,
                    delivery,
                    BlockedTelegramForegroundRecovery {
                        chat_id: context.telegram_config.allowed_chat_id,
                        trace_id,
                        execution_id,
                        primary_ingress: &plan.primary_ingress,
                        selected_ingress_ids: &selected_ingress_ids,
                        outcome: &outcome,
                    },
                )
                .await?;
                if is_backlog_recovery {
                    summary.backlog_recovery_count += 1;
                }
                summary.trigger_rejected_count += 1;
                continue;
            }
        } else if recovered_only_from_scan && is_backlog_recovery {
            let outcome = recovery::recover_foreground_restart_trigger(
                context.pool,
                recovery::ForegroundRestartRecoveryRequest {
                    trace_id,
                    execution_id,
                    interrupted_execution_id,
                    internal_conversation_ref: &internal_conversation_ref,
                    recovery_reason_code: recovery::RecoveryReasonCode::SupervisorRestart,
                    trigger_source: "telegram_foreground_recovery_scan",
                    decision_reason: plan.decision_reason.as_str(),
                    selected_ingress_ids: &selected_ingress_ids,
                    primary_ingress_id: plan.primary_ingress.ingress_id,
                    recovery_mode: "backlog_recovery",
                },
                Utc::now(),
            )
            .await
            .context("failed to route foreground supervisor restart through recovery")?;
            if outcome.decision.decision != recovery::RecoveryDecision::Continue {
                handle_blocked_telegram_foreground_recovery(
                    context.pool,
                    delivery,
                    BlockedTelegramForegroundRecovery {
                        chat_id: context.telegram_config.allowed_chat_id,
                        trace_id,
                        execution_id,
                        primary_ingress: &plan.primary_ingress,
                        selected_ingress_ids: &selected_ingress_ids,
                        outcome: &outcome,
                    },
                )
                .await?;
                summary.backlog_recovery_count += 1;
                summary.trigger_rejected_count += 1;
                continue;
            }
        }

        match foreground_orchestration::orchestrate_telegram_foreground_plan(
            context.pool,
            context.config,
            context.model_gateway_config,
            foreground_orchestration::TelegramForegroundPlanExecution {
                execution: foreground_orchestration::ForegroundExecutionIds {
                    trace_id,
                    execution_id,
                },
                trigger_kind_override: if plan.decision_reason
                    == foreground::ForegroundExecutionDecisionReason::StaleProcessingResume
                    || (recovered_only_from_scan && is_backlog_recovery)
                {
                    Some(contracts::ForegroundTriggerKind::SupervisorRecoveryEvent)
                } else {
                    None
                },
                plan,
            },
            context.transport,
            delivery,
        )
        .await?
        {
            TelegramForegroundOrchestrationOutcome::Completed(_)
            | TelegramForegroundOrchestrationOutcome::ApprovalResolved(_) => {
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

async fn handle_blocked_telegram_foreground_recovery<D>(
    pool: &PgPool,
    delivery: &mut D,
    blocked_recovery: BlockedTelegramForegroundRecovery<'_>,
) -> Result<()>
where
    D: TelegramDelivery,
{
    let BlockedTelegramForegroundRecovery {
        chat_id,
        trace_id,
        execution_id,
        primary_ingress,
        selected_ingress_ids,
        outcome,
    } = blocked_recovery;
    execution::mark_failed(
        pool,
        execution_id,
        &json!({
            "kind": "foreground_recovery_blocked",
            "recovery_decision": format!("{:?}", outcome.decision.decision).to_lowercase(),
            "checkpoint_status": format!("{:?}", outcome.checkpoint.status).to_lowercase(),
            "diagnostic_reason_code": outcome.diagnostic.reason_code.clone(),
            "summary": outcome.decision.summary.clone(),
        }),
    )
    .await?;
    foreground::mark_ingress_events_processed(pool, selected_ingress_ids, execution_id).await?;

    let reply_to_message_id = parse_telegram_reply_target_from_ingress_record(primary_ingress)?;
    let text = format!(
        "I could not automatically resume that interrupted request. Trace: {trace_id}. Recovery was blocked because the prior execution already linked governed actions and replay would not be safe."
    );

    match delivery
        .send_message(&telegram::TelegramOutboundMessage {
            chat_id,
            text,
            reply_to_message_id,
            reply_markup: None,
        })
        .await
    {
        Ok(_) => {}
        Err(error) => {
            warn!(
                trace_id = %trace_id,
                execution_id = %execution_id,
                error = %format_error_chain(&error),
                "telegram blocked-recovery notice delivery failed"
            );
        }
    }

    Ok(())
}

fn parse_telegram_reply_target_from_ingress_record(
    ingress: &foreground::IngressEventRecord,
) -> Result<Option<i64>> {
    ingress
        .external_message_id
        .as_deref()
        .map(|message_id| {
            message_id
                .parse::<i64>()
                .with_context(|| format!("failed to parse Telegram message id '{message_id}'"))
        })
        .transpose()
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
