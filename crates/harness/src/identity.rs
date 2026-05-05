use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use contracts::{
    CanonicalProposal, CanonicalProposalPayload, IdentityDeltaOperation, IdentityItemCategory,
    IdentityItemDelta, IdentityItemSource, IdentityKickstartAction, IdentityMergePolicy,
    IdentityStabilityClass, MergeDecisionTarget, ProposalEvaluation, ProposalEvaluationOutcome,
    ProposalProvenanceKind,
};
use contracts::{CompactIdentityItem, CompactIdentitySnapshot};
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool, Row};
use uuid::Uuid;

use crate::{
    continuity,
    proposal::{self, ProposalProcessingContext},
};

#[derive(Debug, Clone)]
pub struct NewIdentityLifecycle {
    pub identity_lifecycle_id: Uuid,
    pub status: String,
    pub lifecycle_state: String,
    pub active_self_model_artifact_id: Option<Uuid>,
    pub active_interview_id: Option<Uuid>,
    pub transition_reason: String,
    pub transitioned_by: String,
    pub kickstart_started_at: Option<DateTime<Utc>>,
    pub kickstart_completed_at: Option<DateTime<Utc>>,
    pub reset_at: Option<DateTime<Utc>>,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct IdentityLifecycleRecord {
    pub identity_lifecycle_id: Uuid,
    pub status: String,
    pub lifecycle_state: String,
    pub active_self_model_artifact_id: Option<Uuid>,
    pub active_interview_id: Option<Uuid>,
    pub transition_reason: String,
    pub transitioned_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct NewIdentityItem {
    pub identity_item_id: Uuid,
    pub self_model_artifact_id: Option<Uuid>,
    pub proposal_id: Option<Uuid>,
    pub trace_id: Option<Uuid>,
    pub stability_class: String,
    pub category: String,
    pub item_key: String,
    pub value_text: String,
    pub confidence: f64,
    pub weight: Option<f64>,
    pub provenance_kind: String,
    pub source_kind: String,
    pub merge_policy: String,
    pub status: String,
    pub evidence_refs: Value,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_to: Option<DateTime<Utc>>,
    pub supersedes_item_id: Option<Uuid>,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct IdentityItemRecord {
    pub identity_item_id: Uuid,
    pub self_model_artifact_id: Option<Uuid>,
    pub proposal_id: Option<Uuid>,
    pub trace_id: Option<Uuid>,
    pub stability_class: String,
    pub category: String,
    pub item_key: String,
    pub value_text: String,
    pub confidence: f64,
    pub weight: Option<f64>,
    pub provenance_kind: String,
    pub source_kind: String,
    pub merge_policy: String,
    pub status: String,
    pub evidence_refs: Value,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_to: Option<DateTime<Utc>>,
    pub supersedes_item_id: Option<Uuid>,
    pub superseded_by_item_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct NewIdentityTemplate {
    pub identity_template_id: Uuid,
    pub template_key: String,
    pub display_name: String,
    pub description: String,
    pub status: String,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct NewIdentityTemplateItem {
    pub identity_template_item_id: Uuid,
    pub identity_template_id: Uuid,
    pub stability_class: String,
    pub category: String,
    pub item_key: String,
    pub value_text: String,
    pub confidence: f64,
    pub weight: Option<f64>,
    pub merge_policy: String,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct NewIdentityInterview {
    pub identity_interview_id: Uuid,
    pub status: String,
    pub current_step: String,
    pub answered_fields: Value,
    pub required_fields: Value,
    pub last_prompt_text: Option<String>,
    pub selected_template_id: Option<Uuid>,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct IdentityInterviewRecord {
    pub identity_interview_id: Uuid,
    pub status: String,
    pub current_step: String,
    pub answered_fields: Value,
    pub required_fields: Value,
    pub last_prompt_text: Option<String>,
    pub selected_template_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct IdentityResetOutcome {
    pub reset_at: DateTime<Utc>,
    pub previous_lifecycle_state: Option<String>,
    pub superseded_identity_item_count: u32,
    pub cancelled_interview_count: u32,
}

#[derive(Debug, Clone)]
pub struct NewIdentityDiagnostic {
    pub identity_diagnostic_id: Uuid,
    pub diagnostic_kind: String,
    pub severity: String,
    pub status: String,
    pub identity_item_id: Option<Uuid>,
    pub proposal_id: Option<Uuid>,
    pub trace_id: Option<Uuid>,
    pub message: String,
    pub evidence_refs: Value,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct IdentityTemplateSummary {
    pub identity_template_id: Uuid,
    pub template_key: String,
    pub display_name: String,
    pub description: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct IdentityDiagnosticRecord {
    pub identity_diagnostic_id: Uuid,
    pub diagnostic_kind: String,
    pub severity: String,
    pub status: String,
    pub identity_item_id: Option<Uuid>,
    pub proposal_id: Option<Uuid>,
    pub trace_id: Option<Uuid>,
    pub message: String,
    pub evidence_refs: Value,
    pub created_at: DateTime<Utc>,
    pub payload: Value,
}

const CUSTOM_IDENTITY_STEPS: &[&str] = &[
    "name",
    "identity_form",
    "archetype_role",
    "temperament",
    "communication_style",
    "backstory",
    "age_framing",
    "likes",
    "dislikes",
    "values",
    "boundaries",
    "tendencies",
    "goals",
    "relationship_to_user",
];

pub async fn record_lifecycle_transition(
    pool: &PgPool,
    lifecycle: &NewIdentityLifecycle,
) -> Result<()> {
    let mut transaction = pool.begin().await?;
    record_lifecycle_transition_in_tx(&mut transaction, lifecycle).await?;
    transaction.commit().await?;
    Ok(())
}

pub async fn reset_to_bootstrap(
    pool: &PgPool,
    trace_id: Uuid,
    actor_ref: &str,
    reason: Option<&str>,
) -> Result<IdentityResetOutcome> {
    let reset_at = Utc::now();
    let mut transaction = pool.begin().await?;

    let previous_lifecycle = sqlx::query(
        r#"
        SELECT identity_lifecycle_id, status, lifecycle_state, active_self_model_artifact_id,
               active_interview_id, transition_reason, transitioned_by, created_at, updated_at,
               payload_json
        FROM identity_lifecycle
        WHERE status = 'current'
        ORDER BY updated_at DESC
        LIMIT 1
        "#,
    )
    .fetch_optional(&mut *transaction)
    .await?
    .map(identity_lifecycle_from_row)
    .transpose()?;

    let item_rows = sqlx::query(
        r#"
        UPDATE identity_items
        SET status = 'superseded',
            superseded_at = NOW(),
            updated_at = NOW(),
            payload_json = payload_json || $1
        WHERE status = 'active'
        RETURNING identity_item_id
        "#,
    )
    .bind(json!({
        "superseded_by": "identity_reset",
        "reset_trace_id": trace_id,
        "reset_actor_ref": actor_ref,
        "reset_reason": reason,
        "reset_at": reset_at,
    }))
    .fetch_all(&mut *transaction)
    .await?;

    let interview_rows = sqlx::query(
        r#"
        UPDATE identity_kickstart_interviews
        SET status = 'cancelled',
            current_step = 'cancelled',
            cancelled_at = NOW(),
            updated_at = NOW(),
            payload_json = payload_json || $1
        WHERE status = 'in_progress'
        RETURNING identity_interview_id
        "#,
    )
    .bind(json!({
        "cancelled_by": "identity_reset",
        "reset_trace_id": trace_id,
        "reset_actor_ref": actor_ref,
        "reset_reason": reason,
        "reset_at": reset_at,
    }))
    .fetch_all(&mut *transaction)
    .await?;

    record_lifecycle_transition_in_tx(
        &mut transaction,
        &NewIdentityLifecycle {
            identity_lifecycle_id: Uuid::now_v7(),
            status: "current".to_string(),
            lifecycle_state: "bootstrap_seed_only".to_string(),
            active_self_model_artifact_id: None,
            active_interview_id: None,
            transition_reason: reason
                .map(str::to_string)
                .unwrap_or_else(|| "operator identity reset".to_string()),
            transitioned_by: actor_ref.to_string(),
            kickstart_started_at: None,
            kickstart_completed_at: None,
            reset_at: Some(reset_at),
            payload: json!({
                "reset_trace_id": trace_id,
                "reset_actor_ref": actor_ref,
                "reset_reason": reason,
                "previous_lifecycle_state": previous_lifecycle
                    .as_ref()
                    .map(|record| record.lifecycle_state.clone()),
                "superseded_identity_item_count": item_rows.len(),
                "cancelled_interview_count": interview_rows.len(),
            }),
        },
    )
    .await?;

    transaction.commit().await?;

    Ok(IdentityResetOutcome {
        reset_at,
        previous_lifecycle_state: previous_lifecycle.map(|record| record.lifecycle_state),
        superseded_identity_item_count: item_rows.len() as u32,
        cancelled_interview_count: interview_rows.len() as u32,
    })
}

async fn record_lifecycle_transition_in_tx(
    executor: &mut PgConnection,
    lifecycle: &NewIdentityLifecycle,
) -> Result<()> {
    if lifecycle.status == "current" {
        sqlx::query("UPDATE identity_lifecycle SET status = 'superseded', updated_at = NOW() WHERE status = 'current'")
            .execute(&mut *executor)
            .await?;
    }
    sqlx::query(
        r#"
        INSERT INTO identity_lifecycle (
            identity_lifecycle_id,
            status,
            lifecycle_state,
            active_self_model_artifact_id,
            active_interview_id,
            transition_reason,
            transitioned_by,
            kickstart_started_at,
            kickstart_completed_at,
            reset_at,
            payload_json
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        "#,
    )
    .bind(lifecycle.identity_lifecycle_id)
    .bind(&lifecycle.status)
    .bind(&lifecycle.lifecycle_state)
    .bind(lifecycle.active_self_model_artifact_id)
    .bind(lifecycle.active_interview_id)
    .bind(&lifecycle.transition_reason)
    .bind(&lifecycle.transitioned_by)
    .bind(lifecycle.kickstart_started_at)
    .bind(lifecycle.kickstart_completed_at)
    .bind(lifecycle.reset_at)
    .bind(&lifecycle.payload)
    .execute(&mut *executor)
    .await?;
    Ok(())
}

pub async fn get_current_lifecycle(pool: &PgPool) -> Result<Option<IdentityLifecycleRecord>> {
    let row = sqlx::query(
        r#"
        SELECT identity_lifecycle_id, status, lifecycle_state, active_self_model_artifact_id,
               active_interview_id, transition_reason, transitioned_by, created_at, updated_at,
               payload_json
        FROM identity_lifecycle
        WHERE status = 'current'
        ORDER BY updated_at DESC
        LIMIT 1
        "#,
    )
    .fetch_optional(pool)
    .await?;

    row.map(identity_lifecycle_from_row).transpose()
}

pub async fn insert_identity_item(pool: &PgPool, item: &NewIdentityItem) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO identity_items (
            identity_item_id, self_model_artifact_id, proposal_id, trace_id, stability_class,
            category, item_key, value_text, confidence, weight, provenance_kind, source_kind,
            merge_policy, status, evidence_refs_json, valid_from, valid_to, supersedes_item_id,
            payload_json
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19)
        "#,
    )
    .bind(item.identity_item_id)
    .bind(item.self_model_artifact_id)
    .bind(item.proposal_id)
    .bind(item.trace_id)
    .bind(&item.stability_class)
    .bind(&item.category)
    .bind(&item.item_key)
    .bind(&item.value_text)
    .bind(item.confidence)
    .bind(item.weight)
    .bind(&item.provenance_kind)
    .bind(&item.source_kind)
    .bind(&item.merge_policy)
    .bind(&item.status)
    .bind(&item.evidence_refs)
    .bind(item.valid_from)
    .bind(item.valid_to)
    .bind(item.supersedes_item_id)
    .bind(&item.payload)
    .execute(pool)
    .await?;
    Ok(())
}

async fn insert_identity_item_in_tx(
    executor: &mut PgConnection,
    item: &NewIdentityItem,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO identity_items (
            identity_item_id, self_model_artifact_id, proposal_id, trace_id, stability_class,
            category, item_key, value_text, confidence, weight, provenance_kind, source_kind,
            merge_policy, status, evidence_refs_json, valid_from, valid_to, supersedes_item_id,
            payload_json
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19)
        "#,
    )
    .bind(item.identity_item_id)
    .bind(item.self_model_artifact_id)
    .bind(item.proposal_id)
    .bind(item.trace_id)
    .bind(&item.stability_class)
    .bind(&item.category)
    .bind(&item.item_key)
    .bind(&item.value_text)
    .bind(item.confidence)
    .bind(item.weight)
    .bind(&item.provenance_kind)
    .bind(&item.source_kind)
    .bind(&item.merge_policy)
    .bind(&item.status)
    .bind(&item.evidence_refs)
    .bind(item.valid_from)
    .bind(item.valid_to)
    .bind(item.supersedes_item_id)
    .bind(&item.payload)
    .execute(&mut *executor)
    .await?;
    Ok(())
}

enum IdentityDeltaMergeOutcome {
    Accepted(Uuid),
    Rejected(String),
}

struct ActiveIdentityItemSummary {
    identity_item_id: Uuid,
    stability_class: String,
    category: String,
    item_key: String,
    value_text: String,
    merge_policy: String,
}

async fn apply_identity_item_delta_in_tx(
    executor: &mut PgConnection,
    context: &ProposalProcessingContext,
    proposal: &CanonicalProposal,
    delta: &IdentityItemDelta,
) -> Result<IdentityDeltaMergeOutcome> {
    if let Some(reason) = identity_delta_control_rejection_in_tx(executor, delta).await? {
        return Ok(IdentityDeltaMergeOutcome::Rejected(reason));
    }

    match delta.operation {
        IdentityDeltaOperation::Add => {
            let identity_item_id = Uuid::now_v7();
            let item = identity_item_from_delta(context, proposal, delta, identity_item_id)?;
            insert_identity_item_in_tx(executor, &item).await?;
            Ok(IdentityDeltaMergeOutcome::Accepted(identity_item_id))
        }
        IdentityDeltaOperation::Reinforce => {
            let Some(target_item_id) = resolve_identity_delta_target_in_tx(executor, delta).await?
            else {
                return Ok(IdentityDeltaMergeOutcome::Rejected(format!(
                    "identity_delta reinforce target not found for {}",
                    delta.item_key
                )));
            };
            reinforce_identity_item_in_tx(executor, target_item_id, delta, proposal).await?;
            Ok(IdentityDeltaMergeOutcome::Accepted(target_item_id))
        }
        IdentityDeltaOperation::Weaken => {
            let Some(target_item_id) = resolve_identity_delta_target_in_tx(executor, delta).await?
            else {
                return Ok(IdentityDeltaMergeOutcome::Rejected(format!(
                    "identity_delta weaken target not found for {}",
                    delta.item_key
                )));
            };
            weaken_identity_item_in_tx(executor, target_item_id, delta, proposal).await?;
            Ok(IdentityDeltaMergeOutcome::Accepted(target_item_id))
        }
        IdentityDeltaOperation::Revise | IdentityDeltaOperation::Supersede => {
            let Some(target_item_id) = resolve_identity_delta_target_in_tx(executor, delta).await?
            else {
                return Ok(IdentityDeltaMergeOutcome::Rejected(format!(
                    "identity_delta {} target not found for {}",
                    identity_delta_operation_as_str(delta.operation),
                    delta.item_key
                )));
            };
            let identity_item_id = Uuid::now_v7();
            let mut item = identity_item_from_delta(context, proposal, delta, identity_item_id)?;
            item.supersedes_item_id = Some(target_item_id);
            insert_identity_item_in_tx(executor, &item).await?;
            supersede_identity_item_in_tx(executor, target_item_id, identity_item_id).await?;
            Ok(IdentityDeltaMergeOutcome::Accepted(identity_item_id))
        }
        IdentityDeltaOperation::Expire => {
            let Some(target_item_id) = resolve_identity_delta_target_in_tx(executor, delta).await?
            else {
                return Ok(IdentityDeltaMergeOutcome::Rejected(format!(
                    "identity_delta expire target not found for {}",
                    delta.item_key
                )));
            };
            expire_identity_item_in_tx(executor, target_item_id, delta, proposal).await?;
            Ok(IdentityDeltaMergeOutcome::Accepted(target_item_id))
        }
    }
}

async fn apply_self_description_delta_in_tx(
    executor: &mut PgConnection,
    context: &ProposalProcessingContext,
    proposal: &CanonicalProposal,
    delta: &contracts::SelfDescriptionDelta,
) -> Result<IdentityDeltaMergeOutcome> {
    match delta.operation {
        IdentityDeltaOperation::Add => {
            let identity_item_id = Uuid::now_v7();
            insert_identity_item_in_tx(
                executor,
                &self_description_item_from_delta(
                    context,
                    proposal,
                    delta,
                    identity_item_id,
                    None,
                )?,
            )
            .await?;
            Ok(IdentityDeltaMergeOutcome::Accepted(identity_item_id))
        }
        IdentityDeltaOperation::Revise | IdentityDeltaOperation::Supersede => {
            let Some(target_item_id) = find_active_identity_item_by_key_in_tx(
                executor,
                "recurring_self_description",
                "self_description",
            )
            .await?
            else {
                return Ok(IdentityDeltaMergeOutcome::Rejected(
                    "self_description_delta target not found".to_string(),
                ));
            };
            let identity_item_id = Uuid::now_v7();
            insert_identity_item_in_tx(
                executor,
                &self_description_item_from_delta(
                    context,
                    proposal,
                    delta,
                    identity_item_id,
                    Some(target_item_id),
                )?,
            )
            .await?;
            supersede_identity_item_in_tx(executor, target_item_id, identity_item_id).await?;
            Ok(IdentityDeltaMergeOutcome::Accepted(identity_item_id))
        }
        IdentityDeltaOperation::Reinforce => {
            let Some(target_item_id) = find_active_identity_item_by_key_in_tx(
                executor,
                "recurring_self_description",
                "self_description",
            )
            .await?
            else {
                return Ok(IdentityDeltaMergeOutcome::Rejected(
                    "self_description_delta target not found".to_string(),
                ));
            };
            reinforce_identity_item_values_in_tx(
                executor,
                target_item_id,
                f64::from(proposal.confidence_pct) / 100.0,
                Some(0.8),
                json!({
                    "proposal_rationale": proposal.rationale,
                    "delta_operation": identity_delta_operation_as_str(delta.operation),
                }),
            )
            .await?;
            Ok(IdentityDeltaMergeOutcome::Accepted(target_item_id))
        }
        IdentityDeltaOperation::Weaken => {
            let Some(target_item_id) = find_active_identity_item_by_key_in_tx(
                executor,
                "recurring_self_description",
                "self_description",
            )
            .await?
            else {
                return Ok(IdentityDeltaMergeOutcome::Rejected(
                    "self_description_delta target not found".to_string(),
                ));
            };
            weaken_identity_item_values_in_tx(
                executor,
                target_item_id,
                f64::from(proposal.confidence_pct) / 100.0,
                Some(0.2),
                json!({
                    "proposal_rationale": proposal.rationale,
                    "delta_operation": identity_delta_operation_as_str(delta.operation),
                }),
            )
            .await?;
            Ok(IdentityDeltaMergeOutcome::Accepted(target_item_id))
        }
        IdentityDeltaOperation::Expire => {
            let Some(target_item_id) = find_active_identity_item_by_key_in_tx(
                executor,
                "recurring_self_description",
                "self_description",
            )
            .await?
            else {
                return Ok(IdentityDeltaMergeOutcome::Rejected(
                    "self_description_delta target not found".to_string(),
                ));
            };
            expire_identity_item_values_in_tx(
                executor,
                target_item_id,
                json!({
                    "proposal_rationale": proposal.rationale,
                    "delta_operation": identity_delta_operation_as_str(delta.operation),
                }),
            )
            .await?;
            Ok(IdentityDeltaMergeOutcome::Accepted(target_item_id))
        }
    }
}

async fn identity_delta_control_rejection_in_tx(
    executor: &mut PgConnection,
    delta: &IdentityItemDelta,
) -> Result<Option<String>> {
    match delta.operation {
        IdentityDeltaOperation::Add => {
            let active = list_active_identity_items_by_category_in_tx(
                executor,
                identity_category_as_str(delta.category),
            )
            .await?;
            let normalized_delta = normalize_identity_text(&delta.value);
            for item in &active {
                let normalized_existing = normalize_identity_text(&item.value_text);
                if item.item_key == delta.item_key && normalized_existing == normalized_delta {
                    return Ok(Some(format!(
                        "identity_delta duplicate active identity item {} in category {}",
                        item.identity_item_id, item.category
                    )));
                }
                if item.item_key == delta.item_key {
                    return Ok(Some(format!(
                        "identity_delta contradicts active identity item {} in category {}",
                        item.identity_item_id, item.category
                    )));
                }
                if normalized_existing == normalized_delta
                    || identity_texts_are_near_duplicates(&normalized_existing, &normalized_delta)
                {
                    return Ok(Some(format!(
                        "identity_delta near-duplicate active identity item {} in category {}",
                        item.identity_item_id, item.category
                    )));
                }
            }
            Ok(None)
        }
        IdentityDeltaOperation::Reinforce => {
            let Some(target_item_id) = resolve_identity_delta_target_in_tx(executor, delta).await?
            else {
                return Ok(None);
            };
            let Some(target) =
                get_active_identity_item_summary_in_tx(executor, target_item_id).await?
            else {
                return Ok(None);
            };
            if normalize_identity_text(&target.value_text) != normalize_identity_text(&delta.value)
            {
                return Ok(Some(format!(
                    "identity_delta reinforce value contradicts active identity item {}",
                    target.item_key
                )));
            }
            Ok(None)
        }
        IdentityDeltaOperation::Weaken
        | IdentityDeltaOperation::Revise
        | IdentityDeltaOperation::Supersede
        | IdentityDeltaOperation::Expire => {
            let Some(target_item_id) = resolve_identity_delta_target_in_tx(executor, delta).await?
            else {
                return Ok(None);
            };
            let Some(target) =
                get_active_identity_item_summary_in_tx(executor, target_item_id).await?
            else {
                return Ok(None);
            };
            if target.merge_policy == "protected_core" || target.stability_class == "stable" {
                return Ok(Some(format!(
                    "identity_delta {} would drift protected stable identity item {}",
                    identity_delta_operation_as_str(delta.operation),
                    target.item_key
                )));
            }
            Ok(None)
        }
    }
}

async fn resolve_identity_delta_target_in_tx(
    executor: &mut PgConnection,
    delta: &IdentityItemDelta,
) -> Result<Option<Uuid>> {
    if let Some(target_item_id) = delta.target_identity_item_id {
        let exists = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1 FROM identity_items
                WHERE identity_item_id = $1 AND status = 'active'
            )
            "#,
        )
        .bind(target_item_id)
        .fetch_one(&mut *executor)
        .await?;
        return Ok(exists.then_some(target_item_id));
    }

    find_active_identity_item_by_key_in_tx(
        executor,
        identity_category_as_str(delta.category),
        &delta.item_key,
    )
    .await
}

async fn find_active_identity_item_by_key_in_tx(
    executor: &mut PgConnection,
    category: &str,
    item_key: &str,
) -> Result<Option<Uuid>> {
    sqlx::query_scalar(
        r#"
        SELECT identity_item_id
        FROM identity_items
        WHERE status = 'active'
          AND category = $1
          AND item_key = $2
        ORDER BY updated_at DESC
        LIMIT 1
        "#,
    )
    .bind(category)
    .bind(item_key)
    .fetch_optional(&mut *executor)
    .await
    .map_err(Into::into)
}

async fn get_active_identity_item_summary_in_tx(
    executor: &mut PgConnection,
    identity_item_id: Uuid,
) -> Result<Option<ActiveIdentityItemSummary>> {
    let row = sqlx::query(
        r#"
        SELECT identity_item_id, stability_class, category, item_key, value_text, merge_policy
        FROM identity_items
        WHERE identity_item_id = $1 AND status = 'active'
        "#,
    )
    .bind(identity_item_id)
    .fetch_optional(&mut *executor)
    .await?;

    row.map(active_identity_item_summary_from_row).transpose()
}

async fn list_active_identity_items_by_category_in_tx(
    executor: &mut PgConnection,
    category: &str,
) -> Result<Vec<ActiveIdentityItemSummary>> {
    let rows = sqlx::query(
        r#"
        SELECT identity_item_id, stability_class, category, item_key, value_text, merge_policy
        FROM identity_items
        WHERE status = 'active' AND category = $1
        ORDER BY updated_at DESC
        LIMIT 64
        "#,
    )
    .bind(category)
    .fetch_all(&mut *executor)
    .await?;

    rows.into_iter()
        .map(active_identity_item_summary_from_row)
        .collect()
}

fn active_identity_item_summary_from_row(
    row: sqlx::postgres::PgRow,
) -> Result<ActiveIdentityItemSummary> {
    Ok(ActiveIdentityItemSummary {
        identity_item_id: row.try_get("identity_item_id")?,
        stability_class: row.try_get("stability_class")?,
        category: row.try_get("category")?,
        item_key: row.try_get("item_key")?,
        value_text: row.try_get("value_text")?,
        merge_policy: row.try_get("merge_policy")?,
    })
}

async fn reinforce_identity_item_in_tx(
    executor: &mut PgConnection,
    identity_item_id: Uuid,
    delta: &IdentityItemDelta,
    proposal: &CanonicalProposal,
) -> Result<()> {
    reinforce_identity_item_values_in_tx(
        executor,
        identity_item_id,
        f64::from(delta.confidence_pct) / 100.0,
        delta.weight_pct.map(|weight| f64::from(weight) / 100.0),
        json!({
            "proposal_rationale": proposal.rationale,
            "delta_operation": identity_delta_operation_as_str(delta.operation),
            "reinforced_value": delta.value,
        }),
    )
    .await
}

async fn weaken_identity_item_in_tx(
    executor: &mut PgConnection,
    identity_item_id: Uuid,
    delta: &IdentityItemDelta,
    proposal: &CanonicalProposal,
) -> Result<()> {
    weaken_identity_item_values_in_tx(
        executor,
        identity_item_id,
        f64::from(delta.confidence_pct) / 100.0,
        delta.weight_pct.map(|weight| f64::from(weight) / 100.0),
        json!({
            "proposal_rationale": proposal.rationale,
            "delta_operation": identity_delta_operation_as_str(delta.operation),
            "weakened_value": delta.value,
        }),
    )
    .await
}

async fn reinforce_identity_item_values_in_tx(
    executor: &mut PgConnection,
    identity_item_id: Uuid,
    confidence: f64,
    weight: Option<f64>,
    payload: Value,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE identity_items
        SET confidence = LEAST(1.0, GREATEST(confidence, $2)),
            weight = CASE
                WHEN $3::DOUBLE PRECISION IS NULL THEN weight
                WHEN weight IS NULL THEN $3
                ELSE LEAST(1.0, GREATEST(weight, $3))
            END,
            payload_json = payload_json || $4,
            updated_at = NOW()
        WHERE identity_item_id = $1 AND status = 'active'
        "#,
    )
    .bind(identity_item_id)
    .bind(confidence)
    .bind(weight)
    .bind(payload)
    .execute(&mut *executor)
    .await?;
    Ok(())
}

async fn weaken_identity_item_values_in_tx(
    executor: &mut PgConnection,
    identity_item_id: Uuid,
    confidence: f64,
    weight: Option<f64>,
    payload: Value,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE identity_items
        SET confidence = GREATEST(0.0, confidence - $2),
            weight = CASE
                WHEN $3::DOUBLE PRECISION IS NULL OR weight IS NULL THEN weight
                ELSE GREATEST(0.0, weight - $3)
            END,
            payload_json = payload_json || $4,
            updated_at = NOW()
        WHERE identity_item_id = $1 AND status = 'active'
        "#,
    )
    .bind(identity_item_id)
    .bind(confidence)
    .bind(weight)
    .bind(payload)
    .execute(&mut *executor)
    .await?;
    Ok(())
}

async fn supersede_identity_item_in_tx(
    executor: &mut PgConnection,
    identity_item_id: Uuid,
    superseded_by_item_id: Uuid,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE identity_items
        SET status = 'superseded',
            superseded_at = NOW(),
            superseded_by_item_id = $2,
            updated_at = NOW()
        WHERE identity_item_id = $1 AND status = 'active'
        "#,
    )
    .bind(identity_item_id)
    .bind(superseded_by_item_id)
    .execute(&mut *executor)
    .await?;
    Ok(())
}

async fn expire_identity_item_in_tx(
    executor: &mut PgConnection,
    identity_item_id: Uuid,
    delta: &IdentityItemDelta,
    proposal: &CanonicalProposal,
) -> Result<()> {
    expire_identity_item_values_in_tx(
        executor,
        identity_item_id,
        json!({
            "proposal_rationale": proposal.rationale,
            "delta_operation": identity_delta_operation_as_str(delta.operation),
            "expired_value": delta.value,
        }),
    )
    .await
}

async fn expire_identity_item_values_in_tx(
    executor: &mut PgConnection,
    identity_item_id: Uuid,
    payload: Value,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE identity_items
        SET status = 'expired',
            valid_to = CASE
                WHEN valid_to IS NOT NULL THEN valid_to
                WHEN valid_from IS NOT NULL THEN GREATEST(valid_from, NOW())
                ELSE NOW()
            END,
            payload_json = payload_json || $2,
            updated_at = NOW()
        WHERE identity_item_id = $1 AND status = 'active'
        "#,
    )
    .bind(identity_item_id)
    .bind(payload)
    .execute(&mut *executor)
    .await?;
    Ok(())
}

pub async fn apply_identity_delta_proposal_merge(
    pool: &PgPool,
    context: &ProposalProcessingContext,
    proposal: &CanonicalProposal,
) -> Result<ProposalEvaluation> {
    let validation = proposal::validate_proposal(proposal);
    if validation.outcome == ProposalEvaluationOutcome::Rejected {
        continuity::update_merge_decision_outcome(
            pool,
            proposal.proposal_id,
            "rejected",
            &validation.reason,
        )
        .await?;
        return Ok(validation);
    }

    let payload = match &proposal.payload {
        CanonicalProposalPayload::IdentityDelta(payload) => payload,
        _ => {
            let reason = "identity merge requires an identity_delta payload".to_string();
            continuity::update_merge_decision_outcome(
                pool,
                proposal.proposal_id,
                "rejected",
                &reason,
            )
            .await?;
            return Ok(reject(proposal.proposal_id, reason));
        }
    };

    if let Some(action) = &payload.interview_action {
        return apply_identity_interview_action_merge(pool, context, proposal, payload, action)
            .await;
    }

    let current_lifecycle = get_current_lifecycle(pool).await?;
    let target_lifecycle_state = lifecycle_state_as_str(payload.lifecycle_state);
    let mut inserted_item_ids = Vec::new();
    let mut transaction = pool.begin().await?;
    for delta in &payload.item_deltas {
        match apply_identity_item_delta_in_tx(&mut transaction, context, proposal, delta).await? {
            IdentityDeltaMergeOutcome::Accepted(identity_item_id) => {
                inserted_item_ids.push(identity_item_id);
            }
            IdentityDeltaMergeOutcome::Rejected(reason) => {
                transaction.rollback().await?;
                insert_identity_delta_rejection_diagnostic(pool, context, proposal, &reason)
                    .await?;
                continuity::update_merge_decision_outcome(
                    pool,
                    proposal.proposal_id,
                    "rejected",
                    &reason,
                )
                .await?;
                return Ok(reject(proposal.proposal_id, reason));
            }
        }
    }

    if let Some(delta) = &payload.self_description_delta {
        match apply_self_description_delta_in_tx(&mut transaction, context, proposal, delta).await?
        {
            IdentityDeltaMergeOutcome::Accepted(identity_item_id) => {
                inserted_item_ids.push(identity_item_id);
            }
            IdentityDeltaMergeOutcome::Rejected(reason) => {
                transaction.rollback().await?;
                insert_identity_delta_rejection_diagnostic(pool, context, proposal, &reason)
                    .await?;
                continuity::update_merge_decision_outcome(
                    pool,
                    proposal.proposal_id,
                    "rejected",
                    &reason,
                )
                .await?;
                return Ok(reject(proposal.proposal_id, reason));
            }
        }
    }

    let should_record_lifecycle = current_lifecycle
        .as_ref()
        .is_none_or(|lifecycle| lifecycle.lifecycle_state != target_lifecycle_state);
    if should_record_lifecycle {
        record_lifecycle_transition_in_tx(
            &mut transaction,
            &NewIdentityLifecycle {
                identity_lifecycle_id: Uuid::now_v7(),
                status: "current".to_string(),
                lifecycle_state: target_lifecycle_state.to_string(),
                active_self_model_artifact_id: None,
                active_interview_id: None,
                transition_reason: payload.rationale.clone(),
                transitioned_by: "identity_delta_merge".to_string(),
                kickstart_started_at: proposal.valid_from,
                kickstart_completed_at: Some(Utc::now()),
                reset_at: None,
                payload: json!({
                    "proposal_id": proposal.proposal_id,
                    "identity_item_ids": inserted_item_ids,
                }),
            },
        )
        .await?;
    }
    continuity::update_merge_decision_targets_in_tx(
        &mut *transaction,
        proposal.proposal_id,
        None,
        None,
    )
    .await?;
    transaction.commit().await?;

    Ok(ProposalEvaluation {
        proposal_id: proposal.proposal_id,
        outcome: ProposalEvaluationOutcome::Accepted,
        reason: "identity proposal merged into canonical identity store".to_string(),
        target: Some(MergeDecisionTarget::IdentityItems(inserted_item_ids)),
    })
}

async fn apply_identity_interview_action_merge(
    pool: &PgPool,
    context: &ProposalProcessingContext,
    proposal: &CanonicalProposal,
    payload: &contracts::IdentityDeltaProposal,
    action: &IdentityKickstartAction,
) -> Result<ProposalEvaluation> {
    match action {
        IdentityKickstartAction::StartCustomInterview => {
            let interview_id = Uuid::now_v7();
            let mut transaction = pool.begin().await?;
            insert_identity_interview_in_tx(
                &mut transaction,
                &NewIdentityInterview {
                    identity_interview_id: interview_id,
                    status: "in_progress".to_string(),
                    current_step: CUSTOM_IDENTITY_STEPS[0].to_string(),
                    answered_fields: json!({}),
                    required_fields: json!(CUSTOM_IDENTITY_STEPS),
                    last_prompt_text: Some(custom_identity_step_prompt(CUSTOM_IDENTITY_STEPS[0])),
                    selected_template_id: None,
                    payload: json!({
                        "proposal_id": proposal.proposal_id,
                        "rationale": payload.rationale,
                    }),
                },
            )
            .await?;
            record_lifecycle_transition_in_tx(
                &mut transaction,
                &NewIdentityLifecycle {
                    identity_lifecycle_id: Uuid::now_v7(),
                    status: "current".to_string(),
                    lifecycle_state: "identity_kickstart_in_progress".to_string(),
                    active_self_model_artifact_id: None,
                    active_interview_id: Some(interview_id),
                    transition_reason: payload.rationale.clone(),
                    transitioned_by: "foreground_identity_interview".to_string(),
                    kickstart_started_at: Some(Utc::now()),
                    kickstart_completed_at: None,
                    reset_at: None,
                    payload: json!({
                        "proposal_id": proposal.proposal_id,
                        "current_step": CUSTOM_IDENTITY_STEPS[0],
                    }),
                },
            )
            .await?;
            transaction.commit().await?;
            Ok(ProposalEvaluation {
                proposal_id: proposal.proposal_id,
                outcome: ProposalEvaluationOutcome::Accepted,
                reason: "custom identity interview started".to_string(),
                target: None,
            })
        }
        IdentityKickstartAction::AnswerCustomInterview(answer) => {
            let Some(lifecycle) = get_current_lifecycle(pool).await? else {
                let reason = "no active identity interview lifecycle exists".to_string();
                continuity::update_merge_decision_outcome(
                    pool,
                    proposal.proposal_id,
                    "rejected",
                    &reason,
                )
                .await?;
                return Ok(reject(proposal.proposal_id, reason));
            };
            let Some(interview_id) = lifecycle.active_interview_id else {
                let reason = "identity lifecycle has no active interview".to_string();
                continuity::update_merge_decision_outcome(
                    pool,
                    proposal.proposal_id,
                    "rejected",
                    &reason,
                )
                .await?;
                return Ok(reject(proposal.proposal_id, reason));
            };
            let interview = get_identity_interview(pool, interview_id)
                .await?
                .ok_or_else(|| {
                    anyhow::anyhow!("active identity interview {interview_id} missing")
                })?;
            if interview.status != "in_progress" {
                let reason = "identity interview is not in progress".to_string();
                continuity::update_merge_decision_outcome(
                    pool,
                    proposal.proposal_id,
                    "rejected",
                    &reason,
                )
                .await?;
                return Ok(reject(proposal.proposal_id, reason));
            }
            if answer.step_key != interview.current_step {
                let reason = format!(
                    "identity interview expected step '{}' but received '{}'",
                    interview.current_step, answer.step_key
                );
                continuity::update_merge_decision_outcome(
                    pool,
                    proposal.proposal_id,
                    "rejected",
                    &reason,
                )
                .await?;
                return Ok(reject(proposal.proposal_id, reason));
            }

            let mut answered_fields = interview
                .answered_fields
                .as_object()
                .cloned()
                .unwrap_or_default();
            answered_fields.insert(
                answer.step_key.clone(),
                Value::String(answer.answer_text.trim().to_string()),
            );
            let answered_value = Value::Object(answered_fields.clone());
            let next_step = next_missing_step(&answered_fields);

            if let Some(next_step) = next_step {
                update_identity_interview_state(
                    pool,
                    interview_id,
                    "in_progress",
                    next_step,
                    &answered_value,
                    Some(&custom_identity_step_prompt(next_step)),
                    json!({
                        "last_proposal_id": proposal.proposal_id,
                        "answered_step": answer.step_key,
                    }),
                )
                .await?;
                return Ok(ProposalEvaluation {
                    proposal_id: proposal.proposal_id,
                    outcome: ProposalEvaluationOutcome::Accepted,
                    reason: format!("custom identity interview advanced to {next_step}"),
                    target: None,
                });
            }

            let item_deltas = custom_identity_deltas_from_answers(&answered_fields, proposal);
            let self_description = custom_self_description(&answered_fields);
            let mut inserted_item_ids = Vec::new();
            let mut transaction = pool.begin().await?;
            update_identity_interview_state_in_tx(
                &mut transaction,
                interview_id,
                "completed",
                "completed",
                &answered_value,
                None,
                json!({
                    "last_proposal_id": proposal.proposal_id,
                    "completed_at": Utc::now(),
                }),
            )
            .await?;
            for delta in item_deltas {
                let identity_item_id = Uuid::now_v7();
                let item = identity_item_from_delta(context, proposal, &delta, identity_item_id)?;
                insert_identity_item_in_tx(&mut transaction, &item).await?;
                inserted_item_ids.push(identity_item_id);
            }
            let self_description_item_id = Uuid::now_v7();
            insert_identity_item_in_tx(
                &mut transaction,
                &NewIdentityItem {
                    identity_item_id: self_description_item_id,
                    self_model_artifact_id: None,
                    proposal_id: Some(proposal.proposal_id),
                    trace_id: Some(context.trace_id),
                    stability_class: "evolving".to_string(),
                    category: "recurring_self_description".to_string(),
                    item_key: "self_description".to_string(),
                    value_text: self_description,
                    confidence: f64::from(proposal.confidence_pct) / 100.0,
                    weight: Some(0.8),
                    provenance_kind: provenance_kind_as_str(proposal.provenance.provenance_kind)
                        .to_string(),
                    source_kind: "custom_interview".to_string(),
                    merge_policy: "revisable".to_string(),
                    status: "active".to_string(),
                    evidence_refs: json!([{
                        "source_kind": "identity_interview",
                        "source_id": interview_id,
                        "summary": "Completed custom identity interview."
                    }]),
                    valid_from: proposal.valid_from,
                    valid_to: proposal.valid_to,
                    supersedes_item_id: None,
                    payload: json!({
                        "proposal_rationale": proposal.rationale,
                        "interview_id": interview_id,
                    }),
                },
            )
            .await?;
            inserted_item_ids.push(self_description_item_id);
            record_lifecycle_transition_in_tx(
                &mut transaction,
                &NewIdentityLifecycle {
                    identity_lifecycle_id: Uuid::now_v7(),
                    status: "current".to_string(),
                    lifecycle_state: "complete_identity_active".to_string(),
                    active_self_model_artifact_id: None,
                    active_interview_id: None,
                    transition_reason: "custom identity interview completed".to_string(),
                    transitioned_by: "foreground_identity_interview".to_string(),
                    kickstart_started_at: Some(interview.created_at),
                    kickstart_completed_at: Some(Utc::now()),
                    reset_at: None,
                    payload: json!({
                        "proposal_id": proposal.proposal_id,
                        "interview_id": interview_id,
                        "identity_item_ids": inserted_item_ids,
                    }),
                },
            )
            .await?;
            transaction.commit().await?;
            Ok(ProposalEvaluation {
                proposal_id: proposal.proposal_id,
                outcome: ProposalEvaluationOutcome::Accepted,
                reason: "custom identity interview completed and merged".to_string(),
                target: Some(MergeDecisionTarget::IdentityItems(inserted_item_ids)),
            })
        }
        IdentityKickstartAction::Cancel { reason } => {
            let Some(lifecycle) = get_current_lifecycle(pool).await? else {
                return Ok(ProposalEvaluation {
                    proposal_id: proposal.proposal_id,
                    outcome: ProposalEvaluationOutcome::Accepted,
                    reason: "no identity interview was active".to_string(),
                    target: None,
                });
            };
            if let Some(interview_id) = lifecycle.active_interview_id {
                update_identity_interview_state(
                    pool,
                    interview_id,
                    "cancelled",
                    "cancelled",
                    &json!({}),
                    None,
                    json!({
                        "proposal_id": proposal.proposal_id,
                        "cancel_reason": reason,
                    }),
                )
                .await?;
            }
            record_lifecycle_transition(
                pool,
                &NewIdentityLifecycle {
                    identity_lifecycle_id: Uuid::now_v7(),
                    status: "current".to_string(),
                    lifecycle_state: "bootstrap_seed_only".to_string(),
                    active_self_model_artifact_id: None,
                    active_interview_id: None,
                    transition_reason: reason
                        .clone()
                        .unwrap_or_else(|| "custom identity interview cancelled".to_string()),
                    transitioned_by: "foreground_identity_interview".to_string(),
                    kickstart_started_at: None,
                    kickstart_completed_at: None,
                    reset_at: None,
                    payload: json!({
                        "proposal_id": proposal.proposal_id,
                    }),
                },
            )
            .await?;
            Ok(ProposalEvaluation {
                proposal_id: proposal.proposal_id,
                outcome: ProposalEvaluationOutcome::Accepted,
                reason: "custom identity interview cancelled".to_string(),
                target: None,
            })
        }
        IdentityKickstartAction::SelectPredefinedTemplate { .. } => Ok(reject(
            proposal.proposal_id,
            "predefined template selections must be represented as identity item deltas",
        )),
    }
}

pub async fn list_active_identity_items(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<IdentityItemRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT identity_item_id, self_model_artifact_id, proposal_id, trace_id, stability_class,
               category, item_key, value_text, confidence, weight, provenance_kind, source_kind,
               merge_policy, status, evidence_refs_json, valid_from, valid_to, supersedes_item_id,
               superseded_by_item_id, created_at, updated_at, payload_json
        FROM identity_items
        WHERE status = 'active'
        ORDER BY stability_class ASC, category ASC, updated_at DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    rows.into_iter().map(identity_item_from_row).collect()
}

pub async fn supersede_identity_item(
    pool: &PgPool,
    identity_item_id: Uuid,
    superseded_by_item_id: Uuid,
) -> Result<()> {
    let result = sqlx::query(
        r#"
        UPDATE identity_items
        SET status = 'superseded',
            superseded_at = NOW(),
            superseded_by_item_id = $2,
            updated_at = NOW()
        WHERE identity_item_id = $1
        "#,
    )
    .bind(identity_item_id)
    .bind(superseded_by_item_id)
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        bail!("identity item {identity_item_id} was not found");
    }
    Ok(())
}

pub async fn insert_identity_template(pool: &PgPool, template: &NewIdentityTemplate) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO identity_templates (
            identity_template_id, template_key, display_name, description, status, payload_json
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(template.identity_template_id)
    .bind(&template.template_key)
    .bind(&template.display_name)
    .bind(&template.description)
    .bind(&template.status)
    .bind(&template.payload)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_active_identity_templates(pool: &PgPool) -> Result<Vec<IdentityTemplateSummary>> {
    let rows = sqlx::query(
        r#"
        SELECT identity_template_id, template_key, display_name, description, status
        FROM identity_templates
        WHERE status = 'active'
        ORDER BY template_key ASC
        "#,
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(IdentityTemplateSummary {
                identity_template_id: row.try_get("identity_template_id")?,
                template_key: row.try_get("template_key")?,
                display_name: row.try_get("display_name")?,
                description: row.try_get("description")?,
                status: row.try_get("status")?,
            })
        })
        .collect()
}

pub async fn insert_identity_template_item(
    pool: &PgPool,
    item: &NewIdentityTemplateItem,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO identity_template_items (
            identity_template_item_id, identity_template_id, stability_class, category,
            item_key, value_text, confidence, weight, merge_policy, payload_json
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        "#,
    )
    .bind(item.identity_template_item_id)
    .bind(item.identity_template_id)
    .bind(&item.stability_class)
    .bind(&item.category)
    .bind(&item.item_key)
    .bind(&item.value_text)
    .bind(item.confidence)
    .bind(item.weight)
    .bind(&item.merge_policy)
    .bind(&item.payload)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn insert_identity_interview(
    pool: &PgPool,
    interview: &NewIdentityInterview,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO identity_kickstart_interviews (
            identity_interview_id, status, current_step, answered_fields_json,
            required_fields_json, last_prompt_text, selected_template_id, payload_json
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
    )
    .bind(interview.identity_interview_id)
    .bind(&interview.status)
    .bind(&interview.current_step)
    .bind(&interview.answered_fields)
    .bind(&interview.required_fields)
    .bind(&interview.last_prompt_text)
    .bind(interview.selected_template_id)
    .bind(&interview.payload)
    .execute(pool)
    .await?;
    Ok(())
}

async fn insert_identity_interview_in_tx(
    executor: &mut PgConnection,
    interview: &NewIdentityInterview,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO identity_kickstart_interviews (
            identity_interview_id, status, current_step, answered_fields_json,
            required_fields_json, last_prompt_text, selected_template_id, payload_json
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
    )
    .bind(interview.identity_interview_id)
    .bind(&interview.status)
    .bind(&interview.current_step)
    .bind(&interview.answered_fields)
    .bind(&interview.required_fields)
    .bind(&interview.last_prompt_text)
    .bind(interview.selected_template_id)
    .bind(&interview.payload)
    .execute(&mut *executor)
    .await?;
    Ok(())
}

pub async fn get_identity_interview(
    pool: &PgPool,
    identity_interview_id: Uuid,
) -> Result<Option<IdentityInterviewRecord>> {
    let row = sqlx::query(
        r#"
        SELECT identity_interview_id, status, current_step, answered_fields_json,
               required_fields_json, last_prompt_text, selected_template_id,
               started_at AS created_at, updated_at, payload_json
        FROM identity_kickstart_interviews
        WHERE identity_interview_id = $1
        "#,
    )
    .bind(identity_interview_id)
    .fetch_optional(pool)
    .await?;
    row.map(identity_interview_from_row).transpose()
}

pub async fn update_identity_interview_state(
    pool: &PgPool,
    identity_interview_id: Uuid,
    status: &str,
    current_step: &str,
    answered_fields: &Value,
    last_prompt_text: Option<&str>,
    payload: Value,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE identity_kickstart_interviews
        SET status = $2,
            current_step = $3,
            answered_fields_json = $4,
            last_prompt_text = $5,
            payload_json = payload_json || $6,
            updated_at = NOW()
        WHERE identity_interview_id = $1
        "#,
    )
    .bind(identity_interview_id)
    .bind(status)
    .bind(current_step)
    .bind(answered_fields)
    .bind(last_prompt_text)
    .bind(&payload)
    .execute(pool)
    .await?;
    Ok(())
}

async fn update_identity_interview_state_in_tx(
    executor: &mut PgConnection,
    identity_interview_id: Uuid,
    status: &str,
    current_step: &str,
    answered_fields: &Value,
    last_prompt_text: Option<&str>,
    payload: Value,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE identity_kickstart_interviews
        SET status = $2,
            current_step = $3,
            answered_fields_json = $4,
            last_prompt_text = $5,
            payload_json = payload_json || $6,
            updated_at = NOW()
        WHERE identity_interview_id = $1
        "#,
    )
    .bind(identity_interview_id)
    .bind(status)
    .bind(current_step)
    .bind(answered_fields)
    .bind(last_prompt_text)
    .bind(&payload)
    .execute(&mut *executor)
    .await?;
    Ok(())
}

