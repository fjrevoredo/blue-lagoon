use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use contracts::{UnconsciousContext, WorkerRequest, WorkerResult};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    audit::{self, NewAuditEvent},
    background::{
        self, BackgroundJobRecord, BackgroundJobRunStatus, BackgroundJobStatus,
        NewBackgroundJobRun, UpdateBackgroundJobRunStatus, UpdateBackgroundJobStatus,
    },
    config::{ResolvedModelGatewayConfig, RuntimeConfig},
    execution::{self, NewExecutionRecord},
    model_gateway::ModelProviderTransport,
    worker,
};

#[derive(Debug, Clone)]
pub struct LeasedBackgroundExecution {
    pub job: BackgroundJobRecord,
    pub background_job_run_id: Uuid,
    pub execution_id: Uuid,
    pub lease_token: Uuid,
    pub lease_expires_at: DateTime<Utc>,
    pub request: WorkerRequest,
}

#[derive(Debug, Clone)]
pub struct BackgroundExecutionOutcome {
    pub background_job_id: Uuid,
    pub background_job_run_id: Uuid,
    pub execution_id: Uuid,
    pub trace_id: Uuid,
    pub worker_pid: u32,
    pub summary: String,
}

pub async fn lease_next_due_job(
    pool: &PgPool,
    config: &RuntimeConfig,
    now: DateTime<Utc>,
) -> Result<Option<LeasedBackgroundExecution>> {
    let lease_timeout_ms = i64::try_from(config.background.scheduler.lease_timeout_ms)
        .context("background.scheduler.lease_timeout_ms exceeded chrono range")?;
    let lease_expires_at = now + Duration::milliseconds(lease_timeout_ms);
    let mut transaction = pool
        .begin()
        .await
        .context("failed to begin background lease transaction")?;

    let Some(job) = background::lease_due_job(&mut transaction, now, lease_expires_at).await?
    else {
        transaction
            .commit()
            .await
            .context("failed to commit empty background lease transaction")?;
        return Ok(None);
    };

    let execution_id = Uuid::now_v7();
    let request = WorkerRequest::unconscious(
        job.trace_id,
        execution_id,
        UnconsciousContext {
            context_id: Uuid::now_v7(),
            assembled_at: now,
            job_id: job.background_job_id,
            job_kind: job.job_kind,
            trigger: job.trigger.clone(),
            scope: job.scope.clone(),
            budget: job.budget.clone(),
        },
    );
    execution::insert(
        &mut *transaction,
        &NewExecutionRecord {
            execution_id,
            trace_id: job.trace_id,
            trigger_kind: background_trigger_kind_as_str(job.trigger.trigger_kind).to_string(),
            synthetic_trigger: None,
            status: "started".to_string(),
            request_payload: serde_json::to_value(&request)
                .context("failed to serialize background execution request payload")?,
        },
    )
    .await?;

    let background_job_run_id = Uuid::now_v7();
    let lease_token = Uuid::now_v7();
    background::insert_job_run(
        &mut *transaction,
        &NewBackgroundJobRun {
            background_job_run_id,
            background_job_id: job.background_job_id,
            trace_id: job.trace_id,
            execution_id: Some(execution_id),
            lease_token,
            status: BackgroundJobRunStatus::Leased,
            worker_pid: None,
            lease_acquired_at: now,
            lease_expires_at,
            started_at: None,
            completed_at: None,
            result_payload: None,
            failure_payload: None,
        },
    )
    .await?;

    transaction
        .commit()
        .await
        .context("failed to commit background lease transaction")?;

    Ok(Some(LeasedBackgroundExecution {
        job,
        background_job_run_id,
        execution_id,
        lease_token,
        lease_expires_at,
        request,
    }))
}

pub async fn execute_next_due_job<T: ModelProviderTransport>(
    pool: &PgPool,
    config: &RuntimeConfig,
    gateway: &ResolvedModelGatewayConfig,
    transport: &T,
    now: DateTime<Utc>,
) -> Result<Option<BackgroundExecutionOutcome>> {
    let Some(leased) = lease_next_due_job(pool, config, now).await? else {
        return Ok(None);
    };

    let outcome = execute_leased_job(pool, config, gateway, transport, leased).await?;
    Ok(Some(outcome))
}

