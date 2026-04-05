use std::fs;

use anyhow::{Context, Result, bail};
use contracts::{InternalStateSnapshot, SelfModelSnapshot};
use serde::Deserialize;
use serde_json::Value;

use crate::config::RuntimeConfig;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct SelfModelSeedDocument {
    stable_identity: String,
    role: String,
    communication_style: String,
    capabilities: Vec<String>,
    constraints: Vec<String>,
    preferences: Vec<String>,
    current_goals: Vec<String>,
    current_subgoals: Vec<String>,
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

pub fn load_self_model_snapshot(config: &RuntimeConfig) -> Result<SelfModelSnapshot> {
    let resolved = config.require_self_model_config()?;
    let raw = fs::read_to_string(&resolved.seed_path).with_context(|| {
        format!(
            "failed to read self-model seed artifact at {}",
            resolved.seed_path.display()
        )
    })?;
    let seed: SelfModelSeedDocument =
        toml::from_str(&raw).context("failed to parse self-model seed artifact as TOML")?;
    seed.validate()?;
    Ok(seed.into_snapshot())
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

pub fn compact_self_model_view(snapshot: &SelfModelSnapshot) -> Result<Value> {
    serde_json::to_value(snapshot).context("failed to serialize self-model snapshot")
}

pub fn compact_internal_state_view(snapshot: &InternalStateSnapshot) -> Result<Value> {
    serde_json::to_value(snapshot).context("failed to serialize internal-state snapshot")
}

impl SelfModelSeedDocument {
    fn validate(&self) -> Result<()> {
        if self.stable_identity.trim().is_empty() {
            bail!("self-model seed stable_identity must not be empty");
        }
        if self.role.trim().is_empty() {
            bail!("self-model seed role must not be empty");
        }
        if self.communication_style.trim().is_empty() {
            bail!("self-model seed communication_style must not be empty");
        }
        if self.capabilities.is_empty() {
            bail!("self-model seed capabilities must not be empty");
        }
        if self.current_goals.is_empty() {
            bail!("self-model seed current_goals must not be empty");
        }
        Ok(())
    }

    fn into_snapshot(self) -> SelfModelSnapshot {
        SelfModelSnapshot {
            stable_identity: self.stable_identity,
            role: self.role,
            communication_style: self.communication_style,
            capabilities: self.capabilities,
            constraints: self.constraints,
            preferences: self.preferences,
            current_goals: self.current_goals,
            current_subgoals: self.current_subgoals,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{env, path::PathBuf};

    use super::*;
    use crate::config::{
        AppConfig, DatabaseConfig, HarnessConfig, SelfModelConfig, TelegramConfig, WorkerConfig,
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
            worker: WorkerConfig {
                timeout_ms: 5_000,
                command: String::new(),
                args: Vec::new(),
            },
            telegram: Some(TelegramConfig {
                api_base_url: "https://api.telegram.org".to_string(),
                bot_token_env: "BLUE_LAGOON_TEST_TELEGRAM_TOKEN".to_string(),
                allowed_user_id: 42,
                allowed_chat_id: 42,
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
                poll_limit: 10,
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

        let snapshot = load_self_model_snapshot(&config).expect("seed should load");
        assert_eq!(snapshot.stable_identity, "blue-lagoon");
        assert_eq!(snapshot.role, "personal_assistant");
        assert!(snapshot.capabilities.contains(&"conversation".to_string()));
        assert!(
            snapshot
                .current_goals
                .contains(&"support_the_user".to_string())
        );
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

        let error = load_self_model_snapshot(&sample_config(seed_path.clone()))
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
}
