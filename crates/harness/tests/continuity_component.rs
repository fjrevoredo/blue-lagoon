mod support;

use anyhow::Result;
use chrono::Utc;
use contracts::{
    CanonicalProposal, CanonicalProposalKind, CanonicalProposalPayload, CanonicalTargetKind,
    ChannelKind, IdentityDeltaOperation, IdentityDeltaProposal, IdentityEvidenceRef,
    IdentityItemCategory, IdentityItemDelta, IdentityItemSource, IdentityLifecycleState,
    IdentityMergePolicy, IdentityStabilityClass, IngressEventKind, MemoryArtifactProposal,
    MergeDecisionTarget, NormalizedIngress, ProposalConflictPosture, ProposalEvaluationOutcome,
    ProposalProvenance, ProposalProvenanceKind, SelfDescriptionDelta,
};
use harness::{
    audit,
    config::SelfModelConfig,
    continuity, execution,
    foreground::{self, NewEpisode, NewIngressEvent},
    identity, memory, proposal, self_model,
};
use serde_json::json;
use serial_test::serial;
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[tokio::test]
#[serial]
async fn proposal_memory_and_merge_history_persist() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let fixture = seed_foreground_fixture(&ctx.pool).await?;
        let proposal_id = Uuid::now_v7();
        continuity::insert_proposal(
            &ctx.pool,
            &continuity::NewProposalRecord {
                proposal_id,
                trace_id: fixture.trace_id,
                execution_id: fixture.execution_id,
                episode_id: Some(fixture.episode_id),
                source_ingress_id: Some(fixture.ingress_id),
                source_loop_kind: "conscious".to_string(),
                proposal_kind: "memory_artifact".to_string(),
                canonical_target: "memory_artifacts".to_string(),
                status: "proposed".to_string(),
                confidence: 0.91,
                conflict_posture: "independent".to_string(),
                subject_ref: "user:primary".to_string(),
                content_text: "Prefers concise answers.".to_string(),
                rationale: Some("Observed in recent foreground exchange.".to_string()),
                valid_from: Some(Utc::now()),
                valid_to: None,
                supersedes_artifact_id: None,
                supersedes_artifact_kind: None,
                payload: json!({ "artifact_kind": "preference" }),
            },
        )
        .await?;

        let proposal = continuity::get_proposal(&ctx.pool, proposal_id).await?;
        assert_eq!(proposal.subject_ref, "user:primary");
        assert_eq!(proposal.status, "proposed");

        let memory_artifact_id = Uuid::now_v7();
        continuity::insert_memory_artifact(
            &ctx.pool,
            &continuity::NewMemoryArtifact {
                memory_artifact_id,
                proposal_id,
                trace_id: fixture.trace_id,
                execution_id: fixture.execution_id,
                episode_id: Some(fixture.episode_id),
                source_ingress_id: Some(fixture.ingress_id),
                artifact_kind: "preference".to_string(),
                subject_ref: "user:primary".to_string(),
                content_text: "Prefers concise answers.".to_string(),
                confidence: 0.91,
                provenance_kind: "episode_observation".to_string(),
                status: "active".to_string(),
                valid_from: Some(Utc::now()),
                valid_to: None,
                superseded_at: None,
                superseded_by_artifact_id: None,
                supersedes_artifact_id: None,
                payload: json!({ "source": "foreground" }),
            },
        )
        .await?;

        continuity::insert_merge_decision(
            &ctx.pool,
            &continuity::NewMergeDecision {
                merge_decision_id: Uuid::now_v7(),
                proposal_id,
                trace_id: fixture.trace_id,
                execution_id: fixture.execution_id,
                episode_id: Some(fixture.episode_id),
                decision_kind: "accepted".to_string(),
                decision_reason: "proposal is well-formed and in scope".to_string(),
                accepted_memory_artifact_id: Some(memory_artifact_id),
                accepted_self_model_artifact_id: None,
                payload: json!({ "validator": "component_test" }),
            },
        )
        .await?;

        let decisions =
            continuity::list_merge_decisions_for_execution(&ctx.pool, fixture.execution_id).await?;
        assert_eq!(decisions.len(), 1);
        assert_eq!(
            decisions[0].accepted_memory_artifact_id,
            Some(memory_artifact_id)
        );

        let merge = continuity::get_merge_decision_by_proposal(&ctx.pool, proposal_id).await?;
        let merge = merge.expect("merge decision should exist");
        assert_eq!(merge.decision_kind, "accepted");

        let active = continuity::list_active_memory_artifacts(&ctx.pool, 10).await?;
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].memory_artifact_id, memory_artifact_id);
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn identity_repository_persists_lifecycle_items_templates_interviews_and_diagnostics()
-> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let fixture = seed_foreground_fixture(&ctx.pool).await?;
        let lifecycle_id = Uuid::now_v7();
        identity::record_lifecycle_transition(
            &ctx.pool,
            &identity::NewIdentityLifecycle {
                identity_lifecycle_id: lifecycle_id,
                status: "current".to_string(),
                lifecycle_state: "bootstrap_seed_only".to_string(),
                active_self_model_artifact_id: None,
                active_interview_id: None,
                transition_reason: "component test bootstrap".to_string(),
                transitioned_by: "test".to_string(),
                kickstart_started_at: None,
                kickstart_completed_at: None,
                reset_at: None,
                payload: json!({ "test": true }),
            },
        )
        .await?;
        let lifecycle = identity::get_current_lifecycle(&ctx.pool)
            .await?
            .expect("current lifecycle should exist");
        assert_eq!(lifecycle.identity_lifecycle_id, lifecycle_id);
        assert_eq!(lifecycle.lifecycle_state, "bootstrap_seed_only");

        let name_item_id = Uuid::now_v7();
        identity::insert_identity_item(
            &ctx.pool,
            &identity::NewIdentityItem {
                identity_item_id: name_item_id,
                self_model_artifact_id: None,
                proposal_id: None,
                trace_id: Some(fixture.trace_id),
                stability_class: "stable".to_string(),
                category: "name".to_string(),
                item_key: "name".to_string(),
                value_text: "Blue Lagoon".to_string(),
                confidence: 1.0,
                weight: None,
                provenance_kind: "seed".to_string(),
                source_kind: "seed".to_string(),
                merge_policy: "protected_core".to_string(),
                status: "active".to_string(),
                evidence_refs: json!([]),
                valid_from: Some(Utc::now()),
                valid_to: None,
                supersedes_item_id: None,
                payload: json!({}),
            },
        )
        .await?;
        identity::insert_identity_item(
            &ctx.pool,
            &identity::NewIdentityItem {
                identity_item_id: Uuid::now_v7(),
                self_model_artifact_id: None,
                proposal_id: None,
                trace_id: Some(fixture.trace_id),
                stability_class: "stable".to_string(),
                category: "foundational_value".to_string(),
                item_key: "continuity".to_string(),
                value_text: "continuity".to_string(),
                confidence: 0.9,
                weight: Some(0.8),
                provenance_kind: "seed".to_string(),
                source_kind: "seed".to_string(),
                merge_policy: "protected_core".to_string(),
                status: "active".to_string(),
                evidence_refs: json!([]),
                valid_from: Some(Utc::now()),
                valid_to: None,
                supersedes_item_id: None,
                payload: json!({}),
            },
        )
        .await?;
        identity::insert_identity_item(
            &ctx.pool,
            &identity::NewIdentityItem {
                identity_item_id: Uuid::now_v7(),
                self_model_artifact_id: None,
                proposal_id: None,
                trace_id: Some(fixture.trace_id),
                stability_class: "stable".to_string(),
                category: "enduring_boundary".to_string(),
                item_key: "policy".to_string(),
                value_text: "respect harness policy".to_string(),
                confidence: 0.95,
                weight: Some(0.9),
                provenance_kind: "seed".to_string(),
                source_kind: "seed".to_string(),
                merge_policy: "protected_core".to_string(),
                status: "active".to_string(),
                evidence_refs: json!([]),
                valid_from: Some(Utc::now()),
                valid_to: None,
                supersedes_item_id: None,
                payload: json!({}),
            },
        )
        .await?;
        let snapshot = identity::reconstruct_compact_identity_snapshot(&ctx.pool, 10).await?;
        assert_eq!(snapshot.identity_summary, "Blue Lagoon");
        assert!(snapshot.values.contains(&"continuity".to_string()));
        assert!(
            snapshot
                .boundaries
                .contains(&"respect harness policy".to_string())
        );

        let replacement_id = Uuid::now_v7();
        identity::insert_identity_item(
            &ctx.pool,
            &identity::NewIdentityItem {
                identity_item_id: replacement_id,
                self_model_artifact_id: None,
                proposal_id: None,
                trace_id: Some(fixture.trace_id),
                stability_class: "stable".to_string(),
                category: "name".to_string(),
                item_key: "name".to_string(),
                value_text: "Blue Lagoon Prime".to_string(),
                confidence: 1.0,
                weight: None,
                provenance_kind: "operator".to_string(),
                source_kind: "operator_authored".to_string(),
                merge_policy: "approval_required".to_string(),
                status: "active".to_string(),
                evidence_refs: json!([]),
                valid_from: Some(Utc::now()),
                valid_to: None,
                supersedes_item_id: Some(name_item_id),
                payload: json!({}),
            },
        )
        .await?;
        identity::supersede_identity_item(&ctx.pool, name_item_id, replacement_id).await?;
        let active_items = identity::list_active_identity_items(&ctx.pool, 10).await?;
        assert!(
            active_items
                .iter()
                .all(|item| item.identity_item_id != name_item_id)
        );

        let template_id = Uuid::now_v7();
        identity::insert_identity_template(
            &ctx.pool,
            &identity::NewIdentityTemplate {
                identity_template_id: template_id,
                template_key: "direct_operator".to_string(),
                display_name: "Direct Operator".to_string(),
                description: "A direct continuity-focused assistant.".to_string(),
                status: "active".to_string(),
                payload: json!({}),
            },
        )
        .await?;
        identity::insert_identity_template_item(
            &ctx.pool,
            &identity::NewIdentityTemplateItem {
                identity_template_item_id: Uuid::now_v7(),
                identity_template_id: template_id,
                stability_class: "stable".to_string(),
                category: "name".to_string(),
                item_key: "name".to_string(),
                value_text: "Blue Lagoon".to_string(),
                confidence: 1.0,
                weight: None,
                merge_policy: "protected_core".to_string(),
                payload: json!({}),
            },
        )
        .await?;
        let templates = identity::list_active_identity_templates(&ctx.pool).await?;
        assert_eq!(templates.len(), 1);

        identity::insert_identity_interview(
            &ctx.pool,
            &identity::NewIdentityInterview {
                identity_interview_id: Uuid::now_v7(),
                status: "in_progress".to_string(),
                current_step: "name".to_string(),
                answered_fields: json!({}),
                required_fields: json!(["name"]),
                last_prompt_text: Some("What should I be called?".to_string()),
                selected_template_id: Some(template_id),
                payload: json!({}),
            },
        )
        .await?;
        identity::insert_identity_diagnostic(
            &ctx.pool,
            &identity::NewIdentityDiagnostic {
                identity_diagnostic_id: Uuid::now_v7(),
                diagnostic_kind: "drift_check".to_string(),
                severity: "warning".to_string(),
                status: "open".to_string(),
                identity_item_id: Some(replacement_id),
                proposal_id: None,
                trace_id: Some(fixture.trace_id),
                message: "Potential protected-core change requires review.".to_string(),
                evidence_refs: json!([]),
                payload: json!({}),
            },
        )
        .await?;
        let diagnostics = identity::list_open_identity_diagnostics(&ctx.pool, 10).await?;
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].diagnostic_kind, "drift_check");
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn identity_delta_merge_applies_reinforce_weaken_revise_and_expire() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let fixture = seed_foreground_fixture(&ctx.pool).await?;
        let processing_context = proposal_context(&fixture);

        let add_proposal = identity_delta_proposal(
            vec![identity_item_delta(
                IdentityDeltaOperation::Add,
                IdentityItemCategory::Preference,
                "reply_cadence",
                "Prefers concise replies.",
                50,
                Some(40),
                None,
            )],
            None,
        );
        let added_item_id =
            apply_identity_delta(&ctx.pool, &processing_context, &add_proposal).await?;

        let reinforce_proposal = identity_delta_proposal(
            vec![identity_item_delta(
                IdentityDeltaOperation::Reinforce,
                IdentityItemCategory::Preference,
                "reply_cadence",
                "Prefers concise replies.",
                80,
                Some(90),
                Some(added_item_id),
            )],
            None,
        );
        let reinforced_item_id =
            apply_identity_delta(&ctx.pool, &processing_context, &reinforce_proposal).await?;
        assert_eq!(reinforced_item_id, added_item_id);

        let reinforced = identity_item_state(&ctx.pool, added_item_id).await?;
        assert_eq!(reinforced.status, "active");
        assert_eq!(reinforced.confidence_pct(), 80);
        assert_eq!(reinforced.weight_pct(), Some(90));

        let weaken_proposal = identity_delta_proposal(
            vec![identity_item_delta(
                IdentityDeltaOperation::Weaken,
                IdentityItemCategory::Preference,
                "reply_cadence",
                "Prefers concise replies.",
                20,
                Some(30),
                Some(added_item_id),
            )],
            None,
        );
        let weakened_item_id =
            apply_identity_delta(&ctx.pool, &processing_context, &weaken_proposal).await?;
        assert_eq!(weakened_item_id, added_item_id);

        let weakened = identity_item_state(&ctx.pool, added_item_id).await?;
        assert_eq!(weakened.status, "active");
        assert_eq!(weakened.confidence_pct(), 60);
        assert_eq!(weakened.weight_pct(), Some(60));

        let revise_proposal = identity_delta_proposal(
            vec![identity_item_delta(
                IdentityDeltaOperation::Revise,
                IdentityItemCategory::Preference,
                "reply_cadence",
                "Prefers concise replies with decision context.",
                85,
                Some(75),
                Some(added_item_id),
            )],
            None,
        );
        let revised_item_id =
            apply_identity_delta(&ctx.pool, &processing_context, &revise_proposal).await?;
        assert_ne!(revised_item_id, added_item_id);

        let superseded = identity_item_state(&ctx.pool, added_item_id).await?;
        assert_eq!(superseded.status, "superseded");
        assert_eq!(superseded.superseded_by_item_id, Some(revised_item_id));

        let revised = identity_item_state(&ctx.pool, revised_item_id).await?;
        assert_eq!(revised.status, "active");
        assert_eq!(
            revised.value_text,
            "Prefers concise replies with decision context."
        );
        assert_eq!(revised.supersedes_item_id, Some(added_item_id));

        let snapshot = identity::reconstruct_compact_identity_snapshot(&ctx.pool, 32).await?;
        assert!(snapshot.evolving_items.iter().any(|item| {
            item.category == IdentityItemCategory::Preference
                && item.value == "Prefers concise replies with decision context."
        }));
        assert!(
            !snapshot
                .evolving_items
                .iter()
                .any(|item| item.value == "Prefers concise replies.")
        );

        let expire_proposal = identity_delta_proposal(
            vec![identity_item_delta(
                IdentityDeltaOperation::Expire,
                IdentityItemCategory::Preference,
                "reply_cadence",
                "Prefers concise replies with decision context.",
                80,
                Some(75),
                Some(revised_item_id),
            )],
            None,
        );
        let expired_item_id =
            apply_identity_delta(&ctx.pool, &processing_context, &expire_proposal).await?;
        assert_eq!(expired_item_id, revised_item_id);

        let expired = identity_item_state(&ctx.pool, revised_item_id).await?;
        assert_eq!(expired.status, "expired");
        assert!(expired.valid_to.is_some());

        let snapshot = identity::reconstruct_compact_identity_snapshot(&ctx.pool, 32).await?;
        assert!(
            !snapshot
                .evolving_items
                .iter()
                .any(|item| item.category == IdentityItemCategory::Preference)
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn self_description_delta_merge_revises_and_expires_compact_projection() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let fixture = seed_foreground_fixture(&ctx.pool).await?;
        let processing_context = proposal_context(&fixture);

        let add_proposal = identity_delta_proposal(
            Vec::new(),
            Some(self_description_delta(
                IdentityDeltaOperation::Add,
                "Blue Lagoon is a direct personal assistant.",
            )),
        );
        let original_description_id =
            apply_identity_delta(&ctx.pool, &processing_context, &add_proposal).await?;

        let revise_proposal = identity_delta_proposal(
            Vec::new(),
            Some(self_description_delta(
                IdentityDeltaOperation::Revise,
                "Blue Lagoon is a direct personal assistant with durable continuity.",
            )),
        );
        let revised_description_id =
            apply_identity_delta(&ctx.pool, &processing_context, &revise_proposal).await?;
        assert_ne!(revised_description_id, original_description_id);

        let original = identity_item_state(&ctx.pool, original_description_id).await?;
        assert_eq!(original.status, "superseded");
        assert_eq!(original.superseded_by_item_id, Some(revised_description_id));

        let snapshot = identity::reconstruct_compact_identity_snapshot(&ctx.pool, 32).await?;
        assert_eq!(
            snapshot.self_description.as_deref(),
            Some("Blue Lagoon is a direct personal assistant with durable continuity.")
        );

        let expire_proposal = identity_delta_proposal(
            Vec::new(),
            Some(self_description_delta(
                IdentityDeltaOperation::Expire,
                "Blue Lagoon is a direct personal assistant with durable continuity.",
            )),
        );
        let expired_description_id =
            apply_identity_delta(&ctx.pool, &processing_context, &expire_proposal).await?;
        assert_eq!(expired_description_id, revised_description_id);

        let expired = identity_item_state(&ctx.pool, revised_description_id).await?;
        assert_eq!(expired.status, "expired");

        let snapshot = identity::reconstruct_compact_identity_snapshot(&ctx.pool, 32).await?;
        assert_eq!(snapshot.self_description, None);
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn identity_delta_controls_reject_duplicates_and_record_diagnostics() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let fixture = seed_foreground_fixture(&ctx.pool).await?;
        let processing_context = proposal_context(&fixture);

        let add_proposal = identity_delta_proposal(
            vec![identity_item_delta(
                IdentityDeltaOperation::Add,
                IdentityItemCategory::Like,
                "verification",
                "Likes verified implementation work.",
                80,
                Some(70),
                None,
            )],
            None,
        );
        apply_identity_delta(&ctx.pool, &processing_context, &add_proposal).await?;

        let duplicate_proposal = identity_delta_proposal(
            vec![identity_item_delta(
                IdentityDeltaOperation::Add,
                IdentityItemCategory::Like,
                "verification",
                "Likes verified implementation work.",
                80,
                Some(70),
                None,
            )],
            None,
        );
        let rejection =
            apply_rejected_identity_delta(&ctx.pool, &processing_context, &duplicate_proposal)
                .await?;
        assert!(rejection.contains("duplicate"));

        let active = identity::list_active_identity_items(&ctx.pool, 16).await?;
        assert_eq!(
            active
                .iter()
                .filter(|item| item.category == "like" && item.item_key == "verification")
                .count(),
            1
        );

        let diagnostics = identity::list_open_identity_diagnostics(&ctx.pool, 16).await?;
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.diagnostic_kind == "identity_delta_rejected"
                && diagnostic.message.contains("duplicate")
        }));
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn identity_delta_controls_reject_protected_core_drift() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let fixture = seed_foreground_fixture(&ctx.pool).await?;
        let processing_context = proposal_context(&fixture);
        let stable_item_id = Uuid::now_v7();
        identity::insert_identity_item(
            &ctx.pool,
            &identity::NewIdentityItem {
                identity_item_id: stable_item_id,
                self_model_artifact_id: None,
                proposal_id: None,
                trace_id: Some(fixture.trace_id),
                stability_class: "stable".to_string(),
                category: "name".to_string(),
                item_key: "name".to_string(),
                value_text: "Blue Lagoon".to_string(),
                confidence: 1.0,
                weight: None,
                provenance_kind: "seed".to_string(),
                source_kind: "seed".to_string(),
                merge_policy: "protected_core".to_string(),
                status: "active".to_string(),
                evidence_refs: json!([]),
                valid_from: Some(Utc::now()),
                valid_to: None,
                supersedes_item_id: None,
                payload: json!({}),
            },
        )
        .await?;

        let revise_proposal = identity_delta_proposal(
            vec![identity_item_delta(
                IdentityDeltaOperation::Revise,
                IdentityItemCategory::Name,
                "name",
                "Different Name",
                90,
                None,
                Some(stable_item_id),
            )],
            None,
        );
        let rejection =
            apply_rejected_identity_delta(&ctx.pool, &processing_context, &revise_proposal).await?;
        assert!(rejection.contains("protected stable identity"));

        let stable = identity_item_state(&ctx.pool, stable_item_id).await?;
        assert_eq!(stable.status, "active");
        assert_eq!(stable.value_text, "Blue Lagoon");

        let diagnostics = identity::list_open_identity_diagnostics(&ctx.pool, 16).await?;
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.diagnostic_kind == "identity_delta_rejected"
                && diagnostic.message.contains("protected stable identity")
        }));
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn proposal_validation_service_records_accepted_and_rejected_outcomes() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let fixture = seed_foreground_fixture(&ctx.pool).await?;
        let processing_context = proposal::ProposalProcessingContext {
            trace_id: fixture.trace_id,
            execution_id: fixture.execution_id,
            episode_id: Some(fixture.episode_id),
            source_ingress_id: Some(fixture.ingress_id),
            source_loop_kind: "conscious".to_string(),
        };

        let accepted = proposal::validate_and_record_proposal(
            &ctx.pool,
            &processing_context,
            &sample_contract_memory_proposal("Prefers concise answers."),
        )
        .await?;
        assert_eq!(accepted.outcome, ProposalEvaluationOutcome::Accepted);

        let rejected = proposal::validate_and_record_proposal(
            &ctx.pool,
            &processing_context,
            &sample_invalid_contract_memory_proposal(),
        )
        .await?;
        assert_eq!(rejected.outcome, ProposalEvaluationOutcome::Rejected);

        let stored =
            continuity::list_proposals_for_execution(&ctx.pool, fixture.execution_id).await?;
        assert_eq!(stored.len(), 2);
        assert!(stored.iter().any(|proposal| proposal.status == "accepted"));
        assert!(stored.iter().any(|proposal| proposal.status == "rejected"));

        let decisions =
            continuity::list_merge_decisions_for_execution(&ctx.pool, fixture.execution_id).await?;
        assert_eq!(decisions.len(), 2);
        assert!(
            decisions
                .iter()
                .any(|decision| decision.decision_kind == "accepted")
        );
        assert!(
            decisions
                .iter()
                .any(|decision| decision.decision_kind == "rejected")
        );

        let events = audit::list_for_execution(&ctx.pool, fixture.execution_id).await?;
        assert!(
            events
                .iter()
                .any(|event| event.event_kind == "proposal_evaluated")
        );
        assert!(
            events
                .iter()
                .any(|event| event.event_kind == "merge_decision_recorded")
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn memory_merge_service_supersedes_prior_active_artifact() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let fixture = seed_foreground_fixture(&ctx.pool).await?;
        let processing_context = proposal::ProposalProcessingContext {
            trace_id: fixture.trace_id,
            execution_id: fixture.execution_id,
            episode_id: Some(fixture.episode_id),
            source_ingress_id: Some(fixture.ingress_id),
            source_loop_kind: "conscious".to_string(),
        };

        let first_proposal = sample_contract_memory_proposal("Prefers concise answers.");
        let first_validation =
            proposal::validate_and_record_proposal(&ctx.pool, &processing_context, &first_proposal)
                .await?;
        assert_eq!(
            first_validation.outcome,
            ProposalEvaluationOutcome::Accepted
        );
        let first_merge =
            memory::apply_memory_proposal_merge(&ctx.pool, &processing_context, &first_proposal)
                .await?;
        let first_memory_id = match first_merge.target {
            Some(MergeDecisionTarget::MemoryArtifact(artifact_id)) => artifact_id,
            other => panic!("expected memory artifact target, got {other:?}"),
        };

        let second_proposal = sample_contract_memory_proposal_with_posture(
            "Now prefers direct answers.",
            ProposalConflictPosture::Supersedes,
            Some(first_memory_id),
        );
        let second_validation = proposal::validate_and_record_proposal(
            &ctx.pool,
            &processing_context,
            &second_proposal,
        )
        .await?;
        assert_eq!(
            second_validation.outcome,
            ProposalEvaluationOutcome::Accepted
        );

        let second_merge =
            memory::apply_memory_proposal_merge(&ctx.pool, &processing_context, &second_proposal)
                .await?;
        let second_memory_id = match second_merge.target {
            Some(MergeDecisionTarget::MemoryArtifact(artifact_id)) => artifact_id,
            other => panic!("expected memory artifact target, got {other:?}"),
        };

        let superseded = continuity::get_memory_artifact(&ctx.pool, first_memory_id).await?;
        assert_eq!(superseded.status, "superseded");
        assert_eq!(superseded.superseded_by_artifact_id, Some(second_memory_id));

        let active =
            continuity::list_active_memory_artifacts_by_subject(&ctx.pool, "user:primary", 10)
                .await?;
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].memory_artifact_id, second_memory_id);

        let merge_decision =
            continuity::get_merge_decision_by_proposal(&ctx.pool, second_proposal.proposal_id)
                .await?;
        let merge_decision = merge_decision.expect("merge decision should exist");
        assert_eq!(
            merge_decision.accepted_memory_artifact_id,
            Some(second_memory_id)
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn memory_and_retrieval_queries_distinguish_active_from_superseded() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let fixture = seed_foreground_fixture(&ctx.pool).await?;

        let active_proposal_id =
            insert_memory_proposal(&ctx.pool, &fixture, "Prefers direct answers.", "proposed")
                .await?;
        let active_memory_id = Uuid::now_v7();
        continuity::insert_memory_artifact(
            &ctx.pool,
            &continuity::NewMemoryArtifact {
                memory_artifact_id: active_memory_id,
                proposal_id: active_proposal_id,
                trace_id: fixture.trace_id,
                execution_id: fixture.execution_id,
                episode_id: Some(fixture.episode_id),
                source_ingress_id: Some(fixture.ingress_id),
                artifact_kind: "preference".to_string(),
                subject_ref: "user:primary".to_string(),
                content_text: "Prefers direct answers.".to_string(),
                confidence: 0.88,
                provenance_kind: "episode_observation".to_string(),
                status: "active".to_string(),
                valid_from: Some(Utc::now()),
                valid_to: None,
                superseded_at: None,
                superseded_by_artifact_id: None,
                supersedes_artifact_id: None,
                payload: json!({}),
            },
        )
        .await?;

        let superseded_proposal_id = insert_memory_proposal(
            &ctx.pool,
            &fixture,
            "Used to prefer verbose answers.",
            "proposed",
        )
        .await?;
        let superseded_memory_id = Uuid::now_v7();
        continuity::insert_memory_artifact(
            &ctx.pool,
            &continuity::NewMemoryArtifact {
                memory_artifact_id: superseded_memory_id,
                proposal_id: superseded_proposal_id,
                trace_id: fixture.trace_id,
                execution_id: fixture.execution_id,
                episode_id: Some(fixture.episode_id),
                source_ingress_id: Some(fixture.ingress_id),
                artifact_kind: "preference".to_string(),
                subject_ref: "user:primary".to_string(),
                content_text: "Used to prefer verbose answers.".to_string(),
                confidence: 0.50,
                provenance_kind: "episode_observation".to_string(),
                status: "superseded".to_string(),
                valid_from: Some(Utc::now()),
                valid_to: Some(Utc::now()),
                superseded_at: Some(Utc::now()),
                superseded_by_artifact_id: Some(active_memory_id),
                supersedes_artifact_id: None,
                payload: json!({}),
            },
        )
        .await?;

        continuity::insert_retrieval_artifact(
            &ctx.pool,
            &continuity::NewRetrievalArtifact {
                retrieval_artifact_id: Uuid::now_v7(),
                source_kind: "memory_artifact".to_string(),
                source_episode_id: None,
                source_memory_artifact_id: Some(active_memory_id),
                internal_conversation_ref: Some("telegram-primary".to_string()),
                lexical_document: "Prefers direct answers.".to_string(),
                relevance_timestamp: Utc::now(),
                status: "active".to_string(),
                payload: json!({ "reason": "same_conversation" }),
            },
        )
        .await?;
        continuity::insert_retrieval_artifact(
            &ctx.pool,
            &continuity::NewRetrievalArtifact {
                retrieval_artifact_id: Uuid::now_v7(),
                source_kind: "memory_artifact".to_string(),
                source_episode_id: None,
                source_memory_artifact_id: Some(superseded_memory_id),
                internal_conversation_ref: Some("telegram-primary".to_string()),
                lexical_document: "Used to prefer verbose answers.".to_string(),
                relevance_timestamp: Utc::now(),
                status: "inactive".to_string(),
                payload: json!({ "reason": "superseded" }),
            },
        )
        .await?;

        let active_memory = continuity::list_active_memory_artifacts(&ctx.pool, 10).await?;
        assert_eq!(active_memory.len(), 1);
        assert_eq!(active_memory[0].memory_artifact_id, active_memory_id);

        let superseded = continuity::list_superseded_memory_artifacts(&ctx.pool, 10).await?;
        assert_eq!(superseded.len(), 1);
        assert_eq!(superseded[0].memory_artifact_id, superseded_memory_id);

        let retrieval = continuity::list_active_retrieval_artifacts_for_conversation(
            &ctx.pool,
            "telegram-primary",
            10,
        )
        .await?;
        assert_eq!(retrieval.len(), 1);
        assert_eq!(
            retrieval[0].source_memory_artifact_id,
            Some(active_memory_id)
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn self_model_queries_prefer_latest_active_artifact() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let fixture = seed_foreground_fixture(&ctx.pool).await?;
        let old_id = Uuid::now_v7();
        continuity::insert_self_model_artifact(
            &ctx.pool,
            &continuity::NewSelfModelArtifact {
                self_model_artifact_id: old_id,
                proposal_id: None,
                trace_id: Some(fixture.trace_id),
                execution_id: Some(fixture.execution_id),
                episode_id: Some(fixture.episode_id),
                artifact_origin: "bootstrap_seed".to_string(),
                status: "superseded".to_string(),
                stable_identity: "blue-lagoon".to_string(),
                role: "personal_assistant".to_string(),
                communication_style: "direct".to_string(),
                capabilities: vec!["conversation".to_string()],
                constraints: vec!["respect_harness_policy".to_string()],
                preferences: vec!["concise".to_string()],
                current_goals: vec!["support_the_user".to_string()],
                current_subgoals: vec!["bootstrap".to_string()],
                superseded_at: Some(Utc::now()),
                superseded_by_artifact_id: None,
                supersedes_artifact_id: None,
                payload: json!({ "source": "seed" }),
            },
        )
        .await?;

        let active_id = Uuid::now_v7();
        continuity::insert_self_model_artifact(
            &ctx.pool,
            &continuity::NewSelfModelArtifact {
                self_model_artifact_id: active_id,
                proposal_id: None,
                trace_id: Some(fixture.trace_id),
                execution_id: Some(fixture.execution_id),
                episode_id: Some(fixture.episode_id),
                artifact_origin: "proposal_merge".to_string(),
                status: "active".to_string(),
                stable_identity: "blue-lagoon".to_string(),
                role: "personal_assistant".to_string(),
                communication_style: "direct".to_string(),
                capabilities: vec!["conversation".to_string(), "continuity".to_string()],
                constraints: vec!["respect_harness_policy".to_string()],
                preferences: vec!["concise".to_string()],
                current_goals: vec!["support_the_user".to_string()],
                current_subgoals: vec!["preserve_continuity".to_string()],
                superseded_at: None,
                superseded_by_artifact_id: None,
                supersedes_artifact_id: Some(old_id),
                payload: json!({ "source": "proposal" }),
            },
        )
        .await?;

        let latest = continuity::get_latest_active_self_model_artifact(&ctx.pool).await?;
        let latest = latest.expect("active self-model should exist");
        assert_eq!(latest.self_model_artifact_id, active_id);
        assert!(latest.capabilities.contains(&"continuity".to_string()));

        let superseded = continuity::list_superseded_self_model_artifacts(&ctx.pool, 10).await?;
        assert_eq!(superseded.len(), 1);
        assert_eq!(superseded[0].self_model_artifact_id, old_id);
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn canonical_self_model_bootstraps_from_seed_when_absent() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let fixture = seed_foreground_fixture(&ctx.pool).await?;
        let mut config = ctx.config.clone();
        config.self_model = Some(SelfModelConfig {
            seed_path: support::workspace_root()
                .join("config")
                .join("self_model_seed.toml"),
        });

        let loaded = self_model::load_self_model_snapshot(
            &ctx.pool,
            &config,
            &self_model::SelfModelLoadContext {
                trace_id: fixture.trace_id,
                execution_id: fixture.execution_id,
                episode_id: Some(fixture.episode_id),
            },
        )
        .await?;

        assert_eq!(
            loaded.source_kind,
            self_model::SelfModelSourceKind::BootstrapSeed
        );
        assert!(loaded.bootstrap_performed);

        let active = continuity::list_active_self_model_artifacts(&ctx.pool, 10).await?;
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].stable_identity, "blue-lagoon");
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn canonical_self_model_load_prefers_existing_active_artifact() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let fixture = seed_foreground_fixture(&ctx.pool).await?;
        let mut config = ctx.config.clone();
        config.self_model = Some(SelfModelConfig {
            seed_path: support::workspace_root()
                .join("config")
                .join("self_model_seed.toml"),
        });

        continuity::insert_self_model_artifact(
            &ctx.pool,
            &continuity::NewSelfModelArtifact {
                self_model_artifact_id: Uuid::now_v7(),
                proposal_id: None,
                trace_id: Some(fixture.trace_id),
                execution_id: Some(fixture.execution_id),
                episode_id: Some(fixture.episode_id),
                artifact_origin: "proposal_merge".to_string(),
                status: "active".to_string(),
                stable_identity: "blue-lagoon".to_string(),
                role: "personal_assistant".to_string(),
                communication_style: "direct".to_string(),
                capabilities: vec!["conversation".to_string(), "continuity".to_string()],
                constraints: vec!["respect_harness_policy".to_string()],
                preferences: vec!["concise".to_string()],
                current_goals: vec!["support_the_user".to_string()],
                current_subgoals: vec!["read_canonical_state".to_string()],
                superseded_at: None,
                superseded_by_artifact_id: None,
                supersedes_artifact_id: None,
                payload: json!({ "source": "manual_canonical" }),
            },
        )
        .await?;

        let loaded = self_model::load_self_model_snapshot(
            &ctx.pool,
            &config,
            &self_model::SelfModelLoadContext {
                trace_id: fixture.trace_id,
                execution_id: fixture.execution_id,
                episode_id: Some(fixture.episode_id),
            },
        )
        .await?;

        assert_eq!(
            loaded.source_kind,
            self_model::SelfModelSourceKind::CanonicalArtifact
        );
        assert!(!loaded.bootstrap_performed);
        assert!(
            loaded
                .snapshot
                .capabilities
                .contains(&"continuity".to_string())
        );
        Ok(())
    })
    .await
}