pub async fn execute_leased_job<T: ModelProviderTransport>(
    pool: &PgPool,
    config: &RuntimeConfig,
    gateway: &ResolvedModelGatewayConfig,
    transport: &T,
    leased: LeasedBackgroundExecution,
) -> Result<BackgroundExecutionOutcome> {
    let started_at = Utc::now();
    background::update_job_status(
        pool,
        leased.job.background_job_id,
        &UpdateBackgroundJobStatus {
            status: BackgroundJobStatus::Running,
            lease_expires_at: Some(leased.lease_expires_at),
            last_started_at: Some(started_at),
            last_completed_at: leased.job.last_completed_at,
        },
    )
    .await?;
    background::update_job_run_status(
        pool,
        leased.background_job_run_id,
        &UpdateBackgroundJobRunStatus {
            status: BackgroundJobRunStatus::Running,
            worker_pid: None,
            started_at: Some(started_at),
            completed_at: None,
            result_payload: None,
            failure_payload: None,
        },
    )
    .await?;
    audit::insert(
        pool,
        &NewAuditEvent {
            loop_kind: "unconscious".to_string(),
            subsystem: "background_execution".to_string(),
            event_kind: "background_job_started".to_string(),
            severity: "info".to_string(),
            trace_id: leased.job.trace_id,
            execution_id: Some(leased.execution_id),
            worker_pid: None,
            payload: json!({
                "background_job_id": leased.job.background_job_id,
                "background_job_run_id": leased.background_job_run_id,
                "job_kind": job_kind_as_str(leased.job.job_kind),
                "trigger_kind": background_trigger_kind_as_str(leased.job.trigger.trigger_kind),
                "lease_token": leased.lease_token,
            }),
        },
    )
    .await?;

    let response = match worker::launch_unconscious_worker(
        config,
        gateway,
        &leased.request,
        transport,
    )
    .await
    {
        Ok(response) => response,
        Err(error) => {
            let error_message = error.to_string();
            let timed_out = error_message.contains("timed out");
            persist_background_failure(pool, &leased, started_at, None, &error_message, timed_out)
                .await?;
            return Err(error);
        }
    };

    let response_payload = match serde_json::to_value(&response)
        .context("failed to serialize unconscious worker response payload")
    {
        Ok(payload) => payload,
        Err(error) => {
            let error_message = error.to_string();
            persist_background_failure(
                pool,
                &leased,
                started_at,
                Some(response.worker_pid as i32),
                &error_message,
                false,
            )
            .await?;
            return Err(error);
        }
    };

    let summary = match &response.result {
        WorkerResult::Unconscious(result) => result.summary.clone(),
        WorkerResult::Smoke(_) | WorkerResult::Conscious(_) | WorkerResult::Error(_) => {
            let message = "unconscious worker returned an invalid result shape".to_string();
            persist_background_failure(
                pool,
                &leased,
                started_at,
                Some(response.worker_pid as i32),
                &message,
                false,
            )
            .await?;
            anyhow::bail!(message);
        }
    };

    let completed_at = Utc::now();
    background::update_job_run_status(
        pool,
        leased.background_job_run_id,
        &UpdateBackgroundJobRunStatus {
            status: BackgroundJobRunStatus::Completed,
            worker_pid: Some(response.worker_pid as i32),
            started_at: Some(started_at),
            completed_at: Some(completed_at),
            result_payload: Some(response_payload.clone()),
            failure_payload: None,
        },
    )
    .await?;
    background::update_job_status(
        pool,
        leased.job.background_job_id,
        &UpdateBackgroundJobStatus {
            status: BackgroundJobStatus::Completed,
            lease_expires_at: None,
            last_started_at: Some(started_at),
            last_completed_at: Some(completed_at),
        },
    )
    .await?;
    execution::mark_succeeded(
        pool,
        leased.execution_id,
        "unconscious",
        response.worker_pid as i32,
        &response_payload,
    )
    .await?;
    audit::insert(
        pool,
        &NewAuditEvent {
            loop_kind: "unconscious".to_string(),
            subsystem: "background_execution".to_string(),
            event_kind: "background_job_completed".to_string(),
            severity: "info".to_string(),
            trace_id: leased.job.trace_id,
            execution_id: Some(leased.execution_id),
            worker_pid: Some(response.worker_pid as i32),
            payload: json!({
                "background_job_id": leased.job.background_job_id,
                "background_job_run_id": leased.background_job_run_id,
                "job_kind": job_kind_as_str(leased.job.job_kind),
                "summary": summary,
            }),
        },
    )
    .await?;

    Ok(BackgroundExecutionOutcome {
        background_job_id: leased.job.background_job_id,
        background_job_run_id: leased.background_job_run_id,
        execution_id: leased.execution_id,
        trace_id: leased.job.trace_id,
        worker_pid: response.worker_pid,
        summary,
    })
}

