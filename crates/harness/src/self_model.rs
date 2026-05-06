use std::fs;

use anyhow::{Context, Result, bail};
use chrono::Utc;
use contracts::{
    CanonicalProposal, CanonicalProposalPayload, CompactIdentityItem, CompactIdentitySnapshot,
    IdentityItemCategory, IdentityLifecycleContext, IdentityLifecycleState, InternalStateSnapshot,
    MergeDecisionTarget, ProposalEvaluation, ProposalEvaluationOutcome,
    SelfModelObservationProposal, SelfModelSnapshot,
};
use serde::Deserialize;
use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::{
    config::RuntimeConfig,
    continuity::{self, NewSelfModelArtifact, SelfModelArtifactRecord},
    proposal::{self, ProposalProcessingContext},
};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct SelfModelSeedDocument {
    seed_version: Option<u32>,
    stable_identity: String,
    role: String,
    communication_style: String,
    capabilities: Vec<String>,
    constraints: Vec<String>,
    preferences: Vec<String>,
    current_goals: Vec<String>,
    current_subgoals: Vec<String>,
    identity: Option<RichIdentitySeed>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct RichIdentitySeed {
    stable: StableIdentitySeed,
    evolving: EvolvingIdentitySeed,
    compact_self_description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct StableIdentitySeed {
    name: String,
    identity_form: String,
    role: String,
    archetype: String,
    origin_backstory: String,
    age_framing: Option<String>,
    foundational_traits: Vec<String>,
    foundational_values: Vec<String>,
    enduring_boundaries: Vec<String>,
    default_communication_style: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct EvolvingIdentitySeed {
    preferences: Vec<String>,
    likes: Vec<String>,
    dislikes: Vec<String>,
    habits: Vec<String>,
    routines: Vec<String>,
    learned_tendencies: Vec<String>,
    autobiographical_refinements: Vec<String>,
    recurring_self_descriptions: Vec<String>,
    interaction_style_adaptations: Vec<String>,
    goals: Vec<String>,
    subgoals: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InternalStateSeed {
    pub load_pct: u8,
    pub health_pct: u8,
    pub reliability_pct: u8,
    pub resource_pressure_pct: u8,
    pub confidence_pct: u8,
    pub connection_quality_pct: u8,
}

impl Default for InternalStateSeed {
    fn default() -> Self {
        Self {
            load_pct: 10,
            health_pct: 100,
            reliability_pct: 100,
            resource_pressure_pct: 10,
            confidence_pct: 80,
            connection_quality_pct: 100,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfModelLoadContext {
    pub trace_id: Uuid,
    pub execution_id: Uuid,
    pub episode_id: Option<Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfModelSourceKind {
    BootstrapSeed,
    CanonicalArtifact,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedSelfModelSnapshot {
    pub snapshot: SelfModelSnapshot,
    pub source_kind: SelfModelSourceKind,
    pub canonical_artifact_id: Option<Uuid>,
    pub seed_path: String,
    pub bootstrap_performed: bool,
}

pub async fn load_self_model_snapshot(
    pool: &PgPool,
    config: &RuntimeConfig,
    load_context: &SelfModelLoadContext,
) -> Result<LoadedSelfModelSnapshot> {
    let resolved = config.require_self_model_config()?;
    let active_artifacts = continuity::list_active_self_model_artifacts(pool, 2).await?;
    if let Some(active_artifact) = select_single_active_artifact(&active_artifacts)? {
        return Ok(LoadedSelfModelSnapshot {
            snapshot: snapshot_from_canonical_artifact(active_artifact)?,
            source_kind: SelfModelSourceKind::CanonicalArtifact,
            canonical_artifact_id: Some(active_artifact.self_model_artifact_id),
            seed_path: resolved.seed_path.display().to_string(),
            bootstrap_performed: false,
        });
    }

    let snapshot = load_seed_self_model_snapshot(config)?;
    let canonical_artifact_id = Uuid::now_v7();
    continuity::insert_self_model_artifact(
        pool,
        &NewSelfModelArtifact {
            self_model_artifact_id: canonical_artifact_id,
            proposal_id: None,
            trace_id: Some(load_context.trace_id),
            execution_id: Some(load_context.execution_id),
            episode_id: load_context.episode_id,
            artifact_origin: "bootstrap_seed".to_string(),
            status: "active".to_string(),
            stable_identity: snapshot.stable_identity.clone(),
            role: snapshot.role.clone(),
            communication_style: snapshot.communication_style.clone(),
            capabilities: snapshot.capabilities.clone(),
            constraints: snapshot.constraints.clone(),
            preferences: snapshot.preferences.clone(),
            current_goals: snapshot.current_goals.clone(),
            current_subgoals: snapshot.current_subgoals.clone(),
            superseded_at: None,
            superseded_by_artifact_id: None,
            supersedes_artifact_id: None,
            payload: serde_json::json!({
                "seed_path": resolved.seed_path.display().to_string(),
            }),
        },
    )
    .await?;

    Ok(LoadedSelfModelSnapshot {
        snapshot,
        source_kind: SelfModelSourceKind::BootstrapSeed,
        canonical_artifact_id: Some(canonical_artifact_id),
        seed_path: resolved.seed_path.display().to_string(),
        bootstrap_performed: true,
    })
}

pub fn load_seed_self_model_snapshot(config: &RuntimeConfig) -> Result<SelfModelSnapshot> {
    let resolved = config.require_self_model_config()?;
    let raw = fs::read_to_string(&resolved.seed_path).with_context(|| {
        format!(
            "failed to read self-model seed artifact at {}",
            resolved.seed_path.display()
        )
    })?;
    let seed: SelfModelSeedDocument =
        toml::from_str(&raw).context("failed to parse self-model seed artifact as TOML")?;
    let snapshot = seed.into_snapshot()?;
    validate_snapshot(&snapshot)?;
    Ok(snapshot)
}

pub fn build_internal_state_snapshot(
    seed: InternalStateSeed,
    active_conditions: Vec<String>,
) -> InternalStateSnapshot {
    InternalStateSnapshot {
        load_pct: seed.load_pct,
        health_pct: seed.health_pct,
        reliability_pct: seed.reliability_pct,
        resource_pressure_pct: seed.resource_pressure_pct,
        confidence_pct: seed.confidence_pct,
        connection_quality_pct: seed.connection_quality_pct,
        active_conditions,
    }
}

pub async fn derive_live_internal_state_snapshot(
    pool: &PgPool,
    seed: InternalStateSeed,
    active_conditions: Vec<String>,
) -> Result<InternalStateSnapshot> {
    let signals = load_internal_state_signals(pool).await?;
    Ok(build_internal_state_from_signals(
        seed,
        active_conditions,
        signals,
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InternalStateSignals {
    pub active_background_runs: i64,
    pub pending_approvals: i64,
    pub pending_wake_signals: i64,
    pub recent_failed_runs: i64,
    pub recent_critical_diagnostics: i64,
}

fn build_internal_state_from_signals(
    seed: InternalStateSeed,
    mut active_conditions: Vec<String>,
    signals: InternalStateSignals,
) -> InternalStateSnapshot {
    if signals.active_background_runs > 0 {
        active_conditions.push(format!(
            "active_background_runs:{}",
            signals.active_background_runs
        ));
    }
    if signals.pending_approvals > 0 {
        active_conditions.push(format!("pending_approvals:{}", signals.pending_approvals));
    }
    if signals.pending_wake_signals > 0 {
        active_conditions.push(format!(
            "pending_wake_signals:{}",
            signals.pending_wake_signals
        ));
    }
    if signals.recent_failed_runs > 0 {
        active_conditions.push(format!("recent_failed_runs:{}", signals.recent_failed_runs));
    }
    if signals.recent_critical_diagnostics > 0 {
        active_conditions.push(format!(
            "recent_critical_diagnostics:{}",
            signals.recent_critical_diagnostics
        ));
    }

    let load_pressure = signals.active_background_runs * 12
        + signals.pending_approvals * 4
        + signals.pending_wake_signals * 3;
    let reliability_penalty =
        signals.recent_failed_runs * 12 + signals.recent_critical_diagnostics * 8;
    let resource_pressure = signals.active_background_runs * 10
        + signals.pending_wake_signals * 4
        + signals.pending_approvals * 2;

    InternalStateSnapshot {
        load_pct: add_bounded(seed.load_pct, load_pressure, 100),
        health_pct: subtract_bounded(seed.health_pct, reliability_penalty, 0),
        reliability_pct: subtract_bounded(seed.reliability_pct, reliability_penalty, 0),
        resource_pressure_pct: add_bounded(seed.resource_pressure_pct, resource_pressure, 100),
        confidence_pct: subtract_bounded(seed.confidence_pct, reliability_penalty / 2, 0),
        connection_quality_pct: subtract_bounded(
            seed.connection_quality_pct,
            signals.recent_critical_diagnostics * 5,
            0,
        ),
        active_conditions,
    }
}

async fn load_internal_state_signals(pool: &PgPool) -> Result<InternalStateSignals> {
    let row = sqlx::query(
        r#"
        SELECT
            (SELECT COUNT(*) FROM background_job_runs WHERE status IN ('leased', 'running')) AS active_background_runs,
            (SELECT COUNT(*) FROM approval_requests WHERE status = 'pending') AS pending_approvals,
            (SELECT COUNT(*) FROM wake_signals WHERE status = 'pending_review') AS pending_wake_signals,
            (SELECT COUNT(*) FROM background_job_runs WHERE status IN ('failed', 'timed_out') AND lease_acquired_at > NOW() - INTERVAL '1 hour') AS recent_failed_runs,
            (SELECT COUNT(*) FROM operational_diagnostics WHERE severity = 'critical' AND created_at > NOW() - INTERVAL '1 hour') AS recent_critical_diagnostics
        "#,
    )
    .fetch_one(pool)
    .await
    .context("failed to load internal-state runtime signals")?;

    Ok(InternalStateSignals {
        active_background_runs: row.get("active_background_runs"),
        pending_approvals: row.get("pending_approvals"),
        pending_wake_signals: row.get("pending_wake_signals"),
        recent_failed_runs: row.get("recent_failed_runs"),
        recent_critical_diagnostics: row.get("recent_critical_diagnostics"),
    })
}

fn add_bounded(base: u8, delta: i64, max_value: u8) -> u8 {
    let value = i64::from(base)
        .saturating_add(delta)
        .clamp(0, i64::from(max_value));
    value as u8
}

fn subtract_bounded(base: u8, penalty: i64, min_value: u8) -> u8 {
    let value = i64::from(base)
        .saturating_sub(penalty)
        .max(i64::from(min_value));
    value as u8
}

pub fn compact_self_model_view(snapshot: &SelfModelSnapshot) -> Result<Value> {
    serde_json::to_value(snapshot).context("failed to serialize self-model snapshot")
}

pub fn compact_internal_state_view(snapshot: &InternalStateSnapshot) -> Result<Value> {
    serde_json::to_value(snapshot).context("failed to serialize internal-state snapshot")
}

pub async fn apply_self_model_proposal_merge(
    pool: &PgPool,
    config: &RuntimeConfig,
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

    let CanonicalProposalPayload::SelfModelObservation(payload) = &proposal.payload else {
        let evaluation = ProposalEvaluation {
            proposal_id: proposal.proposal_id,
            outcome: ProposalEvaluationOutcome::Rejected,
            reason: "self-model merge requires a self_model_observation payload".to_string(),
            target: None,
        };
        continuity::update_merge_decision_outcome(
            pool,
            proposal.proposal_id,
            "rejected",
            &evaluation.reason,
        )
        .await?;
        return Ok(evaluation);
    };

    let active_artifacts = continuity::list_active_self_model_artifacts(pool, 2).await?;
    let (current, superseded_artifact_id) =
        if let Some(artifact) = select_single_active_artifact(&active_artifacts)? {
            (
                snapshot_from_canonical_artifact(artifact)?,
                Some(artifact.self_model_artifact_id),
            )
        } else {
            let loaded = load_self_model_snapshot(
                pool,
                config,
                &SelfModelLoadContext {
                    trace_id: context.trace_id,
                    execution_id: context.execution_id,
                    episode_id: context.episode_id,
                },
            )
            .await?;
            (loaded.snapshot, loaded.canonical_artifact_id)
        };

    if current
        .preferences
        .iter()
        .any(|value| value == &payload.content_text)
    {
        let evaluation = ProposalEvaluation {
            proposal_id: proposal.proposal_id,
            outcome: ProposalEvaluationOutcome::Rejected,
            reason: "self-model observation is already present in canonical preferences"
                .to_string(),
            target: None,
        };
        continuity::update_merge_decision_outcome(
            pool,
            proposal.proposal_id,
            "rejected",
            &evaluation.reason,
        )
        .await?;
        return Ok(evaluation);
    }

    let next_snapshot = merge_self_model_observation(current, payload);
    let next_artifact_id = Uuid::now_v7();
    let mut transaction = pool.begin().await?;
    continuity::insert_self_model_artifact(
        &mut *transaction,
        &NewSelfModelArtifact {
            self_model_artifact_id: next_artifact_id,
            proposal_id: Some(proposal.proposal_id),
            trace_id: Some(context.trace_id),
            execution_id: Some(context.execution_id),
            episode_id: context.episode_id,
            artifact_origin: "proposal_merge".to_string(),
            status: "active".to_string(),
            stable_identity: next_snapshot.stable_identity.clone(),
            role: next_snapshot.role.clone(),
            communication_style: next_snapshot.communication_style.clone(),
            capabilities: next_snapshot.capabilities.clone(),
            constraints: next_snapshot.constraints.clone(),
            preferences: next_snapshot.preferences.clone(),
            current_goals: next_snapshot.current_goals.clone(),
            current_subgoals: next_snapshot.current_subgoals.clone(),
            superseded_at: None,
            superseded_by_artifact_id: None,
            supersedes_artifact_id: superseded_artifact_id,
            payload: serde_json::json!({
                "observation_kind": payload.observation_kind,
                "content_text": payload.content_text,
            }),
        },
    )
    .await?;
    if let Some(previous_artifact_id) = superseded_artifact_id {
        continuity::mark_self_model_artifact_superseded(
            &mut *transaction,
            previous_artifact_id,
            next_artifact_id,
            Utc::now(),
        )
        .await?;
    }
    continuity::update_merge_decision_targets_in_tx(
        &mut *transaction,
        proposal.proposal_id,
        None,
        Some(next_artifact_id),
    )
    .await?;
    transaction.commit().await?;

    Ok(ProposalEvaluation {
        proposal_id: proposal.proposal_id,
        outcome: ProposalEvaluationOutcome::Accepted,
        reason: "self-model proposal merged into canonical store".to_string(),
        target: Some(MergeDecisionTarget::SelfModelArtifact(next_artifact_id)),
    })
}

fn select_single_active_artifact(
    active_artifacts: &[SelfModelArtifactRecord],
) -> Result<Option<&SelfModelArtifactRecord>> {
    match active_artifacts {
        [] => Ok(None),
        [artifact] => Ok(Some(artifact)),
        _ => bail!("multiple active canonical self-model artifacts found"),
    }
}

fn snapshot_from_canonical_artifact(
    artifact: &SelfModelArtifactRecord,
) -> Result<SelfModelSnapshot> {
    let snapshot = SelfModelSnapshot {
        stable_identity: artifact.stable_identity.clone(),
        role: artifact.role.clone(),
        communication_style: artifact.communication_style.clone(),
        capabilities: artifact.capabilities.clone(),
        constraints: artifact.constraints.clone(),
        preferences: artifact.preferences.clone(),
        current_goals: artifact.current_goals.clone(),
        current_subgoals: artifact.current_subgoals.clone(),
        identity: None,
        identity_lifecycle: Default::default(),
    };
    validate_snapshot(&snapshot)?;
    Ok(snapshot)
}

fn validate_snapshot(snapshot: &SelfModelSnapshot) -> Result<()> {
    if snapshot.stable_identity.trim().is_empty() {
        bail!("self-model stable_identity must not be empty");
    }
    if snapshot.role.trim().is_empty() {
        bail!("self-model role must not be empty");
    }
    if snapshot.communication_style.trim().is_empty() {
        bail!("self-model communication_style must not be empty");
    }
    if snapshot.capabilities.is_empty() {
        bail!("self-model capabilities must not be empty");
    }
    if snapshot.current_goals.is_empty() {
        bail!("self-model current_goals must not be empty");
    }
    if let Some(identity) = &snapshot.identity {
        validate_rich_identity_seed(identity)?;
    }
    Ok(())
}

fn validate_rich_identity_seed(identity: &CompactIdentitySnapshot) -> Result<()> {
    if identity.identity_summary.trim().is_empty() {
        bail!("rich self-model identity summary must not be empty");
    }
    for category in [
        IdentityItemCategory::Name,
        IdentityItemCategory::IdentityForm,
        IdentityItemCategory::Role,
        IdentityItemCategory::Archetype,
        IdentityItemCategory::OriginBackstory,
        IdentityItemCategory::FoundationalTrait,
        IdentityItemCategory::FoundationalValue,
        IdentityItemCategory::EnduringBoundary,
        IdentityItemCategory::DefaultCommunicationStyle,
    ] {
        if !identity
            .stable_items
            .iter()
            .any(|item| item.category == category && !item.value.trim().is_empty())
        {
            bail!("rich self-model identity missing required stable category {category:?}");
        }
    }
    if identity.evolving_items.is_empty() {
        bail!("rich self-model identity must include at least one evolving item");
    }
    Ok(())
}

fn merge_self_model_observation(
    mut snapshot: SelfModelSnapshot,
    payload: &SelfModelObservationProposal,
) -> SelfModelSnapshot {
    match payload.observation_kind.as_str() {
        "interaction_style" | "preference" => {
            snapshot.preferences.push(payload.content_text.clone());
        }
        "subgoal" => snapshot.current_subgoals.push(payload.content_text.clone()),
        _ => snapshot.preferences.push(payload.content_text.clone()),
    }
    snapshot
}

impl SelfModelSeedDocument {
    fn into_snapshot(self) -> Result<SelfModelSnapshot> {
        if matches!(self.seed_version, Some(0)) {
            bail!("self-model seed_version must be 1 or greater when present");
        }
        let identity = self.identity.map(RichIdentitySeed::into_compact_snapshot);
        Ok(SelfModelSnapshot {
            stable_identity: self.stable_identity,
            role: self.role,
            communication_style: self.communication_style,
            capabilities: self.capabilities,
            constraints: self.constraints,
            preferences: self.preferences,
            current_goals: self.current_goals,
            current_subgoals: self.current_subgoals,
            identity,
            identity_lifecycle: IdentityLifecycleContext {
                state: IdentityLifecycleState::BootstrapSeedOnly,
                kickstart_available: true,
                kickstart: None,
            },
        })
    }
}

impl RichIdentitySeed {
    fn into_compact_snapshot(self) -> CompactIdentitySnapshot {
        let values = self.stable.foundational_values.clone();
        let boundaries = self.stable.enduring_boundaries.clone();
        let mut stable_items = vec![
            compact_identity_item(IdentityItemCategory::Name, self.stable.name, 100, None),
            compact_identity_item(
                IdentityItemCategory::IdentityForm,
                self.stable.identity_form,
                100,
                None,
            ),
            compact_identity_item(IdentityItemCategory::Role, self.stable.role, 100, None),
            compact_identity_item(
                IdentityItemCategory::Archetype,
                self.stable.archetype,
                100,
                None,
            ),
            compact_identity_item(
                IdentityItemCategory::OriginBackstory,
                self.stable.origin_backstory,
                90,
                None,
            ),
            compact_identity_item(
                IdentityItemCategory::DefaultCommunicationStyle,
                self.stable.default_communication_style,
                100,
                None,
            ),
        ];
        if let Some(age_framing) = self.stable.age_framing {
            stable_items.push(compact_identity_item(
                IdentityItemCategory::AgeFraming,
                age_framing,
                80,
                None,
            ));
        }
        stable_items.extend(self.stable.foundational_traits.into_iter().map(|value| {
            compact_identity_item(IdentityItemCategory::FoundationalTrait, value, 90, Some(80))
        }));
        stable_items.extend(self.stable.foundational_values.into_iter().map(|value| {
            compact_identity_item(IdentityItemCategory::FoundationalValue, value, 90, Some(80))
        }));
        stable_items.extend(
            self.stable
                .enduring_boundaries
                .iter()
                .cloned()
                .map(|value| {
                    compact_identity_item(
                        IdentityItemCategory::EnduringBoundary,
                        value,
                        95,
                        Some(90),
                    )
                }),
        );

        let mut evolving_items = Vec::new();
        evolving_items.extend(self.evolving.preferences.into_iter().map(|value| {
            compact_identity_item(IdentityItemCategory::Preference, value, 75, Some(60))
        }));
        evolving_items.extend(
            self.evolving.likes.into_iter().map(|value| {
                compact_identity_item(IdentityItemCategory::Like, value, 70, Some(50))
            }),
        );
        evolving_items.extend(self.evolving.dislikes.into_iter().map(|value| {
            compact_identity_item(IdentityItemCategory::Dislike, value, 70, Some(50))
        }));
        evolving_items.extend(
            self.evolving.habits.into_iter().map(|value| {
                compact_identity_item(IdentityItemCategory::Habit, value, 65, Some(45))
            }),
        );
        evolving_items.extend(self.evolving.routines.into_iter().map(|value| {
            compact_identity_item(IdentityItemCategory::Routine, value, 65, Some(45))
        }));
        evolving_items.extend(self.evolving.learned_tendencies.into_iter().map(|value| {
            compact_identity_item(IdentityItemCategory::LearnedTendency, value, 65, Some(45))
        }));
        evolving_items.extend(self.evolving.autobiographical_refinements.into_iter().map(
            |value| {
                compact_identity_item(
                    IdentityItemCategory::AutobiographicalRefinement,
                    value,
                    65,
                    Some(45),
                )
            },
        ));
        evolving_items.extend(
            self.evolving
                .recurring_self_descriptions
                .into_iter()
                .map(|value| {
                    compact_identity_item(
                        IdentityItemCategory::RecurringSelfDescription,
                        value,
                        70,
                        Some(50),
                    )
                }),
        );
        evolving_items.extend(self.evolving.interaction_style_adaptations.into_iter().map(
            |value| {
                compact_identity_item(
                    IdentityItemCategory::InteractionStyleAdaptation,
                    value,
                    70,
                    Some(55),
                )
            },
        ));
        evolving_items.extend(
            self.evolving.goals.into_iter().map(|value| {
                compact_identity_item(IdentityItemCategory::Goal, value, 80, Some(70))
            }),
        );
        evolving_items.extend(self.evolving.subgoals.into_iter().map(|value| {
            compact_identity_item(IdentityItemCategory::Subgoal, value, 75, Some(60))
        }));

        let identity_summary = self
            .compact_self_description
            .clone()
            .or_else(|| {
                stable_items
                    .iter()
                    .find(|item| item.category == IdentityItemCategory::Name)
                    .map(|item| item.value.clone())
            })
            .unwrap_or_else(|| "Bootstrap identity seed".to_string());

        CompactIdentitySnapshot {
            identity_summary,
            stable_items,
            evolving_items,
            values,
            boundaries,
            self_description: self.compact_self_description,
        }
    }
}

fn compact_identity_item(
    category: IdentityItemCategory,
    value: String,
    confidence_pct: u8,
    weight_pct: Option<u8>,
) -> CompactIdentityItem {
    CompactIdentityItem {
        category,
        value,
        confidence_pct,
        weight_pct,
    }
}

#[cfg(test)]
mod tests {
    use std::{env, path::PathBuf};

    use chrono::Utc;

    use super::*;
    use crate::config::{
        AppConfig, ApprovalPromptMode, ApprovalsConfig, BackgroundConfig,
        BackgroundExecutionConfig, BackgroundSchedulerConfig, BackgroundThresholdsConfig,
        BacklogRecoveryConfig, ContinuityConfig, DatabaseConfig, GovernedActionsConfig,
        HarnessConfig, ObservabilityConfig, RetrievalConfig, ScheduledForegroundConfig,
        SelfModelConfig, TelegramConfig, WakeSignalPolicyConfig, WorkerConfig, WorkspaceConfig,
    };
    use uuid::Uuid;

    fn sample_config(seed_path: PathBuf) -> RuntimeConfig {
        RuntimeConfig {
            app: AppConfig {
                name: "blue-lagoon".to_string(),
                log_filter: "info".to_string(),
            },
            database: DatabaseConfig {
                database_url: "postgres://example".to_string(),
                minimum_supported_schema_version: 1,
            },
            harness: HarnessConfig {
                allow_synthetic_smoke: true,
                default_foreground_iteration_budget: 1,
                default_wall_clock_budget_ms: 30_000,
                default_foreground_token_budget: 4_000,
            },
            background: BackgroundConfig {
                scheduler: BackgroundSchedulerConfig {
                    poll_interval_seconds: 300,
                    max_due_jobs_per_iteration: 4,
                    lease_timeout_ms: 300_000,
                },
                thresholds: BackgroundThresholdsConfig {
                    episode_backlog_threshold: 25,
                    candidate_memory_threshold: 10,
                    contradiction_alert_threshold: 3,
                },
                execution: BackgroundExecutionConfig {
                    default_iteration_budget: 2,
                    default_wall_clock_budget_ms: 120_000,
                    default_token_budget: 6_000,
                },
                wake_signals: WakeSignalPolicyConfig {
                    allow_foreground_conversion: true,
                    max_pending_signals: 8,
                    cooldown_seconds: 900,
                },
            },
            continuity: ContinuityConfig {
                retrieval: RetrievalConfig {
                    max_recent_episode_candidates: 3,
                    max_memory_artifact_candidates: 5,
                    max_context_items: 6,
                },
                backlog_recovery: BacklogRecoveryConfig {
                    pending_message_count_threshold: 3,
                    pending_message_span_seconds_threshold: 120,
                    stale_pending_ingress_age_seconds_threshold: 300,
                    max_recovery_batch_size: 8,
                },
            },
            scheduled_foreground: ScheduledForegroundConfig {
                enabled: true,
                max_due_tasks_per_iteration: 2,
                min_cadence_seconds: 300,
                default_cooldown_seconds: 300,
            },
            workspace: WorkspaceConfig {
                root_dir: ".".into(),
                max_artifact_bytes: 1_048_576,
                max_script_bytes: 262_144,
            },
            observability: ObservabilityConfig {
                model_call_payload_retention_days: 30,
            },
            approvals: ApprovalsConfig {
                default_ttl_seconds: 900,
                max_pending_requests: 32,
                allow_cli_resolution: true,
                prompt_mode: ApprovalPromptMode::InlineKeyboardWithFallback,
            },
            governed_actions: GovernedActionsConfig {
                approval_required_min_risk_tier: contracts::GovernedActionRiskTier::Tier2,
                default_subprocess_timeout_ms: 30_000,
                max_subprocess_timeout_ms: 120_000,
                max_actions_per_foreground_turn: 10,
                cap_exceeded_behavior: contracts::GovernedActionCapExceededBehavior::Escalate,
                max_filesystem_roots_per_action: 4,
                default_network_access: contracts::NetworkAccessPosture::Disabled,
                allowlisted_environment_variables: vec!["BLUE_LAGOON_DATABASE_URL".to_string()],
                max_environment_variables_per_action: 8,
                max_captured_output_bytes: 65_536,
                max_web_fetch_timeout_ms: 15_000,
                max_web_fetch_response_bytes: 524_288,
            },
            worker: WorkerConfig {
                timeout_ms: 5_000,
                command: String::new(),
                args: Vec::new(),
            },
            telegram: Some(TelegramConfig {
                api_base_url: "https://api.telegram.org".to_string(),
                bot_token_env: "BLUE_LAGOON_TEST_TELEGRAM_TOKEN".to_string(),
                poll_limit: 10,
                foreground_binding: Some(crate::config::TelegramForegroundBindingConfig {
                    allowed_user_id: 42,
                    allowed_chat_id: 42,
                    internal_principal_ref: "primary-user".to_string(),
                    internal_conversation_ref: "telegram-primary".to_string(),
                }),
            }),
            model_gateway: None,
            self_model: Some(SelfModelConfig { seed_path }),
        }
    }

    #[test]
    fn loads_self_model_seed_into_snapshot() {
        let config = sample_config(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("config")
                .join("self_model_seed.toml"),
        );

        let snapshot = load_seed_self_model_snapshot(&config).expect("seed should load");
        assert_eq!(snapshot.stable_identity, "blue-lagoon");
        assert_eq!(snapshot.role, "personal_assistant");
        assert!(snapshot.capabilities.contains(&"conversation".to_string()));
        assert!(
            snapshot
                .current_goals
                .contains(&"support_the_user".to_string())
        );
        let identity = snapshot.identity.expect("rich identity seed should load");
        assert!(
            identity
                .stable_items
                .iter()
                .any(|item| item.category == IdentityItemCategory::IdentityForm)
        );
        assert_eq!(
            snapshot.identity_lifecycle.state,
            IdentityLifecycleState::BootstrapSeedOnly
        );
        assert!(snapshot.identity_lifecycle.kickstart_available);
    }

    #[test]
    fn invalid_seed_is_rejected() {
        let temp_root = env::temp_dir().join(format!("blue-lagoon-self-model-{}", Uuid::now_v7()));
        fs::create_dir_all(&temp_root).expect("temp directory should exist");
        let seed_path = temp_root.join("invalid_seed.toml");
        fs::write(
            &seed_path,
            "stable_identity = ''\nrole = 'assistant'\ncommunication_style = 'direct'\ncapabilities = []\nconstraints = []\npreferences = []\ncurrent_goals = []\ncurrent_subgoals = []\n",
        )
        .expect("invalid seed should be written");

        let error = load_seed_self_model_snapshot(&sample_config(seed_path.clone()))
            .expect_err("invalid seed should fail");
        assert!(error.to_string().contains("stable_identity"));

        let _ = fs::remove_dir_all(temp_root);
    }

    #[test]
    fn builds_internal_state_snapshot_from_seed() {
        let snapshot = build_internal_state_snapshot(
            InternalStateSeed {
                load_pct: 20,
                health_pct: 95,
                reliability_pct: 90,
                resource_pressure_pct: 30,
                confidence_pct: 70,
                connection_quality_pct: 85,
            },
            vec!["degraded_network".to_string()],
        );

        assert_eq!(snapshot.load_pct, 20);
        assert_eq!(snapshot.health_pct, 95);
        assert_eq!(
            snapshot.active_conditions,
            vec!["degraded_network".to_string()]
        );
    }

    #[test]
    fn live_internal_state_signals_adjust_bounded_metrics() {
        let snapshot = build_internal_state_from_signals(
            InternalStateSeed {
                load_pct: 20,
                health_pct: 95,
                reliability_pct: 90,
                resource_pressure_pct: 30,
                confidence_pct: 70,
                connection_quality_pct: 85,
            },
            vec!["postgres_ready".to_string()],
            InternalStateSignals {
                active_background_runs: 2,
                pending_approvals: 3,
                pending_wake_signals: 1,
                recent_failed_runs: 1,
                recent_critical_diagnostics: 2,
            },
        );

        assert_eq!(snapshot.load_pct, 59);
        assert_eq!(snapshot.resource_pressure_pct, 60);
        assert_eq!(snapshot.reliability_pct, 62);
        assert_eq!(snapshot.health_pct, 67);
        assert_eq!(snapshot.confidence_pct, 56);
        assert_eq!(snapshot.connection_quality_pct, 75);
        assert!(
            snapshot
                .active_conditions
                .contains(&"pending_approvals:3".to_string())
        );
        assert!(
            snapshot
                .active_conditions
                .contains(&"recent_critical_diagnostics:2".to_string())
        );
    }

    #[test]
    fn compact_views_serialize_for_conscious_context() {
        let self_model = SelfModelSnapshot {
            stable_identity: "blue-lagoon".to_string(),
            role: "personal_assistant".to_string(),
            communication_style: "direct".to_string(),
            capabilities: vec!["conversation".to_string()],
            constraints: vec!["respect_harness_policy".to_string()],
            preferences: vec!["concise".to_string()],
            current_goals: vec!["support_the_user".to_string()],
            current_subgoals: vec![],
            identity: None,
            identity_lifecycle: Default::default(),
        };
        let internal_state = build_internal_state_snapshot(InternalStateSeed::default(), vec![]);

        let self_model_view =
            compact_self_model_view(&self_model).expect("self-model should serialize");
        let internal_state_view =
            compact_internal_state_view(&internal_state).expect("state should serialize");

        assert_eq!(
            self_model_view
                .get("stable_identity")
                .and_then(Value::as_str),
            Some("blue-lagoon")
        );
        assert_eq!(
            internal_state_view.get("load_pct").and_then(Value::as_u64),
            Some(10)
        );
    }

    #[test]
    fn select_single_active_artifact_prefers_the_only_active_row() {
        let artifact = sample_self_model_artifact("active");
        let selected = select_single_active_artifact(std::slice::from_ref(&artifact))
            .expect("selection should succeed")
            .expect("artifact should be present");
        assert_eq!(
            selected.self_model_artifact_id,
            artifact.self_model_artifact_id
        );
    }

    #[test]
    fn select_single_active_artifact_rejects_multiple_active_rows() {
        let artifacts = vec![
            sample_self_model_artifact("active"),
            sample_self_model_artifact("active"),
        ];
        let error = select_single_active_artifact(&artifacts)
            .expect_err("multiple active rows should fail closed");
        assert!(error.to_string().contains("multiple active"));
    }

    #[test]
    fn snapshot_from_canonical_artifact_rejects_invalid_state() {
        let mut artifact = sample_self_model_artifact("active");
        artifact.capabilities.clear();
        let error = snapshot_from_canonical_artifact(&artifact)
            .expect_err("invalid canonical artifact should fail");
        assert!(error.to_string().contains("capabilities"));
    }

    fn sample_self_model_artifact(status: &str) -> SelfModelArtifactRecord {
        SelfModelArtifactRecord {
            self_model_artifact_id: Uuid::now_v7(),
            proposal_id: None,
            trace_id: Some(Uuid::now_v7()),
            execution_id: Some(Uuid::now_v7()),
            episode_id: Some(Uuid::now_v7()),
            artifact_origin: "bootstrap_seed".to_string(),
            status: status.to_string(),
            stable_identity: "blue-lagoon".to_string(),
            role: "personal_assistant".to_string(),
            communication_style: "direct".to_string(),
            capabilities: vec!["conversation".to_string()],
            constraints: vec!["respect_harness_policy".to_string()],
            preferences: vec!["concise".to_string()],
            current_goals: vec!["support_the_user".to_string()],
            current_subgoals: vec!["preserve_continuity".to_string()],
            superseded_at: None,
            superseded_by_artifact_id: None,
            supersedes_artifact_id: None,
            created_at: Utc::now(),
            payload: serde_json::json!({}),
        }
    }

    #[test]
    fn merge_self_model_observation_appends_preferences_for_interaction_style() {
        let snapshot = sample_self_model_artifact("active");
        let merged = merge_self_model_observation(
            snapshot_from_canonical_artifact(&snapshot).expect("snapshot should decode"),
            &SelfModelObservationProposal {
                observation_kind: "interaction_style".to_string(),
                content_text: "Prefer concise progress updates.".to_string(),
            },
        );

        assert!(
            merged
                .preferences
                .contains(&"Prefer concise progress updates.".to_string())
        );
    }

    #[test]
    fn merge_self_model_observation_routes_subgoals_to_current_subgoals() {
        let snapshot = sample_self_model_artifact("active");
        let merged = merge_self_model_observation(
            snapshot_from_canonical_artifact(&snapshot).expect("snapshot should decode"),
            &SelfModelObservationProposal {
                observation_kind: "subgoal".to_string(),
                content_text: "Monitor drift signals during maintenance.".to_string(),
            },
        );

        assert!(
            merged
                .current_subgoals
                .contains(&"Monitor drift signals during maintenance.".to_string())
        );
    }
}
