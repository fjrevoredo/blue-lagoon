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
    migration,
    policy::{self, PolicyDecision},
    schema::{self, SchemaPolicy},
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

pub async fn run_migrate(config: &RuntimeConfig) -> Result<migration::MigrationSummary> {
    let pool = db::connect(config).await?;
    migration::apply_pending_migrations(&pool, env!("CARGO_PKG_VERSION")).await
}

pub async fn run_harness_once(
    config: &RuntimeConfig,
    options: HarnessOptions,
) -> Result<HarnessOutcome> {
    if !options.once {
        bail!("Phase 1 only supports one-shot harness execution");
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
