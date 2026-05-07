use anyhow::Result;
use contracts::{
    CanonicalProposal, CanonicalProposalKind, CanonicalProposalPayload, CanonicalTargetKind,
    IdentityDeltaProposal, IdentityItemSource, IdentityStabilityClass, MemoryArtifactProposal,
    MergeDecisionTarget, ProposalConflictPosture, ProposalEvaluation, ProposalEvaluationOutcome,
    ProposalProvenanceKind, SelfModelObservationProposal,
};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    audit::{self, NewAuditEvent},
    config::RuntimeConfig,
    continuity::{self, NewMergeDecision, NewProposalRecord},
    identity, memory, self_model,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalProcessingContext {
    pub trace_id: Uuid,
    pub execution_id: Uuid,
    pub episode_id: Option<Uuid>,
    pub source_ingress_id: Option<Uuid>,
    pub source_loop_kind: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProposalApplicationSummary {
    pub evaluated_count: usize,
    pub accepted_count: usize,
    pub rejected_count: usize,
    pub canonical_write_count: usize,
}

pub async fn apply_candidate_proposals(
    pool: &PgPool,
    config: &RuntimeConfig,
    context: &ProposalProcessingContext,
    subsystem: &str,
    worker_pid: Option<i32>,
    proposals: &[CanonicalProposal],
) -> Result<ProposalApplicationSummary> {
    let mut summary = ProposalApplicationSummary::default();

    for candidate in proposals {
        summary.evaluated_count += 1;
        let validation = validate_and_record_proposal(pool, context, candidate).await?;
        match validation.outcome {
            ProposalEvaluationOutcome::Rejected => {
                summary.rejected_count += 1;
            }
            ProposalEvaluationOutcome::Accepted => {
                let merge_outcome = match candidate.proposal_kind {
                    CanonicalProposalKind::MemoryArtifact => {
                        memory::apply_memory_proposal_merge(pool, context, candidate).await?
                    }
                    CanonicalProposalKind::SelfModelObservation => {
                        self_model::apply_self_model_proposal_merge(
                            pool, config, context, candidate,
                        )
                        .await?
                    }
                    CanonicalProposalKind::IdentityDelta => {
                        identity::apply_identity_delta_proposal_merge(pool, context, candidate)
                            .await?
                    }
                };

                match merge_outcome.outcome {
                    ProposalEvaluationOutcome::Accepted => {
                        summary.accepted_count += 1;
                        if merge_outcome.target.is_some() {
                            summary.canonical_write_count += 1;
                            audit::insert(
                                pool,
                                &NewAuditEvent {
                                    loop_kind: context.source_loop_kind.clone(),
                                    subsystem: subsystem.to_string(),
                                    event_kind: "canonical_write_applied".to_string(),
                                    severity: "info".to_string(),
                                    trace_id: context.trace_id,
                                    execution_id: Some(context.execution_id),
                                    worker_pid,
                                    payload: json!({
                                        "proposal_id": candidate.proposal_id,
                                        "outcome": "accepted",
                                        "target": merge_outcome.target,
                                    }),
                                },
                            )
                            .await?;
                        }
                    }
                    ProposalEvaluationOutcome::Rejected => {
                        summary.rejected_count += 1;
                    }
                }
            }
        }
    }

    Ok(summary)
}

pub async fn validate_and_record_proposal(
    pool: &PgPool,
    context: &ProposalProcessingContext,
    proposal: &CanonicalProposal,
) -> Result<ProposalEvaluation> {
    let evaluation = validate_proposal(proposal);
    let decision_kind = match evaluation.outcome {
        ProposalEvaluationOutcome::Accepted => "accepted",
        ProposalEvaluationOutcome::Rejected => "rejected",
    };
    let proposal_status = decision_kind.to_string();

    let mut transaction = pool.begin().await?;
    continuity::insert_proposal(
        &mut *transaction,
        &NewProposalRecord {
            proposal_id: proposal.proposal_id,
            trace_id: context.trace_id,
            execution_id: context.execution_id,
            episode_id: context.episode_id,
            source_ingress_id: context.source_ingress_id,
            source_loop_kind: context.source_loop_kind.clone(),
            proposal_kind: proposal_kind_as_str(proposal.proposal_kind).to_string(),
            canonical_target: canonical_target_as_str(proposal.canonical_target).to_string(),
            status: proposal_status,
            confidence: f64::from(proposal.confidence_pct) / 100.0,
            conflict_posture: conflict_posture_as_str(proposal.conflict_posture).to_string(),
            subject_ref: proposal.subject_ref.clone(),
            content_text: proposal_content_text(proposal).to_string(),
            rationale: proposal.rationale.clone(),
            valid_from: proposal.valid_from,
            valid_to: proposal.valid_to,
            supersedes_artifact_id: proposal.supersedes_artifact_id,
            supersedes_artifact_kind: proposal
                .supersedes_artifact_id
                .map(|_| canonical_target_as_str(proposal.canonical_target).to_string()),
            payload: proposal_payload_json(proposal),
        },
    )
    .await?;

    continuity::insert_merge_decision(
        &mut *transaction,
        &NewMergeDecision {
            merge_decision_id: Uuid::now_v7(),
            proposal_id: proposal.proposal_id,
            trace_id: context.trace_id,
            execution_id: context.execution_id,
            episode_id: context.episode_id,
            decision_kind: decision_kind.to_string(),
            decision_reason: evaluation.reason.clone(),
            accepted_memory_artifact_id: accepted_memory_target(&evaluation),
            accepted_self_model_artifact_id: accepted_self_model_target(&evaluation),
            payload: json!({
                "proposal_kind": proposal_kind_as_str(proposal.proposal_kind),
                "canonical_target": canonical_target_as_str(proposal.canonical_target),
                "outcome": decision_kind,
                "reason": evaluation.reason,
            }),
        },
    )
    .await?;

    audit::insert(
        &mut *transaction,
        &NewAuditEvent {
            loop_kind: context.source_loop_kind.clone(),
            subsystem: "proposal_validation".to_string(),
            event_kind: "proposal_evaluated".to_string(),
            severity: severity_for_outcome(evaluation.outcome).to_string(),
            trace_id: context.trace_id,
            execution_id: Some(context.execution_id),
            worker_pid: None,
            payload: json!({
                "proposal_id": proposal.proposal_id,
                "proposal_kind": proposal_kind_as_str(proposal.proposal_kind),
                "canonical_target": canonical_target_as_str(proposal.canonical_target),
                "outcome": decision_kind,
                "reason": evaluation.reason,
            }),
        },
    )
    .await?;

    audit::insert(
        &mut *transaction,
        &NewAuditEvent {
            loop_kind: context.source_loop_kind.clone(),
            subsystem: "proposal_validation".to_string(),
            event_kind: "merge_decision_recorded".to_string(),
            severity: severity_for_outcome(evaluation.outcome).to_string(),
            trace_id: context.trace_id,
            execution_id: Some(context.execution_id),
            worker_pid: None,
            payload: json!({
                "proposal_id": proposal.proposal_id,
                "decision_kind": decision_kind,
            }),
        },
    )
    .await?;

    transaction.commit().await?;
    Ok(evaluation)
}

pub fn validate_proposal(proposal: &CanonicalProposal) -> ProposalEvaluation {
    if proposal.subject_ref.trim().is_empty() {
        return reject(
            proposal.proposal_id,
            "proposal subject_ref must not be empty",
        );
    }
    if proposal.confidence_pct == 0 {
        return reject(
            proposal.proposal_id,
            "proposal confidence_pct must be greater than zero",
        );
    }
    if proposal.provenance.source_ingress_ids.is_empty()
        && proposal.provenance.source_episode_id.is_none()
    {
        return reject(
            proposal.proposal_id,
            "proposal provenance must reference at least one ingress or source episode",
        );
    }
    if proposal.provenance.provenance_kind == ProposalProvenanceKind::SelfModelReflection
        && !matches!(
            proposal.canonical_target,
            CanonicalTargetKind::SelfModelArtifacts | CanonicalTargetKind::IdentityItems
        )
    {
        return reject(
            proposal.proposal_id,
            "self_model_reflection provenance may target only self_model_artifacts or identity_items",
        );
    }
    if let (Some(valid_from), Some(valid_to)) = (proposal.valid_from, proposal.valid_to)
        && valid_to < valid_from
    {
        return reject(
            proposal.proposal_id,
            "proposal valid_to must not be earlier than valid_from",
        );
    }
    if let Some(message) = validate_conflict_posture(proposal) {
        return reject(proposal.proposal_id, message);
    }

    match (
        &proposal.proposal_kind,
        &proposal.canonical_target,
        &proposal.payload,
    ) {
        (
            CanonicalProposalKind::MemoryArtifact,
            CanonicalTargetKind::MemoryArtifacts,
            CanonicalProposalPayload::MemoryArtifact(payload),
        ) => validate_memory_payload(proposal.proposal_id, payload),
        (
            CanonicalProposalKind::SelfModelObservation,
            CanonicalTargetKind::SelfModelArtifacts,
            CanonicalProposalPayload::SelfModelObservation(payload),
        ) => validate_self_model_payload(proposal.proposal_id, payload),
        (
            CanonicalProposalKind::IdentityDelta,
            CanonicalTargetKind::IdentityItems,
            CanonicalProposalPayload::IdentityDelta(payload),
        ) => validate_identity_delta_payload(proposal.proposal_id, payload),
        (
            CanonicalProposalKind::MemoryArtifact,
            CanonicalTargetKind::SelfModelArtifacts | CanonicalTargetKind::IdentityItems,
            _,
        ) => reject(
            proposal.proposal_id,
            "memory_artifact proposals may not target self_model_artifacts or identity_items",
        ),
        (
            CanonicalProposalKind::SelfModelObservation,
            CanonicalTargetKind::MemoryArtifacts | CanonicalTargetKind::IdentityItems,
            _,
        ) => reject(
            proposal.proposal_id,
            "self_model_observation proposals may not target memory_artifacts or identity_items",
        ),
        (
            CanonicalProposalKind::IdentityDelta,
            CanonicalTargetKind::MemoryArtifacts | CanonicalTargetKind::SelfModelArtifacts,
            _,
        ) => reject(
            proposal.proposal_id,
            "identity_delta proposals may target only identity_items",
        ),
        _ => reject(
            proposal.proposal_id,
            "proposal payload kind does not match proposal_kind and canonical_target",
        ),
    }
}

fn validate_memory_payload(
    proposal_id: Uuid,
    payload: &MemoryArtifactProposal,
) -> ProposalEvaluation {
    if payload.artifact_kind.trim().is_empty() {
        return reject(
            proposal_id,
            "memory_artifact proposal artifact_kind must not be empty",
        );
    }
    if payload.content_text.trim().is_empty() {
        return reject(
            proposal_id,
            "memory_artifact proposal content_text must not be empty",
        );
    }
    accept(proposal_id)
}

fn validate_self_model_payload(
    proposal_id: Uuid,
    payload: &SelfModelObservationProposal,
) -> ProposalEvaluation {
    if payload.observation_kind.trim().is_empty() {
        return reject(
            proposal_id,
            "self_model_observation proposal observation_kind must not be empty",
        );
    }
    if payload.content_text.trim().is_empty() {
        return reject(
            proposal_id,
            "self_model_observation proposal content_text must not be empty",
        );
    }
    accept(proposal_id)
}

fn validate_identity_delta_payload(
    proposal_id: Uuid,
    payload: &IdentityDeltaProposal,
) -> ProposalEvaluation {
    if payload.rationale.trim().is_empty() {
        return reject(
            proposal_id,
            "identity_delta proposal rationale must not be empty",
        );
    }
    if payload.item_deltas.is_empty()
        && payload.self_description_delta.is_none()
        && payload.interview_action.is_none()
    {
        return reject(
            proposal_id,
            "identity_delta proposal must include item_deltas, self_description_delta, or interview_action",
        );
    }
    for delta in &payload.item_deltas {
        if delta.item_key.trim().is_empty() {
            return reject(proposal_id, "identity_delta item_key must not be empty");
        }
        if delta.value.trim().is_empty() {
            return reject(proposal_id, "identity_delta value must not be empty");
        }
        if delta.confidence_pct == 0 {
            return reject(
                proposal_id,
                "identity_delta confidence_pct must be greater than zero",
            );
        }
        if matches!(delta.weight_pct, Some(weight) if weight > 100) {
            return reject(proposal_id, "identity_delta weight_pct must be 0..=100");
        }
        if matches!(delta.stability_class, IdentityStabilityClass::Stable)
            && matches!(delta.source, IdentityItemSource::ModelInferred)
        {
            return reject(
                proposal_id,
                "model-inferred stable identity deltas require explicit approval policy",
            );
        }
        if matches!(delta.source, IdentityItemSource::ModelInferred)
            && delta.evidence_refs.is_empty()
        {
            return reject(
                proposal_id,
                "model-inferred identity deltas must include evidence_refs",
            );
        }
        if let (Some(valid_from), Some(valid_to)) = (delta.valid_from, delta.valid_to)
            && valid_to < valid_from
        {
            return reject(
                proposal_id,
                "identity_delta valid_to must not be earlier than valid_from",
            );
        }
    }
    if let Some(delta) = &payload.self_description_delta
        && delta.description.trim().is_empty()
    {
        return reject(
            proposal_id,
            "self_description_delta description must not be empty",
        );
    }
    accept(proposal_id)
}

fn validate_conflict_posture(proposal: &CanonicalProposal) -> Option<&'static str> {
    match proposal.conflict_posture {
        ProposalConflictPosture::Independent | ProposalConflictPosture::Conflicts
            if proposal.supersedes_artifact_id.is_some() =>
        {
            Some("proposal conflict posture allows no supersedes_artifact_id")
        }
        ProposalConflictPosture::Revises | ProposalConflictPosture::Supersedes
            if proposal.supersedes_artifact_id.is_none() =>
        {
            Some("proposal conflict posture requires supersedes_artifact_id")
        }
        _ => None,
    }
}

