use anyhow::Result;
use chrono::Utc;
use contracts::{
    CanonicalProposal, CanonicalProposalKind, CanonicalProposalPayload, CanonicalTargetKind,
    MemoryArtifactProposal, MergeDecisionTarget, ProposalConflictPosture, ProposalEvaluation,
    ProposalEvaluationOutcome, ProposalProvenanceKind,
};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    continuity::{self, MemoryArtifactRecord, NewMemoryArtifact},
    proposal::{self, ProposalProcessingContext},
};

const ACTIVE_MEMORY_SCAN_LIMIT: i64 = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
enum MemoryMergePlan {
    InsertNew,
    Supersede { target_artifact_id: Uuid },
    Reject { reason: String },
}

pub async fn apply_memory_proposal_merge(
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

    let active_artifacts = continuity::list_active_memory_artifacts_by_subject(
        pool,
        &proposal.subject_ref,
        ACTIVE_MEMORY_SCAN_LIMIT,
    )
    .await?;
    let plan = plan_memory_merge(proposal, &active_artifacts);

    match plan {
        MemoryMergePlan::Reject { reason } => {
            continuity::update_merge_decision_outcome(
                pool,
                proposal.proposal_id,
                "rejected",
                &reason,
            )
            .await?;
            Ok(ProposalEvaluation {
                proposal_id: proposal.proposal_id,
                outcome: ProposalEvaluationOutcome::Rejected,
                reason,
                target: None,
            })
        }
        MemoryMergePlan::InsertNew | MemoryMergePlan::Supersede { .. } => {
            let memory_artifact_id = Uuid::now_v7();
            let payload = memory_payload(proposal);

            let mut transaction = pool.begin().await?;
            continuity::insert_memory_artifact(
                &mut *transaction,
                &NewMemoryArtifact {
                    memory_artifact_id,
                    proposal_id: proposal.proposal_id,
                    trace_id: context.trace_id,
                    execution_id: context.execution_id,
                    episode_id: context.episode_id,
                    source_ingress_id: context.source_ingress_id,
                    artifact_kind: payload.artifact_kind,
                    subject_ref: proposal.subject_ref.clone(),
                    content_text: payload.content_text,
                    confidence: f64::from(proposal.confidence_pct) / 100.0,
                    provenance_kind: provenance_kind_as_str(proposal.provenance.provenance_kind)
                        .to_string(),
                    status: "active".to_string(),
                    valid_from: proposal.valid_from,
                    valid_to: proposal.valid_to,
                    superseded_at: None,
                    superseded_by_artifact_id: None,
                    supersedes_artifact_id: proposal.supersedes_artifact_id,
                    payload: json!({
                        "rationale": proposal.rationale,
                        "conflict_posture": conflict_posture_as_str(proposal.conflict_posture),
                    }),
                },
            )
            .await?;
            if let MemoryMergePlan::Supersede { target_artifact_id } = plan {
                continuity::mark_memory_artifact_superseded(
                    &mut *transaction,
                    target_artifact_id,
                    memory_artifact_id,
                    Utc::now(),
                )
                .await?;
            }
            continuity::update_merge_decision_targets_in_tx(
                &mut *transaction,
                proposal.proposal_id,
                Some(memory_artifact_id),
                None,
            )
            .await?;
            transaction.commit().await?;

            Ok(ProposalEvaluation {
                proposal_id: proposal.proposal_id,
                outcome: ProposalEvaluationOutcome::Accepted,
                reason: "memory proposal merged into canonical store".to_string(),
                target: Some(MergeDecisionTarget::MemoryArtifact(memory_artifact_id)),
            })
        }
    }
}

fn plan_memory_merge(
    proposal: &CanonicalProposal,
    active_artifacts: &[MemoryArtifactRecord],
) -> MemoryMergePlan {
    if proposal.proposal_kind != CanonicalProposalKind::MemoryArtifact
        || proposal.canonical_target != CanonicalTargetKind::MemoryArtifacts
    {
        return MemoryMergePlan::Reject {
            reason: "memory merge accepts only memory_artifact proposals".to_string(),
        };
    }

    let payload = match &proposal.payload {
        CanonicalProposalPayload::MemoryArtifact(payload) => payload,
        _ => {
            return MemoryMergePlan::Reject {
                reason: "memory merge requires a memory_artifact payload".to_string(),
            };
        }
    };

    if active_artifacts.iter().any(|artifact| {
        artifact.artifact_kind == payload.artifact_kind
            && artifact.subject_ref == proposal.subject_ref
            && artifact.content_text == payload.content_text
    }) {
        return MemoryMergePlan::Reject {
            reason: "duplicate active memory artifact already exists".to_string(),
        };
    }

    match proposal.conflict_posture {
        ProposalConflictPosture::Independent => MemoryMergePlan::InsertNew,
        ProposalConflictPosture::Conflicts => MemoryMergePlan::Reject {
            reason: "conflicting memory proposals remain non-destructive in the current continuity merge baseline".to_string(),
        },
        ProposalConflictPosture::Revises | ProposalConflictPosture::Supersedes => {
            let Some(target_artifact_id) = proposal.supersedes_artifact_id else {
                return MemoryMergePlan::Reject {
                    reason: "superseding memory proposals require supersedes_artifact_id"
                        .to_string(),
                };
            };

            let Some(target_artifact) = active_artifacts
                .iter()
                .find(|artifact| artifact.memory_artifact_id == target_artifact_id)
            else {
                return MemoryMergePlan::Reject {
                    reason: "superseded memory artifact must exist in the active canonical set"
                        .to_string(),
                };
            };

            if target_artifact.subject_ref != proposal.subject_ref {
                return MemoryMergePlan::Reject {
                    reason: "superseded memory artifact must belong to the same subject_ref"
                        .to_string(),
                };
            }

            MemoryMergePlan::Supersede { target_artifact_id }
        }
    }
}

