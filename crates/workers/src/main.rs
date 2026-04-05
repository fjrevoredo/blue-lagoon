use std::io::{Read, Write};
use std::{path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use contracts::{
    SmokeWorkerResult, WorkerErrorCode, WorkerFailure, WorkerPayload, WorkerRequest,
    WorkerResponse, WorkerResult,
};

#[derive(Debug, Parser)]
#[command(name = "workers", about = "Blue Lagoon worker runtime")]
struct Cli {
    #[command(subcommand)]
    command: WorkerCommand,
}

#[derive(Debug, Subcommand)]
enum WorkerCommand {
    SmokeWorker,
    #[command(hide = true)]
    StallWorker {
        #[arg(long)]
        sleep_ms: u64,
        #[arg(long)]
        pid_file: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        WorkerCommand::SmokeWorker => run_smoke_worker(),
        WorkerCommand::StallWorker { sleep_ms, pid_file } => run_stall_worker(sleep_ms, pid_file),
    }
}

fn run_smoke_worker() -> Result<()> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .context("failed to read worker request from stdin")?;

    let response = match serde_json::from_str::<WorkerRequest>(&input) {
        Ok(request) => handle_request(request),
        Err(error) => error_response(
            WorkerErrorCode::InvalidRequest,
            format!("invalid worker request: {error}"),
        ),
    };

    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer(&mut handle, &response).context("failed to serialize worker response")?;
    handle
        .write_all(b"\n")
        .context("failed to terminate worker response line")?;
    Ok(())
}

fn run_stall_worker(sleep_ms: u64, pid_file: Option<PathBuf>) -> Result<()> {
    if let Some(path) = pid_file {
        std::fs::write(path, std::process::id().to_string())
            .context("failed to write stall-worker pid file")?;
    }
    std::thread::sleep(Duration::from_millis(sleep_ms));
    Ok(())
}

fn handle_request(request: WorkerRequest) -> WorkerResponse {
    if let Err(error) = request.validate() {
        return error_response(WorkerErrorCode::InvalidRequest, error.to_string());
    }

    match request.payload {
        WorkerPayload::Smoke(payload) => WorkerResponse {
            request_id: request.request_id,
            trace_id: request.trace_id,
            execution_id: request.execution_id,
            finished_at: chrono::Utc::now(),
            worker_pid: std::process::id(),
            result: WorkerResult::Smoke(SmokeWorkerResult {
                status: "completed".to_string(),
                summary: format!(
                    "synthetic trigger '{}' completed by smoke worker",
                    payload.synthetic_trigger
                ),
            }),
        },
    }
}

fn error_response(code: WorkerErrorCode, message: String) -> WorkerResponse {
    WorkerResponse {
        request_id: uuid::Uuid::nil(),
        trace_id: uuid::Uuid::nil(),
        execution_id: uuid::Uuid::nil(),
        finished_at: chrono::Utc::now(),
        worker_pid: std::process::id(),
        result: WorkerResult::Error(WorkerFailure { code, message }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke_worker_returns_structured_result() {
        let request = WorkerRequest::smoke(uuid::Uuid::now_v7(), uuid::Uuid::now_v7(), "smoke");
        let response = handle_request(request.clone());
        assert_eq!(response.request_id, request.request_id);
        assert_eq!(response.trace_id, request.trace_id);
        assert_eq!(response.execution_id, request.execution_id);
        match response.result {
            WorkerResult::Smoke(result) => {
                assert_eq!(result.status, "completed");
                assert!(result.summary.contains("smoke"));
            }
            WorkerResult::Error(_) => panic!("smoke worker should not return an error"),
        }
    }
}