struct ForegroundFixture {
    trace_id: Uuid,
    execution_id: Uuid,
    episode_id: Uuid,
    ingress_id: Uuid,
}

async fn seed_foreground_fixture(pool: &PgPool) -> Result<ForegroundFixture> {
    let execution_id = Uuid::now_v7();
    let trace_id = Uuid::now_v7();
    execution::insert(
        pool,
        &execution::NewExecutionRecord {
            execution_id,
            trace_id,
            trigger_kind: "user_ingress".to_string(),
            synthetic_trigger: None,
            status: "started".to_string(),
            request_payload: json!({ "fixture": true }),
        },
    )
    .await?;

    let ingress = NormalizedIngress {
        ingress_id: Uuid::now_v7(),
        channel_kind: ChannelKind::Telegram,
        external_user_id: "42".to_string(),
        external_conversation_id: "42".to_string(),
        external_event_id: format!("update-{}", Uuid::now_v7()),
        external_message_id: Some("message-42".to_string()),
        internal_principal_ref: "primary-user".to_string(),
        internal_conversation_ref: "telegram-primary".to_string(),
        event_kind: IngressEventKind::MessageCreated,
        occurred_at: Utc::now(),
        text_body: Some("hello from continuity component".to_string()),
        reply_to: None,
        attachments: Vec::new(),
        command_hint: None,
        approval_payload: None,
        raw_payload_ref: Some("continuity-component".to_string()),
    };

    foreground::insert_ingress_event(
        pool,
        &NewIngressEvent {
            ingress: ingress.clone(),
            conversation_binding_id: None,
            trace_id,
            execution_id: Some(execution_id),
            status: "accepted".to_string(),
            rejection_reason: None,
        },
    )
    .await?;

    let episode_id = Uuid::now_v7();
    foreground::insert_episode(
        pool,
        &NewEpisode {
            episode_id,
            trace_id,
            execution_id,
            ingress_id: Some(ingress.ingress_id),
            internal_principal_ref: ingress.internal_principal_ref.clone(),
            internal_conversation_ref: ingress.internal_conversation_ref.clone(),
            trigger_kind: "user_ingress".to_string(),
            trigger_source: "telegram".to_string(),
            status: "completed".to_string(),
            started_at: ingress.occurred_at,
        },
    )
    .await?;

    Ok(ForegroundFixture {
        trace_id,
        execution_id,
        episode_id,
        ingress_id: ingress.ingress_id,
    })
}

