use std::{env, fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use contracts::ModelProviderKind;
use serde::Deserialize;

const DEFAULT_CONFIG_PATH: &str = "config/default.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeConfig {
    pub app: AppConfig,
    pub database: DatabaseConfig,
    pub harness: HarnessConfig,
    pub worker: WorkerConfig,
    pub telegram: Option<TelegramConfig>,
    pub model_gateway: Option<ModelGatewayConfig>,
    pub self_model: Option<SelfModelConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AppConfig {
    pub name: String,
    pub log_filter: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseConfig {
    pub database_url: String,
    pub minimum_supported_schema_version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct HarnessConfig {
    pub allow_synthetic_smoke: bool,
    pub default_wall_clock_budget_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WorkerConfig {
    pub timeout_ms: u64,
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TelegramConfig {
    pub api_base_url: String,
    pub bot_token_env: String,
    pub allowed_user_id: i64,
    pub allowed_chat_id: i64,
    pub internal_principal_ref: String,
    pub internal_conversation_ref: String,
    pub poll_limit: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTelegramConfig {
    pub api_base_url: String,
    pub bot_token: String,
    pub allowed_user_id: i64,
    pub allowed_chat_id: i64,
    pub internal_principal_ref: String,
    pub internal_conversation_ref: String,
    pub poll_limit: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ModelGatewayConfig {
    pub foreground: ForegroundModelRouteConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ForegroundModelRouteConfig {
    pub provider: ModelProviderKind,
    pub model: String,
    pub api_base_url: String,
    pub api_key_env: String,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModelGatewayConfig {
    pub foreground: ResolvedForegroundModelRouteConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedForegroundModelRouteConfig {
    pub provider: ModelProviderKind,
    pub model: String,
    pub api_base_url: String,
    pub api_key: String,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SelfModelConfig {
    pub seed_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSelfModelConfig {
    pub seed_path: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
struct FileConfig {
    app: AppConfig,
    database: FileDatabaseConfig,
    harness: HarnessConfig,
    worker: WorkerConfig,
    #[serde(default)]
    telegram: Option<TelegramConfig>,
    #[serde(default)]
    model_gateway: Option<ModelGatewayConfig>,
    #[serde(default)]
    self_model: Option<SelfModelConfig>,
}

#[derive(Debug, Clone, Deserialize)]
struct FileDatabaseConfig {
    minimum_supported_schema_version: i64,
}

impl RuntimeConfig {
    pub fn load() -> Result<Self> {
        let config_path =
            env::var("BLUE_LAGOON_CONFIG").unwrap_or_else(|_| DEFAULT_CONFIG_PATH.to_string());
        let file_contents = fs::read_to_string(&config_path)
            .with_context(|| format!("failed to read config file at {config_path}"))?;
        let file_config: FileConfig =
            toml::from_str(&file_contents).context("failed to parse config file as TOML")?;

        let database_url = env::var("BLUE_LAGOON_DATABASE_URL")
            .context("missing required environment variable BLUE_LAGOON_DATABASE_URL")?;
        if database_url.trim().is_empty() {
            bail!("BLUE_LAGOON_DATABASE_URL must not be empty");
        }

        let log_filter =
            env::var("BLUE_LAGOON_LOG").unwrap_or_else(|_| file_config.app.log_filter.clone());
        let worker_command = env::var("BLUE_LAGOON_WORKER_COMMAND")
            .unwrap_or_else(|_| file_config.worker.command.clone());
        let worker_timeout_ms = env::var("BLUE_LAGOON_WORKER_TIMEOUT_MS")
            .ok()
            .map(|value| value.parse::<u64>())
            .transpose()
            .context("BLUE_LAGOON_WORKER_TIMEOUT_MS must be an unsigned integer")?
            .unwrap_or(file_config.worker.timeout_ms);

        let worker_args = env::var("BLUE_LAGOON_WORKER_ARGS")
            .ok()
            .as_deref()
            .map(parse_worker_args_override)
            .transpose()?
            .unwrap_or_else(|| file_config.worker.args.clone());

        let config = Self {
            app: AppConfig {
                name: file_config.app.name,
                log_filter,
            },
            database: DatabaseConfig {
                database_url,
                minimum_supported_schema_version: file_config
                    .database
                    .minimum_supported_schema_version,
            },
            harness: file_config.harness,
            worker: WorkerConfig {
                timeout_ms: worker_timeout_ms,
                command: worker_command,
                args: worker_args,
            },
            telegram: file_config.telegram,
            model_gateway: file_config.model_gateway,
            self_model: file_config.self_model,
        };

        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if self.app.name.trim().is_empty() {
            bail!("app.name must not be empty");
        }
        if self.app.log_filter.trim().is_empty() {
            bail!("app.log_filter must not be empty");
        }
        if self.database.minimum_supported_schema_version <= 0 {
            bail!("database.minimum_supported_schema_version must be positive");
        }
        if self.harness.default_wall_clock_budget_ms == 0 {
            bail!("harness.default_wall_clock_budget_ms must be greater than zero");
        }
        if self.worker.timeout_ms == 0 {
            bail!("worker.timeout_ms must be greater than zero");
        }
        if let Some(telegram) = &self.telegram {
            telegram.validate()?;
        }
        if let Some(model_gateway) = &self.model_gateway {
            model_gateway.validate()?;
        }
        if let Some(self_model) = &self.self_model {
            self_model.validate()?;
        }
        Ok(())
    }

    pub fn config_path() -> PathBuf {
        env::var("BLUE_LAGOON_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_CONFIG_PATH))
    }

    pub fn require_telegram_config(&self) -> Result<ResolvedTelegramConfig> {
        let telegram = self
            .telegram
            .as_ref()
            .context("missing Phase 2 Telegram configuration")?;
        telegram.validate()?;

        Ok(ResolvedTelegramConfig {
            api_base_url: telegram.api_base_url.clone(),
            bot_token: require_secret_env(
                &telegram.bot_token_env,
                "Phase 2 Telegram bot token environment variable",
            )?,
            allowed_user_id: telegram.allowed_user_id,
            allowed_chat_id: telegram.allowed_chat_id,
            internal_principal_ref: telegram.internal_principal_ref.clone(),
            internal_conversation_ref: telegram.internal_conversation_ref.clone(),
            poll_limit: telegram.poll_limit,
        })
    }

    pub fn require_model_gateway_config(&self) -> Result<ResolvedModelGatewayConfig> {
        let model_gateway = self
            .model_gateway
            .as_ref()
            .context("missing Phase 2 model gateway configuration")?;
        model_gateway.validate()?;

        Ok(ResolvedModelGatewayConfig {
            foreground: ResolvedForegroundModelRouteConfig {
                provider: model_gateway.foreground.provider,
                model: model_gateway.foreground.model.clone(),
                api_base_url: model_gateway.foreground.api_base_url.clone(),
                api_key: require_secret_env(
                    &model_gateway.foreground.api_key_env,
                    "Phase 2 model gateway API key environment variable",
                )?,
                timeout_ms: model_gateway.foreground.timeout_ms,
            },
        })
    }

    pub fn require_self_model_config(&self) -> Result<ResolvedSelfModelConfig> {
        let self_model = self
            .self_model
            .as_ref()
            .context("missing Phase 2 self-model seed configuration")?;
        self_model.validate()?;

        let seed_path = resolve_relative_to_config(&self_model.seed_path);
        let metadata = fs::metadata(&seed_path).with_context(|| {
            format!(
                "failed to access Phase 2 self-model seed artifact at {}",
                seed_path.display()
            )
        })?;
        if !metadata.is_file() {
            bail!(
                "Phase 2 self-model seed artifact path is not a file: {}",
                seed_path.display()
            );
        }

        Ok(ResolvedSelfModelConfig { seed_path })
    }
}

fn parse_worker_args_override(raw: &str) -> Result<Vec<String>> {
    serde_json::from_str(raw).context("BLUE_LAGOON_WORKER_ARGS must be a JSON array of strings")
}

impl TelegramConfig {
    fn validate(&self) -> Result<()> {
        if self.api_base_url.trim().is_empty() {
            bail!("telegram.api_base_url must not be empty");
        }
        if self.bot_token_env.trim().is_empty() {
            bail!("telegram.bot_token_env must not be empty");
        }
        if self.allowed_user_id == 0 {
            bail!("telegram.allowed_user_id must not be zero");
        }
        if self.allowed_chat_id == 0 {
            bail!("telegram.allowed_chat_id must not be zero");
        }
        if self.internal_principal_ref.trim().is_empty() {
            bail!("telegram.internal_principal_ref must not be empty");
        }
        if self.internal_conversation_ref.trim().is_empty() {
            bail!("telegram.internal_conversation_ref must not be empty");
        }
        if self.poll_limit == 0 {
            bail!("telegram.poll_limit must be greater than zero");
        }
        Ok(())
    }
}

impl ModelGatewayConfig {
    fn validate(&self) -> Result<()> {
        if self.foreground.model.trim().is_empty() {
            bail!("model_gateway.foreground.model must not be empty");
        }
        if self.foreground.api_base_url.trim().is_empty() {
            bail!("model_gateway.foreground.api_base_url must not be empty");
        }
        if self.foreground.api_key_env.trim().is_empty() {
            bail!("model_gateway.foreground.api_key_env must not be empty");
        }
        if self.foreground.timeout_ms == 0 {
            bail!("model_gateway.foreground.timeout_ms must be greater than zero");
        }
        Ok(())
    }
}

impl SelfModelConfig {
    fn validate(&self) -> Result<()> {
        if self.seed_path.as_os_str().is_empty() {
            bail!("self_model.seed_path must not be empty");
        }
        Ok(())
    }
}

fn require_secret_env(env_name: &str, description: &str) -> Result<String> {
    let value = env::var(env_name).with_context(|| {
        format!("missing required {description}: set environment variable {env_name}")
    })?;
    if value.trim().is_empty() {
        bail!("{env_name} must not be empty");
    }
    Ok(value)
}

fn resolve_relative_to_config(path: &PathBuf) -> PathBuf {
    if path.is_absolute() {
        return path.clone();
    }

    let config_path = RuntimeConfig::config_path();
    let base_dir = config_path
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base_dir.join(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn sample_config() -> RuntimeConfig {
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
                default_wall_clock_budget_ms: 30_000,
            },
            worker: WorkerConfig {
                timeout_ms: 5_000,
                command: String::new(),
                args: Vec::new(),
            },
            telegram: None,
            model_gateway: None,
            self_model: None,
        }
    }

    #[test]
    fn validate_accepts_reasonable_values() {
        sample_config().validate().expect("config should be valid");
    }

    #[test]
    fn validate_rejects_zero_worker_timeout() {
        let mut config = sample_config();
        config.worker.timeout_ms = 0;
        let error = config.validate().expect_err("config should be rejected");
        assert!(error.to_string().contains("worker.timeout_ms"));
    }

    #[test]
    fn validate_rejects_non_positive_schema_version() {
        let mut config = sample_config();
        config.database.minimum_supported_schema_version = 0;
        let error = config.validate().expect_err("config should be rejected");
        assert!(
            error
                .to_string()
                .contains("minimum_supported_schema_version")
        );
    }

    #[test]
    fn parse_worker_args_override_accepts_json_array() {
        let args = parse_worker_args_override(r#"["--flag","value with spaces"]"#)
            .expect("worker args should parse");
        assert_eq!(args, vec!["--flag", "value with spaces"]);
    }

    #[test]
    fn parse_worker_args_override_rejects_shell_style_string() {
        let error = parse_worker_args_override("--flag value")
            .expect_err("shell-style strings should be rejected");
        assert!(error.to_string().contains("JSON array"));
    }

    #[test]
    fn validate_accepts_phase_1_only_configuration() {
        sample_config()
            .validate()
            .expect("phase 1 configuration should remain valid");
    }

    #[test]
    fn require_telegram_config_fails_when_section_is_missing() {
        let error = sample_config()
            .require_telegram_config()
            .expect_err("telegram config should be required for foreground telegram paths");
        assert!(
            error
                .to_string()
                .contains("missing Phase 2 Telegram configuration")
        );
    }

    #[test]
    fn require_telegram_config_fails_when_secret_is_missing() {
        let mut config = sample_config();
        config.telegram = Some(TelegramConfig {
            api_base_url: "https://api.telegram.org".to_string(),
            bot_token_env: format!("BLUE_LAGOON_TEST_TELEGRAM_TOKEN_{}", Uuid::now_v7()),
            allowed_user_id: 42,
            allowed_chat_id: 42,
            internal_principal_ref: "primary-user".to_string(),
            internal_conversation_ref: "telegram-primary".to_string(),
            poll_limit: 10,
        });

        let error = config
            .require_telegram_config()
            .expect_err("missing telegram bot token should fail closed");
        assert!(
            error
                .to_string()
                .contains("missing required Phase 2 Telegram bot token")
        );
    }

    #[test]
    fn require_model_gateway_config_fails_when_secret_is_missing() {
        let mut config = sample_config();
        config.model_gateway = Some(ModelGatewayConfig {
            foreground: ForegroundModelRouteConfig {
                provider: ModelProviderKind::ZAi,
                model: "zai-foreground".to_string(),
                api_base_url: "https://api.z.ai".to_string(),
                api_key_env: format!("BLUE_LAGOON_TEST_ZAI_API_KEY_{}", Uuid::now_v7()),
                timeout_ms: 30_000,
            },
        });

        let error = config
            .require_model_gateway_config()
            .expect_err("missing model gateway API key should fail closed");
        assert!(
            error
                .to_string()
                .contains("missing required Phase 2 model gateway API key")
        );
    }

    #[test]
    fn require_self_model_config_resolves_relative_seed_path() {
        let temp_root = env::temp_dir().join(format!("blue-lagoon-config-test-{}", Uuid::now_v7()));
        fs::create_dir_all(temp_root.join("config")).expect("temp config dir should be created");
        let seed_path = temp_root.join("config").join("self_model_seed.toml");
        fs::write(&seed_path, "role = 'assistant'\n").expect("seed file should be written");

        let original_config = env::var_os("BLUE_LAGOON_CONFIG");
        unsafe {
            env::set_var(
                "BLUE_LAGOON_CONFIG",
                temp_root.join("config").join("default.toml"),
            );
        }

        let mut config = sample_config();
        config.self_model = Some(SelfModelConfig {
            seed_path: PathBuf::from("self_model_seed.toml"),
        });

        let resolved = config
            .require_self_model_config()
            .expect("seed path should resolve relative to config path");
        assert_eq!(resolved.seed_path, seed_path);

        match original_config {
            Some(value) => unsafe { env::set_var("BLUE_LAGOON_CONFIG", value) },
            None => unsafe { env::remove_var("BLUE_LAGOON_CONFIG") },
        }
        let _ = fs::remove_dir_all(temp_root);
    }
}
