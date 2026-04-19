use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use contracts::ModelProviderKind;
use serde::Deserialize;
use toml::Value as TomlValue;

const DEFAULT_CONFIG_REL_PATH: &str = "config/default.toml";
const LOCAL_CONFIG_REL_PATH: &str = "config/local.toml";
const DOTENV_FILENAME: &str = ".env";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeConfig {
    pub app: AppConfig,
    pub database: DatabaseConfig,
    pub harness: HarnessConfig,
    pub background: BackgroundConfig,
    pub continuity: ContinuityConfig,
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
pub struct BackgroundConfig {
    pub scheduler: BackgroundSchedulerConfig,
    pub thresholds: BackgroundThresholdsConfig,
    pub execution: BackgroundExecutionConfig,
    pub wake_signals: WakeSignalPolicyConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BackgroundSchedulerConfig {
    pub poll_interval_seconds: u64,
    pub max_due_jobs_per_iteration: u32,
    pub lease_timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BackgroundThresholdsConfig {
    pub episode_backlog_threshold: u32,
    pub candidate_memory_threshold: u32,
    pub contradiction_alert_threshold: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BackgroundExecutionConfig {
    pub default_iteration_budget: u32,
    pub default_wall_clock_budget_ms: u64,
    pub default_token_budget: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WakeSignalPolicyConfig {
    pub allow_foreground_conversion: bool,
    pub max_pending_signals: u32,
    pub cooldown_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ContinuityConfig {
    pub retrieval: RetrievalConfig,
    pub backlog_recovery: BacklogRecoveryConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RetrievalConfig {
    pub max_recent_episode_candidates: u32,
    pub max_memory_artifact_candidates: u32,
    pub max_context_items: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BacklogRecoveryConfig {
    pub pending_message_count_threshold: u32,
    pub pending_message_span_seconds_threshold: u64,
    pub stale_pending_ingress_age_seconds_threshold: u64,
    pub max_recovery_batch_size: u32,
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
    pub poll_limit: u16,
    #[serde(default)]
    pub foreground_binding: Option<TelegramForegroundBindingConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TelegramForegroundBindingConfig {
    pub allowed_user_id: i64,
    pub allowed_chat_id: i64,
    pub internal_principal_ref: String,
    pub internal_conversation_ref: String,
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
    background: BackgroundConfig,
    continuity: ContinuityConfig,
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
        let config_root = discover_config_root()?;
        Self::load_from_root(&config_root)
    }

    fn load_from_root(config_root: &Path) -> Result<Self> {
        load_dotenv_from_root(config_root)?;
        let file_config = load_file_config_from_root(config_root)?;

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
            background: file_config.background,
            continuity: file_config.continuity,
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
        self.background.validate()?;
        self.continuity.validate()?;
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

    pub fn require_telegram_config(&self) -> Result<ResolvedTelegramConfig> {
        let telegram = self
            .telegram
            .as_ref()
            .context("missing Telegram foreground configuration")?;
        telegram.validate_transport()?;
        let binding = telegram.foreground_binding.as_ref().context(
            "missing Telegram foreground binding configuration: configure [telegram.foreground_binding] in config/local.toml or an equivalent local override",
        )?;
        binding.validate()?;

        Ok(ResolvedTelegramConfig {
            api_base_url: telegram.api_base_url.clone(),
            bot_token: require_secret_env(
                &telegram.bot_token_env,
                "Telegram bot token environment variable",
            )?,
            allowed_user_id: binding.allowed_user_id,
            allowed_chat_id: binding.allowed_chat_id,
            internal_principal_ref: binding.internal_principal_ref.clone(),
            internal_conversation_ref: binding.internal_conversation_ref.clone(),
            poll_limit: telegram.poll_limit,
        })
    }

    pub fn require_model_gateway_config(&self) -> Result<ResolvedModelGatewayConfig> {
        let model_gateway = self
            .model_gateway
            .as_ref()
            .context("missing foreground model gateway configuration")?;
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
            .context("missing foreground self-model seed configuration")?;
        self_model.validate()?;

        let seed_path =
            resolve_relative_to_config_root(&discover_config_root()?, &self_model.seed_path);
        let metadata = fs::metadata(&seed_path).with_context(|| {
            format!(
                "failed to access foreground self-model seed artifact at {}",
                seed_path.display()
            )
        })?;
        if !metadata.is_file() {
            bail!(
                "foreground self-model seed artifact path is not a file: {}",
                seed_path.display()
            );
        }

        Ok(ResolvedSelfModelConfig { seed_path })
    }
}

fn discover_config_root() -> Result<PathBuf> {
    let current_dir = env::current_dir()
        .context("failed to determine current working directory for config discovery")?;
    discover_config_root_from(&current_dir)
}

fn discover_config_root_from(start_dir: &Path) -> Result<PathBuf> {
    let mut candidate = start_dir.canonicalize().with_context(|| {
        format!(
            "failed to canonicalize config discovery start path {}",
            start_dir.display()
        )
    })?;

    loop {
        if candidate.join("Cargo.toml").is_file()
            && candidate.join(DEFAULT_CONFIG_REL_PATH).is_file()
        {
            return Ok(candidate);
        }
        if !candidate.pop() {
            bail!(
                "failed to discover Blue Lagoon config root from {}: expected Cargo.toml and {} in an ancestor directory",
                start_dir.display(),
                DEFAULT_CONFIG_REL_PATH
            );
        }
    }
}

fn load_dotenv_from_root(config_root: &Path) -> Result<()> {
    let dotenv_path = config_root.join(DOTENV_FILENAME);
    if !dotenv_path.exists() {
        return Ok(());
    }
    dotenvy::from_path(&dotenv_path)
        .with_context(|| format!("failed to load .env file at {}", dotenv_path.display()))?;
    Ok(())
}

fn load_file_config_from_root(config_root: &Path) -> Result<FileConfig> {
    let default_path = config_root.join(DEFAULT_CONFIG_REL_PATH);
    let mut merged = parse_toml_file(&default_path)?;

    let local_path = config_root.join(LOCAL_CONFIG_REL_PATH);
    if local_path.exists() {
        merge_toml_values(&mut merged, parse_toml_file(&local_path)?);
    }

    merged
        .try_into()
        .context("failed to parse merged runtime configuration as TOML")
}

fn parse_toml_file(path: &Path) -> Result<TomlValue> {
    let file_contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read config file at {}", path.display()))?;
    toml::from_str(&file_contents)
        .with_context(|| format!("failed to parse TOML config file at {}", path.display()))
}

fn merge_toml_values(base: &mut TomlValue, overlay: TomlValue) {
    match (base, overlay) {
        (TomlValue::Table(base_table), TomlValue::Table(overlay_table)) => {
            for (key, overlay_value) in overlay_table {
                match base_table.get_mut(&key) {
                    Some(base_value) => merge_toml_values(base_value, overlay_value),
                    None => {
                        base_table.insert(key, overlay_value);
                    }
                }
            }
        }
        (base_value, overlay_value) => {
            *base_value = overlay_value;
        }
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
        self.validate_transport()?;
        if let Some(binding) = &self.foreground_binding {
            binding.validate()?;
        }
        Ok(())
    }

    fn validate_transport(&self) -> Result<()> {
        if self.api_base_url.trim().is_empty() {
            bail!("telegram.api_base_url must not be empty");
        }
        if self.bot_token_env.trim().is_empty() {
            bail!("telegram.bot_token_env must not be empty");
        }
        if self.poll_limit == 0 {
            bail!("telegram.poll_limit must be greater than zero");
        }
        Ok(())
    }
}

impl TelegramForegroundBindingConfig {
    fn validate(&self) -> Result<()> {
        if self.allowed_user_id == 0 {
            bail!("telegram.foreground_binding.allowed_user_id must not be zero");
        }
        if self.allowed_chat_id == 0 {
            bail!("telegram.foreground_binding.allowed_chat_id must not be zero");
        }
        if self.internal_principal_ref.trim().is_empty() {
            bail!("telegram.foreground_binding.internal_principal_ref must not be empty");
        }
        if self.internal_conversation_ref.trim().is_empty() {
            bail!("telegram.foreground_binding.internal_conversation_ref must not be empty");
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

impl ContinuityConfig {
    fn validate(&self) -> Result<()> {
        self.retrieval.validate()?;
        self.backlog_recovery.validate()?;
        Ok(())
    }
}

impl BackgroundConfig {
    fn validate(&self) -> Result<()> {
        self.scheduler.validate()?;
        self.thresholds.validate()?;
        self.execution.validate()?;
        self.wake_signals.validate()?;
        Ok(())
    }
}

impl BackgroundSchedulerConfig {
    fn validate(&self) -> Result<()> {
        if self.poll_interval_seconds == 0 {
            bail!("background.scheduler.poll_interval_seconds must be greater than zero");
        }
        if self.max_due_jobs_per_iteration == 0 {
            bail!("background.scheduler.max_due_jobs_per_iteration must be greater than zero");
        }
        if self.lease_timeout_ms == 0 {
            bail!("background.scheduler.lease_timeout_ms must be greater than zero");
        }
        Ok(())
    }
}

impl BackgroundThresholdsConfig {
    fn validate(&self) -> Result<()> {
        if self.episode_backlog_threshold == 0 {
            bail!("background.thresholds.episode_backlog_threshold must be greater than zero");
        }
        if self.candidate_memory_threshold == 0 {
            bail!("background.thresholds.candidate_memory_threshold must be greater than zero");
        }
        if self.contradiction_alert_threshold == 0 {
            bail!("background.thresholds.contradiction_alert_threshold must be greater than zero");
        }
        Ok(())
    }
}

impl BackgroundExecutionConfig {
    fn validate(&self) -> Result<()> {
        if self.default_iteration_budget == 0 {
            bail!("background.execution.default_iteration_budget must be greater than zero");
        }
        if self.default_wall_clock_budget_ms == 0 {
            bail!("background.execution.default_wall_clock_budget_ms must be greater than zero");
        }
        if self.default_token_budget == 0 {
            bail!("background.execution.default_token_budget must be greater than zero");
        }
        Ok(())
    }
}

impl WakeSignalPolicyConfig {
    fn validate(&self) -> Result<()> {
        if self.max_pending_signals == 0 {
            bail!("background.wake_signals.max_pending_signals must be greater than zero");
        }
        if self.cooldown_seconds == 0 {
            bail!("background.wake_signals.cooldown_seconds must be greater than zero");
        }
        Ok(())
    }
}

impl RetrievalConfig {
    fn validate(&self) -> Result<()> {
        if self.max_recent_episode_candidates == 0 {
            bail!("continuity.retrieval.max_recent_episode_candidates must be greater than zero");
        }
        if self.max_memory_artifact_candidates == 0 {
            bail!("continuity.retrieval.max_memory_artifact_candidates must be greater than zero");
        }
        if self.max_context_items == 0 {
            bail!("continuity.retrieval.max_context_items must be greater than zero");
        }
        if self.max_context_items
            < self
                .max_recent_episode_candidates
                .min(self.max_memory_artifact_candidates)
        {
            bail!(
                "continuity.retrieval.max_context_items must be at least the smaller retrieval candidate bound"
            );
        }
        Ok(())
    }
}

impl BacklogRecoveryConfig {
    fn validate(&self) -> Result<()> {
        if self.pending_message_count_threshold < 2 {
            bail!(
                "continuity.backlog_recovery.pending_message_count_threshold must be at least two"
            );
        }
        if self.pending_message_span_seconds_threshold == 0 {
            bail!(
                "continuity.backlog_recovery.pending_message_span_seconds_threshold must be greater than zero"
            );
        }
        if self.stale_pending_ingress_age_seconds_threshold == 0 {
            bail!(
                "continuity.backlog_recovery.stale_pending_ingress_age_seconds_threshold must be greater than zero"
            );
        }
        if self.max_recovery_batch_size == 0 {
            bail!("continuity.backlog_recovery.max_recovery_batch_size must be greater than zero");
        }
        if self.max_recovery_batch_size < self.pending_message_count_threshold {
            bail!(
                "continuity.backlog_recovery.max_recovery_batch_size must be greater than or equal to pending_message_count_threshold"
            );
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
        "foreground model gateway API key environment variable",
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

fn resolve_relative_to_config_root(config_root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    config_root.join(path)
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
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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

    fn scoped_current_dir(path: &Path) -> ScopedCurrentDir {
        ScopedCurrentDir::set(path)
    }

    fn write_test_root(
        default_toml: &str,
        local_toml: Option<&str>,
        dotenv: Option<&str>,
    ) -> PathBuf {
        let root = env::temp_dir().join(format!("blue-lagoon-config-test-{}", Uuid::now_v7()));
        fs::create_dir_all(root.join("config")).expect("config dir should be created");
        fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n")
            .expect("cargo manifest should be written");
        fs::write(root.join(DEFAULT_CONFIG_REL_PATH), default_toml)
            .expect("default config should be written");
        if let Some(local_toml) = local_toml {
            fs::write(root.join(LOCAL_CONFIG_REL_PATH), local_toml)
                .expect("local config should be written");
        }
        if let Some(dotenv) = dotenv {
            fs::write(root.join(DOTENV_FILENAME), dotenv).expect(".env should be written");
        }
        root
    }

    fn minimal_file_config() -> &'static str {
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

[background.scheduler]
poll_interval_seconds = 300
max_due_jobs_per_iteration = 4
lease_timeout_ms = 300000

[background.thresholds]
episode_backlog_threshold = 25
candidate_memory_threshold = 10
contradiction_alert_threshold = 3

[background.execution]
default_iteration_budget = 2
default_wall_clock_budget_ms = 120000
default_token_budget = 6000

[background.wake_signals]
allow_foreground_conversion = true
max_pending_signals = 8
cooldown_seconds = 900

[continuity.retrieval]
max_recent_episode_candidates = 3
max_memory_artifact_candidates = 5
max_context_items = 6

[continuity.backlog_recovery]
pending_message_count_threshold = 3
pending_message_span_seconds_threshold = 120
stale_pending_ingress_age_seconds_threshold = 300
max_recovery_batch_size = 8

[worker]
timeout_ms = 10000
command = ""
args = []
"#
    }

    struct ScopedCurrentDir {
        original: PathBuf,
    }

    impl ScopedCurrentDir {
        fn set(path: &Path) -> Self {
            let original = env::current_dir().expect("current dir should be readable");
            env::set_current_dir(path).expect("current dir should be set");
            Self { original }
        }
    }

    impl Drop for ScopedCurrentDir {
        fn drop(&mut self) {
            env::set_current_dir(&self.original).expect("current dir should be restored");
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
    fn validate_accepts_minimal_configuration_without_foreground_sections() {
        sample_config()
            .validate()
            .expect("minimal configuration should remain valid");
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
    fn validate_rejects_zero_retrieval_bounds() {
        let mut config = sample_config();
        config.continuity.retrieval.max_recent_episode_candidates = 0;
        let error = config.validate().expect_err("config should be rejected");
        assert!(error.to_string().contains("max_recent_episode_candidates"));

        let mut config = sample_config();
        config.continuity.retrieval.max_memory_artifact_candidates = 0;
        let error = config.validate().expect_err("config should be rejected");
        assert!(error.to_string().contains("max_memory_artifact_candidates"));

        let mut config = sample_config();
        config.continuity.retrieval.max_context_items = 0;
        let error = config.validate().expect_err("config should be rejected");
        assert!(error.to_string().contains("max_context_items"));
    }

    #[test]
    fn validate_rejects_invalid_background_settings() {
        let mut config = sample_config();
        config.background.scheduler.poll_interval_seconds = 0;
        let error = config.validate().expect_err("config should be rejected");
        assert!(
            error
                .to_string()
                .contains("background.scheduler.poll_interval_seconds")
        );

        let mut config = sample_config();
        config.background.execution.default_iteration_budget = 0;
        let error = config.validate().expect_err("config should be rejected");
        assert!(
            error
                .to_string()
                .contains("background.execution.default_iteration_budget")
        );

        let mut config = sample_config();
        config.background.wake_signals.max_pending_signals = 0;
        let error = config.validate().expect_err("config should be rejected");
        assert!(
            error
                .to_string()
                .contains("background.wake_signals.max_pending_signals")
        );
    }

    #[test]
    fn load_reads_background_sections_from_file_config() {
        let _env_lock = env_lock();
        let temp_root = write_test_root(minimal_file_config(), None, None);
        let original_database_url = env::var_os("BLUE_LAGOON_DATABASE_URL");
        unsafe {
            env::set_var("BLUE_LAGOON_DATABASE_URL", "postgres://example");
        }

        let loaded =
            RuntimeConfig::load_from_root(&temp_root).expect("config should load from file");
        assert_eq!(loaded.background.scheduler.poll_interval_seconds, 300);
        assert_eq!(loaded.background.scheduler.max_due_jobs_per_iteration, 4);
        assert_eq!(loaded.background.execution.default_token_budget, 6_000);
        assert!(loaded.background.wake_signals.allow_foreground_conversion);

        match original_database_url {
            Some(value) => unsafe { env::set_var("BLUE_LAGOON_DATABASE_URL", value) },
            None => unsafe { env::remove_var("BLUE_LAGOON_DATABASE_URL") },
        }
        let _ = fs::remove_dir_all(temp_root);
    }

    #[test]
    fn validate_rejects_invalid_backlog_recovery_thresholds() {
        let mut config = sample_config();
        config
            .continuity
            .backlog_recovery
            .pending_message_count_threshold = 1;
        let error = config.validate().expect_err("config should be rejected");
        assert!(
            error
                .to_string()
                .contains("pending_message_count_threshold")
        );

        let mut config = sample_config();
        config.continuity.backlog_recovery.max_recovery_batch_size = 2;
        let error = config.validate().expect_err("config should be rejected");
        assert!(error.to_string().contains("max_recovery_batch_size"));
    }

    #[test]
    fn require_telegram_config_fails_when_section_is_missing() {
        let error = sample_config()
            .require_telegram_config()
            .expect_err("telegram config should be required for foreground telegram paths");
        assert!(
            error
                .to_string()
                .contains("missing Telegram foreground configuration")
        );
    }

    #[test]
    fn require_telegram_config_fails_when_secret_is_missing() {
        let _env_lock = env_lock();
        let mut config = sample_config();
        config.telegram = Some(TelegramConfig {
            api_base_url: "https://api.telegram.org".to_string(),
            bot_token_env: format!("BLUE_LAGOON_TEST_TELEGRAM_TOKEN_{}", Uuid::now_v7()),
            poll_limit: 10,
            foreground_binding: Some(TelegramForegroundBindingConfig {
                allowed_user_id: 42,
                allowed_chat_id: 42,
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
            }),
        });

        let error = config
            .require_telegram_config()
            .expect_err("missing telegram bot token should fail closed");
        assert!(
            error
                .to_string()
                .contains("missing required Telegram bot token")
        );
    }

    #[test]
    fn require_telegram_config_fails_when_binding_is_missing() {
        let mut config = sample_config();
        config.telegram = Some(TelegramConfig {
            api_base_url: "https://api.telegram.org".to_string(),
            bot_token_env: "BLUE_LAGOON_TEST_TELEGRAM_TOKEN".to_string(),
            poll_limit: 10,
            foreground_binding: None,
        });

        let error = config
            .require_telegram_config()
            .expect_err("missing foreground binding should fail closed");
        assert!(error.to_string().contains("foreground binding"));
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

        let original_api_key = env::var_os("BLUE_LAGOON_FOREGROUND_API_KEY");
        unsafe {
            env::remove_var("BLUE_LAGOON_FOREGROUND_API_KEY");
        }

        let error = config
            .require_model_gateway_config()
            .expect_err("missing model gateway API key should fail closed");
        assert!(
            error
                .to_string()
                .contains("missing required foreground model gateway API key")
        );

        match original_api_key {
            Some(value) => unsafe { env::set_var("BLUE_LAGOON_FOREGROUND_API_KEY", value) },
            None => unsafe { env::remove_var("BLUE_LAGOON_FOREGROUND_API_KEY") },
        }
    }

    #[test]
    fn load_applies_foreground_route_env_override() {
        let _env_lock = env_lock();
        let temp_root = write_test_root(
            &format!(
                r#"
{}
[model_gateway.foreground]
provider = "z_ai"
model = "configured-model"
api_key_env = "BLUE_LAGOON_TEST_FOREGROUND_API_KEY"
timeout_ms = 30000

[model_gateway.z_ai]
api_surface = "coding"
"#,
                minimal_file_config()
            ),
            None,
            None,
        );
        let original_database_url = env::var_os("BLUE_LAGOON_DATABASE_URL");
        let original_route = env::var_os("BLUE_LAGOON_FOREGROUND_ROUTE");

        unsafe {
            env::set_var("BLUE_LAGOON_DATABASE_URL", "postgres://example");
            env::set_var("BLUE_LAGOON_FOREGROUND_ROUTE", "zai/override-model");
        }

        let loaded =
            RuntimeConfig::load_from_root(&temp_root).expect("config should load with overrides");
        let foreground = loaded
            .model_gateway
            .expect("model gateway should be present")
            .foreground;
        assert_eq!(foreground.provider, ModelProviderKind::ZAi);
        assert_eq!(foreground.model, "override-model");

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
        let temp_root = write_test_root(
            &format!(
                r#"
{}
[model_gateway.foreground]
provider = "z_ai"
model = "configured-model"
api_key_env = "BLUE_LAGOON_TEST_FOREGROUND_API_KEY"
timeout_ms = 30000

[model_gateway.z_ai]
api_surface = "coding"
"#,
                minimal_file_config()
            ),
            None,
            None,
        );
        let original_database_url = env::var_os("BLUE_LAGOON_DATABASE_URL");
        let original_api_key = env::var_os("BLUE_LAGOON_FOREGROUND_API_KEY");

        unsafe {
            env::set_var("BLUE_LAGOON_DATABASE_URL", "postgres://example");
            env::set_var("BLUE_LAGOON_FOREGROUND_API_KEY", "provider-key");
        }

        let loaded = RuntimeConfig::load_from_root(&temp_root)
            .expect("config should load with provider-specific z_ai surface");
        let resolved = loaded
            .require_model_gateway_config()
            .expect("provider-specific api surface should resolve without legacy base url");
        assert_eq!(
            resolved.foreground.api_base_url,
            "https://api.z.ai/api/coding/paas/v4"
        );

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
        let temp_root = write_test_root(minimal_file_config(), None, None);
        let seed_path = temp_root.join("config").join("self_model_seed.toml");
        fs::write(&seed_path, "role = 'assistant'\n").expect("seed file should be written");
        let nested_dir = temp_root.join("crates").join("runtime");
        fs::create_dir_all(&nested_dir).expect("nested cwd should be created");
        {
            let _cwd = scoped_current_dir(&nested_dir);

            let mut config = sample_config();
            config.self_model = Some(SelfModelConfig {
                seed_path: PathBuf::from("config/self_model_seed.toml"),
            });

            let resolved = config
                .require_self_model_config()
                .expect("seed path should resolve relative to config root");
            assert_eq!(
                resolved
                    .seed_path
                    .canonicalize()
                    .expect("resolved path should exist"),
                seed_path.canonicalize().expect("seed path should exist"),
            );
        }

        let _ = fs::remove_dir_all(temp_root);
    }

    #[test]
    fn load_merges_optional_local_config_recursively() {
        let _env_lock = env_lock();
        let temp_root = write_test_root(
            &format!(
                r#"
{}
[telegram]
api_base_url = "https://api.telegram.org"
bot_token_env = "BLUE_LAGOON_TEST_TELEGRAM_TOKEN"
poll_limit = 10
"#,
                minimal_file_config()
            ),
            Some(
                r#"
[telegram.foreground_binding]
allowed_user_id = 42
allowed_chat_id = 24
internal_principal_ref = "primary-user"
internal_conversation_ref = "telegram-primary"
"#,
            ),
            None,
        );
        let original_database_url = env::var_os("BLUE_LAGOON_DATABASE_URL");
        unsafe {
            env::set_var("BLUE_LAGOON_DATABASE_URL", "postgres://example");
        }

        let loaded = RuntimeConfig::load_from_root(&temp_root).expect("layered config should load");
        let telegram = loaded.telegram.expect("telegram config should be present");
        assert_eq!(telegram.api_base_url, "https://api.telegram.org");
        let binding = telegram
            .foreground_binding
            .expect("foreground binding should merge from local config");
        assert_eq!(binding.allowed_user_id, 42);
        assert_eq!(binding.allowed_chat_id, 24);

        match original_database_url {
            Some(value) => unsafe { env::set_var("BLUE_LAGOON_DATABASE_URL", value) },
            None => unsafe { env::remove_var("BLUE_LAGOON_DATABASE_URL") },
        }
        let _ = fs::remove_dir_all(temp_root);
    }

    #[test]
    fn load_reads_database_url_from_dotenv_without_overriding_process_env() {
        let _env_lock = env_lock();
        let temp_root = write_test_root(
            minimal_file_config(),
            None,
            Some("BLUE_LAGOON_DATABASE_URL=postgres://from-dotenv\nBLUE_LAGOON_LOG=warn\n"),
        );
        let original_database_url = env::var_os("BLUE_LAGOON_DATABASE_URL");
        let original_log = env::var_os("BLUE_LAGOON_LOG");
        unsafe {
            env::set_var("BLUE_LAGOON_LOG", "debug");
            env::remove_var("BLUE_LAGOON_DATABASE_URL");
        }

        let loaded =
            RuntimeConfig::load_from_root(&temp_root).expect("dotenv-backed config should load");
        assert_eq!(loaded.database.database_url, "postgres://from-dotenv");
        assert_eq!(loaded.app.log_filter, "debug");

        match original_database_url {
            Some(value) => unsafe { env::set_var("BLUE_LAGOON_DATABASE_URL", value) },
            None => unsafe { env::remove_var("BLUE_LAGOON_DATABASE_URL") },
        }
        match original_log {
            Some(value) => unsafe { env::set_var("BLUE_LAGOON_LOG", value) },
            None => unsafe { env::remove_var("BLUE_LAGOON_LOG") },
        }
        let _ = fs::remove_dir_all(temp_root);
    }

    #[test]
    fn load_discovers_config_root_from_nested_current_directory() {
        let _env_lock = env_lock();
        let temp_root = write_test_root(minimal_file_config(), None, None);
        let nested_dir = temp_root.join("crates").join("runtime");
        fs::create_dir_all(&nested_dir).expect("nested cwd should be created");
        let original_database_url = env::var_os("BLUE_LAGOON_DATABASE_URL");
        unsafe {
            env::set_var("BLUE_LAGOON_DATABASE_URL", "postgres://example");
        }

        {
            let _cwd = scoped_current_dir(&nested_dir);
            let loaded =
                RuntimeConfig::load().expect("config root should be discovered from nested cwd");
            assert_eq!(loaded.database.database_url, "postgres://example");
        }

        match original_database_url {
            Some(value) => unsafe { env::set_var("BLUE_LAGOON_DATABASE_URL", value) },
            None => unsafe { env::remove_var("BLUE_LAGOON_DATABASE_URL") },
        }
        let _ = fs::remove_dir_all(temp_root);
    }
}
