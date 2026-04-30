use anyhow::{Result, bail};
use contracts::{
    BackgroundExecutionBudget, ForegroundBudget, GovernedActionKind, GovernedActionProposal,
    GovernedActionRiskTier, IngressEventKind, NetworkAccessPosture, NormalizedIngress, WakeSignal,
    WakeSignalDecision, WakeSignalDecisionKind, WakeSignalPriority,
};

use crate::config::{ResolvedModelGatewayConfig, ResolvedTelegramConfig, RuntimeConfig};

pub const FOREGROUND_WORKER_TIMEOUT_GRACE_MS: u64 = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionBudget {
    pub wall_clock_budget_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    Allowed,
    Denied { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WakeSignalEvaluationContext {
    pub pending_signal_count: u32,
    pub cooldown_active: bool,
    pub foreground_channel_available: bool,
}

pub fn default_foreground_budget(config: &RuntimeConfig) -> ForegroundBudget {
    ForegroundBudget {
        iteration_budget: config.harness.default_foreground_iteration_budget,
        wall_clock_budget_ms: config.harness.default_wall_clock_budget_ms,
        token_budget: config.harness.default_foreground_token_budget,
    }
}

pub fn default_background_budget(config: &RuntimeConfig) -> BackgroundExecutionBudget {
    BackgroundExecutionBudget {
        iteration_budget: config.background.execution.default_iteration_budget,
        wall_clock_budget_ms: config.background.execution.default_wall_clock_budget_ms,
        token_budget: config.background.execution.default_token_budget,
    }
}

pub fn default_budget(config: &RuntimeConfig) -> ExecutionBudget {
    ExecutionBudget {
        wall_clock_budget_ms: config.harness.default_wall_clock_budget_ms,
    }
}

pub fn effective_foreground_model_timeout_ms(
    config: &RuntimeConfig,
    gateway: &ResolvedModelGatewayConfig,
) -> u64 {
    config
        .harness
        .default_wall_clock_budget_ms
        .min(gateway.foreground.timeout_ms)
}

pub fn effective_foreground_worker_timeout_ms(config: &RuntimeConfig) -> u64 {
    config
        .harness
        .default_wall_clock_budget_ms
        .saturating_add(FOREGROUND_WORKER_TIMEOUT_GRACE_MS)
}

pub fn validate_budget(budget: ExecutionBudget) -> Result<()> {
    if budget.wall_clock_budget_ms == 0 {
        bail!("wall-clock budget must be greater than zero");
    }
    Ok(())
}

pub fn validate_foreground_budget(budget: &ForegroundBudget) -> Result<()> {
    if budget.iteration_budget == 0 {
        bail!("foreground iteration budget must be greater than zero");
    }
    if budget.wall_clock_budget_ms == 0 {
        bail!("foreground wall-clock budget must be greater than zero");
    }
    if budget.token_budget == 0 {
        bail!("foreground token budget must be greater than zero");
    }
    Ok(())
}

pub fn validate_background_budget(budget: &BackgroundExecutionBudget) -> Result<()> {
    if budget.iteration_budget == 0 {
        bail!("background iteration budget must be greater than zero");
    }
    if budget.wall_clock_budget_ms == 0 {
        bail!("background wall-clock budget must be greater than zero");
    }
    if budget.token_budget == 0 {
        bail!("background token budget must be greater than zero");
    }
    Ok(())
}

pub fn evaluate_telegram_foreground_trigger(
    config: &ResolvedTelegramConfig,
    ingress: &NormalizedIngress,
) -> PolicyDecision {
    if ingress.external_user_id != config.allowed_user_id.to_string() {
        return PolicyDecision::Denied {
            reason: "Telegram ingress actor does not match the configured single-user boundary"
                .to_string(),
        };
    }

    if ingress.external_conversation_id != config.allowed_chat_id.to_string() {
        return PolicyDecision::Denied {
            reason:
                "Telegram ingress conversation does not match the configured conversation boundary"
                    .to_string(),
        };
    }

    if ingress.internal_principal_ref != config.internal_principal_ref {
        return PolicyDecision::Denied {
            reason: "Telegram ingress principal binding does not match configured policy"
                .to_string(),
        };
    }

    if ingress.internal_conversation_ref != config.internal_conversation_ref {
        return PolicyDecision::Denied {
            reason:
                "Telegram ingress internal conversation binding does not match configured policy"
                    .to_string(),
        };
    }

    match ingress.event_kind {
        IngressEventKind::ApprovalCallback => {
            if ingress.approval_payload.is_some() {
                PolicyDecision::Allowed
            } else {
                PolicyDecision::Denied {
                    reason: "approval callbacks require approval payload metadata".to_string(),
                }
            }
        }
        IngressEventKind::MessageCreated | IngressEventKind::CommandIssued => {
            match ingress.text_body.as_deref() {
                Some(text) if !text.trim().is_empty() => PolicyDecision::Allowed,
                _ => PolicyDecision::Denied {
                    reason: "foreground Telegram triggers require a non-empty text body"
                        .to_string(),
                },
            }
        }
    }
}

pub fn evaluate_synthetic_smoke(config: &RuntimeConfig) -> PolicyDecision {
    if config.harness.allow_synthetic_smoke {
        PolicyDecision::Allowed
    } else {
        PolicyDecision::Denied {
            reason: "synthetic smoke trigger is disabled by policy".to_string(),
        }
    }
}

pub fn classify_governed_action_risk(proposal: &GovernedActionProposal) -> GovernedActionRiskTier {
    let has_write_scope = !proposal.capability_scope.filesystem.write_roots.is_empty();
    let has_env_scope = !proposal
        .capability_scope
        .environment
        .allow_variables
        .is_empty();
    let has_network = proposal.capability_scope.network != NetworkAccessPosture::Disabled;

    let intrinsic = match proposal.action_kind {
        GovernedActionKind::InspectWorkspaceArtifact
        | GovernedActionKind::ListWorkspaceArtifacts
        | GovernedActionKind::ListWorkspaceScripts
        | GovernedActionKind::InspectWorkspaceScript
        | GovernedActionKind::ListWorkspaceScriptRuns => GovernedActionRiskTier::Tier0,
        GovernedActionKind::CreateWorkspaceArtifact
        | GovernedActionKind::UpdateWorkspaceArtifact
        | GovernedActionKind::RequestBackgroundJob => GovernedActionRiskTier::Tier1,
        GovernedActionKind::CreateWorkspaceScript
        | GovernedActionKind::AppendWorkspaceScriptVersion
        | GovernedActionKind::UpsertScheduledForegroundTask => GovernedActionRiskTier::Tier2,
        GovernedActionKind::WebFetch => GovernedActionRiskTier::Tier2,
        GovernedActionKind::RunSubprocess | GovernedActionKind::RunWorkspaceScript => {
            if has_network && has_write_scope {
                GovernedActionRiskTier::Tier3
            } else if has_network || has_write_scope || has_env_scope {
                GovernedActionRiskTier::Tier2
            } else {
                GovernedActionRiskTier::Tier1
            }
        }
    };

    proposal
        .requested_risk_tier
        .filter(|requested| *requested > intrinsic)
        .unwrap_or(intrinsic)
}

pub fn governed_action_requires_approval(
    config: &RuntimeConfig,
    risk_tier: GovernedActionRiskTier,
) -> bool {
    risk_tier >= config.governed_actions.approval_required_min_risk_tier
}

pub fn evaluate_wake_signal(
    config: &RuntimeConfig,
    signal: &WakeSignal,
    context: WakeSignalEvaluationContext,
) -> WakeSignalDecision {
    if !context.foreground_channel_available {
        return WakeSignalDecision {
            signal_id: signal.signal_id,
            decision: WakeSignalDecisionKind::Rejected,
            reason: "no foreground conversation binding is configured for wake-signal conversion"
                .to_string(),
        };
    }

    if !config.background.wake_signals.allow_foreground_conversion {
        return WakeSignalDecision {
            signal_id: signal.signal_id,
            decision: WakeSignalDecisionKind::Deferred,
            reason: "wake-signal foreground conversion is disabled by policy".to_string(),
        };
    }

    if context.cooldown_active && signal.priority != WakeSignalPriority::High {
        return WakeSignalDecision {
            signal_id: signal.signal_id,
            decision: WakeSignalDecisionKind::Deferred,
            reason: format!(
                "wake signal '{}' remains within the active cooldown window",
                signal.reason_code
            ),
        };
    }

    if context.pending_signal_count >= config.background.wake_signals.max_pending_signals {
        return WakeSignalDecision {
            signal_id: signal.signal_id,
            decision: match signal.priority {
                WakeSignalPriority::Low => WakeSignalDecisionKind::Suppressed,
                WakeSignalPriority::Normal => WakeSignalDecisionKind::Deferred,
                WakeSignalPriority::High => WakeSignalDecisionKind::Accepted,
            },
            reason: format!(
                "wake-signal queue is at or above the configured limit ({})",
                config.background.wake_signals.max_pending_signals
            ),
        };
    }

    WakeSignalDecision {
        signal_id: signal.signal_id,
        decision: WakeSignalDecisionKind::Accepted,
        reason: "wake signal satisfies configured foreground conversion policy".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use contracts::{
        CapabilityScope, ChannelKind, EnvironmentCapabilityScope, ExecutionCapabilityBudget,
        FilesystemCapabilityScope, GovernedActionPayload, GovernedActionProposal, IngressEventKind,
        NormalizedIngress, SubprocessAction, WakeSignal, WakeSignalDecisionKind,
        WakeSignalPriority, WakeSignalReason,
    };

    use crate::config::{
        AppConfig, ApprovalPromptMode, ApprovalsConfig, BackgroundConfig,
        BackgroundExecutionConfig, BackgroundSchedulerConfig, BackgroundThresholdsConfig,
        BacklogRecoveryConfig, ContinuityConfig, DatabaseConfig, GovernedActionsConfig,
        HarnessConfig, ResolvedTelegramConfig, RetrievalConfig, ScheduledForegroundConfig,
        WakeSignalPolicyConfig, WorkerConfig, WorkspaceConfig,
    };

    fn config(allow_synthetic_smoke: bool) -> RuntimeConfig {
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
                allow_synthetic_smoke,
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
            observability: crate::config::ObservabilityConfig {
                model_call_payload_retention_days: 30,
            },
            approvals: ApprovalsConfig {
                default_ttl_seconds: 900,
                max_pending_requests: 32,
                allow_cli_resolution: true,
                prompt_mode: ApprovalPromptMode::InlineKeyboardWithFallback,
            },
            governed_actions: GovernedActionsConfig {
                approval_required_min_risk_tier: GovernedActionRiskTier::Tier2,
                default_subprocess_timeout_ms: 30_000,
                max_subprocess_timeout_ms: 120_000,
                max_filesystem_roots_per_action: 4,
                default_network_access: NetworkAccessPosture::Disabled,
                allowlisted_environment_variables: vec!["BLUE_LAGOON_DATABASE_URL".to_string()],
                max_environment_variables_per_action: 8,
                max_captured_output_bytes: 65_536,
                max_web_fetch_timeout_ms: 15_000,
                max_web_fetch_response_bytes: 524_288,
            },
            worker: WorkerConfig {
                timeout_ms: 10_000,
                command: String::new(),
                args: Vec::new(),
            },
            telegram: None,
            model_gateway: None,
            self_model: None,
        }
    }

    #[test]
    fn synthetic_smoke_can_be_allowed() {
        assert_eq!(
            evaluate_synthetic_smoke(&config(true)),
            PolicyDecision::Allowed
        );
    }

    #[test]
    fn synthetic_smoke_can_be_denied() {
        match evaluate_synthetic_smoke(&config(false)) {
            PolicyDecision::Allowed => panic!("policy should deny the trigger"),
            PolicyDecision::Denied { reason } => {
                assert!(reason.contains("disabled"));
            }
        }
    }

    #[test]
    fn validate_budget_rejects_zero() {
        let error = validate_budget(ExecutionBudget {
            wall_clock_budget_ms: 0,
        })
        .expect_err("budget should be rejected");
        assert!(error.to_string().contains("greater than zero"));
    }

    #[test]
    fn foreground_budget_uses_explicit_iteration_wall_clock_and_token_limits() {
        let budget = default_foreground_budget(&config(true));
        assert_eq!(budget.iteration_budget, 1);
        assert_eq!(budget.wall_clock_budget_ms, 30_000);
        assert_eq!(budget.token_budget, 4_000);
    }

    #[test]
    fn background_budget_uses_explicit_iteration_wall_clock_and_token_limits() {
        let budget = default_background_budget(&config(true));
        assert_eq!(budget.iteration_budget, 2);
        assert_eq!(budget.wall_clock_budget_ms, 120_000);
        assert_eq!(budget.token_budget, 6_000);
    }

    #[test]
    fn foreground_budget_validation_rejects_zero_fields() {
        let error = validate_foreground_budget(&ForegroundBudget {
            iteration_budget: 0,
            wall_clock_budget_ms: 30_000,
            token_budget: 4_000,
        })
        .expect_err("zero iteration budget should be rejected");
        assert!(error.to_string().contains("iteration"));

        let error = validate_foreground_budget(&ForegroundBudget {
            iteration_budget: 1,
            wall_clock_budget_ms: 0,
            token_budget: 4_000,
        })
        .expect_err("zero wall-clock budget should be rejected");
        assert!(error.to_string().contains("wall-clock"));

        let error = validate_foreground_budget(&ForegroundBudget {
            iteration_budget: 1,
            wall_clock_budget_ms: 30_000,
            token_budget: 0,
        })
        .expect_err("zero token budget should be rejected");
        assert!(error.to_string().contains("token"));
    }

    #[test]
    fn background_budget_validation_rejects_zero_fields() {
        let error = validate_background_budget(&BackgroundExecutionBudget {
            iteration_budget: 0,
            wall_clock_budget_ms: 120_000,
            token_budget: 6_000,
        })
        .expect_err("zero background iteration budget should be rejected");
        assert!(error.to_string().contains("iteration"));

        let error = validate_background_budget(&BackgroundExecutionBudget {
            iteration_budget: 2,
            wall_clock_budget_ms: 0,
            token_budget: 6_000,
        })
        .expect_err("zero background wall-clock budget should be rejected");
        assert!(error.to_string().contains("wall-clock"));

        let error = validate_background_budget(&BackgroundExecutionBudget {
            iteration_budget: 2,
            wall_clock_budget_ms: 120_000,
            token_budget: 0,
        })
        .expect_err("zero background token budget should be rejected");
        assert!(error.to_string().contains("token"));
    }

    #[test]
    fn foreground_model_timeout_is_clamped_to_harness_budget() {
        let timeout = effective_foreground_model_timeout_ms(
            &config(true),
            &crate::config::ResolvedModelGatewayConfig {
                foreground: crate::config::ResolvedForegroundModelRouteConfig {
                    provider: contracts::ModelProviderKind::ZAi,
                    model: "glm".to_string(),
                    api_base_url: "https://api.z.ai/api/paas/v4".to_string(),
                    api_key: "secret".to_string(),
                    timeout_ms: 45_000,
                },
            },
        );
        assert_eq!(timeout, 30_000);
    }

    #[test]
    fn foreground_worker_timeout_derives_from_harness_budget() {
        assert_eq!(
            effective_foreground_worker_timeout_ms(&config(true)),
            35_000
        );
    }

    #[test]
    fn telegram_foreground_policy_allows_private_text_ingress() {
        assert_eq!(
            evaluate_telegram_foreground_trigger(&telegram_config(), &telegram_ingress()),
            PolicyDecision::Allowed
        );
    }

    #[test]
    fn telegram_foreground_policy_allows_approval_callbacks_with_payload() {
        let mut ingress = telegram_ingress();
        ingress.event_kind = IngressEventKind::ApprovalCallback;
        ingress.text_body = None;
        ingress.approval_payload = Some(contracts::ApprovalPayload {
            token: "callback-query-id".to_string(),
            callback_data: Some("approve:approval-token".to_string()),
        });

        match evaluate_telegram_foreground_trigger(&telegram_config(), &ingress) {
            PolicyDecision::Allowed => {}
            PolicyDecision::Denied { reason } => {
                panic!("approval callbacks with payload should be allowed, got {reason}");
            }
        }
    }

    #[test]
    fn telegram_foreground_policy_rejects_approval_callbacks_without_payload() {
        let mut ingress = telegram_ingress();
        ingress.event_kind = IngressEventKind::ApprovalCallback;
        ingress.text_body = None;

        match evaluate_telegram_foreground_trigger(&telegram_config(), &ingress) {
            PolicyDecision::Allowed => panic!("payload-less approval callbacks should be rejected"),
            PolicyDecision::Denied { reason } => {
                assert!(reason.contains("approval payload"));
            }
        }
    }

    #[test]
    fn telegram_foreground_policy_rejects_empty_text() {
        let mut ingress = telegram_ingress();
        ingress.text_body = Some("   ".to_string());

        match evaluate_telegram_foreground_trigger(&telegram_config(), &ingress) {
            PolicyDecision::Allowed => panic!("empty text should be rejected"),
            PolicyDecision::Denied { reason } => {
                assert!(reason.contains("non-empty text body"));
            }
        }
    }

    #[test]
    fn telegram_foreground_policy_rejects_mismatched_actor() {
        let mut ingress = telegram_ingress();
        ingress.external_user_id = "99".to_string();

        match evaluate_telegram_foreground_trigger(&telegram_config(), &ingress) {
            PolicyDecision::Allowed => panic!("unauthorized actor should be rejected"),
            PolicyDecision::Denied { reason } => {
                assert!(reason.contains("single-user boundary"));
            }
        }
    }

    fn telegram_config() -> ResolvedTelegramConfig {
        ResolvedTelegramConfig {
            api_base_url: "https://api.telegram.org".to_string(),
            bot_token: "secret".to_string(),
            allowed_user_id: 42,
            allowed_chat_id: 24,
            internal_principal_ref: "primary-user".to_string(),
            internal_conversation_ref: "telegram-primary".to_string(),
            poll_limit: 10,
        }
    }

    fn telegram_ingress() -> NormalizedIngress {
        NormalizedIngress {
            ingress_id: uuid::Uuid::now_v7(),
            channel_kind: ChannelKind::Telegram,
            external_user_id: "42".to_string(),
            external_conversation_id: "24".to_string(),
            external_event_id: "update-42".to_string(),
            external_message_id: Some("message-42".to_string()),
            internal_principal_ref: "primary-user".to_string(),
            internal_conversation_ref: "telegram-primary".to_string(),
            event_kind: IngressEventKind::MessageCreated,
            occurred_at: Utc::now(),
            text_body: Some("hello".to_string()),
            reply_to: None,
            attachments: Vec::new(),
            command_hint: None,
            approval_payload: None,
            raw_payload_ref: None,
        }
    }

    fn sample_governed_action_proposal() -> GovernedActionProposal {
        GovernedActionProposal {
            proposal_id: uuid::Uuid::now_v7(),
            title: "Run scoped workspace command".to_string(),
            rationale: None,
            action_kind: GovernedActionKind::RunSubprocess,
            requested_risk_tier: None,
            capability_scope: CapabilityScope {
                filesystem: FilesystemCapabilityScope {
                    read_roots: vec!["D:/Repos/blue-lagoon".to_string()],
                    write_roots: Vec::new(),
                },
                network: NetworkAccessPosture::Disabled,
                environment: EnvironmentCapabilityScope {
                    allow_variables: Vec::new(),
                },
                execution: ExecutionCapabilityBudget {
                    timeout_ms: 30_000,
                    max_stdout_bytes: 65_536,
                    max_stderr_bytes: 32_768,
                },
            },
            payload: GovernedActionPayload::RunSubprocess(SubprocessAction {
                command: "cargo".to_string(),
                args: vec!["check".to_string()],
                working_directory: Some("D:/Repos/blue-lagoon".to_string()),
            }),
        }
    }

    fn wake_signal(priority: WakeSignalPriority) -> WakeSignal {
        WakeSignal {
            signal_id: uuid::Uuid::now_v7(),
            reason: WakeSignalReason::MaintenanceInsightReady,
            priority,
            reason_code: "maintenance_insight_ready".to_string(),
            summary: "Background maintenance produced a user-relevant insight.".to_string(),
            payload_ref: Some("background_job:123".to_string()),
        }
    }

    #[test]
    fn wake_signal_policy_accepts_nominal_signal() {
        let decision = evaluate_wake_signal(
            &config(true),
            &wake_signal(WakeSignalPriority::Normal),
            WakeSignalEvaluationContext {
                pending_signal_count: 1,
                cooldown_active: false,
                foreground_channel_available: true,
            },
        );
        assert_eq!(decision.decision, WakeSignalDecisionKind::Accepted);
    }

    #[test]
    fn wake_signal_policy_rejects_when_no_foreground_channel_is_available() {
        let decision = evaluate_wake_signal(
            &config(true),
            &wake_signal(WakeSignalPriority::Normal),
            WakeSignalEvaluationContext {
                pending_signal_count: 1,
                cooldown_active: false,
                foreground_channel_available: false,
            },
        );
        assert_eq!(decision.decision, WakeSignalDecisionKind::Rejected);
        assert!(
            decision
                .reason
                .contains("no foreground conversation binding")
        );
    }

    #[test]
    fn wake_signal_policy_defers_when_cooldown_is_active_for_non_high_priority() {
        let decision = evaluate_wake_signal(
            &config(true),
            &wake_signal(WakeSignalPriority::Normal),
            WakeSignalEvaluationContext {
                pending_signal_count: 1,
                cooldown_active: true,
                foreground_channel_available: true,
            },
        );
        assert_eq!(decision.decision, WakeSignalDecisionKind::Deferred);
        assert!(decision.reason.contains("cooldown"));
    }

    #[test]
    fn wake_signal_policy_allows_high_priority_signal_through_cooldown() {
        let decision = evaluate_wake_signal(
            &config(true),
            &wake_signal(WakeSignalPriority::High),
            WakeSignalEvaluationContext {
                pending_signal_count: 1,
                cooldown_active: true,
                foreground_channel_available: true,
            },
        );
        assert_eq!(decision.decision, WakeSignalDecisionKind::Accepted);
    }

    #[test]
    fn wake_signal_policy_suppresses_low_priority_signal_when_queue_is_full() {
        let decision = evaluate_wake_signal(
            &config(true),
            &wake_signal(WakeSignalPriority::Low),
            WakeSignalEvaluationContext {
                pending_signal_count: 8,
                cooldown_active: false,
                foreground_channel_available: true,
            },
        );
        assert_eq!(decision.decision, WakeSignalDecisionKind::Suppressed);
        assert!(decision.reason.contains("configured limit"));
    }

    #[test]
    fn governed_action_risk_classification_escalates_with_write_and_network_scope() {
        let proposal = sample_governed_action_proposal();
        assert_eq!(
            classify_governed_action_risk(&proposal),
            GovernedActionRiskTier::Tier1
        );

        let mut write_scoped = sample_governed_action_proposal();
        write_scoped
            .capability_scope
            .filesystem
            .write_roots
            .push("D:/Repos/blue-lagoon/docs".to_string());
        assert_eq!(
            classify_governed_action_risk(&write_scoped),
            GovernedActionRiskTier::Tier2
        );

        let mut high_risk = write_scoped.clone();
        high_risk.capability_scope.network = NetworkAccessPosture::Enabled;
        assert_eq!(
            classify_governed_action_risk(&high_risk),
            GovernedActionRiskTier::Tier3
        );
    }

    #[test]
    fn governed_action_approval_requirement_follows_configured_threshold() {
        let config = config(true);
        assert!(!governed_action_requires_approval(
            &config,
            GovernedActionRiskTier::Tier1
        ));
        assert!(governed_action_requires_approval(
            &config,
            GovernedActionRiskTier::Tier2
        ));
        assert!(governed_action_requires_approval(
            &config,
            GovernedActionRiskTier::Tier3
        ));
    }
}
