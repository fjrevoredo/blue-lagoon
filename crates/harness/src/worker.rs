use std::{ffi::OsString, path::PathBuf, process::Stdio, time::Duration};

use anyhow::{Context, Result, bail};
use contracts::{WorkerRequest, WorkerResponse, WorkerResult};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    time::timeout,
};

use crate::config::RuntimeConfig;

pub async fn launch_smoke_worker(
    config: &RuntimeConfig,
    request: &WorkerRequest,
) -> Result<WorkerResponse> {
    let command_spec = resolve_command(config)?;
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

#[derive(Debug, Clone)]
struct CommandSpec {
    command: OsString,
    args: Vec<OsString>,
}

fn resolve_command(config: &RuntimeConfig) -> Result<CommandSpec> {
    if !config.worker.command.trim().is_empty() {
        return Ok(CommandSpec {
            command: OsString::from(&config.worker.command),
            args: config.worker.args.iter().map(OsString::from).collect(),
        });
    }

    if let Some(path) = sibling_worker_binary() {
        return Ok(CommandSpec {
            command: path.into_os_string(),
            args: vec![OsString::from("smoke-worker")],
        });
    }

    Ok(CommandSpec {
        command: OsString::from("cargo"),
        args: vec![
            OsString::from("run"),
            OsString::from("--quiet"),
            OsString::from("-p"),
            OsString::from("workers"),
            OsString::from("--"),
            OsString::from("smoke-worker"),
        ],
    })
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