fn proposal_payload_json(proposal: &CanonicalProposal) -> serde_json::Value {
    match &proposal.payload {
        CanonicalProposalPayload::MemoryArtifact(payload) => json!({
            "artifact_kind": payload.artifact_kind,
            "content_text": payload.content_text,
            "provenance_kind": proposal_provenance_kind_as_str(proposal.provenance.provenance_kind),
        }),
        CanonicalProposalPayload::SelfModelObservation(payload) => json!({
            "observation_kind": payload.observation_kind,
            "content_text": payload.content_text,
            "provenance_kind": proposal_provenance_kind_as_str(proposal.provenance.provenance_kind),
        }),
        CanonicalProposalPayload::IdentityDelta(payload) => json!({
            "item_delta_count": payload.item_deltas.len(),
            "has_self_description_delta": payload.self_description_delta.is_some(),
            "rationale": payload.rationale,
            "provenance_kind": proposal_provenance_kind_as_str(proposal.provenance.provenance_kind),
            "payload": payload,
        }),
    }
}

fn proposal_content_text(proposal: &CanonicalProposal) -> &str {
    match &proposal.payload {
        CanonicalProposalPayload::MemoryArtifact(payload) => &payload.content_text,
        CanonicalProposalPayload::SelfModelObservation(payload) => &payload.content_text,
        CanonicalProposalPayload::IdentityDelta(payload) => &payload.rationale,
    }
}