async fn insert_memory_proposal(
    pool: &PgPool,
    fixture: &ForegroundFixture,
    content_text: &str,
    status: &str,
) -> Result<Uuid> {
    let proposal_id = Uuid::now_v7();
    continuity::insert_proposal(
        pool,
        &continuity::NewProposalRecord {
            proposal_id,
            trace_id: fixture.trace_id,
            execution_id: fixture.execution_id,
            episode_id: Some(fixture.episode_id),
            source_ingress_id: Some(fixture.ingress_id),
            source_loop_kind: "conscious".to_string(),
            proposal_kind: "memory_artifact".to_string(),
            canonical_target: "memory_artifacts".to_string(),
            status: status.to_string(),
            confidence: 0.75,
            conflict_posture: "independent".to_string(),
            subject_ref: "user:primary".to_string(),
            content_text: content_text.to_string(),
            rationale: None,
            valid_from: Some(Utc::now()),
            valid_to: None,
            supersedes_artifact_id: None,
            supersedes_artifact_kind: None,
            payload: json!({ "artifact_kind": "preference" }),
        },
    )
    .await?;
    Ok(proposal_id)
}

fn proposal_context(fixture: &ForegroundFixture) -> proposal::ProposalProcessingContext {
    proposal::ProposalProcessingContext {
        trace_id: fixture.trace_id,
        execution_id: fixture.execution_id,
        episode_id: Some(fixture.episode_id),
        source_ingress_id: Some(fixture.ingress_id),
        source_loop_kind: "conscious".to_string(),
    }
}

