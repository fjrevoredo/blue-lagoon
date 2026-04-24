use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use contracts::{
    DiagnosticAlert, DiagnosticSeverity, UnconsciousContext, WakeSignal, WakeSignalDecision,
    WakeSignalDecisionKind, WorkerRequest, WorkerResult,
};
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
    foreground::{self, StagedForegroundIngressOutcome},
    model_gateway::ModelProviderTransport,
    policy, proposal, recovery, retrieval, worker,
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

    let worker_lease = create_background_worker_lease(pool, &leased, started_at).await?;
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
            if timed_out {
                recovery::recover_observed_worker_timeout(
                    pool,
                    worker_lease.worker_lease_id,
                    Utc::now(),
                    "background_worker_timeout",
                    &error_message,
                )
                .await
                .context("failed to route timed-out background worker lease through recovery")?;
            } else {
                release_background_worker_lease(pool, worker_lease.worker_lease_id).await?;
            }
            return Err(error);
        }
    };
    recovery::refresh_worker_lease_progress(pool, worker_lease.worker_lease_id, Utc::now())
        .await
        .context("failed to refresh background worker lease after worker response")?;
    release_background_worker_lease(pool, worker_lease.worker_lease_id).await?;

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

    let uncon_result = match &response.result {
        WorkerResult::Unconscious(result) => result,
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
    let proposal_summary = match proposal::apply_candidate_proposals(
        pool,
        config,
        &proposal::ProposalProcessingContext {
            trace_id: leased.job.trace_id,
            execution_id: leased.execution_id,
            episode_id: None,
            source_ingress_id: None,
            source_loop_kind: "unconscious".to_string(),
        },
        "background_execution",
        Some(response.worker_pid as i32),
        &uncon_result.maintenance_outputs.canonical_proposals,
    )
    .await
    {
        Ok(summary) => summary,
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
    let retrieval_summary = match retrieval::apply_retrieval_updates(
        pool,
        retrieval::RetrievalUpdateApplicationContext {
            trace_id: leased.job.trace_id,
            execution_id: leased.execution_id,
            source_loop_kind: "unconscious",
            subsystem: "background_execution",
            worker_pid: Some(response.worker_pid as i32),
            scope: &leased.job.scope,
        },
        &uncon_result.maintenance_outputs.retrieval_updates,
    )
    .await
    {
        Ok(summary) => summary,
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
    let diagnostic_count = match persist_diagnostic_alerts(
        pool,
        &leased,
        Some(response.worker_pid as i32),
        &uncon_result.maintenance_outputs.diagnostics,
    )
    .await
    {
        Ok(count) => count,
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
    let wake_signal_summary = match persist_wake_signals(
        pool,
        config,
        &leased,
        &uncon_result.maintenance_outputs.wake_signals,
    )
    .await
    {
        Ok(summary) => summary,
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
    let summary = format!(
        "{} | proposals evaluated={}, accepted={}, rejected={}, canonical_writes={}, retrieval_updates={}, retrieval_upserts={}, retrieval_archives={}, diagnostics={}, wake_signals={} (accepted={}, deferred={}, suppressed={}, rejected={}, staged={})",
        uncon_result.summary,
        proposal_summary.evaluated_count,
        proposal_summary.accepted_count,
        proposal_summary.rejected_count,
        proposal_summary.canonical_write_count,
        retrieval_summary.evaluated_count,
        retrieval_summary.upserted_count,
        retrieval_summary.archived_count,
        diagnostic_count,
        wake_signal_summary.total_count,
        wake_signal_summary.accepted_count,
        wake_signal_summary.deferred_count,
        wake_signal_summary.suppressed_count,
        wake_signal_summary.rejected_count,
        wake_signal_summary.staged_count
    );

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

async fn create_background_worker_lease(
    pool: &PgPool,
    leased: &LeasedBackgroundExecution,
    started_at: DateTime<Utc>,
) -> Result<recovery::WorkerLeaseRecord> {
    recovery::create_worker_lease(
        pool,
        &recovery::NewWorkerLease {
            worker_lease_id: Uuid::now_v7(),
            trace_id: leased.job.trace_id,
            execution_id: Some(leased.execution_id),
            background_job_id: Some(leased.job.background_job_id),
            background_job_run_id: Some(leased.background_job_run_id),
            governed_action_execution_id: None,
            worker_kind: recovery::WorkerLeaseKind::Background,
            lease_token: leased.lease_token,
            worker_pid: None,
            lease_acquired_at: started_at,
            lease_expires_at: leased.lease_expires_at,
            last_heartbeat_at: started_at,
            metadata: json!({
                "source": "background_execution",
                "job_kind": job_kind_as_str(leased.job.job_kind),
            }),
        },
    )
    .await
}

async fn release_background_worker_lease(pool: &PgPool, worker_lease_id: Uuid) -> Result<()> {
    recovery::release_worker_lease(pool, worker_lease_id, Utc::now())
        .await
        .map(|_| ())
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

async fn persist_diagnostic_alerts(
    pool: &PgPool,
    leased: &LeasedBackgroundExecution,
    worker_pid: Option<i32>,
    diagnostics: &[DiagnosticAlert],
) -> Result<usize> {
    for diagnostic in diagnostics {
        validate_diagnostic_alert(diagnostic)?;
        audit::insert(
            pool,
            &NewAuditEvent {
                loop_kind: "unconscious".to_string(),
                subsystem: "background_execution".to_string(),
                event_kind: "background_diagnostic_recorded".to_string(),
                severity: diagnostic_severity_as_str(diagnostic.severity).to_string(),
                trace_id: leased.job.trace_id,
                execution_id: Some(leased.execution_id),
                worker_pid,
                payload: json!({
                    "alert_id": diagnostic.alert_id,
                    "code": diagnostic.code,
                    "summary": diagnostic.summary,
                    "details": diagnostic.details,
                    "background_job_id": leased.job.background_job_id,
                    "background_job_run_id": leased.background_job_run_id,
                }),
            },
        )
        .await?;
    }

    Ok(diagnostics.len())
}

#[derive(Debug, Clone, Default)]
struct WakeSignalPersistenceSummary {
    total_count: usize,
    accepted_count: usize,
    deferred_count: usize,
    suppressed_count: usize,
    rejected_count: usize,
    staged_count: usize,
}

async fn persist_wake_signals(
    pool: &PgPool,
    config: &RuntimeConfig,
    leased: &LeasedBackgroundExecution,
    wake_signals: &[WakeSignal],
) -> Result<WakeSignalPersistenceSummary> {
    let mut summary = WakeSignalPersistenceSummary::default();
    let foreground_binding = config
        .telegram
        .as_ref()
        .and_then(|telegram| telegram.foreground_binding.as_ref());

    for signal in wake_signals {
        validate_wake_signal(signal)?;
        summary.total_count += 1;

        let requested_at = Utc::now();
        background::insert_wake_signal(
            pool,
            &background::NewWakeSignalRecord {
                background_job_id: leased.job.background_job_id,
                background_job_run_id: Some(leased.background_job_run_id),
                trace_id: leased.job.trace_id,
                execution_id: Some(leased.execution_id),
                signal: signal.clone(),
                status: background::WakeSignalStatus::PendingReview,
                requested_at,
                cooldown_until: None,
            },
        )
        .await?;

        let recorded_signal = background::get_wake_signal(pool, signal.signal_id).await?;
        audit::insert(
            pool,
            &NewAuditEvent {
                loop_kind: "unconscious".to_string(),
                subsystem: "background_execution".to_string(),
                event_kind: "wake_signal_recorded".to_string(),
                severity: "info".to_string(),
                trace_id: leased.job.trace_id,
                execution_id: Some(leased.execution_id),
                worker_pid: None,
                payload: json!({
                    "wake_signal_id": recorded_signal.wake_signal_id,
                    "background_job_id": leased.job.background_job_id,
                    "background_job_run_id": leased.background_job_run_id,
                    "reason": recorded_signal.signal.reason,
                    "priority": recorded_signal.signal.priority,
                    "reason_code": recorded_signal.signal.reason_code,
                    "summary": recorded_signal.signal.summary,
                }),
            },
        )
        .await?;

        let evaluation_context = policy::WakeSignalEvaluationContext {
            pending_signal_count: background::count_open_wake_signals(pool, requested_at).await?,
            cooldown_active: background::has_active_wake_signal_cooldown(
                pool,
                &recorded_signal.signal.reason_code,
                requested_at,
                recorded_signal.wake_signal_id,
            )
            .await?,
            foreground_channel_available: foreground_binding.is_some(),
        };
        let policy_decision =
            policy::evaluate_wake_signal(config, &recorded_signal.signal, evaluation_context);
        let mut persisted_decision = policy_decision.clone();
        let cooldown_until = cooldown_until(config, policy_decision.decision, requested_at);

        if policy_decision.decision == WakeSignalDecisionKind::Rejected {
            recovery::recover_wake_signal_policy_block(
                pool,
                &recorded_signal,
                requested_at,
                "wake_signal_routing_rejected",
                &policy_decision.reason,
            )
            .await
            .context("failed to route rejected wake-signal conversion through recovery")?;
        }

        if policy_decision.decision == WakeSignalDecisionKind::Accepted {
            let binding = foreground_binding.context(
                "accepted wake signal requires a configured Telegram foreground binding",
            )?;
            match foreground::stage_approved_wake_signal_foreground_ingress(
                pool,
                binding,
                &recorded_signal,
            )
            .await?
            {
                StagedForegroundIngressOutcome::Accepted(staged) => {
                    summary.staged_count += 1;
                    audit::insert(
                        pool,
                        &NewAuditEvent {
                            loop_kind: "unconscious".to_string(),
                            subsystem: "background_execution".to_string(),
                            event_kind: "wake_signal_foreground_conversion_staged".to_string(),
                            severity: "info".to_string(),
                            trace_id: leased.job.trace_id,
                            execution_id: Some(leased.execution_id),
                            worker_pid: None,
                            payload: json!({
                                "wake_signal_id": recorded_signal.wake_signal_id,
                                "ingress_id": staged.ingress_id,
                                "internal_conversation_ref": staged.internal_conversation_ref,
                            }),
                        },
                    )
                    .await?;
                }
                StagedForegroundIngressOutcome::Duplicate(duplicate) => {
                    persisted_decision = WakeSignalDecision {
                        signal_id: recorded_signal.wake_signal_id,
                        decision: WakeSignalDecisionKind::Accepted,
                        reason: format!(
                            "{}; equivalent foreground ingress was already staged",
                            policy_decision.reason
                        ),
                    };
                    audit::insert(
                        pool,
                        &NewAuditEvent {
                            loop_kind: "unconscious".to_string(),
                            subsystem: "background_execution".to_string(),
                            event_kind: "wake_signal_foreground_conversion_duplicate".to_string(),
                            severity: "info".to_string(),
                            trace_id: leased.job.trace_id,
                            execution_id: Some(leased.execution_id),
                            worker_pid: None,
                            payload: json!({
                                "wake_signal_id": recorded_signal.wake_signal_id,
                                "existing_ingress_id": duplicate.ingress_id,
                                "existing_trace_id": duplicate.trace_id,
                            }),
                        },
                    )
                    .await?;
                }
                StagedForegroundIngressOutcome::Rejected(_) => {
                    anyhow::bail!(
                        "approved wake-signal staging returned an unexpected rejected outcome"
                    );
                }
            }
        }

        background::record_wake_signal_decision(
            pool,
            recorded_signal.wake_signal_id,
            &persisted_decision,
            requested_at,
            cooldown_until,
        )
        .await?;
        record_wake_signal_decision_audit(
            pool,
            leased,
            &recorded_signal,
            &persisted_decision,
            cooldown_until,
        )
        .await?;

        match persisted_decision.decision {
            WakeSignalDecisionKind::Accepted => summary.accepted_count += 1,
            WakeSignalDecisionKind::Deferred => summary.deferred_count += 1,
            WakeSignalDecisionKind::Suppressed => summary.suppressed_count += 1,
            WakeSignalDecisionKind::Rejected => summary.rejected_count += 1,
        }
    }

    Ok(summary)
}

async fn record_wake_signal_decision_audit(
    pool: &PgPool,
    leased: &LeasedBackgroundExecution,
    recorded_signal: &background::WakeSignalRecord,
    decision: &WakeSignalDecision,
    cooldown_until: Option<DateTime<Utc>>,
) -> Result<()> {
    audit::insert(
        pool,
        &NewAuditEvent {
            loop_kind: "unconscious".to_string(),
            subsystem: "background_execution".to_string(),
            event_kind: "wake_signal_reviewed".to_string(),
            severity: match decision.decision {
                WakeSignalDecisionKind::Accepted => "info",
                WakeSignalDecisionKind::Deferred
                | WakeSignalDecisionKind::Suppressed
                | WakeSignalDecisionKind::Rejected => "warn",
            }
            .to_string(),
            trace_id: leased.job.trace_id,
            execution_id: Some(leased.execution_id),
            worker_pid: None,
            payload: json!({
                "wake_signal_id": recorded_signal.wake_signal_id,
                "decision": decision.decision,
                "reason": decision.reason,
                "reason_code": recorded_signal.signal.reason_code,
                "cooldown_until": cooldown_until,
            }),
        },
    )
    .await?;
    Ok(())
}

fn validate_wake_signal(signal: &WakeSignal) -> Result<()> {
    if signal.reason_code.trim().is_empty() {
        anyhow::bail!("wake signal reason_code must not be empty");
    }
    if signal.summary.trim().is_empty() {
        anyhow::bail!("wake signal summary must not be empty");
    }
    Ok(())
}

fn cooldown_until(
    config: &RuntimeConfig,
    decision_kind: WakeSignalDecisionKind,
    reviewed_at: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    match decision_kind {
        WakeSignalDecisionKind::Accepted
        | WakeSignalDecisionKind::Deferred
        | WakeSignalDecisionKind::Suppressed => Some(
            reviewed_at + Duration::seconds(config.background.wake_signals.cooldown_seconds as i64),
        ),
        WakeSignalDecisionKind::Rejected => None,
    }
}

fn validate_diagnostic_alert(diagnostic: &DiagnosticAlert) -> Result<()> {
    if diagnostic.code.trim().is_empty() {
        anyhow::bail!("diagnostic alert code must not be empty");
    }
    if diagnostic.summary.trim().is_empty() {
        anyhow::bail!("diagnostic alert summary must not be empty");
    }
    Ok(())
}

fn diagnostic_severity_as_str(severity: DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::Info => "info",
        DiagnosticSeverity::Warning => "warning",
        DiagnosticSeverity::Critical => "critical",
    }
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
        BackgroundExecutionBudget, BackgroundTrigger, BackgroundTriggerKind, DiagnosticAlert,
        DiagnosticSeverity, UnconsciousJobKind, UnconsciousScope, WakeSignalPriority,
        WakeSignalReason, WorkerPayload,
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
                    internal_principal_ref: Some("primary-user".to_string()),
                    internal_conversation_ref: Some("telegram-primary".to_string()),
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

    #[test]
    fn validate_diagnostic_alert_accepts_structured_alert() {
        let diagnostic = DiagnosticAlert {
            alert_id: Uuid::now_v7(),
            code: "drift_scan_clear".to_string(),
            severity: DiagnosticSeverity::Info,
            summary: "No contradiction detected.".to_string(),
            details: None,
        };

        assert!(validate_diagnostic_alert(&diagnostic).is_ok());
    }

    #[test]
    fn validate_diagnostic_alert_rejects_empty_summary() {
        let diagnostic = DiagnosticAlert {
            alert_id: Uuid::now_v7(),
            code: "drift_scan_clear".to_string(),
            severity: DiagnosticSeverity::Info,
            summary: "   ".to_string(),
            details: None,
        };

        let error = validate_diagnostic_alert(&diagnostic)
            .expect_err("empty diagnostic summary should fail validation");
        assert!(
            error
                .to_string()
                .contains("diagnostic alert summary must not be empty")
        );
    }

    #[test]
    fn validate_wake_signal_rejects_empty_reason_code() {
        let signal = WakeSignal {
            signal_id: Uuid::now_v7(),
            reason: WakeSignalReason::MaintenanceInsightReady,
            priority: WakeSignalPriority::Normal,
            reason_code: "   ".to_string(),
            summary: "A user-relevant maintenance insight is ready.".to_string(),
            payload_ref: None,
        };

        let error =
            validate_wake_signal(&signal).expect_err("empty wake signal reason_code should fail");
        assert!(error.to_string().contains("reason_code"));
    }

    #[test]
    fn validate_wake_signal_rejects_empty_summary() {
        let signal = WakeSignal {
            signal_id: Uuid::now_v7(),
            reason: WakeSignalReason::MaintenanceInsightReady,
            priority: WakeSignalPriority::High,
            reason_code: "maintenance_insight_ready".to_string(),
            summary: "   ".to_string(),
            payload_ref: None,
        };

        let error =
            validate_wake_signal(&signal).expect_err("empty wake signal summary should fail");
        assert!(error.to_string().contains("summary"));
    }

    #[test]
    fn cooldown_until_uses_policy_window_for_non_rejected_decisions() {
        let config = crate::config::RuntimeConfig {
            app: crate::config::AppConfig {
                name: "blue-lagoon".to_string(),
                log_filter: "info".to_string(),
            },
            database: crate::config::DatabaseConfig {
                database_url: "postgres://unused".to_string(),
                minimum_supported_schema_version: 1,
            },
            harness: crate::config::HarnessConfig {
                allow_synthetic_smoke: true,
                default_foreground_iteration_budget: 1,
                default_wall_clock_budget_ms: 30_000,
                default_foreground_token_budget: 4_000,
            },
            background: crate::config::BackgroundConfig {
                scheduler: crate::config::BackgroundSchedulerConfig {
                    poll_interval_seconds: 300,
                    max_due_jobs_per_iteration: 4,
                    lease_timeout_ms: 300_000,
                },
                thresholds: crate::config::BackgroundThresholdsConfig {
                    episode_backlog_threshold: 25,
                    candidate_memory_threshold: 10,
                    contradiction_alert_threshold: 3,
                },
                execution: crate::config::BackgroundExecutionConfig {
                    default_iteration_budget: 2,
                    default_wall_clock_budget_ms: 120_000,
                    default_token_budget: 6_000,
                },
                wake_signals: crate::config::WakeSignalPolicyConfig {
                    allow_foreground_conversion: true,
                    max_pending_signals: 8,
                    cooldown_seconds: 900,
                },
            },
            continuity: crate::config::ContinuityConfig {
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
            scheduled_foreground: crate::config::ScheduledForegroundConfig {
                enabled: true,
                max_due_tasks_per_iteration: 2,
                min_cadence_seconds: 300,
                default_cooldown_seconds: 300,
            },
            workspace: crate::config::WorkspaceConfig {
                root_dir: ".".into(),
                max_artifact_bytes: 1_048_576,
                max_script_bytes: 262_144,
            },
            approvals: crate::config::ApprovalsConfig {
                default_ttl_seconds: 900,
                max_pending_requests: 32,
                allow_cli_resolution: true,
                prompt_mode: crate::config::ApprovalPromptMode::InlineKeyboardWithFallback,
            },
            governed_actions: crate::config::GovernedActionsConfig {
                approval_required_min_risk_tier: contracts::GovernedActionRiskTier::Tier2,
                default_subprocess_timeout_ms: 30_000,
                max_subprocess_timeout_ms: 120_000,
                max_filesystem_roots_per_action: 4,
                default_network_access: contracts::NetworkAccessPosture::Disabled,
                allowlisted_environment_variables: vec!["BLUE_LAGOON_DATABASE_URL".to_string()],
                max_environment_variables_per_action: 8,
                max_captured_output_bytes: 65_536,
            },
            worker: crate::config::WorkerConfig {
                timeout_ms: 20_000,
                command: String::new(),
                args: Vec::new(),
            },
            telegram: None,
            model_gateway: None,
            self_model: None,
        };
        let reviewed_at = Utc::now();

        let accepted = cooldown_until(&config, WakeSignalDecisionKind::Accepted, reviewed_at)
            .expect("accepted wake signals should record cooldown");
        let deferred = cooldown_until(&config, WakeSignalDecisionKind::Deferred, reviewed_at)
            .expect("deferred wake signals should record cooldown");
        let suppressed = cooldown_until(&config, WakeSignalDecisionKind::Suppressed, reviewed_at)
            .expect("suppressed wake signals should record cooldown");
        let rejected = cooldown_until(&config, WakeSignalDecisionKind::Rejected, reviewed_at);

        assert_eq!(accepted, reviewed_at + Duration::seconds(900));
        assert_eq!(deferred, reviewed_at + Duration::seconds(900));
        assert_eq!(suppressed, reviewed_at + Duration::seconds(900));
        assert!(rejected.is_none());
    }
}