pub async fn insert_identity_diagnostic(
    pool: &PgPool,
    diagnostic: &NewIdentityDiagnostic,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO identity_diagnostics (
            identity_diagnostic_id, diagnostic_kind, severity, status, identity_item_id,
            proposal_id, trace_id, message, evidence_refs_json, payload_json
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        "#,
    )
    .bind(diagnostic.identity_diagnostic_id)
    .bind(&diagnostic.diagnostic_kind)
    .bind(&diagnostic.severity)
    .bind(&diagnostic.status)
    .bind(diagnostic.identity_item_id)
    .bind(diagnostic.proposal_id)
    .bind(diagnostic.trace_id)
    .bind(&diagnostic.message)
    .bind(&diagnostic.evidence_refs)
    .bind(&diagnostic.payload)
    .execute(pool)
    .await?;
    Ok(())
}

async fn insert_identity_delta_rejection_diagnostic(
    pool: &PgPool,
    context: &ProposalProcessingContext,
    proposal: &CanonicalProposal,
    reason: &str,
) -> Result<()> {
    insert_identity_diagnostic(
        pool,
        &NewIdentityDiagnostic {
            identity_diagnostic_id: Uuid::now_v7(),
            diagnostic_kind: "identity_delta_rejected".to_string(),
            severity: "warning".to_string(),
            status: "open".to_string(),
            identity_item_id: None,
            proposal_id: Some(proposal.proposal_id),
            trace_id: Some(context.trace_id),
            message: reason.to_string(),
            evidence_refs: json!([]),
            payload: json!({
                "proposal_id": proposal.proposal_id,
                "reason": reason,
            }),
        },
    )
    .await
}

