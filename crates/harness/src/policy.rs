use anyhow::{Result, bail};
use contracts::{ForegroundBudget, IngressEventKind, NormalizedIngress};

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

pub fn default_foreground_budget(config: &RuntimeConfig) -> ForegroundBudget {
    ForegroundBudget {
        iteration_budget: config.harness.default_foreground_iteration_budget,
        wall_clock_budget_ms: config.harness.default_wall_clock_budget_ms,
        token_budget: config.harness.default_foreground_token_budget,
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
        IngressEventKind::MessageCreated | IngressEventKind::CommandIssued => {}
        IngressEventKind::ApprovalCallback => {
            return PolicyDecision::Denied {
                reason: "approval callbacks are not yet supported as foreground Telegram triggers"
                    .to_string(),
            };
        }
    }

    match ingress.text_body.as_deref() {
        Some(text) if !text.trim().is_empty() => PolicyDecision::Allowed,
        _ => PolicyDecision::Denied {
            reason: "foreground Telegram triggers require a non-empty text body".to_string(),
        },
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use contracts::{ChannelKind, IngressEventKind, NormalizedIngress};

    use crate::config::{
        AppConfig, BacklogRecoveryConfig, ContinuityConfig, DatabaseConfig, HarnessConfig,
        ResolvedTelegramConfig, RetrievalConfig, WorkerConfig,
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
    fn telegram_foreground_policy_rejects_approval_callbacks() {
        let mut ingress = telegram_ingress();
        ingress.event_kind = IngressEventKind::ApprovalCallback;
        ingress.text_body = None;

        match evaluate_telegram_foreground_trigger(&telegram_config(), &ingress) {
            PolicyDecision::Allowed => panic!("approval callbacks should be rejected"),
            PolicyDecision::Denied { reason } => {
                assert!(reason.contains("approval callbacks"));
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
}
