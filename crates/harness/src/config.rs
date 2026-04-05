use std::{env, fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

const DEFAULT_CONFIG_PATH: &str = "config/default.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeConfig {
    pub app: AppConfig,
    pub database: DatabaseConfig,
    pub harness: HarnessConfig,
    pub worker: WorkerConfig,
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

#[derive(Debug, Clone, Deserialize)]
struct FileConfig {
    app: AppConfig,
    database: FileDatabaseConfig,
    harness: HarnessConfig,
    worker: WorkerConfig,
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
        Ok(())
    }

    pub fn config_path() -> PathBuf {
        env::var("BLUE_LAGOON_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_CONFIG_PATH))
    }
}

fn parse_worker_args_override(raw: &str) -> Result<Vec<String>> {
    serde_json::from_str(raw).context("BLUE_LAGOON_WORKER_ARGS must be a JSON array of strings")
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