pub async fn list_open_identity_diagnostics(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<IdentityDiagnosticRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT identity_diagnostic_id, diagnostic_kind, severity, status, identity_item_id,
               proposal_id, trace_id, message, evidence_refs_json, created_at, payload_json
        FROM identity_diagnostics
        WHERE status = 'open'
        ORDER BY created_at DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(IdentityDiagnosticRecord {
                identity_diagnostic_id: row.try_get("identity_diagnostic_id")?,
                diagnostic_kind: row.try_get("diagnostic_kind")?,
                severity: row.try_get("severity")?,
                status: row.try_get("status")?,
                identity_item_id: row.try_get("identity_item_id")?,
                proposal_id: row.try_get("proposal_id")?,
                trace_id: row.try_get("trace_id")?,
                message: row.try_get("message")?,
                evidence_refs: row.try_get("evidence_refs_json")?,
                created_at: row.try_get("created_at")?,
                payload: row.try_get("payload_json")?,
            })
        })
        .collect()
}

pub async fn reconstruct_compact_identity_snapshot(
    pool: &PgPool,
    limit: i64,
) -> Result<CompactIdentitySnapshot> {
    let items = list_active_identity_items(pool, limit).await?;
    let mut snapshot = CompactIdentitySnapshot::default();
    for item in items {
        let category = identity_category_from_str(&item.category)?;
        let compact = CompactIdentityItem {
            category,
            value: item.value_text,
            confidence_pct: pct_from_f64(item.confidence),
            weight_pct: item.weight.map(pct_from_f64),
        };
        match item.stability_class.as_str() {
            "stable" => snapshot.stable_items.push(compact),
            _ => snapshot.evolving_items.push(compact),
        }
    }
    snapshot.identity_summary = snapshot
        .stable_items
        .iter()
        .find(|item| item.category == IdentityItemCategory::Name)
        .map(|item| item.value.clone())
        .unwrap_or_else(|| "Identity snapshot".to_string());
    snapshot.values = snapshot
        .stable_items
        .iter()
        .filter(|item| item.category == IdentityItemCategory::FoundationalValue)
        .map(|item| item.value.clone())
        .collect();
    snapshot.boundaries = snapshot
        .stable_items
        .iter()
        .filter(|item| item.category == IdentityItemCategory::EnduringBoundary)
        .map(|item| item.value.clone())
        .collect();
    snapshot.self_description = snapshot
        .evolving_items
        .iter()
        .find(|item| item.category == IdentityItemCategory::RecurringSelfDescription)
        .map(|item| item.value.clone());
    Ok(snapshot)
}

