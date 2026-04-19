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
    continuity, foreground, policy,
};

const DEFAULT_EPISODE_SCOPE_LIMIT: i64 = 4;
const DEFAULT_MEMORY_SCOPE_LIMIT: i64 = 8;
const DEFAULT_RETRIEVAL_SCOPE_LIMIT: i64 = 6;

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

pub async fn plan_background_job(
    pool: &PgPool,
    config: &RuntimeConfig,
    request: BackgroundPlanningRequest,
) -> Result<BackgroundPlanningDecision> {
    let validation = validate_background_trigger(&request.trigger);
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
        | BackgroundTriggerKind::ForegroundDelegation
        | BackgroundTriggerKind::MaintenanceTrigger => Ok(()),
        BackgroundTriggerKind::DriftOrAnomalySignal => Err(
            "drift_or_anomaly_signal is recognized but not yet schedulable in the initial Phase 4 trigger slice"
                .to_string(),
        ),
        BackgroundTriggerKind::ExternalPassiveEvent => Err(
            "external_passive_event is recognized but not yet schedulable in the initial Phase 4 trigger slice"
                .to_string(),
        ),
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
    let episode_count = episode_ids.len();
    let memory_count = memory_artifact_ids.len();
    let retrieval_count = retrieval_artifact_ids.len();

    Ok(UnconsciousScope {
        episode_ids,
        memory_artifact_ids,
        retrieval_artifact_ids,
        self_model_artifact_id,
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
    fn validation_accepts_and_defers_expected_trigger_kinds() {
        let allowed = BackgroundTrigger {
            trigger_id: Uuid::now_v7(),
            trigger_kind: BackgroundTriggerKind::TimeSchedule,
            requested_at: Utc::now(),
            reason_summary: "scheduled sweep".to_string(),
            payload_ref: None,
        };
        assert!(validate_background_trigger(&allowed).is_ok());

        let deferred = BackgroundTrigger {
            trigger_id: Uuid::now_v7(),
            trigger_kind: BackgroundTriggerKind::DriftOrAnomalySignal,
            requested_at: Utc::now(),
            reason_summary: "diagnostic event".to_string(),
            payload_ref: None,
        };
        let error = validate_background_trigger(&deferred)
            .expect_err("drift triggers should fail closed for now");
        assert!(error.contains("recognized"));
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
}
