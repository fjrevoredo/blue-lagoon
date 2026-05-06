use std::{ffi::OsString, path::PathBuf, process::Stdio, time::Duration};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use contracts::{
    ConsciousWorkerInboundMessage, ConsciousWorkerOutboundMessage, WorkerRequest, WorkerResponse,
    WorkerResult,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::Command,
    time::timeout,
};

use crate::{
    config::{ResolvedModelGatewayConfig, RuntimeConfig},
    db, model_calls,
    model_gateway::{self, ModelProviderTransport},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerResolutionKind {
    ExplicitCommand,
    SiblingBinary,
    Unresolved,
}

impl WorkerResolutionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExplicitCommand => "explicit_command",
            Self::SiblingBinary => "sibling_binary",
            Self::Unresolved => "unresolved",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerResolutionSummary {
    pub resolution_kind: WorkerResolutionKind,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub notes: String,
}

pub async fn launch_smoke_worker(
    config: &RuntimeConfig,
    request: &WorkerRequest,
) -> Result<WorkerResponse> {
    let command_spec = resolve_command(config, "smoke-worker")?;
    let mut command = Command::new(&command_spec.command);
    command
        .args(&command_spec.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .context("failed to spawn worker subprocess")?;
    let request_json = serde_json::to_vec(request).context("failed to encode worker request")?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(&request_json)
            .await
            .context("failed to write worker request to stdin")?;
    }

    let stdout_task = child.stdout.take().map(|stdout| {
        tokio::spawn(async move {
            let mut bytes = Vec::new();
            let mut stdout = stdout;
            stdout.read_to_end(&mut bytes).await.map(|_| bytes)
        })
    });
    let stderr_task = child.stderr.take().map(|stderr| {
        tokio::spawn(async move {
            let mut bytes = Vec::new();
            let mut stderr = stderr;
            stderr.read_to_end(&mut bytes).await.map(|_| bytes)
        })
    });

    let status = match timeout(
        Duration::from_millis(config.worker.timeout_ms),
        child.wait(),
    )
    .await
    {
        Ok(status) => status.context("worker subprocess failed while waiting for exit status")?,
        Err(_) => {
            child
                .start_kill()
                .context("failed to terminate timed-out worker subprocess")?;
            let _ = child.wait().await;
            bail!(
                "worker subprocess timed out after {} ms and was terminated",
                config.worker.timeout_ms
            );
        }
    };

    let stdout = read_child_stream(stdout_task, "stdout").await?;
    let stderr = read_child_stream(stderr_task, "stderr").await?;

    if !status.success() {
        let stderr = String::from_utf8_lossy(&stderr);
        bail!("worker subprocess failed: {stderr}");
    }

    let response: WorkerResponse =
        serde_json::from_slice(&stdout).context("failed to decode worker response")?;
    if let WorkerResult::Error(error) = &response.result {
        bail!("worker returned an error response: {}", error.message);
    }
    Ok(response)
}

pub async fn launch_conscious_worker<T: ModelProviderTransport>(
    config: &RuntimeConfig,
    gateway: &ResolvedModelGatewayConfig,
    request: &WorkerRequest,
    transport: &T,
) -> Result<WorkerResponse> {
    launch_conscious_worker_with_timeout(
        config,
        gateway,
        request,
        transport,
        config.worker.timeout_ms,
    )
    .await
}

pub async fn launch_conscious_worker_with_timeout<T: ModelProviderTransport>(
    config: &RuntimeConfig,
    gateway: &ResolvedModelGatewayConfig,
    request: &WorkerRequest,
    transport: &T,
    timeout_ms: u64,
) -> Result<WorkerResponse> {
    let command_spec = resolve_command(config, "conscious-worker")?;
    let mut command = Command::new(&command_spec.command);
    command
        .args(&command_spec.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .context("failed to spawn conscious worker subprocess")?;
    let mut stdin = child
        .stdin
        .take()
        .context("failed to take conscious worker stdin")?;
    let stdout = child
        .stdout
        .take()
        .context("failed to take conscious worker stdout")?;
    let stderr_task = child.stderr.take().map(|stderr| {
        tokio::spawn(async move {
            let mut bytes = Vec::new();
            let mut stderr = stderr;
            stderr.read_to_end(&mut bytes).await.map(|_| bytes)
        })
    });
    let mut stdout_lines = BufReader::new(stdout).lines();
    let pool = db::connect(config).await?;

    let operation = async {
        write_json_line(&mut stdin, request).await?;

        let first_line = stdout_lines
            .next_line()
            .await
            .context("failed to read conscious worker model-request line")?
            .context("conscious worker exited before sending a protocol message")?;
        let first_message: ConsciousWorkerOutboundMessage = serde_json::from_str(&first_line)
            .context("failed to decode first worker protocol message")?;

        let model_request = match first_message {
            ConsciousWorkerOutboundMessage::ModelCallRequest(model_request) => model_request,
            ConsciousWorkerOutboundMessage::FinalResponse(response) => {
                let status = child
                    .wait()
                    .await
                    .context("conscious worker failed while waiting for exit status")?;
                return Ok((response, status));
            }
        };

        let model_call_started_at = Utc::now();
        let model_call_id = match model_calls::insert_pending_model_call_record(
            &pool,
            gateway,
            &model_request,
            model_call_started_at,
            config.observability.model_call_payload_retention_days,
        )
        .await
        {
            Ok(model_call_id) => Some(model_call_id),
            Err(error) if model_calls::is_missing_model_call_schema(&error) => None,
            Err(error) => return Err(error),
        };
        let model_response =
            match model_gateway::execute_foreground_model_call(gateway, &model_request, transport)
                .await
            {
                Ok(model_response) => {
                    if let Some(model_call_id) = model_call_id {
                        model_calls::mark_model_call_succeeded(
                            &pool,
                            model_call_id,
                            &model_response,
                            Utc::now(),
                        )
                        .await?;
                    }
                    model_response
                }
                Err(error) => {
                    if let Some(model_call_id) = model_call_id {
                        model_calls::mark_model_call_failed(
                            &pool,
                            model_call_id,
                            &error.to_string(),
                            Utc::now(),
                        )
                        .await?;
                    }
                    return Err(error)
                        .context("conscious worker model-call execution failed in the harness");
                }
            };
        write_json_line(
            &mut stdin,
            &ConsciousWorkerInboundMessage::ModelCallResponse(model_response),
        )
        .await?;
        drop(stdin);

        let final_line = stdout_lines
            .next_line()
            .await
            .context("failed to read conscious worker final-response line")?
            .context("conscious worker exited before sending a final response")?;
        let final_message: ConsciousWorkerOutboundMessage = serde_json::from_str(&final_line)
            .context("failed to decode final worker protocol message")?;
        let response = match final_message {
            ConsciousWorkerOutboundMessage::ModelCallRequest(_) => {
                bail!(
                    "conscious worker emitted more than one model-call request; foreground execution supports only one"
                )
            }
            ConsciousWorkerOutboundMessage::FinalResponse(response) => response,
        };

        let status = child
            .wait()
            .await
            .context("conscious worker failed while waiting for exit status")?;
        Ok((response, status))
    };

    let (response, status) = match timeout(Duration::from_millis(timeout_ms), operation).await {
        Ok(result) => result?,
        Err(_) => {
            child
                .start_kill()
                .context("failed to terminate timed-out conscious worker subprocess")?;
            let _ = child.wait().await;
            let _ = read_child_stream(stderr_task, "stderr").await;
            bail!(
                "conscious worker subprocess timed out after {} ms and was terminated",
                timeout_ms
            );
        }
    };

    let stderr = read_child_stream(stderr_task, "stderr").await?;
    if !status.success() {
        let stderr = String::from_utf8_lossy(&stderr);
        bail!("conscious worker subprocess failed: {stderr}");
    }
    if let WorkerResult::Error(error) = &response.result {
        bail!(
            "conscious worker returned an error response: worker_error_code={} message={}",
            error.code.as_str(),
            error.message
        );
    }
    if !matches!(response.result, WorkerResult::Conscious(_)) {
        bail!("conscious worker returned a non-conscious result payload");
    }
    Ok(response)
}

pub async fn launch_unconscious_worker<T: ModelProviderTransport>(
    config: &RuntimeConfig,
    gateway: &ResolvedModelGatewayConfig,
    request: &WorkerRequest,
    transport: &T,
) -> Result<WorkerResponse> {
    launch_unconscious_worker_with_timeout(
        config,
        gateway,
        request,
        transport,
        config.worker.timeout_ms,
    )
    .await
}

pub async fn launch_unconscious_worker_with_timeout<T: ModelProviderTransport>(
    config: &RuntimeConfig,
    gateway: &ResolvedModelGatewayConfig,
    request: &WorkerRequest,
    transport: &T,
    timeout_ms: u64,
) -> Result<WorkerResponse> {
    let command_spec = resolve_command(config, "unconscious-worker")?;
    let mut command = Command::new(&command_spec.command);
    command
        .args(&command_spec.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .context("failed to spawn unconscious worker subprocess")?;
    let mut stdin = child
        .stdin
        .take()
        .context("failed to take unconscious worker stdin")?;
    let stdout = child
        .stdout
        .take()
        .context("failed to take unconscious worker stdout")?;
    let stderr_task = child.stderr.take().map(|stderr| {
        tokio::spawn(async move {
            let mut bytes = Vec::new();
            let mut stderr = stderr;
            stderr.read_to_end(&mut bytes).await.map(|_| bytes)
        })
    });
    let mut stdout_lines = BufReader::new(stdout).lines();
    let pool = db::connect(config).await?;

    let operation = async {
        write_json_line(&mut stdin, request).await?;

        let first_line = stdout_lines
            .next_line()
            .await
            .context("failed to read unconscious worker model-request line")?
            .context("unconscious worker exited before sending a protocol message")?;
        let first_message: ConsciousWorkerOutboundMessage = serde_json::from_str(&first_line)
            .context("failed to decode first worker protocol message")?;

        let model_request = match first_message {
            ConsciousWorkerOutboundMessage::ModelCallRequest(model_request) => model_request,
            ConsciousWorkerOutboundMessage::FinalResponse(response) => {
                let status = child
                    .wait()
                    .await
                    .context("unconscious worker failed while waiting for exit status")?;
                return Ok((response, status));
            }
        };

        let model_call_started_at = Utc::now();
        let model_call_id = match model_calls::insert_pending_model_call_record(
            &pool,
            gateway,
            &model_request,
            model_call_started_at,
            config.observability.model_call_payload_retention_days,
        )
        .await
        {
            Ok(model_call_id) => Some(model_call_id),
            Err(error) if model_calls::is_missing_model_call_schema(&error) => None,
            Err(error) => return Err(error),
        };
        let model_response =
            match model_gateway::execute_background_model_call(gateway, &model_request, transport)
                .await
            {
                Ok(model_response) => {
                    if let Some(model_call_id) = model_call_id {
                        model_calls::mark_model_call_succeeded(
                            &pool,
                            model_call_id,
                            &model_response,
                            Utc::now(),
                        )
                        .await?;
                    }
                    model_response
                }
                Err(error) => {
                    if let Some(model_call_id) = model_call_id {
                        model_calls::mark_model_call_failed(
                            &pool,
                            model_call_id,
                            &error.to_string(),
                            Utc::now(),
                        )
                        .await?;
                    }
                    return Err(error)
                        .context("unconscious worker model-call execution failed in the harness");
                }
            };
        write_json_line(
            &mut stdin,
            &ConsciousWorkerInboundMessage::ModelCallResponse(model_response),
        )
        .await?;
        drop(stdin);

        let final_line = stdout_lines
            .next_line()
            .await
            .context("failed to read unconscious worker final-response line")?
            .context("unconscious worker exited before sending a final response")?;
        let final_message: ConsciousWorkerOutboundMessage = serde_json::from_str(&final_line)
            .context("failed to decode final worker protocol message")?;
        let response = match final_message {
            ConsciousWorkerOutboundMessage::ModelCallRequest(_) => {
                bail!(
                    "unconscious worker emitted more than one model-call request; background execution supports only one"
                )
            }
            ConsciousWorkerOutboundMessage::FinalResponse(response) => response,
        };

        let status = child
            .wait()
            .await
            .context("unconscious worker failed while waiting for exit status")?;
        Ok((response, status))
    };

    let (response, status) = match timeout(Duration::from_millis(timeout_ms), operation).await {
        Ok(result) => result?,
        Err(_) => {
            child
                .start_kill()
                .context("failed to terminate timed-out unconscious worker subprocess")?;
            let _ = child.wait().await;
            let _ = read_child_stream(stderr_task, "stderr").await;
            bail!(
                "unconscious worker subprocess timed out after {} ms and was terminated",
                timeout_ms
            );
        }
    };

    let stderr = read_child_stream(stderr_task, "stderr").await?;
    if !status.success() {
        let stderr = String::from_utf8_lossy(&stderr);
        bail!("unconscious worker subprocess failed: {stderr}");
    }
    if let WorkerResult::Error(error) = &response.result {
        bail!(
            "unconscious worker returned an error response: worker_error_code={} message={}",
            error.code.as_str(),
            error.message
        );
    }
    if !matches!(response.result, WorkerResult::Unconscious(_)) {
        bail!("unconscious worker returned a non-unconscious result payload");
    }
    Ok(response)
}

pub fn inspect_resolution(config: &RuntimeConfig) -> WorkerResolutionSummary {
    if !config.worker.command.trim().is_empty() {
        return WorkerResolutionSummary {
            resolution_kind: WorkerResolutionKind::ExplicitCommand,
            command: Some(config.worker.command.clone()),
            args: config.worker.args.clone(),
            notes: "worker subprocesses use the configured command directly".to_string(),
        };
    }

    if let Some(path) = sibling_worker_binary() {
        return WorkerResolutionSummary {
            resolution_kind: WorkerResolutionKind::SiblingBinary,
            command: Some(path.display().to_string()),
            args: vec![
                "smoke-worker".to_string(),
                "conscious-worker".to_string(),
                "unconscious-worker".to_string(),
            ],
            notes: "worker subprocesses use the sibling workers binary with per-worker subcommands"
                .to_string(),
        };
    }

    WorkerResolutionSummary {
        resolution_kind: WorkerResolutionKind::Unresolved,
        command: None,
        args: Vec::new(),
        notes: "worker command is not configured and no sibling workers binary is available"
            .to_string(),
    }
}

#[derive(Debug, Clone)]
struct CommandSpec {
    command: OsString,
    args: Vec<OsString>,
}

fn resolve_command(config: &RuntimeConfig, default_subcommand: &str) -> Result<CommandSpec> {
    if !config.worker.command.trim().is_empty() {
        return Ok(CommandSpec {
            command: OsString::from(&config.worker.command),
            args: config.worker.args.iter().map(OsString::from).collect(),
        });
    }

    if let Some(path) = sibling_worker_binary() {
        return Ok(CommandSpec {
            command: path.into_os_string(),
            args: vec![OsString::from(default_subcommand)],
        });
    }

    bail!(
        "worker command is not configured and no sibling workers binary was found; set worker.command or BLUE_LAGOON_WORKER_COMMAND explicitly"
    )
}

fn sibling_worker_binary() -> Option<PathBuf> {
    let current_exe = std::env::current_exe().ok()?;
    let parent = current_exe.parent()?;
    let candidates = [
        parent.join("workers"),
        parent.join("workers.exe"),
        parent.join("workers-bin"),
        parent.join("workers-bin.exe"),
    ];
    candidates.into_iter().find(|candidate| candidate.exists())
}

async fn read_child_stream(
    task: Option<tokio::task::JoinHandle<std::io::Result<Vec<u8>>>>,
    stream_name: &str,
) -> Result<Vec<u8>> {
    match task {
        Some(task) => task
            .await
            .with_context(|| format!("failed to join worker {stream_name} reader task"))?
            .with_context(|| format!("failed to read worker {stream_name}")),
        None => Ok(Vec::new()),
    }
}

async fn write_json_line<T: serde::Serialize>(
    writer: &mut (impl AsyncWriteExt + Unpin),
    value: &T,
) -> Result<()> {
    let json = serde_json::to_string(value).context("failed to encode worker protocol message")?;
    writer
        .write_all(json.as_bytes())
        .await
        .context("failed to write worker protocol line")?;
    writer
        .write_all(b"\n")
        .await
        .context("failed to terminate worker protocol line")?;
    writer
        .flush()
        .await
        .context("failed to flush worker stdin")?;
    Ok(())
}