fn identity_lifecycle_from_row(row: sqlx::postgres::PgRow) -> Result<IdentityLifecycleRecord> {
    Ok(IdentityLifecycleRecord {
        identity_lifecycle_id: row.try_get("identity_lifecycle_id")?,
        status: row.try_get("status")?,
        lifecycle_state: row.try_get("lifecycle_state")?,
        active_self_model_artifact_id: row.try_get("active_self_model_artifact_id")?,
        active_interview_id: row.try_get("active_interview_id")?,
        transition_reason: row.try_get("transition_reason")?,
        transitioned_by: row.try_get("transitioned_by")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        payload: row.try_get("payload_json")?,
    })
}

fn identity_item_from_row(row: sqlx::postgres::PgRow) -> Result<IdentityItemRecord> {
    Ok(IdentityItemRecord {
        identity_item_id: row.try_get("identity_item_id")?,
        self_model_artifact_id: row.try_get("self_model_artifact_id")?,
        proposal_id: row.try_get("proposal_id")?,
        trace_id: row.try_get("trace_id")?,
        stability_class: row.try_get("stability_class")?,
        category: row.try_get("category")?,
        item_key: row.try_get("item_key")?,
        value_text: row.try_get("value_text")?,
        confidence: row.try_get("confidence")?,
        weight: row.try_get("weight")?,
        provenance_kind: row.try_get("provenance_kind")?,
        source_kind: row.try_get("source_kind")?,
        merge_policy: row.try_get("merge_policy")?,
        status: row.try_get("status")?,
        evidence_refs: row.try_get("evidence_refs_json")?,
        valid_from: row.try_get("valid_from")?,
        valid_to: row.try_get("valid_to")?,
        supersedes_item_id: row.try_get("supersedes_item_id")?,
        superseded_by_item_id: row.try_get("superseded_by_item_id")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        payload: row.try_get("payload_json")?,
    })
}

