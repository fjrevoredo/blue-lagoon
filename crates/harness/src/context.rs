use anyhow::{Result, bail};
use chrono::Utc;
use contracts::{
    ConsciousContext, EpisodeExcerpt, ForegroundRecoveryContext, ForegroundTrigger,
    IdentityKickstartActionKind, IdentityKickstartContext, IdentityLifecycleContext,
    IdentityLifecycleState, SelfModelSnapshot, predefined_identity_templates,
};
use serde::Serialize;
use tracing::info;
use uuid::Uuid;

use crate::{
    config::RuntimeConfig,
    foreground, identity, retrieval,
    self_model::{self, InternalStateSeed},
};

pub const DEFAULT_RECENT_HISTORY_LIMIT: i64 = 8;
pub const DEFAULT_TRIGGER_TEXT_CHAR_LIMIT: usize = 2_000;
pub const DEFAULT_HISTORY_MESSAGE_CHAR_LIMIT: usize = 400;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextAssemblyLimits {
    pub recent_history_limit: i64,
    pub trigger_text_char_limit: usize,
    pub history_message_char_limit: usize,
}

impl Default for ContextAssemblyLimits {
    fn default() -> Self {
        Self {
            recent_history_limit: DEFAULT_RECENT_HISTORY_LIMIT,
            trigger_text_char_limit: DEFAULT_TRIGGER_TEXT_CHAR_LIMIT,
            history_message_char_limit: DEFAULT_HISTORY_MESSAGE_CHAR_LIMIT,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContextAssemblyOptions {
    pub limits: ContextAssemblyLimits,
    pub internal_state_seed: InternalStateSeed,
    pub active_conditions: Vec<String>,
    pub episode_id: Option<Uuid>,
    pub recovery_context: ForegroundRecoveryContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextAssemblyMetadata {
    pub source_ingress_id: Uuid,
    pub foreground_execution_mode: String,
    pub recovery_ingress_count: usize,
    pub recovery_ingress_ids: Vec<Uuid>,
    pub self_model_seed_path: String,
    pub self_model_source_kind: String,
    pub self_model_canonical_artifact_id: Option<Uuid>,
    pub self_model_bootstrap_performed: bool,
    pub identity_lifecycle_state: String,
    pub identity_kickstart_available: bool,
    pub recent_history_limit: i64,
    pub selected_recent_history_count: usize,
    pub selected_recent_history_episode_ids: Vec<Uuid>,
    pub selected_retrieved_context_count: usize,
    pub selected_retrieved_context_item_ids: Vec<Uuid>,
    pub trigger_text_char_limit: usize,
    pub trigger_text_truncated: bool,
    pub history_message_char_limit: usize,
    pub truncated_history_message_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextAssemblyResult {
    pub context: ConsciousContext,
    pub metadata: ContextAssemblyMetadata,
}

pub async fn assemble_foreground_context(
    pool: &sqlx::PgPool,
    config: &RuntimeConfig,
    trigger: ForegroundTrigger,
    options: ContextAssemblyOptions,
) -> Result<ContextAssemblyResult> {
    options.limits.validate()?;

    let loaded_self_model = self_model::load_self_model_snapshot(
        pool,
        config,
        &self_model::SelfModelLoadContext {
            trace_id: trigger.trace_id,
            execution_id: trigger.execution_id,
            episode_id: options.episode_id,
        },
    )
    .await?;
    let mut self_model_snapshot = loaded_self_model.snapshot;
    apply_identity_lifecycle_context(pool, &mut self_model_snapshot).await?;

    let internal_state = self_model::derive_live_internal_state_snapshot(
        pool,
        options.internal_state_seed,
        options.active_conditions,
    )
    .await?;
    let (trigger, trigger_text_truncated) =
        shape_trigger(trigger, options.limits.trigger_text_char_limit);

    let recent_history = foreground::list_recent_episode_excerpts_before(
        pool,
        &trigger.ingress.internal_conversation_ref,
        trigger.received_at,
        options.limits.recent_history_limit,
    )
    .await?;
    let (recent_history, truncated_history_message_count) =
        shape_recent_history(recent_history, options.limits.history_message_char_limit);
    let retrieved_context = retrieval::assemble_retrieved_context(pool, config, &trigger).await?;

    let metadata = ContextAssemblyMetadata {
        source_ingress_id: trigger.ingress.ingress_id,
        foreground_execution_mode: match options.recovery_context.mode {
            contracts::ForegroundExecutionMode::SingleIngress => "single_ingress".to_string(),
            contracts::ForegroundExecutionMode::BacklogRecovery => "backlog_recovery".to_string(),
        },
        recovery_ingress_count: options.recovery_context.ordered_ingress.len(),
        recovery_ingress_ids: options
            .recovery_context
            .ordered_ingress
            .iter()
            .map(|ingress| ingress.ingress_id)
            .collect(),
        self_model_seed_path: loaded_self_model.seed_path.clone(),
        self_model_source_kind: match loaded_self_model.source_kind {
            self_model::SelfModelSourceKind::BootstrapSeed => "bootstrap_seed".to_string(),
            self_model::SelfModelSourceKind::CanonicalArtifact => "canonical_artifact".to_string(),
        },
        self_model_canonical_artifact_id: loaded_self_model.canonical_artifact_id,
        self_model_bootstrap_performed: loaded_self_model.bootstrap_performed,
        identity_lifecycle_state: identity_lifecycle_state_as_str(
            self_model_snapshot.identity_lifecycle.state,
        )
        .to_string(),
        identity_kickstart_available: self_model_snapshot.identity_lifecycle.kickstart_available,
        recent_history_limit: options.limits.recent_history_limit,
        selected_recent_history_count: recent_history.len(),
        selected_recent_history_episode_ids: recent_history
            .iter()
            .map(|episode| episode.episode_id)
            .collect(),
        selected_retrieved_context_count: retrieved_context.items.len(),
        selected_retrieved_context_item_ids: retrieved_context
            .items
            .iter()
            .map(retrieved_context_item_id)
            .collect(),
        trigger_text_char_limit: options.limits.trigger_text_char_limit,
        trigger_text_truncated,
        history_message_char_limit: options.limits.history_message_char_limit,
        truncated_history_message_count,
    };

    info!(
        source_ingress_id = %metadata.source_ingress_id,
        foreground_execution_mode = %metadata.foreground_execution_mode,
        recent_history_limit = metadata.recent_history_limit,
        selected_recent_history_count = metadata.selected_recent_history_count,
        selected_recent_history_episode_ids = ?metadata.selected_recent_history_episode_ids,
        selected_retrieved_context_count = metadata.selected_retrieved_context_count,
        selected_retrieved_context_item_ids = ?metadata.selected_retrieved_context_item_ids,
        truncated_history_message_count = metadata.truncated_history_message_count,
        trigger_text_truncated = metadata.trigger_text_truncated,
        "assembled foreground context"
    );

    Ok(ContextAssemblyResult {
        context: ConsciousContext {
            context_id: Uuid::now_v7(),
            assembled_at: Utc::now(),
            trigger,
            self_model: self_model_snapshot,
            internal_state,
            recent_history,
            retrieved_context,
            governed_action_observations: Vec::new(),
            recovery_context: options.recovery_context,
        },
        metadata,
    })
}

impl ContextAssemblyLimits {
    fn validate(&self) -> Result<()> {
        if self.recent_history_limit <= 0 {
            bail!("context recent_history_limit must be greater than zero");
        }
        if self.trigger_text_char_limit == 0 {
            bail!("context trigger_text_char_limit must be greater than zero");
        }
        if self.history_message_char_limit == 0 {
            bail!("context history_message_char_limit must be greater than zero");
        }
        Ok(())
    }
}

async fn apply_identity_lifecycle_context(
    pool: &sqlx::PgPool,
    snapshot: &mut SelfModelSnapshot,
) -> Result<()> {
    let Some(lifecycle) = identity::get_current_lifecycle(pool).await? else {
        if snapshot.identity_lifecycle.state == IdentityLifecycleState::BootstrapSeedOnly {
            snapshot.identity_lifecycle =
                identity_lifecycle_context(IdentityLifecycleState::BootstrapSeedOnly, None);
        }
        return Ok(());
    };

    let state = identity_lifecycle_state_from_str(&lifecycle.lifecycle_state)?;
    snapshot.identity_lifecycle = identity_lifecycle_context(state, lifecycle.active_interview_id);
    if state == IdentityLifecycleState::IdentityKickstartInProgress
        && let Some(interview_id) = lifecycle.active_interview_id
        && let Some(interview) = identity::get_identity_interview(pool, interview_id).await?
        && let Some(kickstart) = &mut snapshot.identity_lifecycle.kickstart
    {
        kickstart.next_step = Some(interview.current_step);
        kickstart.resume_summary = Some(format!(
            "custom identity interview {} is waiting for the next answer",
            interview.identity_interview_id
        ));
    }

    if state == IdentityLifecycleState::CompleteIdentityActive {
        let compact_identity = identity::reconstruct_compact_identity_snapshot(pool, 32).await?;
        if !compact_identity.stable_items.is_empty()
            || !compact_identity.evolving_items.is_empty()
            || compact_identity.self_description.is_some()
        {
            snapshot.identity = Some(compact_identity);
        }
    }

    Ok(())
}

fn identity_lifecycle_context(
    state: IdentityLifecycleState,
    active_interview_id: Option<Uuid>,
) -> IdentityLifecycleContext {
    let kickstart = match state {
        IdentityLifecycleState::BootstrapSeedOnly => Some(IdentityKickstartContext {
            available_actions: vec![
                IdentityKickstartActionKind::SelectPredefinedTemplate,
                IdentityKickstartActionKind::StartCustomInterview,
            ],
            next_step: Some("choose_predefined_identity_or_start_custom_interview".to_string()),
            resume_summary: None,
            predefined_templates: predefined_identity_templates(),
        }),
        IdentityLifecycleState::IdentityKickstartInProgress => Some(IdentityKickstartContext {
            available_actions: vec![
                IdentityKickstartActionKind::AnswerCustomInterview,
                IdentityKickstartActionKind::Cancel,
            ],
            next_step: Some("resume_identity_interview".to_string()),
            resume_summary: active_interview_id
                .map(|interview_id| format!("identity interview {interview_id} is in progress")),
            predefined_templates: Vec::new(),
        }),
        IdentityLifecycleState::CompleteIdentityActive
        | IdentityLifecycleState::IdentityResetPending => None,
    };

    IdentityLifecycleContext {
        state,
        kickstart_available: kickstart.is_some(),
        kickstart,
    }
}

fn identity_lifecycle_state_from_str(value: &str) -> Result<IdentityLifecycleState> {
    match value {
        "bootstrap_seed_only" => Ok(IdentityLifecycleState::BootstrapSeedOnly),
        "identity_kickstart_in_progress" => Ok(IdentityLifecycleState::IdentityKickstartInProgress),
        "complete_identity_active" => Ok(IdentityLifecycleState::CompleteIdentityActive),
        "identity_reset_pending" => Ok(IdentityLifecycleState::IdentityResetPending),
        _ => bail!("unknown identity lifecycle state: {value}"),
    }
}

fn identity_lifecycle_state_as_str(state: IdentityLifecycleState) -> &'static str {
    match state {
        IdentityLifecycleState::BootstrapSeedOnly => "bootstrap_seed_only",
        IdentityLifecycleState::IdentityKickstartInProgress => "identity_kickstart_in_progress",
        IdentityLifecycleState::CompleteIdentityActive => "complete_identity_active",
        IdentityLifecycleState::IdentityResetPending => "identity_reset_pending",
    }
}

fn shape_trigger(
    mut trigger: ForegroundTrigger,
    trigger_text_char_limit: usize,
) -> (ForegroundTrigger, bool) {
    let Some(text_body) = trigger.ingress.text_body.clone() else {
        return (trigger, false);
    };

    let (text_body, truncated) = truncate_text(&text_body, trigger_text_char_limit);
    trigger.ingress.text_body = Some(text_body);
    (trigger, truncated)
}

fn shape_recent_history(
    recent_history: Vec<EpisodeExcerpt>,
    history_message_char_limit: usize,
) -> (Vec<EpisodeExcerpt>, u32) {
    let mut truncated_count = 0_u32;
    let shaped = recent_history
        .into_iter()
        .map(|mut episode| {
            if let Some(text) = &episode.user_message {
                let (truncated, did_truncate) = truncate_text(text, history_message_char_limit);
                if did_truncate {
                    truncated_count += 1;
                }
                episode.user_message = Some(truncated);
            }

            if let Some(text) = &episode.assistant_message {
                let (truncated, did_truncate) = truncate_text(text, history_message_char_limit);
                if did_truncate {
                    truncated_count += 1;
                }
                episode.assistant_message = Some(truncated);
            }

            episode
        })
        .collect();

    (shaped, truncated_count)
}

fn truncate_text(value: &str, max_chars: usize) -> (String, bool) {
    let char_count = value.chars().count();
    if char_count <= max_chars {
        return (value.to_string(), false);
    }

    (value.chars().take(max_chars).collect(), true)
}

fn retrieved_context_item_id(item: &contracts::RetrievedContextItem) -> Uuid {
    match item {
        contracts::RetrievedContextItem::Episode(episode) => episode.episode_id,
        contracts::RetrievedContextItem::MemoryArtifact(artifact) => artifact.memory_artifact_id,
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};
    use contracts::{
        ChannelKind, EpisodeExcerpt, ForegroundBudget, ForegroundTrigger, ForegroundTriggerKind,
        IngressEventKind, NormalizedIngress,
    };

    use super::*;

    #[test]
    fn context_limits_reject_zero_values() {
        let error = ContextAssemblyLimits {
            recent_history_limit: 0,
            ..ContextAssemblyLimits::default()
        }
        .validate()
        .expect_err("zero recent history limit should fail");
        assert!(error.to_string().contains("recent_history_limit"));

        let error = ContextAssemblyLimits {
            trigger_text_char_limit: 0,
            ..ContextAssemblyLimits::default()
        }
        .validate()
        .expect_err("zero trigger text limit should fail");
        assert!(error.to_string().contains("trigger_text_char_limit"));

        let error = ContextAssemblyLimits {
            history_message_char_limit: 0,
            ..ContextAssemblyLimits::default()
        }
        .validate()
        .expect_err("zero history message limit should fail");
        assert!(error.to_string().contains("history_message_char_limit"));
    }

    #[test]
    fn default_context_limits_keep_more_than_short_approval_window() {
        assert_eq!(ContextAssemblyLimits::default().recent_history_limit, 8);
    }

    #[test]
    fn shape_trigger_truncates_text_body_to_limit() {
        let trigger = sample_trigger(Some("abcdefghijklmnopqrstuvwxyz"));
        let (shaped, truncated) = shape_trigger(trigger, 8);

        assert!(truncated);
        assert_eq!(shaped.trigger_kind, ForegroundTriggerKind::UserIngress,);
        assert_eq!(shaped.ingress.text_body.as_deref(), Some("abcdefgh"));
    }

    #[test]
    fn shape_recent_history_truncates_message_bodies() {
        let (recent_history, truncated_count) = shape_recent_history(
            vec![EpisodeExcerpt {
                episode_id: Uuid::now_v7(),
                trace_id: Uuid::now_v7(),
                started_at: Utc::now(),
                user_message: Some("1234567890".to_string()),
                assistant_message: Some("abcdefghij".to_string()),
                outcome: "completed".to_string(),
            }],
            5,
        );

        assert_eq!(recent_history.len(), 1);
        assert_eq!(recent_history[0].user_message.as_deref(), Some("12345"));
        assert_eq!(
            recent_history[0].assistant_message.as_deref(),
            Some("abcde")
        );
        assert_eq!(truncated_count, 2);
    }

    fn sample_trigger(text_body: Option<&str>) -> ForegroundTrigger {
        ForegroundTrigger {
            trigger_id: Uuid::now_v7(),
            trace_id: Uuid::now_v7(),
            execution_id: Uuid::now_v7(),
            trigger_kind: ForegroundTriggerKind::UserIngress,
            ingress: NormalizedIngress {
                ingress_id: Uuid::now_v7(),
                channel_kind: ChannelKind::Telegram,
                external_user_id: "42".to_string(),
                external_conversation_id: "42".to_string(),
                external_event_id: "update-42".to_string(),
                external_message_id: Some("message-42".to_string()),
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
                event_kind: IngressEventKind::MessageCreated,
                occurred_at: Utc::now() - Duration::seconds(5),
                text_body: text_body.map(ToString::to_string),
                reply_to: None,
                attachments: Vec::new(),
                command_hint: None,
                approval_payload: None,
                raw_payload_ref: None,
            },
            received_at: Utc::now(),
            deduplication_key: "telegram:update-42".to_string(),
            budget: ForegroundBudget {
                iteration_budget: 1,
                wall_clock_budget_ms: 30_000,
                token_budget: 4_000,
            },
        }
    }
}