async fn apply_identity_delta(
    pool: &PgPool,
    context: &proposal::ProposalProcessingContext,
    proposal: &CanonicalProposal,
) -> Result<Uuid> {
    let validation = proposal::validate_and_record_proposal(pool, context, proposal).await?;
    assert_eq!(validation.outcome, ProposalEvaluationOutcome::Accepted);

    let merge = identity::apply_identity_delta_proposal_merge(pool, context, proposal).await?;
    assert_eq!(merge.outcome, ProposalEvaluationOutcome::Accepted);
    let Some(MergeDecisionTarget::IdentityItems(identity_item_ids)) = merge.target else {
        panic!("expected identity item target, got {:?}", merge.target);
    };
    assert_eq!(identity_item_ids.len(), 1);
    Ok(identity_item_ids[0])
}

async fn apply_rejected_identity_delta(
    pool: &PgPool,
    context: &proposal::ProposalProcessingContext,
    proposal: &CanonicalProposal,
) -> Result<String> {
    let validation = proposal::validate_and_record_proposal(pool, context, proposal).await?;
    assert_eq!(validation.outcome, ProposalEvaluationOutcome::Accepted);

    let merge = identity::apply_identity_delta_proposal_merge(pool, context, proposal).await?;
    assert_eq!(merge.outcome, ProposalEvaluationOutcome::Rejected);
    assert_eq!(merge.target, None);
    Ok(merge.reason)
}