fn identity_interview_from_row(row: sqlx::postgres::PgRow) -> Result<IdentityInterviewRecord> {
    Ok(IdentityInterviewRecord {
        identity_interview_id: row.try_get("identity_interview_id")?,
        status: row.try_get("status")?,
        current_step: row.try_get("current_step")?,
        answered_fields: row.try_get("answered_fields_json")?,
        required_fields: row.try_get("required_fields_json")?,
        last_prompt_text: row.try_get("last_prompt_text")?,
        selected_template_id: row.try_get("selected_template_id")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        payload: row.try_get("payload_json")?,
    })
}

fn next_missing_step(answered_fields: &serde_json::Map<String, Value>) -> Option<&'static str> {
    CUSTOM_IDENTITY_STEPS
        .iter()
        .copied()
        .find(|step| !answered_fields.contains_key(*step))
}

pub fn custom_identity_step_user_prompt(step: &str) -> &'static str {
    match step {
        "name" => "What name should this assistant identity use?",
        "identity_form" => "What kind of identity form should this assistant have?",
        "archetype_role" => "What archetype or role should define this identity?",
        "temperament" => "What core temperament traits should this identity have?",
        "communication_style" => "How should this identity usually communicate?",
        "backstory" => "What short origin or backstory should this identity have?",
        "age_framing" => "How should this identity frame its age or maturity?",
        "likes" => "What should this identity tend to like?",
        "dislikes" => "What should this identity tend to dislike?",
        "values" => "What foundational values should this identity hold?",
        "boundaries" => "What enduring boundaries should this identity keep?",
        "tendencies" => "What learned tendencies or habits should this identity have?",
        "goals" => "What goals or subgoals should this identity carry?",
        "relationship_to_user" => "How should this identity relate to the user?",
        _ => "What should the next identity interview answer be?",
    }
}

