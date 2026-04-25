use std::{
    collections::BTreeSet,
    env,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use contracts::{CapabilityScope, NetworkAccessPosture, SubprocessAction, WebFetchAction};
use tokio::{io::AsyncReadExt, process::Command, time::timeout};

use crate::config::RuntimeConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedSubprocessOutcome {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

pub async fn execute_bounded_subprocess(
    config: &RuntimeConfig,
    capability_scope: &CapabilityScope,
    action: &SubprocessAction,
) -> Result<BoundedSubprocessOutcome> {
    if capability_scope.network != NetworkAccessPosture::Disabled {
        bail!("the first governed-action backend does not support network-enabled execution");
    }

    let allowed_roots = resolve_allowed_roots(config, capability_scope)?;
    if allowed_roots.is_empty() {
        bail!("bounded subprocess execution requires at least one resolved filesystem root");
    }

    let working_directory = resolve_working_directory(config, action, &allowed_roots)?;
    let executable_path = resolve_executable_path(action, &allowed_roots)?;

    let mut command = if let Some(path) = executable_path {
        Command::new(path)
    } else {
        Command::new(&action.command)
    };
    command
        .args(&action.args)
        .current_dir(&working_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear();
    apply_bootstrap_environment(&mut command);
    apply_scoped_environment(&mut command, capability_scope)?;

    let mut child = command
        .spawn()
        .with_context(|| format!("failed to spawn bounded subprocess '{}'", action.command))?;
    let stdout_task = child.stdout.take().map(|stdout| {
        tokio::spawn(async move {
            let mut stdout = stdout;
            let mut bytes = Vec::new();
            stdout.read_to_end(&mut bytes).await.map(|_| bytes)
        })
    });
    let stderr_task = child.stderr.take().map(|stderr| {
        tokio::spawn(async move {
            let mut stderr = stderr;
            let mut bytes = Vec::new();
            stderr.read_to_end(&mut bytes).await.map(|_| bytes)
        })
    });

    let timeout_ms = capability_scope.execution.timeout_ms;
    let status = match timeout(Duration::from_millis(timeout_ms), child.wait()).await {
        Ok(status) => status.context("bounded subprocess failed while waiting for exit status")?,
        Err(_) => {
            child
                .start_kill()
                .context("failed to terminate timed-out bounded subprocess")?;
            let _ = child.wait().await;
            let stdout = read_child_stream(stdout_task, "stdout")
                .await
                .unwrap_or_default();
            let stderr = read_child_stream(stderr_task, "stderr")
                .await
                .unwrap_or_default();
            return Ok(BoundedSubprocessOutcome {
                exit_code: None,
                stdout: truncate_utf8_lossy(&stdout, capability_scope.execution.max_stdout_bytes),
                stderr: truncate_utf8_lossy(&stderr, capability_scope.execution.max_stderr_bytes),
                timed_out: true,
            });
        }
    };

    let stdout = read_child_stream(stdout_task, "stdout").await?;
    let stderr = read_child_stream(stderr_task, "stderr").await?;

    Ok(BoundedSubprocessOutcome {
        exit_code: status.code(),
        stdout: truncate_utf8_lossy(&stdout, capability_scope.execution.max_stdout_bytes),
        stderr: truncate_utf8_lossy(&stderr, capability_scope.execution.max_stderr_bytes),
        timed_out: false,
    })
}

fn resolve_allowed_roots(
    config: &RuntimeConfig,
    capability_scope: &CapabilityScope,
) -> Result<Vec<PathBuf>> {
    let workspace_root = canonical_workspace_root(config)?;
    let requested_roots = capability_scope
        .filesystem
        .read_roots
        .iter()
        .chain(capability_scope.filesystem.write_roots.iter())
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>();

    let mut resolved = Vec::new();
    for root in requested_roots {
        let candidate = canonicalize_scoped_path(&workspace_root, root).with_context(|| {
            format!("failed to resolve governed-action filesystem root '{root}'")
        })?;
        ensure_within_workspace(&workspace_root, &candidate)?;
        resolved.push(candidate);
    }
    Ok(resolved)
}

fn resolve_working_directory(
    config: &RuntimeConfig,
    action: &SubprocessAction,
    allowed_roots: &[PathBuf],
) -> Result<PathBuf> {
    let workspace_root = canonical_workspace_root(config)?;
    let working_directory = match action.working_directory.as_deref() {
        Some(path) => canonicalize_scoped_path(&workspace_root, path).with_context(|| {
            format!("failed to resolve governed-action working directory '{path}'")
        })?,
        None => workspace_root.clone(),
    };
    ensure_within_workspace(&workspace_root, &working_directory)?;
    ensure_within_allowed_roots(&working_directory, allowed_roots)?;
    Ok(working_directory)
}

fn resolve_executable_path(
    action: &SubprocessAction,
    allowed_roots: &[PathBuf],
) -> Result<Option<PathBuf>> {
    let command_path = Path::new(&action.command);
    if !command_path.components().skip(1).any(|_| true) && !command_path.is_absolute() {
        return Ok(None);
    }

    let resolved = command_path.canonicalize().with_context(|| {
        format!(
            "failed to resolve governed-action command path '{}'",
            action.command
        )
    })?;
    ensure_within_allowed_roots(&resolved, allowed_roots)?;
    Ok(Some(resolved))
}

fn canonical_workspace_root(config: &RuntimeConfig) -> Result<PathBuf> {
    config
        .workspace
        .root_dir
        .canonicalize()
        .context("failed to resolve configured workspace.root_dir")
}

fn canonicalize_scoped_path(workspace_root: &Path, value: &str) -> Result<PathBuf> {
    let candidate = PathBuf::from(value);
    let absolute = if candidate.is_absolute() {
        candidate
    } else {
        workspace_root.join(candidate)
    };
    absolute
        .canonicalize()
        .with_context(|| format!("path '{}' must exist before execution", absolute.display()))
}

fn ensure_within_workspace(workspace_root: &Path, candidate: &Path) -> Result<()> {
    if candidate.starts_with(workspace_root) {
        Ok(())
    } else {
        bail!(
            "resolved path '{}' escapes the configured workspace root '{}'",
            candidate.display(),
            workspace_root.display()
        );
    }
}

fn ensure_within_allowed_roots(candidate: &Path, allowed_roots: &[PathBuf]) -> Result<()> {
    if allowed_roots.iter().any(|root| candidate.starts_with(root)) {
        Ok(())
    } else {
        bail!(
            "resolved path '{}' falls outside the governed-action filesystem scope",
            candidate.display()
        );
    }
}

fn apply_bootstrap_environment(command: &mut Command) {
    for variable in bootstrap_environment_variables() {
        if let Ok(value) = env::var(variable) {
            command.env(variable, value);
        }
    }
}

fn bootstrap_environment_variables() -> &'static [&'static str] {
    if cfg!(windows) {
        &[
            "PATH",
            "PATHEXT",
            "SystemRoot",
            "SYSTEMROOT",
            "ComSpec",
            "COMSPEC",
            "TEMP",
            "TMP",
            "USERPROFILE",
        ]
    } else {
        &["PATH", "HOME", "TMPDIR", "TMP"]
    }
}

fn apply_scoped_environment(
    command: &mut Command,
    capability_scope: &CapabilityScope,
) -> Result<()> {
    for variable in &capability_scope.environment.allow_variables {
        let value = env::var(variable).with_context(|| {
            format!(
                "governed action requested environment variable '{variable}', but it is not set"
            )
        })?;
        command.env(variable, value);
    }
    Ok(())
}

async fn read_child_stream(
    task: Option<tokio::task::JoinHandle<std::io::Result<Vec<u8>>>>,
    stream_name: &str,
) -> Result<Vec<u8>> {
    match task {
        Some(task) => task
            .await
            .with_context(|| format!("failed to join bounded subprocess {stream_name} task"))?
            .with_context(|| format!("failed to read bounded subprocess {stream_name}")),
        None => Ok(Vec::new()),
    }
}

fn truncate_utf8_lossy(bytes: &[u8], max_bytes: u64) -> String {
    let capped_len = usize::try_from(max_bytes).unwrap_or(usize::MAX);
    let slice = if bytes.len() > capped_len {
        &bytes[..capped_len]
    } else {
        bytes
    };
    String::from_utf8_lossy(slice).to_string()
}

pub struct WebFetchOutcome {
    pub body: String,
    pub truncated: bool,
}

pub async fn execute_web_fetch(action: &WebFetchAction) -> Result<WebFetchOutcome> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(action.timeout_ms))
        .build()
        .context("failed to build HTTP client for web fetch")?;

    let response = client
        .get(&action.url)
        .send()
        .await
        .with_context(|| format!("HTTP GET failed for '{}'", action.url))?;

    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("failed to read HTTP response body from '{}'", action.url))?;

    let max = usize::try_from(action.max_response_bytes).unwrap_or(usize::MAX);
    let truncated = bytes.len() > max;
    let body = truncate_utf8_lossy(&bytes, action.max_response_bytes);

    Ok(WebFetchOutcome { body, truncated })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        AppConfig, ApprovalPromptMode, ApprovalsConfig, BackgroundConfig,
        BackgroundExecutionConfig, BackgroundSchedulerConfig, BackgroundThresholdsConfig,
        BacklogRecoveryConfig, ContinuityConfig, DatabaseConfig, GovernedActionsConfig,
        HarnessConfig, RetrievalConfig, RuntimeConfig, ScheduledForegroundConfig,
        WakeSignalPolicyConfig, WorkerConfig, WorkspaceConfig,
    };
    use contracts::{
        EnvironmentCapabilityScope, ExecutionCapabilityBudget, FilesystemCapabilityScope,
        GovernedActionRiskTier,
    };

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
            scheduled_foreground: ScheduledForegroundConfig {
                enabled: true,
                max_due_tasks_per_iteration: 2,
                min_cadence_seconds: 300,
                default_cooldown_seconds: 300,
            },
            workspace: WorkspaceConfig {
                root_dir: env::current_dir().expect("current dir should resolve"),
                max_artifact_bytes: 1_048_576,
                max_script_bytes: 262_144,
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
                timeout_ms: 20_000,
                command: String::new(),
                args: Vec::new(),
            },
            telegram: None,
            model_gateway: None,
            self_model: None,
        }
    }

    fn sample_scope() -> CapabilityScope {
        let root = env::current_dir()
            .expect("current dir should resolve")
            .display()
            .to_string();
        CapabilityScope {
            filesystem: FilesystemCapabilityScope {
                read_roots: vec![root.clone()],
                write_roots: vec![root],
            },
            network: NetworkAccessPosture::Disabled,
            environment: EnvironmentCapabilityScope {
                allow_variables: Vec::new(),
            },
            execution: ExecutionCapabilityBudget {
                timeout_ms: 30_000,
                max_stdout_bytes: 4_096,
                max_stderr_bytes: 4_096,
            },
        }
    }

    #[tokio::test]
    async fn bounded_subprocess_rejects_network_enabled_scope() {
        let config = sample_config();
        let mut scope = sample_scope();
        scope.network = NetworkAccessPosture::Enabled;
        let action = platform_echo_action("hello");

        let error = execute_bounded_subprocess(&config, &scope, &action)
            .await
            .expect_err("network-enabled execution should be rejected");
        assert!(
            error
                .to_string()
                .contains("does not support network-enabled execution")
        );
    }

    #[tokio::test]
    async fn bounded_subprocess_times_out_and_captures_output() {
        let config = sample_config();
        let mut scope = sample_scope();
        scope.execution.timeout_ms = 50;
        let action = platform_sleep_action();

        let outcome = execute_bounded_subprocess(&config, &scope, &action)
            .await
            .expect("timeout outcome should be returned");
        assert!(outcome.timed_out);
    }

    fn platform_echo_action(message: &str) -> SubprocessAction {
        if cfg!(windows) {
            SubprocessAction {
                command: "powershell".to_string(),
                args: vec![
                    "-NoProfile".to_string(),
                    "-Command".to_string(),
                    format!("Write-Output '{}'", message.replace('\'', "''")),
                ],
                working_directory: Some(
                    env::current_dir()
                        .expect("current dir should resolve")
                        .display()
                        .to_string(),
                ),
            }
        } else {
            SubprocessAction {
                command: "sh".to_string(),
                args: vec![
                    "-c".to_string(),
                    format!("printf '%s\\n' '{}'", message.replace('\'', "'\\''")),
                ],
                working_directory: Some(
                    env::current_dir()
                        .expect("current dir should resolve")
                        .display()
                        .to_string(),
                ),
            }
        }
    }

    fn platform_sleep_action() -> SubprocessAction {
        if cfg!(windows) {
            SubprocessAction {
                command: "powershell".to_string(),
                args: vec![
                    "-NoProfile".to_string(),
                    "-Command".to_string(),
                    "Start-Sleep -Milliseconds 250".to_string(),
                ],
                working_directory: Some(
                    env::current_dir()
                        .expect("current dir should resolve")
                        .display()
                        .to_string(),
                ),
            }
        } else {
            SubprocessAction {
                command: "sh".to_string(),
                args: vec!["-c".to_string(), "sleep 0.25".to_string()],
                working_directory: Some(
                    env::current_dir()
                        .expect("current dir should resolve")
                        .display()
                        .to_string(),
                ),
            }
        }
    }
}