#[derive(Debug)]
struct IdentityItemState {
    status: String,
    value_text: String,
    confidence: f64,
    weight: Option<f64>,
    valid_to: Option<chrono::DateTime<Utc>>,
    supersedes_item_id: Option<Uuid>,
    superseded_by_item_id: Option<Uuid>,
}

impl IdentityItemState {
    fn confidence_pct(&self) -> u8 {
        (self.confidence * 100.0).round() as u8
    }

    fn weight_pct(&self) -> Option<u8> {
        self.weight.map(|weight| (weight * 100.0).round() as u8)
    }
}

async fn identity_item_state(pool: &PgPool, identity_item_id: Uuid) -> Result<IdentityItemState> {
    let row = sqlx::query(
        r#"
        SELECT status, value_text, confidence, weight, valid_to, supersedes_item_id,
               superseded_by_item_id
        FROM identity_items
        WHERE identity_item_id = $1
        "#,
    )
    .bind(identity_item_id)
    .fetch_one(pool)
    .await?;
    Ok(IdentityItemState {
        status: row.get("status"),
        value_text: row.get("value_text"),
        confidence: row.get("confidence"),
        weight: row.get("weight"),
        valid_to: row.get("valid_to"),
        supersedes_item_id: row.get("supersedes_item_id"),
        superseded_by_item_id: row.get("superseded_by_item_id"),
    })
}