fn custom_identity_step_prompt(step: &str) -> String {
    custom_identity_step_user_prompt(step).to_string()
}

fn custom_identity_deltas_from_answers(
    answered_fields: &serde_json::Map<String, Value>,
    proposal: &CanonicalProposal,
) -> Vec<IdentityItemDelta> {
    let evidence = vec![contracts::IdentityEvidenceRef {
        source_kind: "custom_identity_interview".to_string(),
        source_id: None,
        summary: "User answered the custom identity interview.".to_string(),
    }];
    let valid_from = proposal.valid_from;
    let mut deltas = Vec::new();
    let mut push =
        |stability_class, category, item_key: &str, field: &str, merge_policy, weight| {
            let value = answer_value(answered_fields, field);
            deltas.push(IdentityItemDelta {
                operation: IdentityDeltaOperation::Add,
                stability_class,
                category,
                item_key: item_key.to_string(),
                value,
                confidence_pct: 100,
                weight_pct: weight,
                source: IdentityItemSource::CustomInterview,
                merge_policy,
                evidence_refs: evidence.clone(),
                valid_from,
                valid_to: None,
                target_identity_item_id: None,
            });
        };

    push(
        IdentityStabilityClass::Stable,
        IdentityItemCategory::Name,
        "name",
        "name",
        IdentityMergePolicy::ProtectedCore,
        Some(100),
    );
    push(
        IdentityStabilityClass::Stable,
        IdentityItemCategory::IdentityForm,
        "identity_form",
        "identity_form",
        IdentityMergePolicy::ProtectedCore,
        Some(100),
    );
    push(
        IdentityStabilityClass::Stable,
        IdentityItemCategory::Role,
        "role",
        "archetype_role",
        IdentityMergePolicy::ProtectedCore,
        Some(100),
    );
    push(
        IdentityStabilityClass::Stable,
        IdentityItemCategory::Archetype,
        "archetype",
        "archetype_role",
        IdentityMergePolicy::ProtectedCore,
        Some(100),
    );
    push(
        IdentityStabilityClass::Stable,
        IdentityItemCategory::OriginBackstory,
        "origin_backstory",
        "backstory",
        IdentityMergePolicy::ProtectedCore,
        Some(100),
    );
    push(
        IdentityStabilityClass::Stable,
        IdentityItemCategory::AgeFraming,
        "age_framing",
        "age_framing",
        IdentityMergePolicy::ProtectedCore,
        Some(100),
    );
    push(
        IdentityStabilityClass::Stable,
        IdentityItemCategory::FoundationalTrait,
        "foundational_trait",
        "temperament",
        IdentityMergePolicy::ProtectedCore,
        Some(100),
    );
    push(
        IdentityStabilityClass::Stable,
        IdentityItemCategory::FoundationalValue,
        "foundational_value",
        "values",
        IdentityMergePolicy::ProtectedCore,
        Some(100),
    );
    push(
        IdentityStabilityClass::Stable,
        IdentityItemCategory::EnduringBoundary,
        "enduring_boundary",
        "boundaries",
        IdentityMergePolicy::ProtectedCore,
        Some(100),
    );
    push(
        IdentityStabilityClass::Stable,
        IdentityItemCategory::DefaultCommunicationStyle,
        "default_communication_style",
        "communication_style",
        IdentityMergePolicy::ProtectedCore,
        Some(100),
    );
    push(
        IdentityStabilityClass::Evolving,
        IdentityItemCategory::Like,
        "like",
        "likes",
        IdentityMergePolicy::Revisable,
        Some(80),
    );
    push(
        IdentityStabilityClass::Evolving,
        IdentityItemCategory::Dislike,
        "dislike",
        "dislikes",
        IdentityMergePolicy::Revisable,
        Some(80),
    );
    push(
        IdentityStabilityClass::Evolving,
        IdentityItemCategory::LearnedTendency,
        "learned_tendency",
        "tendencies",
        IdentityMergePolicy::Revisable,
        Some(80),
    );
    push(
        IdentityStabilityClass::Evolving,
        IdentityItemCategory::Goal,
        "goal",
        "goals",
        IdentityMergePolicy::Revisable,
        Some(80),
    );
    push(
        IdentityStabilityClass::Evolving,
        IdentityItemCategory::InteractionStyleAdaptation,
        "interaction_style_adaptation",
        "relationship_to_user",
        IdentityMergePolicy::Revisable,
        Some(80),
    );
    deltas
}

