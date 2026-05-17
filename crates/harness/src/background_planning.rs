use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use contracts::{
    BackgroundExecutionBudget, BackgroundTrigger, BackgroundTriggerKind, UnconsciousJobKind,
    UnconsciousScope,
};
use serde_json::json;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::{
    audit::{self, NewAuditEvent},
    background::{self, BackgroundJobStatus},
    config::RuntimeConfig,
    continuity, foreground, policy, recovery,
};

const DEFAULT_EPISODE_SCOPE_LIMIT: i64 = 4;
const DEFAULT_MEMORY_SCOPE_LIMIT: i64 = 8;
const DEFAULT_RETRIEVAL_SCOPE_LIMIT: i64 = 6;
const SELF_MODEL_REFLECTION_SCHEDULE_MULTIPLIER: u64 = 12;
const PLANNER_DIAGNOSTIC_MIN_COOLDOWN_SECONDS: i64 = 900;

#[derive(Debug, Clone)]
pub struct BackgroundPlanningRequest {
    pub trace_id: Uuid,
    pub job_kind: UnconsciousJobKind,
    pub trigger: BackgroundTrigger,
    pub internal_conversation_ref: Option<String>,
    pub available_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackgroundPlanningDecision {
    Planned(PlannedBackgroundJob),
    SuppressedDuplicate {
        existing_job_id: Uuid,
        deduplication_key: String,
        reason: String,
    },
    Rejected {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedBackgroundJob {
    pub background_job_id: Uuid,
    pub deduplication_key: String,
    pub scope: UnconsciousScope,
    pub budget: BackgroundExecutionBudget,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SchedulerPlanningPassSummary {
    pub request_count: usize,
    pub planned_count: usize,
    pub suppressed_duplicate_count: usize,
    pub rejected_count: usize,
}

pub async fn run_scheduler_planning_pass(
    pool: &PgPool,
    config: &RuntimeConfig,
    scheduled_at: DateTime<Utc>,
) -> Result<SchedulerPlanningPassSummary> {
    let requests = build_scheduler_planning_requests(pool, config, scheduled_at).await?;
    let summary = run_scheduler_planning_pass_with_requests(pool, config, requests).await?;
    record_scheduler_planning_telemetry(pool, config, scheduled_at, &summary).await?;
    Ok(summary)
}

pub async fn run_scheduler_planning_pass_with_requests(
    pool: &PgPool,
    config: &RuntimeConfig,
    requests: Vec<BackgroundPlanningRequest>,
) -> Result<SchedulerPlanningPassSummary> {
    let mut summary = SchedulerPlanningPassSummary {
        request_count: requests.len(),
        ..SchedulerPlanningPassSummary::default()
    };

    for request in requests {
        match plan_background_job(pool, config, request).await? {
            BackgroundPlanningDecision::Planned(_) => summary.planned_count += 1,
            BackgroundPlanningDecision::SuppressedDuplicate { .. } => {
                summary.suppressed_duplicate_count += 1;
            }
            BackgroundPlanningDecision::Rejected { .. } => summary.rejected_count += 1,
        }
    }

    Ok(summary)
}

pub async fn plan_background_job(
    pool: &PgPool,
    config: &RuntimeConfig,
    request: BackgroundPlanningRequest,
) -> Result<BackgroundPlanningDecision> {
    let validation = validate_background_trigger(&request.trigger)
        .and_then(|()| validate_job_trigger_compatibility(request.job_kind, &request.trigger));
    if let Err(reason) = validation {
        audit::insert(
            pool,
            &NewAuditEvent {
                loop_kind: "unconscious".to_string(),
                subsystem: "background_planning".to_string(),
                event_kind: "background_job_rejected".to_string(),
                severity: "info".to_string(),
                trace_id: request.trace_id,
                execution_id: None,
                worker_pid: None,
                payload: json!({
                    "job_kind": job_kind_as_str(request.job_kind),
                    "trigger_kind": trigger_kind_as_str(request.trigger.trigger_kind),
                    "reason": reason,
                    "internal_conversation_ref": request.internal_conversation_ref,
                }),
            },
        )
        .await?;

        return Ok(BackgroundPlanningDecision::Rejected { reason });
    }

    let budget = policy::default_background_budget(config);
    policy::validate_background_budget(&budget)?;
    let deduplication_key = build_deduplication_key(
        request.job_kind,
        &request.trigger,
        request.internal_conversation_ref.as_deref(),
    );
    let scope = assemble_scope(
        pool,
        request.job_kind,
        request.internal_conversation_ref.as_deref(),
    )
    .await?;

    let mut tx = pool
        .begin()
        .await
        .context("failed to begin background planning transaction")?;

    if let Some(existing) =
        background::find_active_job_by_deduplication_key(&mut *tx, &deduplication_key).await?
    {
        let reason = "active background job with matching deduplication key already exists";
        audit::insert(
            &mut *tx,
            &NewAuditEvent {
                loop_kind: "unconscious".to_string(),
                subsystem: "background_planning".to_string(),
                event_kind: "background_job_suppressed".to_string(),
                severity: "info".to_string(),
                trace_id: request.trace_id,
                execution_id: None,
                worker_pid: None,
                payload: json!({
                    "job_kind": job_kind_as_str(request.job_kind),
                    "trigger_kind": trigger_kind_as_str(request.trigger.trigger_kind),
                    "deduplication_key": deduplication_key,
                    "existing_job_id": existing.background_job_id,
                    "reason": reason,
                }),
            },
        )
        .await?;

        tx.commit()
            .await
            .context("failed to commit duplicate-suppression audit")?;

        return Ok(BackgroundPlanningDecision::SuppressedDuplicate {
            existing_job_id: existing.background_job_id,
            deduplication_key,
            reason: reason.to_string(),
        });
    }

    let background_job_id = Uuid::now_v7();
    background::insert_job(
        &mut *tx,
        &background::NewBackgroundJob {
            background_job_id,
            trace_id: request.trace_id,
            job_kind: request.job_kind,
            trigger: request.trigger.clone(),
            deduplication_key: deduplication_key.clone(),
            scope: scope.clone(),
            budget: budget.clone(),
            status: BackgroundJobStatus::Planned,
            available_at: request.available_at,
            lease_expires_at: None,
            last_started_at: None,
            last_completed_at: None,
        },
    )
    .await?;

    audit::insert(
        &mut *tx,
        &NewAuditEvent {
            loop_kind: "unconscious".to_string(),
            subsystem: "background_planning".to_string(),
            event_kind: "background_job_planned".to_string(),
            severity: "info".to_string(),
            trace_id: request.trace_id,
            execution_id: None,
            worker_pid: None,
            payload: json!({
                "background_job_id": background_job_id,
                "job_kind": job_kind_as_str(request.job_kind),
                "trigger_kind": trigger_kind_as_str(request.trigger.trigger_kind),
                "deduplication_key": deduplication_key,
                "internal_conversation_ref": request.internal_conversation_ref,
                "episode_scope_count": scope.episode_ids.len(),
                "memory_scope_count": scope.memory_artifact_ids.len(),
                "retrieval_scope_count": scope.retrieval_artifact_ids.len(),
                "self_model_artifact_id": scope.self_model_artifact_id,
                "iteration_budget": budget.iteration_budget,
                "wall_clock_budget_ms": budget.wall_clock_budget_ms,
                "token_budget": budget.token_budget,
            }),
        },
    )
    .await?;

    tx.commit()
        .await
        .context("failed to commit background planning transaction")?;

    Ok(BackgroundPlanningDecision::Planned(PlannedBackgroundJob {
        background_job_id,
        deduplication_key,
        scope,
        budget,
    }))
}

pub fn validate_background_trigger(trigger: &BackgroundTrigger) -> std::result::Result<(), String> {
    match trigger.trigger_kind {
        BackgroundTriggerKind::TimeSchedule
        | BackgroundTriggerKind::VolumeThreshold
        | BackgroundTriggerKind::DriftOrAnomalySignal
        | BackgroundTriggerKind::ForegroundDelegation
        | BackgroundTriggerKind::ExternalPassiveEvent
        | BackgroundTriggerKind::MaintenanceTrigger => Ok(()),
    }
}

fn validate_job_trigger_compatibility(
    job_kind: UnconsciousJobKind,
    trigger: &BackgroundTrigger,
) -> std::result::Result<(), String> {
    match (job_kind, trigger.trigger_kind) {
        (
            UnconsciousJobKind::ContradictionAndDriftScan,
            BackgroundTriggerKind::DriftOrAnomalySignal,
        ) => Err(format!(
            "trigger kind '{}' is recognized but not supported for job kind '{}'",
            trigger_kind_as_str(trigger.trigger_kind),
            job_kind_as_str(job_kind),
        )),
        _ => Ok(()),
    }
}

fn build_deduplication_key(
    job_kind: UnconsciousJobKind,
    trigger: &BackgroundTrigger,
    internal_conversation_ref: Option<&str>,
) -> String {
    let discriminator = match trigger.trigger_kind {
        BackgroundTriggerKind::TimeSchedule | BackgroundTriggerKind::VolumeThreshold => {
            internal_conversation_ref.unwrap_or("global").to_string()
        }
        BackgroundTriggerKind::ForegroundDelegation
        | BackgroundTriggerKind::MaintenanceTrigger
        | BackgroundTriggerKind::DriftOrAnomalySignal
        | BackgroundTriggerKind::ExternalPassiveEvent => trigger
            .payload_ref
            .clone()
            .or_else(|| internal_conversation_ref.map(str::to_string))
            .unwrap_or_else(|| trigger.reason_summary.clone()),
    };

    format!(
        "background:{job_kind}:{trigger_kind}:{discriminator}",
        job_kind = job_kind_as_str(job_kind),
        trigger_kind = trigger_kind_as_str(trigger.trigger_kind),
        discriminator = normalize_dedup_component(&discriminator),
    )
}

async fn build_scheduler_planning_requests(
    pool: &PgPool,
    config: &RuntimeConfig,
    scheduled_at: DateTime<Utc>,
) -> Result<Vec<BackgroundPlanningRequest>> {
    let mut requests = build_volume_threshold_requests(pool, config, scheduled_at).await?;
    if is_periodic_schedule_due(
        pool,
        UnconsciousJobKind::SelfModelReflection,
        scheduled_at,
        config
            .background
            .scheduler
            .poll_interval_seconds
            .max(1)
            .saturating_mul(SELF_MODEL_REFLECTION_SCHEDULE_MULTIPLIER),
    )
    .await?
    {
        requests.push(BackgroundPlanningRequest {
            trace_id: Uuid::now_v7(),
            job_kind: UnconsciousJobKind::SelfModelReflection,
            trigger: BackgroundTrigger {
                trigger_id: Uuid::now_v7(),
                trigger_kind: BackgroundTriggerKind::TimeSchedule,
                requested_at: scheduled_at,
                reason_summary: "periodic self-model reflection schedule is due".to_string(),
                payload_ref: Some(format!(
                    "schedule://self_model_reflection/every_{}s",
                    config
                        .background
                        .scheduler
                        .poll_interval_seconds
                        .max(1)
                        .saturating_mul(SELF_MODEL_REFLECTION_SCHEDULE_MULTIPLIER)
                )),
            },
            internal_conversation_ref: None,
            available_at: scheduled_at,
        });
    }

    Ok(requests)
}

async fn build_volume_threshold_requests(
    pool: &PgPool,
    config: &RuntimeConfig,
    scheduled_at: DateTime<Utc>,
) -> Result<Vec<BackgroundPlanningRequest>> {
    let mut requests = Vec::new();

    let episode_backlog_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM episodes")
        .fetch_one(pool)
        .await
        .context("failed to count episodes for background threshold planning")?;
    let episode_threshold = i64::from(config.background.thresholds.episode_backlog_threshold);
    if episode_backlog_count >= episode_threshold {
        requests.push(BackgroundPlanningRequest {
            trace_id: Uuid::now_v7(),
            job_kind: UnconsciousJobKind::MemoryConsolidation,
            trigger: BackgroundTrigger {
                trigger_id: Uuid::now_v7(),
                trigger_kind: BackgroundTriggerKind::VolumeThreshold,
                requested_at: scheduled_at,
                reason_summary: format!(
                    "episode backlog reached threshold ({episode_backlog_count} >= {episode_threshold})"
                ),
                payload_ref: Some(format!(
                    "threshold://episodes/count={episode_backlog_count}/threshold={episode_threshold}"
                )),
            },
            internal_conversation_ref: None,
            available_at: scheduled_at,
        });
    }

    let candidate_memory_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM memory_artifacts WHERE status = 'active'",
    )
    .fetch_one(pool)
    .await
    .context("failed to count active memory artifacts for background threshold planning")?;
    let candidate_memory_threshold =
        i64::from(config.background.thresholds.candidate_memory_threshold);
    if candidate_memory_count >= candidate_memory_threshold {
        requests.push(BackgroundPlanningRequest {
            trace_id: Uuid::now_v7(),
            job_kind: UnconsciousJobKind::RetrievalMaintenance,
            trigger: BackgroundTrigger {
                trigger_id: Uuid::now_v7(),
                trigger_kind: BackgroundTriggerKind::VolumeThreshold,
                requested_at: scheduled_at,
                reason_summary: format!(
                    "active memory artifacts reached threshold ({candidate_memory_count} >= {candidate_memory_threshold})"
                ),
                payload_ref: Some(format!(
                    "threshold://memory_artifacts/count={candidate_memory_count}/threshold={candidate_memory_threshold}"
                )),
            },
            internal_conversation_ref: None,
            available_at: scheduled_at,
        });
    }

    Ok(requests)
}

async fn is_periodic_schedule_due(
    pool: &PgPool,
    job_kind: UnconsciousJobKind,
    scheduled_at: DateTime<Utc>,
    interval_seconds: u64,
) -> Result<bool> {
    let latest_activity_at = sqlx::query_scalar::<_, Option<DateTime<Utc>>>(
        r#"
        SELECT MAX(COALESCE(last_completed_at, last_started_at, available_at))
        FROM background_jobs
        WHERE job_kind = $1
          AND trigger_kind = $2
        "#,
    )
    .bind(job_kind_as_str(job_kind))
    .bind(trigger_kind_as_str(BackgroundTriggerKind::TimeSchedule))
    .fetch_one(pool)
    .await
    .context("failed to inspect latest periodic schedule timestamp")?;

    let interval_seconds =
        i64::try_from(interval_seconds).context("periodic schedule interval exceeded i64 range")?;
    Ok(match latest_activity_at {
        Some(latest) => latest + chrono::Duration::seconds(interval_seconds) <= scheduled_at,
        None => true,
    })
}

async fn record_scheduler_planning_telemetry(
    pool: &PgPool,
    config: &RuntimeConfig,
    scheduled_at: DateTime<Utc>,
    summary: &SchedulerPlanningPassSummary,
) -> Result<()> {
    let trace_id = Uuid::now_v7();
    audit::insert(
        pool,
        &NewAuditEvent {
            loop_kind: "unconscious".to_string(),
            subsystem: "background_planning".to_string(),
            event_kind: "background_scheduler_planning_pass_completed".to_string(),
            severity: "info".to_string(),
            trace_id,
            execution_id: None,
            worker_pid: None,
            payload: json!({
                "scheduled_at": scheduled_at,
                "request_count": summary.request_count,
                "planned_count": summary.planned_count,
                "suppressed_duplicate_count": summary.suppressed_duplicate_count,
                "rejected_count": summary.rejected_count,
            }),
        },
    )
    .await?;

    let cooldown_seconds = i64::try_from(config.background.scheduler.poll_interval_seconds)
        .context("background scheduler poll interval exceeded i64 range")?
        .max(PLANNER_DIAGNOSTIC_MIN_COOLDOWN_SECONDS);

    if summary.suppressed_duplicate_count > 0 {
        insert_scheduler_planning_diagnostic_with_cooldown(
            pool,
            scheduled_at,
            cooldown_seconds,
            "background_scheduler_planning_duplicates_suppressed",
            recovery::OperationalDiagnosticSeverity::Info,
            format!(
                "background scheduler suppressed {} duplicate planning request(s)",
                summary.suppressed_duplicate_count
            ),
            json!({
                "scheduled_at": scheduled_at,
                "request_count": summary.request_count,
                "suppressed_duplicate_count": summary.suppressed_duplicate_count,
                "planned_count": summary.planned_count,
            }),
        )
        .await?;
    }

    if summary.rejected_count > 0 {
        insert_scheduler_planning_diagnostic_with_cooldown(
            pool,
            scheduled_at,
            cooldown_seconds,
            "background_scheduler_planning_requests_rejected",
            recovery::OperationalDiagnosticSeverity::Warn,
            format!(
                "background scheduler rejected {} planning request(s)",
                summary.rejected_count
            ),
            json!({
                "scheduled_at": scheduled_at,
                "request_count": summary.request_count,
                "rejected_count": summary.rejected_count,
                "planned_count": summary.planned_count,
            }),
        )
        .await?;
    }

    Ok(())
}

async fn insert_scheduler_planning_diagnostic_with_cooldown(
    pool: &PgPool,
    scheduled_at: DateTime<Utc>,
    cooldown_seconds: i64,
    reason_code: &str,
    severity: recovery::OperationalDiagnosticSeverity,
    summary: String,
    diagnostic_payload: serde_json::Value,
) -> Result<()> {
    let window_start = scheduled_at - chrono::Duration::seconds(cooldown_seconds);
    let already_recorded = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM operational_diagnostics
            WHERE reason_code = $1
              AND created_at >= $2
        )
        "#,
    )
    .bind(reason_code)
    .bind(window_start)
    .fetch_one(pool)
    .await
    .with_context(|| format!("failed to check recent planner diagnostic for {reason_code}"))?;
    if already_recorded {
        return Ok(());
    }

    recovery::insert_operational_diagnostic(
        pool,
        &recovery::NewOperationalDiagnostic {
            operational_diagnostic_id: Uuid::now_v7(),
            trace_id: None,
            execution_id: None,
            subsystem: "background_planning".to_string(),
            severity,
            reason_code: reason_code.to_string(),
            summary,
            diagnostic_payload,
        },
    )
    .await
    .with_context(|| format!("failed to insert planner diagnostic for {reason_code}"))?;
    Ok(())
}

async fn assemble_scope(
    pool: &PgPool,
    job_kind: UnconsciousJobKind,
    internal_conversation_ref: Option<&str>,
) -> Result<UnconsciousScope> {
    let episode_ids = match internal_conversation_ref {
        Some(internal_conversation_ref) => foreground::list_recent_episode_excerpts(
            pool,
            internal_conversation_ref,
            DEFAULT_EPISODE_SCOPE_LIMIT,
        )
        .await?
        .into_iter()
        .map(|episode| episode.episode_id)
        .collect(),
        None => list_recent_episode_ids(pool, DEFAULT_EPISODE_SCOPE_LIMIT).await?,
    };
    let memory_artifact_ids =
        continuity::list_active_memory_artifacts(pool, DEFAULT_MEMORY_SCOPE_LIMIT)
            .await?
            .into_iter()
            .map(|artifact| artifact.memory_artifact_id)
            .collect::<Vec<_>>();
    let retrieval_artifact_ids = match internal_conversation_ref {
        Some(internal_conversation_ref) => {
            continuity::list_active_retrieval_artifacts_for_conversation(
                pool,
                internal_conversation_ref,
                DEFAULT_RETRIEVAL_SCOPE_LIMIT,
            )
            .await?
            .into_iter()
            .map(|artifact| artifact.retrieval_artifact_id)
            .collect()
        }
        None => list_recent_retrieval_artifact_ids(pool, DEFAULT_RETRIEVAL_SCOPE_LIMIT).await?,
    };
    let self_model_artifact_id = continuity::get_latest_active_self_model_artifact(pool)
        .await?
        .map(|artifact| artifact.self_model_artifact_id);
    let internal_principal_ref =
        resolve_scope_internal_principal_ref(pool, internal_conversation_ref).await?;
    let episode_count = episode_ids.len();
    let memory_count = memory_artifact_ids.len();
    let retrieval_count = retrieval_artifact_ids.len();

    Ok(UnconsciousScope {
        episode_ids,
        memory_artifact_ids,
        retrieval_artifact_ids,
        self_model_artifact_id,
        internal_principal_ref,
        internal_conversation_ref: internal_conversation_ref.map(str::to_string),
        summary: format!(
            "{} scope with {} episodes, {} memory artifacts, {} retrieval artifacts, self-model: {}",
            human_job_kind(job_kind),
            episode_count,
            memory_count,
            retrieval_count,
            if self_model_artifact_id.is_some() {
                "present"
            } else {
                "absent"
            }
        ),
    })
}

async fn resolve_scope_internal_principal_ref(
    pool: &PgPool,
    internal_conversation_ref: Option<&str>,
) -> Result<Option<String>> {
    let Some(internal_conversation_ref) = internal_conversation_ref else {
        return Ok(None);
    };

    let binding_row = sqlx::query(
        r#"
        SELECT internal_principal_ref
        FROM conversation_bindings
        WHERE internal_conversation_ref = $1
        ORDER BY updated_at DESC, conversation_binding_id DESC
        LIMIT 1
        "#,
    )
    .bind(internal_conversation_ref)
    .fetch_optional(pool)
    .await
    .context("failed to resolve background scope principal from conversation binding")?;
    if let Some(row) = binding_row {
        let internal_principal_ref: String = row.get("internal_principal_ref");
        return Ok(Some(internal_principal_ref));
    }

    let episode_row = sqlx::query(
        r#"
        SELECT internal_principal_ref
        FROM episodes
        WHERE internal_conversation_ref = $1
        ORDER BY started_at DESC, episode_id DESC
        LIMIT 1
        "#,
    )
    .bind(internal_conversation_ref)
    .fetch_optional(pool)
    .await
    .context("failed to resolve background scope principal from recent episodes")?;

    Ok(episode_row.map(|row| row.get("internal_principal_ref")))
}

async fn list_recent_episode_ids(pool: &PgPool, limit: i64) -> Result<Vec<Uuid>> {
    let rows = sqlx::query(
        r#"
        SELECT episode_id
        FROM episodes
        ORDER BY started_at DESC, episode_id DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("failed to list recent episode ids for background scope")?;

    Ok(rows.into_iter().map(|row| row.get("episode_id")).collect())
}

async fn list_recent_retrieval_artifact_ids(pool: &PgPool, limit: i64) -> Result<Vec<Uuid>> {
    let rows = sqlx::query(
        r#"
        SELECT retrieval_artifact_id
        FROM retrieval_artifacts
        WHERE status = 'active'
        ORDER BY relevance_timestamp DESC, retrieval_artifact_id DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("failed to list recent retrieval artifact ids for background scope")?;

    Ok(rows
        .into_iter()
        .map(|row| row.get("retrieval_artifact_id"))
        .collect())
}

fn job_kind_as_str(value: UnconsciousJobKind) -> &'static str {
    match value {
        UnconsciousJobKind::MemoryConsolidation => "memory_consolidation",
        UnconsciousJobKind::RetrievalMaintenance => "retrieval_maintenance",
        UnconsciousJobKind::ContradictionAndDriftScan => "contradiction_and_drift_scan",
        UnconsciousJobKind::SelfModelReflection => "self_model_reflection",
    }
}

fn human_job_kind(value: UnconsciousJobKind) -> &'static str {
    match value {
        UnconsciousJobKind::MemoryConsolidation => "memory consolidation",
        UnconsciousJobKind::RetrievalMaintenance => "retrieval maintenance",
        UnconsciousJobKind::ContradictionAndDriftScan => "contradiction and drift scan",
        UnconsciousJobKind::SelfModelReflection => "self-model reflection",
    }
}

fn trigger_kind_as_str(value: BackgroundTriggerKind) -> &'static str {
    match value {
        BackgroundTriggerKind::TimeSchedule => "time_schedule",
        BackgroundTriggerKind::VolumeThreshold => "volume_threshold",
        BackgroundTriggerKind::DriftOrAnomalySignal => "drift_or_anomaly_signal",
        BackgroundTriggerKind::ForegroundDelegation => "foreground_delegation",
        BackgroundTriggerKind::ExternalPassiveEvent => "external_passive_event",
        BackgroundTriggerKind::MaintenanceTrigger => "maintenance_trigger",
    }
}