fn identity_delta_proposal(
    item_deltas: Vec<IdentityItemDelta>,
    self_description_delta: Option<SelfDescriptionDelta>,
) -> CanonicalProposal {
    CanonicalProposal {
        proposal_id: Uuid::now_v7(),
        proposal_kind: CanonicalProposalKind::IdentityDelta,
        canonical_target: CanonicalTargetKind::IdentityItems,
        confidence_pct: 90,
        conflict_posture: ProposalConflictPosture::Independent,
        subject_ref: "self:blue-lagoon".to_string(),
        rationale: Some("Identity component test proposal.".to_string()),
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
            item_deltas,
            self_description_delta,
            interview_action: None,
            rationale: "Identity component test proposal.".to_string(),
        }),
    }
}

fn identity_item_delta(
    operation: IdentityDeltaOperation,
    category: IdentityItemCategory,
    item_key: &str,
    value: &str,
    confidence_pct: u8,
    weight_pct: Option<u8>,
    target_identity_item_id: Option<Uuid>,
) -> IdentityItemDelta {
    IdentityItemDelta {
        operation,
        stability_class: IdentityStabilityClass::Evolving,
        category,
        item_key: item_key.to_string(),
        value: value.to_string(),
        confidence_pct,
        weight_pct,
        source: IdentityItemSource::UserAuthored,
        merge_policy: match operation {
            IdentityDeltaOperation::Add => IdentityMergePolicy::Revisable,
            IdentityDeltaOperation::Reinforce => IdentityMergePolicy::Reinforceable,
            IdentityDeltaOperation::Weaken => IdentityMergePolicy::Reinforceable,
            IdentityDeltaOperation::Revise | IdentityDeltaOperation::Supersede => {
                IdentityMergePolicy::Revisable
            }
            IdentityDeltaOperation::Expire => IdentityMergePolicy::Expirable,
        },
        evidence_refs: vec![identity_evidence_ref()],
        valid_from: Some(Utc::now()),
        valid_to: None,
        target_identity_item_id,
    }
}