async fn persist_background_failure(
    pool: &PgPool,
    leased: &LeasedBackgroundExecution,
    started_at: DateTime<Utc>,
    worker_pid: Option<i32>,
    error_message: &str,
    timed_out: bool,
) -> Result<()> {
    let completed_at = Utc::now();
    let failure_kind = if timed_out {
        "worker_timeout"
    } else {
        "worker_failure"
    };
    let run_status = if timed_out {
        BackgroundJobRunStatus::TimedOut
    } else {
        BackgroundJobRunStatus::Failed
    };
    let event_kind = if timed_out {
        "background_job_timed_out"
    } else {
        "background_job_failed"
    };
    let failure_payload = json!({
        "kind": failure_kind,
        "message": error_message,
    });

    background::update_job_run_status(
        pool,
        leased.background_job_run_id,
        &UpdateBackgroundJobRunStatus {
            status: run_status,
            worker_pid,
            started_at: Some(started_at),
            completed_at: Some(completed_at),
            result_payload: None,
            failure_payload: Some(failure_payload.clone()),
        },
    )
    .await?;
    background::update_job_status(
        pool,
        leased.job.background_job_id,
        &UpdateBackgroundJobStatus {
            status: BackgroundJobStatus::Failed,
            lease_expires_at: None,
            last_started_at: Some(started_at),
            last_completed_at: Some(completed_at),
        },
    )
    .await?;
    execution::mark_failed(pool, leased.execution_id, &failure_payload).await?;
    audit::insert(
        pool,
        &NewAuditEvent {
            loop_kind: "unconscious".to_string(),
            subsystem: "background_execution".to_string(),
            event_kind: event_kind.to_string(),
            severity: "error".to_string(),
            trace_id: leased.job.trace_id,
            execution_id: Some(leased.execution_id),
            worker_pid,
            payload: json!({
                "background_job_id": leased.job.background_job_id,
                "background_job_run_id": leased.background_job_run_id,
                "job_kind": job_kind_as_str(leased.job.job_kind),
                "error": error_message,
            }),
        },
    )
    .await?;
    Ok(())
}

fn background_trigger_kind_as_str(kind: contracts::BackgroundTriggerKind) -> &'static str {
    match kind {
        contracts::BackgroundTriggerKind::TimeSchedule => "time_schedule",
        contracts::BackgroundTriggerKind::VolumeThreshold => "volume_threshold",
        contracts::BackgroundTriggerKind::DriftOrAnomalySignal => "drift_or_anomaly_signal",
        contracts::BackgroundTriggerKind::ForegroundDelegation => "foreground_delegation",
        contracts::BackgroundTriggerKind::ExternalPassiveEvent => "external_passive_event",
        contracts::BackgroundTriggerKind::MaintenanceTrigger => "maintenance_trigger",
    }
}

fn job_kind_as_str(kind: contracts::UnconsciousJobKind) -> &'static str {
    match kind {
        contracts::UnconsciousJobKind::MemoryConsolidation => "memory_consolidation",
        contracts::UnconsciousJobKind::RetrievalMaintenance => "retrieval_maintenance",
        contracts::UnconsciousJobKind::ContradictionAndDriftScan => "contradiction_and_drift_scan",
        contracts::UnconsciousJobKind::SelfModelReflection => "self_model_reflection",
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;
    use contracts::{
        BackgroundExecutionBudget, BackgroundTrigger, BackgroundTriggerKind, UnconsciousJobKind,
        UnconsciousScope, WorkerPayload,
    };

    use super::*;

    #[test]
    fn builds_unconscious_request_from_leased_job_shape() {
        let now = Utc::now();
        let trace_id = Uuid::now_v7();
        let execution_id = Uuid::now_v7();
        let request = WorkerRequest::unconscious(
            trace_id,
            execution_id,
            UnconsciousContext {
                context_id: Uuid::now_v7(),
                assembled_at: now,
                job_id: Uuid::now_v7(),
                job_kind: UnconsciousJobKind::MemoryConsolidation,
                trigger: BackgroundTrigger {
                    trigger_id: Uuid::now_v7(),
                    trigger_kind: BackgroundTriggerKind::TimeSchedule,
                    requested_at: now - Duration::minutes(5),
                    reason_summary: "scheduled maintenance".to_string(),
                    payload_ref: None,
                },
                scope: UnconsciousScope {
                    episode_ids: vec![Uuid::now_v7()],
                    memory_artifact_ids: vec![Uuid::now_v7()],
                    retrieval_artifact_ids: vec![],
                    self_model_artifact_id: None,
                    summary: "memory scope".to_string(),
                },
                budget: BackgroundExecutionBudget {
                    iteration_budget: 2,
                    wall_clock_budget_ms: 120_000,
                    token_budget: 6_000,
                },
            },
        );

        assert_eq!(request.trace_id, trace_id);
        assert_eq!(request.execution_id, execution_id);
        match request.payload {
            WorkerPayload::Unconscious(_) => {}
            WorkerPayload::Smoke(_) | WorkerPayload::Conscious(_) => {
                panic!("expected unconscious request payload")
            }
        }
    }
}
