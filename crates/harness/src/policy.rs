use anyhow::{Result, bail};

use crate::config::RuntimeConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionBudget {
    pub wall_clock_budget_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    Allowed,
    Denied { reason: String },
}

pub fn default_budget(config: &RuntimeConfig) -> ExecutionBudget {
    ExecutionBudget {
        wall_clock_budget_ms: config.harness.default_wall_clock_budget_ms,
    }
}

pub fn validate_budget(budget: ExecutionBudget) -> Result<()> {
    if budget.wall_clock_budget_ms == 0 {
        bail!("wall-clock budget must be greater than zero");
    }
    Ok(())
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
    use crate::config::{AppConfig, DatabaseConfig, HarnessConfig, WorkerConfig};

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
                default_wall_clock_budget_ms: 30_000,
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
}