fn self_description_delta(
    operation: IdentityDeltaOperation,
    description: &str,
) -> SelfDescriptionDelta {
    SelfDescriptionDelta {
        operation,
        description: description.to_string(),
        evidence_refs: vec![identity_evidence_ref()],
    }
}

fn identity_evidence_ref() -> IdentityEvidenceRef {
    IdentityEvidenceRef {
        source_kind: "component_test".to_string(),
        source_id: Some(Uuid::now_v7()),
        summary: "Continuity component test evidence.".to_string(),
    }
}

fn sample_contract_memory_proposal(content_text: &str) -> CanonicalProposal {
    sample_contract_memory_proposal_with_posture(
        content_text,
        ProposalConflictPosture::Independent,
        None,
    )
}

fn sample_contract_memory_proposal_with_posture(
    content_text: &str,
    conflict_posture: ProposalConflictPosture,
    supersedes_artifact_id: Option<Uuid>,
) -> CanonicalProposal {
    CanonicalProposal {
        proposal_id: Uuid::now_v7(),
        proposal_kind: CanonicalProposalKind::MemoryArtifact,
        canonical_target: CanonicalTargetKind::MemoryArtifacts,
        confidence_pct: 90,
        conflict_posture,
        subject_ref: "user:primary".to_string(),
        rationale: Some("Observed in continuity component test.".to_string()),
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
            content_text: content_text.to_string(),
        }),
    }
}

fn sample_invalid_contract_memory_proposal() -> CanonicalProposal {
    let mut proposal = sample_contract_memory_proposal(" ");
    proposal.payload = CanonicalProposalPayload::MemoryArtifact(MemoryArtifactProposal {
        artifact_kind: "".to_string(),
        content_text: "".to_string(),
    });
    proposal
}