fn accepted_memory_target(evaluation: &ProposalEvaluation) -> Option<Uuid> {
    match evaluation.target {
        Some(MergeDecisionTarget::MemoryArtifact(artifact_id)) => Some(artifact_id),
        _ => None,
    }
}

fn accepted_self_model_target(evaluation: &ProposalEvaluation) -> Option<Uuid> {
    match evaluation.target {
        Some(MergeDecisionTarget::SelfModelArtifact(artifact_id)) => Some(artifact_id),
        _ => None,
    }
}

fn accept(proposal_id: Uuid) -> ProposalEvaluation {
    ProposalEvaluation {
        proposal_id,
        outcome: ProposalEvaluationOutcome::Accepted,
        reason: "proposal accepted for canonical merge".to_string(),
        target: None,
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

fn proposal_kind_as_str(kind: CanonicalProposalKind) -> &'static str {
    match kind {
        CanonicalProposalKind::MemoryArtifact => "memory_artifact",
        CanonicalProposalKind::SelfModelObservation => "self_model_observation",
        CanonicalProposalKind::IdentityDelta => "identity_delta",
    }
}

fn canonical_target_as_str(target: CanonicalTargetKind) -> &'static str {
    match target {
        CanonicalTargetKind::MemoryArtifacts => "memory_artifacts",
        CanonicalTargetKind::SelfModelArtifacts => "self_model_artifacts",
        CanonicalTargetKind::IdentityItems => "identity_items",
    }
}