fn normalize_dedup_component(value: &str) -> String {
    let normalized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    let collapsed = normalized
        .split('-')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if collapsed.is_empty() {
        "unspecified".to_string()
    } else {
        collapsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_accepts_required_trigger_kinds() {
        for trigger_kind in [
            BackgroundTriggerKind::TimeSchedule,
            BackgroundTriggerKind::VolumeThreshold,
            BackgroundTriggerKind::DriftOrAnomalySignal,
            BackgroundTriggerKind::ForegroundDelegation,
            BackgroundTriggerKind::ExternalPassiveEvent,
            BackgroundTriggerKind::MaintenanceTrigger,
        ] {
            let trigger = BackgroundTrigger {
                trigger_id: Uuid::now_v7(),
                trigger_kind,
                requested_at: Utc::now(),
                reason_summary: "trigger validation".to_string(),
                payload_ref: None,
            };
            assert!(validate_background_trigger(&trigger).is_ok());
        }
    }

    #[test]
    fn deduplication_key_normalizes_payload_or_reason_context() {
        let trigger = BackgroundTrigger {
            trigger_id: Uuid::now_v7(),
            trigger_kind: BackgroundTriggerKind::ForegroundDelegation,
            requested_at: Utc::now(),
            reason_summary: "Delegate after backlog recovery".to_string(),
            payload_ref: Some("ingress://telegram-primary/42".to_string()),
        };

        let key = build_deduplication_key(
            UnconsciousJobKind::RetrievalMaintenance,
            &trigger,
            Some("telegram-primary"),
        );
        assert_eq!(
            key,
            "background:retrieval_maintenance:foreground_delegation:ingress-telegram-primary-42"
        );
    }

    #[test]
    fn deduplication_key_falls_back_to_global_schedule_scope() {
        let trigger = BackgroundTrigger {
            trigger_id: Uuid::now_v7(),
            trigger_kind: BackgroundTriggerKind::TimeSchedule,
            requested_at: Utc::now(),
            reason_summary: "nightly".to_string(),
            payload_ref: None,
        };

        let key = build_deduplication_key(UnconsciousJobKind::MemoryConsolidation, &trigger, None);
        assert_eq!(key, "background:memory_consolidation:time_schedule:global");
    }

    #[test]
    fn deduplication_key_falls_back_to_reason_summary_when_no_payload_or_conversation_exists() {
        let trigger = BackgroundTrigger {
            trigger_id: Uuid::now_v7(),
            trigger_kind: BackgroundTriggerKind::MaintenanceTrigger,
            requested_at: Utc::now(),
            reason_summary: "Maintenance: contradiction scan / nightly".to_string(),
            payload_ref: None,
        };

        let key = build_deduplication_key(
            UnconsciousJobKind::ContradictionAndDriftScan,
            &trigger,
            None,
        );
        assert_eq!(
            key,
            "background:contradiction_and_drift_scan:maintenance_trigger:maintenance-contradiction-scan-nightly"
        );
    }

    #[test]
    fn validate_job_trigger_compatibility_rejects_drift_signal_for_contradiction_scan() {
        let trigger = BackgroundTrigger {
            trigger_id: Uuid::now_v7(),
            trigger_kind: BackgroundTriggerKind::DriftOrAnomalySignal,
            requested_at: Utc::now(),
            reason_summary: "drift".to_string(),
            payload_ref: Some("diagnostic://event".to_string()),
        };

        let error = validate_job_trigger_compatibility(
            UnconsciousJobKind::ContradictionAndDriftScan,
            &trigger,
        )
        .expect_err("drift signal should be rejected for contradiction scan");
        assert!(error.contains("not supported"));
    }
}
