use std::{ffi::OsString, path::PathBuf, process::Stdio, time::Duration};

use anyhow::{Context, Result, bail};
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
    model_gateway::{self, ModelProviderTransport},
};

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

        let model_response =
            model_gateway::execute_foreground_model_call(gateway, &model_request, transport)
                .await
                .context("conscious worker model-call execution failed in the harness")?;
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
            "conscious worker returned an error response: {}",
            error.message
        );
    }
    Ok(response)
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