fn conflict_posture_as_str(posture: ProposalConflictPosture) -> &'static str {
    match posture {
        ProposalConflictPosture::Independent => "independent",
        ProposalConflictPosture::Revises => "revises",
        ProposalConflictPosture::Supersedes => "supersedes",
        ProposalConflictPosture::Conflicts => "conflicts",
    }
}

fn proposal_provenance_kind_as_str(kind: ProposalProvenanceKind) -> &'static str {
    match kind {
        ProposalProvenanceKind::EpisodeObservation => "episode_observation",
        ProposalProvenanceKind::BacklogRecovery => "backlog_recovery",
        ProposalProvenanceKind::SelfModelReflection => "self_model_reflection",
    }
}

fn severity_for_outcome(outcome: ProposalEvaluationOutcome) -> &'static str {
    match outcome {
        ProposalEvaluationOutcome::Accepted => "info",
        ProposalEvaluationOutcome::Rejected => "warn",
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use contracts::{
        CanonicalProposalPayload, IdentityDeltaOperation, IdentityDeltaProposal,
        IdentityEvidenceRef, IdentityItemCategory, IdentityItemDelta, IdentityItemSource,
        IdentityLifecycleState, IdentityMergePolicy, IdentityStabilityClass,
        ProposalConflictPosture, ProposalProvenance, ProposalProvenanceKind,
    };

    use super::*;

    #[test]
    fn validate_proposal_accepts_well_formed_memory_artifact() {
        let evaluation = validate_proposal(&sample_memory_proposal());
        assert_eq!(evaluation.outcome, ProposalEvaluationOutcome::Accepted);
    }

    #[test]
    fn validate_proposal_rejects_missing_provenance() {
        let mut proposal = sample_memory_proposal();
        proposal.provenance.source_ingress_ids.clear();
        proposal.provenance.source_episode_id = None;

        let evaluation = validate_proposal(&proposal);
        assert_eq!(evaluation.outcome, ProposalEvaluationOutcome::Rejected);
        assert!(evaluation.reason.contains("provenance"));
    }

    #[test]
    fn validate_proposal_rejects_supersession_without_target() {
        let mut proposal = sample_memory_proposal();
        proposal.conflict_posture = ProposalConflictPosture::Supersedes;
        proposal.supersedes_artifact_id = None;

        let evaluation = validate_proposal(&proposal);
        assert_eq!(evaluation.outcome, ProposalEvaluationOutcome::Rejected);
        assert!(evaluation.reason.contains("supersedes_artifact_id"));
    }

    #[test]
    fn validate_proposal_rejects_mismatched_target() {
        let mut proposal = sample_memory_proposal();
        proposal.canonical_target = CanonicalTargetKind::SelfModelArtifacts;

        let evaluation = validate_proposal(&proposal);
        assert_eq!(evaluation.outcome, ProposalEvaluationOutcome::Rejected);
        assert!(evaluation.reason.contains("may not target"));
    }

    #[test]
    fn validate_proposal_accepts_self_model_observation() {
        let proposal = CanonicalProposal {
            proposal_id: Uuid::now_v7(),
            proposal_kind: CanonicalProposalKind::SelfModelObservation,
            canonical_target: CanonicalTargetKind::SelfModelArtifacts,
            confidence_pct: 90,
            conflict_posture: ProposalConflictPosture::Independent,
            subject_ref: "self".to_string(),
            rationale: Some("Observed a stable execution preference.".to_string()),
            valid_from: Some(Utc::now()),
            valid_to: None,
            supersedes_artifact_id: None,
            provenance: ProposalProvenance {
                provenance_kind: ProposalProvenanceKind::SelfModelReflection,
                source_ingress_ids: vec![Uuid::now_v7()],
                source_episode_id: Some(Uuid::now_v7()),
            },
            payload: CanonicalProposalPayload::SelfModelObservation(SelfModelObservationProposal {
                observation_kind: "preference".to_string(),
                content_text: "Prefers concise progress updates.".to_string(),
            }),
        };

        let evaluation = validate_proposal(&proposal);
        assert_eq!(evaluation.outcome, ProposalEvaluationOutcome::Accepted);
    }

    #[test]
    fn validate_proposal_accepts_identity_delta() {
        let evaluation = validate_proposal(&sample_identity_delta_proposal());
        assert_eq!(evaluation.outcome, ProposalEvaluationOutcome::Accepted);
    }

    #[test]
    fn validate_proposal_rejects_inferred_stable_identity_delta() {
        let mut proposal = sample_identity_delta_proposal();
        let CanonicalProposalPayload::IdentityDelta(payload) = &mut proposal.payload else {
            panic!("expected identity delta");
        };
        payload.item_deltas[0].source = IdentityItemSource::ModelInferred;

        let evaluation = validate_proposal(&proposal);
        assert_eq!(evaluation.outcome, ProposalEvaluationOutcome::Rejected);
        assert!(evaluation.reason.contains("stable identity"));
    }

    fn sample_memory_proposal() -> CanonicalProposal {
        CanonicalProposal {
            proposal_id: Uuid::now_v7(),
            proposal_kind: CanonicalProposalKind::MemoryArtifact,
            canonical_target: CanonicalTargetKind::MemoryArtifacts,
            confidence_pct: 88,
            conflict_posture: ProposalConflictPosture::Independent,
            subject_ref: "user:primary".to_string(),
            rationale: Some("Observed in foreground.".to_string()),
            valid_from: Some(Utc::now()),
            valid_to: None,
            supersedes_artifact_id: None,
            provenance: ProposalProvenance {
                provenance_kind: ProposalProvenanceKind::EpisodeObservation,
                source_ingress_ids: vec![Uuid::now_v7()],
                source_episode_id: Some(Uuid::now_v7()),
            },
            payload: CanonicalProposalPayload::MemoryArtifact(MemoryArtifactProposal {
                artifact_kind: "preference".to_string(),
                content_text: "Prefers concise replies.".to_string(),
            }),
        }
    }

    fn sample_identity_delta_proposal() -> CanonicalProposal {
        CanonicalProposal {
            proposal_id: Uuid::now_v7(),
            proposal_kind: CanonicalProposalKind::IdentityDelta,
            canonical_target: CanonicalTargetKind::IdentityItems,
            confidence_pct: 90,
            conflict_posture: ProposalConflictPosture::Independent,
            subject_ref: "self:blue-lagoon".to_string(),
            rationale: Some("User selected an initial identity template.".to_string()),
            valid_from: Some(Utc::now()),
            valid_to: None,
            supersedes_artifact_id: None,
            provenance: ProposalProvenance {
                provenance_kind: ProposalProvenanceKind::EpisodeObservation,
                source_ingress_ids: vec![Uuid::now_v7()],
                source_episode_id: Some(Uuid::now_v7()),
            },
            payload: CanonicalProposalPayload::IdentityDelta(IdentityDeltaProposal {
                lifecycle_state: IdentityLifecycleState::CompleteIdentityActive,
                item_deltas: vec![IdentityItemDelta {
                    operation: IdentityDeltaOperation::Add,
                    stability_class: IdentityStabilityClass::Stable,
                    category: IdentityItemCategory::Name,
                    item_key: "name".to_string(),
                    value: "Blue Lagoon".to_string(),
                    confidence_pct: 100,
                    weight_pct: None,
                    source: IdentityItemSource::PredefinedTemplate,
                    merge_policy: IdentityMergePolicy::ProtectedCore,
                    evidence_refs: vec![IdentityEvidenceRef {
                        source_kind: "template".to_string(),
                        source_id: None,
                        summary: "Selected predefined template.".to_string(),
                    }],
                    valid_from: Some(Utc::now()),
                    valid_to: None,
                    target_identity_item_id: None,
                }],
                self_description_delta: None,
                interview_action: None,
                rationale: "Commit first identity item.".to_string(),
            }),
        }
    }
}
