mod support;

use anyhow::Result;
use chrono::Utc;
use contracts::{
    CanonicalProposal, CanonicalProposalKind, CanonicalProposalPayload, CanonicalTargetKind,
    ChannelKind, IngressEventKind, MemoryArtifactProposal, MergeDecisionTarget, NormalizedIngress,
    ProposalConflictPosture, ProposalEvaluationOutcome, ProposalProvenance, ProposalProvenanceKind,
};
use harness::{
    audit,
    config::SelfModelConfig,
    continuity, execution,
    foreground::{self, NewEpisode, NewIngressEvent},
    memory, proposal, self_model,
};
use serde_json::json;
use serial_test::serial;
use sqlx::PgPool;
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
