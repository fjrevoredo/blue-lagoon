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
    pub default_foreground_iteration_budget: u32,
    pub default_wall_clock_budget_ms: u64,
    pub default_foreground_token_budget: u32,
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
    #[serde(default)]
    pub z_ai: Option<ZAiProviderConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ForegroundModelRouteConfig {
    pub provider: ModelProviderKind,
    pub model: String,
    #[serde(default)]
    pub api_base_url: Option<String>,
    pub api_key_env: String,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ZAiProviderConfig {
    #[serde(default)]
    pub api_surface: Option<ZAiApiSurface>,
    #[serde(default)]
    pub api_base_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZAiApiSurface {
    General,
    Coding,
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
        let foreground_route_override = env::var("BLUE_LAGOON_FOREGROUND_ROUTE")
            .ok()
            .as_deref()
            .map(parse_foreground_route_override)
            .transpose()?;

        let model_gateway = file_config.model_gateway.map(|mut model_gateway| {
            if let Some((provider, model)) = &foreground_route_override {
                model_gateway.foreground.provider = *provider;
                model_gateway.foreground.model = model.clone();
            }
            model_gateway
        });

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
            model_gateway,
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
        if self.harness.default_foreground_iteration_budget == 0 {
            bail!("harness.default_foreground_iteration_budget must be greater than zero");
        }
        if self.harness.default_foreground_token_budget == 0 {
            bail!("harness.default_foreground_token_budget must be greater than zero");
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
                api_base_url: require_foreground_api_base_url(model_gateway)?,
                api_key: require_foreground_api_key(&model_gateway.foreground.api_key_env)?,
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

fn parse_model_provider_override(raw: &str) -> Result<ModelProviderKind> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "z_ai" | "zai" | "z-ai" => Ok(ModelProviderKind::ZAi),
        other => bail!("BLUE_LAGOON_FOREGROUND_ROUTE provider must be one of: z_ai; got '{other}'"),
    }
}

fn parse_foreground_route_override(raw: &str) -> Result<(ModelProviderKind, String)> {
    let trimmed = raw.trim();
    let (provider_raw, model_raw) = trimmed.split_once('/').with_context(|| {
        format!(
            "BLUE_LAGOON_FOREGROUND_ROUTE must use '<provider>/<model>' format; got '{trimmed}'"
        )
    })?;
    let provider = parse_model_provider_override(provider_raw)?;
    let model = model_raw.trim();
    if model.is_empty() {
        bail!("BLUE_LAGOON_FOREGROUND_ROUTE model segment must not be empty");
    }
    Ok((provider, model.to_string()))
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
        if self.foreground.provider == ModelProviderKind::ZAi
            && self
                .foreground
                .api_base_url
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
            && self.z_ai.as_ref().is_none_or(|config| {
                config
                    .api_base_url
                    .as_deref()
                    .is_none_or(|value| value.trim().is_empty())
                    && config.api_surface.is_none()
            })
        {
            bail!(
                "model_gateway.foreground.api_base_url must not be empty unless model_gateway.z_ai config defines api_surface or api_base_url"
            );
        }
        if self.foreground.api_key_env.trim().is_empty() {
            bail!("model_gateway.foreground.api_key_env must not be empty");
        }
        if self.foreground.timeout_ms == 0 {
            bail!("model_gateway.foreground.timeout_ms must be greater than zero");
        }
        if let Some(z_ai) = &self.z_ai
            && z_ai
                .api_base_url
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
        {
            bail!("model_gateway.z_ai.api_base_url must not be empty");
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

fn require_foreground_api_key(configured_env_name: &str) -> Result<String> {
    if let Ok(value) = env::var("BLUE_LAGOON_FOREGROUND_API_KEY") {
        if value.trim().is_empty() {
            bail!("BLUE_LAGOON_FOREGROUND_API_KEY must not be empty");
        }
        return Ok(value);
    }

    require_secret_env(
        configured_env_name,
        "Phase 2 model gateway API key environment variable",
    )
}

fn require_foreground_api_base_url(config: &ModelGatewayConfig) -> Result<String> {
    if let Ok(value) = env::var("BLUE_LAGOON_FOREGROUND_API_BASE_URL") {
        if value.trim().is_empty() {
            bail!("BLUE_LAGOON_FOREGROUND_API_BASE_URL must not be empty");
        }
        return Ok(value);
    }

    match config.foreground.provider {
        ModelProviderKind::ZAi => {
            if let Some(z_ai) = &config.z_ai {
                if let Some(api_base_url) = z_ai.api_base_url.as_deref()
                    && !api_base_url.trim().is_empty()
                {
                    return Ok(api_base_url.to_string());
                }
                if let Some(api_surface) = z_ai.api_surface {
                    return Ok(match api_surface {
                        ZAiApiSurface::General => "https://api.z.ai/api/paas/v4".to_string(),
                        ZAiApiSurface::Coding => "https://api.z.ai/api/coding/paas/v4".to_string(),
                    });
                }
            }
            config
                .foreground
                .api_base_url
                .clone()
                .context("missing foreground api base url after provider-specific resolution")
        }
    }
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
    use std::sync::{Mutex, OnceLock};
    use uuid::Uuid;

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env test mutex should not be poisoned")
    }

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
                default_foreground_iteration_budget: 1,
                default_wall_clock_budget_ms: 30_000,
                default_foreground_token_budget: 4_000,
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
    fn parse_model_provider_override_accepts_supported_aliases() {
        assert_eq!(
            parse_model_provider_override("z_ai").expect("provider should parse"),
            ModelProviderKind::ZAi
        );
        assert_eq!(
            parse_model_provider_override("zai").expect("provider should parse"),
            ModelProviderKind::ZAi
        );
        assert_eq!(
            parse_model_provider_override("z-ai").expect("provider should parse"),
            ModelProviderKind::ZAi
        );
    }

    #[test]
    fn parse_model_provider_override_rejects_unknown_value() {
        let error = parse_model_provider_override("unknown")
            .expect_err("unknown provider should be rejected");
        assert!(error.to_string().contains("BLUE_LAGOON_FOREGROUND_ROUTE"));
    }

    #[test]
    fn parse_foreground_route_override_accepts_exact_model_segment() {
        let (provider, model) =
            parse_foreground_route_override("zai/glm-5-turbo").expect("route should parse");
        assert_eq!(provider, ModelProviderKind::ZAi);
        assert_eq!(model, "glm-5-turbo");
    }

    #[test]
    fn parse_foreground_route_override_rejects_missing_separator() {
        let error = parse_foreground_route_override("zai")
            .expect_err("route without separator should be rejected");
        assert!(error.to_string().contains("BLUE_LAGOON_FOREGROUND_ROUTE"));
    }

    #[test]
    fn parse_foreground_route_override_rejects_empty_model() {
        let error = parse_foreground_route_override("zai/")
            .expect_err("route without model should be rejected");
        assert!(error.to_string().contains("model segment"));
    }

    #[test]
    fn validate_accepts_phase_1_only_configuration() {
        sample_config()
            .validate()
            .expect("phase 1 configuration should remain valid");
    }

    #[test]
    fn validate_rejects_zero_foreground_iteration_budget() {
        let mut config = sample_config();
        config.harness.default_foreground_iteration_budget = 0;
        let error = config.validate().expect_err("config should be rejected");
        assert!(
            error
                .to_string()
                .contains("default_foreground_iteration_budget")
        );
    }

    #[test]
    fn validate_rejects_zero_foreground_token_budget() {
        let mut config = sample_config();
        config.harness.default_foreground_token_budget = 0;
        let error = config.validate().expect_err("config should be rejected");
        assert!(
            error
                .to_string()
                .contains("default_foreground_token_budget")
        );
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
        let _env_lock = env_lock();
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
        let _env_lock = env_lock();
        let mut config = sample_config();
        config.model_gateway = Some(ModelGatewayConfig {
            foreground: ForegroundModelRouteConfig {
                provider: ModelProviderKind::ZAi,
                model: "zai-foreground".to_string(),
                api_base_url: Some("https://api.z.ai".to_string()),
                api_key_env: format!("BLUE_LAGOON_TEST_FOREGROUND_API_KEY_{}", Uuid::now_v7()),
                timeout_ms: 30_000,
            },
            z_ai: None,
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
    fn load_applies_foreground_route_env_override() {
        let _env_lock = env_lock();
        let temp_root = env::temp_dir().join(format!("blue-lagoon-config-test-{}", Uuid::now_v7()));
        fs::create_dir_all(&temp_root).expect("temp dir should be created");
        let config_path = temp_root.join("default.toml");
        fs::write(
            &config_path,
            r#"
[app]
name = "blue-lagoon"
log_filter = "info"

[database]
minimum_supported_schema_version = 1

[harness]
allow_synthetic_smoke = true
default_foreground_iteration_budget = 1
default_wall_clock_budget_ms = 30000
default_foreground_token_budget = 4000

[worker]
timeout_ms = 10000
command = ""
args = []

[model_gateway.foreground]
provider = "z_ai"
model = "configured-model"
api_key_env = "BLUE_LAGOON_TEST_FOREGROUND_API_KEY"
timeout_ms = 30000

[model_gateway.z_ai]
api_surface = "coding"
"#,
        )
        .expect("config file should be written");

        let original_config = env::var_os("BLUE_LAGOON_CONFIG");
        let original_database_url = env::var_os("BLUE_LAGOON_DATABASE_URL");
        let original_route = env::var_os("BLUE_LAGOON_FOREGROUND_ROUTE");

        unsafe {
            env::set_var("BLUE_LAGOON_CONFIG", &config_path);
            env::set_var("BLUE_LAGOON_DATABASE_URL", "postgres://example");
            env::set_var("BLUE_LAGOON_FOREGROUND_ROUTE", "zai/override-model");
        }

        let loaded = RuntimeConfig::load().expect("config should load with overrides");
        let foreground = loaded
            .model_gateway
            .expect("model gateway should be present")
            .foreground;
        assert_eq!(foreground.provider, ModelProviderKind::ZAi);
        assert_eq!(foreground.model, "override-model");

        match original_config {
            Some(value) => unsafe { env::set_var("BLUE_LAGOON_CONFIG", value) },
            None => unsafe { env::remove_var("BLUE_LAGOON_CONFIG") },
        }
        match original_database_url {
            Some(value) => unsafe { env::set_var("BLUE_LAGOON_DATABASE_URL", value) },
            None => unsafe { env::remove_var("BLUE_LAGOON_DATABASE_URL") },
        }
        match original_route {
            Some(value) => unsafe { env::set_var("BLUE_LAGOON_FOREGROUND_ROUTE", value) },
            None => unsafe { env::remove_var("BLUE_LAGOON_FOREGROUND_ROUTE") },
        }
        let _ = fs::remove_dir_all(temp_root);
    }

    #[test]
    fn load_accepts_provider_specific_zai_surface_without_legacy_foreground_api_base_url() {
        let _env_lock = env_lock();
        let temp_root = env::temp_dir().join(format!("blue-lagoon-config-test-{}", Uuid::now_v7()));
        fs::create_dir_all(&temp_root).expect("temp dir should be created");
        let config_path = temp_root.join("default.toml");
        fs::write(
            &config_path,
            r#"
[app]
name = "blue-lagoon"
log_filter = "info"

[database]
minimum_supported_schema_version = 1

[harness]
allow_synthetic_smoke = true
default_foreground_iteration_budget = 1
default_wall_clock_budget_ms = 30000
default_foreground_token_budget = 4000

[worker]
timeout_ms = 10000
command = ""
args = []

[model_gateway.foreground]
provider = "z_ai"
model = "configured-model"
api_key_env = "BLUE_LAGOON_TEST_FOREGROUND_API_KEY"
timeout_ms = 30000

[model_gateway.z_ai]
api_surface = "coding"
"#,
        )
        .expect("config file should be written");

        let original_config = env::var_os("BLUE_LAGOON_CONFIG");
        let original_database_url = env::var_os("BLUE_LAGOON_DATABASE_URL");
        let original_api_key = env::var_os("BLUE_LAGOON_FOREGROUND_API_KEY");

        unsafe {
            env::set_var("BLUE_LAGOON_CONFIG", &config_path);
            env::set_var("BLUE_LAGOON_DATABASE_URL", "postgres://example");
            env::set_var("BLUE_LAGOON_FOREGROUND_API_KEY", "provider-key");
        }

        let loaded =
            RuntimeConfig::load().expect("config should load with provider-specific z_ai surface");
        let resolved = loaded
            .require_model_gateway_config()
            .expect("provider-specific api surface should resolve without legacy base url");
        assert_eq!(
            resolved.foreground.api_base_url,
            "https://api.z.ai/api/coding/paas/v4"
        );

        match original_config {
            Some(value) => unsafe { env::set_var("BLUE_LAGOON_CONFIG", value) },
            None => unsafe { env::remove_var("BLUE_LAGOON_CONFIG") },
        }
        match original_database_url {
            Some(value) => unsafe { env::set_var("BLUE_LAGOON_DATABASE_URL", value) },
            None => unsafe { env::remove_var("BLUE_LAGOON_DATABASE_URL") },
        }
        match original_api_key {
            Some(value) => unsafe { env::set_var("BLUE_LAGOON_FOREGROUND_API_KEY", value) },
            None => unsafe { env::remove_var("BLUE_LAGOON_FOREGROUND_API_KEY") },
        }
        let _ = fs::remove_dir_all(temp_root);
    }

    #[test]
    fn require_model_gateway_config_prefers_direct_foreground_api_key_override() {
        let _env_lock = env_lock();
        let mut config = sample_config();
        config.model_gateway = Some(ModelGatewayConfig {
            foreground: ForegroundModelRouteConfig {
                provider: ModelProviderKind::ZAi,
                model: "zai-foreground".to_string(),
                api_base_url: Some("https://api.z.ai".to_string()),
                api_key_env: format!("BLUE_LAGOON_TEST_FOREGROUND_API_KEY_{}", Uuid::now_v7()),
                timeout_ms: 30_000,
            },
            z_ai: None,
        });

        let original_api_key = env::var_os("BLUE_LAGOON_FOREGROUND_API_KEY");
        unsafe {
            env::set_var("BLUE_LAGOON_FOREGROUND_API_KEY", "direct-override-key");
        }

        let resolved = config
            .require_model_gateway_config()
            .expect("direct foreground api key override should resolve");
        assert_eq!(resolved.foreground.api_key, "direct-override-key");

        match original_api_key {
            Some(value) => unsafe { env::set_var("BLUE_LAGOON_FOREGROUND_API_KEY", value) },
            None => unsafe { env::remove_var("BLUE_LAGOON_FOREGROUND_API_KEY") },
        }
    }

    #[test]
    fn require_model_gateway_config_prefers_direct_foreground_api_base_url_override() {
        let _env_lock = env_lock();
        let mut config = sample_config();
        config.model_gateway = Some(ModelGatewayConfig {
            foreground: ForegroundModelRouteConfig {
                provider: ModelProviderKind::ZAi,
                model: "zai-foreground".to_string(),
                api_base_url: Some("https://api.z.ai/api/paas/v4".to_string()),
                api_key_env: format!("BLUE_LAGOON_TEST_FOREGROUND_API_KEY_{}", Uuid::now_v7()),
                timeout_ms: 30_000,
            },
            z_ai: None,
        });

        let original_api_key = env::var_os("BLUE_LAGOON_FOREGROUND_API_KEY");
        let original_api_base_url = env::var_os("BLUE_LAGOON_FOREGROUND_API_BASE_URL");
        unsafe {
            env::set_var("BLUE_LAGOON_FOREGROUND_API_KEY", "direct-override-key");
            env::set_var(
                "BLUE_LAGOON_FOREGROUND_API_BASE_URL",
                "https://api.z.ai/api/coding/paas/v4",
            );
        }

        let resolved = config
            .require_model_gateway_config()
            .expect("direct foreground api base url override should resolve");
        assert_eq!(
            resolved.foreground.api_base_url,
            "https://api.z.ai/api/coding/paas/v4"
        );

        match original_api_key {
            Some(value) => unsafe { env::set_var("BLUE_LAGOON_FOREGROUND_API_KEY", value) },
            None => unsafe { env::remove_var("BLUE_LAGOON_FOREGROUND_API_KEY") },
        }
        match original_api_base_url {
            Some(value) => unsafe { env::set_var("BLUE_LAGOON_FOREGROUND_API_BASE_URL", value) },
            None => unsafe { env::remove_var("BLUE_LAGOON_FOREGROUND_API_BASE_URL") },
        }
    }

    #[test]
    fn require_model_gateway_config_prefers_provider_specific_zai_api_base_url() {
        let _env_lock = env_lock();
        let mut config = sample_config();
        config.model_gateway = Some(ModelGatewayConfig {
            foreground: ForegroundModelRouteConfig {
                provider: ModelProviderKind::ZAi,
                model: "zai-foreground".to_string(),
                api_base_url: Some("https://api.z.ai/api/paas/v4".to_string()),
                api_key_env: format!("BLUE_LAGOON_TEST_FOREGROUND_API_KEY_{}", Uuid::now_v7()),
                timeout_ms: 30_000,
            },
            z_ai: Some(ZAiProviderConfig {
                api_surface: None,
                api_base_url: Some("https://api.z.ai/api/coding/paas/v4".to_string()),
            }),
        });

        let original_api_key = env::var_os("BLUE_LAGOON_FOREGROUND_API_KEY");
        let original_api_base_url = env::var_os("BLUE_LAGOON_FOREGROUND_API_BASE_URL");
        unsafe {
            env::set_var("BLUE_LAGOON_FOREGROUND_API_KEY", "direct-override-key");
            env::remove_var("BLUE_LAGOON_FOREGROUND_API_BASE_URL");
        }

        let resolved = config
            .require_model_gateway_config()
            .expect("provider-specific z_ai api base url should resolve");
        assert_eq!(
            resolved.foreground.api_base_url,
            "https://api.z.ai/api/coding/paas/v4"
        );

        match original_api_key {
            Some(value) => unsafe { env::set_var("BLUE_LAGOON_FOREGROUND_API_KEY", value) },
            None => unsafe { env::remove_var("BLUE_LAGOON_FOREGROUND_API_KEY") },
        }
        match original_api_base_url {
            Some(value) => unsafe { env::set_var("BLUE_LAGOON_FOREGROUND_API_BASE_URL", value) },
            None => unsafe { env::remove_var("BLUE_LAGOON_FOREGROUND_API_BASE_URL") },
        }
    }

    #[test]
    fn require_model_gateway_config_resolves_provider_specific_zai_api_surface() {
        let _env_lock = env_lock();
        let mut config = sample_config();
        config.model_gateway = Some(ModelGatewayConfig {
            foreground: ForegroundModelRouteConfig {
                provider: ModelProviderKind::ZAi,
                model: "zai-foreground".to_string(),
                api_base_url: None,
                api_key_env: format!("BLUE_LAGOON_TEST_FOREGROUND_API_KEY_{}", Uuid::now_v7()),
                timeout_ms: 30_000,
            },
            z_ai: Some(ZAiProviderConfig {
                api_surface: Some(ZAiApiSurface::Coding),
                api_base_url: None,
            }),
        });

        let original_api_key = env::var_os("BLUE_LAGOON_FOREGROUND_API_KEY");
        let original_api_base_url = env::var_os("BLUE_LAGOON_FOREGROUND_API_BASE_URL");
        unsafe {
            env::set_var("BLUE_LAGOON_FOREGROUND_API_KEY", "direct-override-key");
            env::remove_var("BLUE_LAGOON_FOREGROUND_API_BASE_URL");
        }

        let resolved = config
            .require_model_gateway_config()
            .expect("provider-specific z_ai api surface should resolve");
        assert_eq!(
            resolved.foreground.api_base_url,
            "https://api.z.ai/api/coding/paas/v4"
        );

        match original_api_key {
            Some(value) => unsafe { env::set_var("BLUE_LAGOON_FOREGROUND_API_KEY", value) },
            None => unsafe { env::remove_var("BLUE_LAGOON_FOREGROUND_API_KEY") },
        }
        match original_api_base_url {
            Some(value) => unsafe { env::set_var("BLUE_LAGOON_FOREGROUND_API_BASE_URL", value) },
            None => unsafe { env::remove_var("BLUE_LAGOON_FOREGROUND_API_BASE_URL") },
        }
    }

    #[test]
    fn require_self_model_config_resolves_relative_seed_path() {
        let _env_lock = env_lock();
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