fn answer_value(answered_fields: &serde_json::Map<String, Value>, field: &str) -> String {
    answered_fields
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

fn custom_self_description(answered_fields: &serde_json::Map<String, Value>) -> String {
    format!(
        "{} is a {} with a {} role. It communicates in a {} way, values {}, and keeps boundaries around {}.",
        answer_value(answered_fields, "name"),
        answer_value(answered_fields, "identity_form"),
        answer_value(answered_fields, "archetype_role"),
        answer_value(answered_fields, "communication_style"),
        answer_value(answered_fields, "values"),
        answer_value(answered_fields, "boundaries"),
    )
}

fn identity_item_from_delta(
    context: &ProposalProcessingContext,
    proposal: &CanonicalProposal,
    delta: &IdentityItemDelta,
    identity_item_id: Uuid,
) -> Result<NewIdentityItem> {
    Ok(NewIdentityItem {
        identity_item_id,
        self_model_artifact_id: None,
        proposal_id: Some(proposal.proposal_id),
        trace_id: Some(context.trace_id),
        stability_class: stability_class_as_str(delta.stability_class).to_string(),
        category: identity_category_as_str(delta.category).to_string(),
        item_key: delta.item_key.clone(),
        value_text: delta.value.clone(),
        confidence: f64::from(delta.confidence_pct) / 100.0,
        weight: delta.weight_pct.map(|weight| f64::from(weight) / 100.0),
        provenance_kind: provenance_kind_as_str(proposal.provenance.provenance_kind).to_string(),
        source_kind: item_source_as_str(delta.source).to_string(),
        merge_policy: merge_policy_as_str(delta.merge_policy).to_string(),
        status: "active".to_string(),
        evidence_refs: serde_json::to_value(&delta.evidence_refs)?,
        valid_from: delta.valid_from.or(proposal.valid_from),
        valid_to: delta.valid_to.or(proposal.valid_to),
        supersedes_item_id: delta.target_identity_item_id,
        payload: json!({
            "proposal_rationale": proposal.rationale,
            "delta_operation": identity_delta_operation_as_str(delta.operation),
        }),
    })
}

fn self_description_item_from_delta(
    context: &ProposalProcessingContext,
    proposal: &CanonicalProposal,
    delta: &contracts::SelfDescriptionDelta,
    identity_item_id: Uuid,
    supersedes_item_id: Option<Uuid>,
) -> Result<NewIdentityItem> {
    Ok(NewIdentityItem {
        identity_item_id,
        self_model_artifact_id: None,
        proposal_id: Some(proposal.proposal_id),
        trace_id: Some(context.trace_id),
        stability_class: "evolving".to_string(),
        category: "recurring_self_description".to_string(),
        item_key: "self_description".to_string(),
        value_text: delta.description.clone(),
        confidence: f64::from(proposal.confidence_pct) / 100.0,
        weight: Some(0.8),
        provenance_kind: provenance_kind_as_str(proposal.provenance.provenance_kind).to_string(),
        source_kind: "identity_delta".to_string(),
        merge_policy: "revisable".to_string(),
        status: "active".to_string(),
        evidence_refs: serde_json::to_value(&delta.evidence_refs)?,
        valid_from: proposal.valid_from,
        valid_to: proposal.valid_to,
        supersedes_item_id,
        payload: json!({
            "proposal_rationale": proposal.rationale,
            "delta_operation": identity_delta_operation_as_str(delta.operation),
        }),
    })
}

fn pct_from_f64(value: f64) -> u8 {
    (value.clamp(0.0, 1.0) * 100.0).round() as u8
}

fn normalize_identity_text(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn identity_texts_are_near_duplicates(left: &str, right: &str) -> bool {
    if left.is_empty() || right.is_empty() {
        return false;
    }
    let left_tokens = left.split_whitespace().collect::<Vec<_>>();
    let right_tokens = right.split_whitespace().collect::<Vec<_>>();
    if left_tokens.len() < 4 || right_tokens.len() < 4 {
        return false;
    }
    let shared = left_tokens
        .iter()
        .filter(|token| right_tokens.contains(token))
        .count();
    let smaller = left_tokens.len().min(right_tokens.len());
    shared * 5 >= smaller * 4
}

fn identity_category_from_str(value: &str) -> Result<IdentityItemCategory> {
    let category = match value {
        "name" => IdentityItemCategory::Name,
        "identity_form" => IdentityItemCategory::IdentityForm,
        "role" => IdentityItemCategory::Role,
        "archetype" => IdentityItemCategory::Archetype,
        "origin_backstory" => IdentityItemCategory::OriginBackstory,
        "age_framing" => IdentityItemCategory::AgeFraming,
        "foundational_trait" => IdentityItemCategory::FoundationalTrait,
        "foundational_value" => IdentityItemCategory::FoundationalValue,
        "enduring_boundary" => IdentityItemCategory::EnduringBoundary,
        "default_communication_style" => IdentityItemCategory::DefaultCommunicationStyle,
        "preference" => IdentityItemCategory::Preference,
        "like" => IdentityItemCategory::Like,
        "dislike" => IdentityItemCategory::Dislike,
        "habit" => IdentityItemCategory::Habit,
        "routine" => IdentityItemCategory::Routine,
        "learned_tendency" => IdentityItemCategory::LearnedTendency,
        "autobiographical_refinement" => IdentityItemCategory::AutobiographicalRefinement,
        "recurring_self_description" => IdentityItemCategory::RecurringSelfDescription,
        "interaction_style_adaptation" => IdentityItemCategory::InteractionStyleAdaptation,
        "goal" => IdentityItemCategory::Goal,
        "subgoal" => IdentityItemCategory::Subgoal,
        _ => bail!("unsupported identity item category {value}"),
    };
    Ok(category)
}

fn identity_category_as_str(category: IdentityItemCategory) -> &'static str {
    match category {
        IdentityItemCategory::Name => "name",
        IdentityItemCategory::IdentityForm => "identity_form",
        IdentityItemCategory::Role => "role",
        IdentityItemCategory::Archetype => "archetype",
        IdentityItemCategory::OriginBackstory => "origin_backstory",
        IdentityItemCategory::AgeFraming => "age_framing",
        IdentityItemCategory::FoundationalTrait => "foundational_trait",
        IdentityItemCategory::FoundationalValue => "foundational_value",
        IdentityItemCategory::EnduringBoundary => "enduring_boundary",
        IdentityItemCategory::DefaultCommunicationStyle => "default_communication_style",
        IdentityItemCategory::Preference => "preference",
        IdentityItemCategory::Like => "like",
        IdentityItemCategory::Dislike => "dislike",
        IdentityItemCategory::Habit => "habit",
        IdentityItemCategory::Routine => "routine",
        IdentityItemCategory::LearnedTendency => "learned_tendency",
        IdentityItemCategory::AutobiographicalRefinement => "autobiographical_refinement",
        IdentityItemCategory::RecurringSelfDescription => "recurring_self_description",
        IdentityItemCategory::InteractionStyleAdaptation => "interaction_style_adaptation",
        IdentityItemCategory::Goal => "goal",
        IdentityItemCategory::Subgoal => "subgoal",
    }
}

fn stability_class_as_str(class: IdentityStabilityClass) -> &'static str {
    match class {
        IdentityStabilityClass::Stable => "stable",
        IdentityStabilityClass::Evolving => "evolving",
        IdentityStabilityClass::TransientProjection => "transient_projection",
    }
}