fn memory_payload(proposal: &CanonicalProposal) -> MemoryArtifactProposal {
    match &proposal.payload {
        CanonicalProposalPayload::MemoryArtifact(payload) => payload.clone(),
        _ => panic!("memory payload requested for non-memory proposal"),
    }
}

fn provenance_kind_as_str(kind: ProposalProvenanceKind) -> &'static str {
    match kind {
        ProposalProvenanceKind::EpisodeObservation => "episode_observation",
        ProposalProvenanceKind::BacklogRecovery => "backlog_recovery",
        ProposalProvenanceKind::SelfModelReflection => "self_model_reflection",
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

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use contracts::{ProposalProvenance, ProposalProvenanceKind};

    use super::*;

    #[test]
    fn plan_memory_merge_accepts_independent_new_artifact() {
        let plan = plan_memory_merge(
            &sample_memory_proposal(None, ProposalConflictPosture::Independent),
            &[],
        );
        assert_eq!(plan, MemoryMergePlan::InsertNew);
    }

    #[test]
    fn plan_memory_merge_rejects_duplicate_active_artifact() {
        let proposal = sample_memory_proposal(None, ProposalConflictPosture::Independent);
        let existing = sample_memory_artifact(
            proposal.subject_ref.clone(),
            "preference".to_string(),
            "Prefers concise replies.".to_string(),
        );
        let plan = plan_memory_merge(&proposal, &[existing]);
        assert_eq!(
            plan,
            MemoryMergePlan::Reject {
                reason: "duplicate active memory artifact already exists".to_string(),
            }
        );
    }

    #[test]
    fn plan_memory_merge_supersedes_active_target() {
        let target = sample_memory_artifact(
            "user:primary".to_string(),
            "preference".to_string(),
            "Prefers concise replies.".to_string(),
        );
        let mut proposal = sample_memory_proposal(
            Some(target.memory_artifact_id),
            ProposalConflictPosture::Supersedes,
        );
        proposal.payload = CanonicalProposalPayload::MemoryArtifact(MemoryArtifactProposal {
            artifact_kind: "preference".to_string(),
            content_text: "Now prefers direct replies.".to_string(),
        });
        let plan = plan_memory_merge(&proposal, std::slice::from_ref(&target));
        assert_eq!(
            plan,
            MemoryMergePlan::Supersede {
                target_artifact_id: target.memory_artifact_id,
            }
        );
    }

    #[test]
    fn plan_memory_merge_rejects_conflicting_proposals() {
        let plan = plan_memory_merge(
            &sample_memory_proposal(None, ProposalConflictPosture::Conflicts),
            &[],
        );
        assert_eq!(
            plan,
            MemoryMergePlan::Reject {
                reason: "conflicting memory proposals remain non-destructive in the current continuity merge baseline"
                    .to_string(),
            }
        );
    }

    fn sample_memory_proposal(
        supersedes_artifact_id: Option<Uuid>,
        conflict_posture: ProposalConflictPosture,
    ) -> CanonicalProposal {
        CanonicalProposal {
            proposal_id: Uuid::now_v7(),
            proposal_kind: CanonicalProposalKind::MemoryArtifact,
            canonical_target: CanonicalTargetKind::MemoryArtifacts,
            confidence_pct: 90,
            conflict_posture,
            subject_ref: "user:primary".to_string(),
            rationale: Some("Observed in unit test.".to_string()),
            valid_from: Some(Utc::now()),
            valid_to: None,
            supersedes_artifact_id,
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

    fn sample_memory_artifact(
        subject_ref: String,
        artifact_kind: String,
        content_text: String,
    ) -> MemoryArtifactRecord {
        MemoryArtifactRecord {
            memory_artifact_id: Uuid::now_v7(),
            proposal_id: Uuid::now_v7(),
            trace_id: Uuid::now_v7(),
            execution_id: Uuid::now_v7(),
            episode_id: Some(Uuid::now_v7()),
            source_ingress_id: Some(Uuid::now_v7()),
            artifact_kind,
            subject_ref,
            content_text,
            confidence: 0.8,
            provenance_kind: "episode_observation".to_string(),
            status: "active".to_string(),
            valid_from: Some(Utc::now()),
            valid_to: None,
            superseded_at: None,
            superseded_by_artifact_id: None,
            supersedes_artifact_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            payload: json!({}),
        }
    }
}