fn item_source_as_str(source: IdentityItemSource) -> &'static str {
    match source {
        IdentityItemSource::Seed => "seed",
        IdentityItemSource::PredefinedTemplate => "predefined_template",
        IdentityItemSource::CustomInterview => "custom_interview",
        IdentityItemSource::UserAuthored => "user_authored",
        IdentityItemSource::OperatorAuthored => "operator_authored",
        IdentityItemSource::ModelInferred => "model_inferred",
    }
}

fn merge_policy_as_str(policy: IdentityMergePolicy) -> &'static str {
    match policy {
        IdentityMergePolicy::ProtectedCore => "protected_core",
        IdentityMergePolicy::ApprovalRequired => "approval_required",
        IdentityMergePolicy::Reinforceable => "reinforceable",
        IdentityMergePolicy::Revisable => "revisable",
        IdentityMergePolicy::Expirable => "expirable",
    }
}

fn identity_delta_operation_as_str(operation: IdentityDeltaOperation) -> &'static str {
    match operation {
        IdentityDeltaOperation::Add => "add",
        IdentityDeltaOperation::Reinforce => "reinforce",
        IdentityDeltaOperation::Weaken => "weaken",
        IdentityDeltaOperation::Revise => "revise",
        IdentityDeltaOperation::Supersede => "supersede",
        IdentityDeltaOperation::Expire => "expire",
    }
}

fn lifecycle_state_as_str(state: contracts::IdentityLifecycleState) -> &'static str {
    match state {
        contracts::IdentityLifecycleState::BootstrapSeedOnly => "bootstrap_seed_only",
        contracts::IdentityLifecycleState::IdentityKickstartInProgress => {
            "identity_kickstart_in_progress"
        }
        contracts::IdentityLifecycleState::CompleteIdentityActive => "complete_identity_active",
        contracts::IdentityLifecycleState::IdentityResetPending => "identity_reset_pending",
    }
}

fn provenance_kind_as_str(kind: ProposalProvenanceKind) -> &'static str {
    match kind {
        ProposalProvenanceKind::EpisodeObservation => "episode_observation",
        ProposalProvenanceKind::BacklogRecovery => "backlog_recovery",
        ProposalProvenanceKind::SelfModelReflection => "self_model_reflection",
    }
}

fn reject(proposal_id: Uuid, reason: impl Into<String>) -> ProposalEvaluation {
    ProposalEvaluation {
        proposal_id,
        outcome: ProposalEvaluationOutcome::Rejected,
        reason: reason.into(),
        target: None,
    }
}
