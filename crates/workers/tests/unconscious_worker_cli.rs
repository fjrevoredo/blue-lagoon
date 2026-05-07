use contracts::{
    BackgroundExecutionBudget, BackgroundTrigger, BackgroundTriggerKind,
    ConsciousWorkerInboundMessage, ConsciousWorkerOutboundMessage, ModelCallResponse, ModelOutput,
    ModelProviderKind, ModelUsage, UnconsciousContext, UnconsciousJobKind, UnconsciousScope,
    WorkerRequest, WorkerResult,
};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
fn unconscious_worker_cli_emits_model_request_and_final_response() {
    let request =
        WorkerRequest::unconscious(uuid::Uuid::now_v7(), uuid::Uuid::now_v7(), sample_context());
    let request_json = serde_json::to_string(&request).expect("request should serialize");

    let mut child = Command::new(env!("CARGO_BIN_EXE_workers"))
        .arg("unconscious-worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("workers command should run");

    let mut stdin = child.stdin.take().expect("stdin should be piped");
    writeln!(stdin, "{request_json}").expect("request should be written");
    stdin.flush().expect("request should flush");

    let stdout = child.stdout.take().expect("stdout should be piped");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .expect("first protocol line should be readable");

    let outbound: ConsciousWorkerOutboundMessage =
        serde_json::from_str(line.trim_end()).expect("first line should deserialize");
    let model_request = match outbound {
        ConsciousWorkerOutboundMessage::ModelCallRequest(model_request) => model_request,
        ConsciousWorkerOutboundMessage::FinalResponse(_) => {
            panic!("first unconscious worker line should be a model call request")
        }
    };

    let model_response = ModelCallResponse {
        request_id: model_request.request_id,
        trace_id: request.trace_id,
        execution_id: request.execution_id,
        provider: ModelProviderKind::ZAi,
        model: "z-ai-background".to_string(),
        received_at: chrono::Utc::now(),
        output: ModelOutput {
            text: "maintenance summary".to_string(),
            json: None,
            finish_reason: "stop".to_string(),
        },
        usage: ModelUsage {
            input_tokens: 12,
            output_tokens: 5,
        },
    };
    writeln!(
        stdin,
        "{}",
        serde_json::to_string(&ConsciousWorkerInboundMessage::ModelCallResponse(
            model_response
        ))
        .expect("model response should serialize")
    )
    .expect("model response should be written");
    stdin.flush().expect("model response should flush");
    drop(stdin);

    line.clear();
    reader
        .read_line(&mut line)
        .expect("second protocol line should be readable");
    let final_response =
        match serde_json::from_str::<ConsciousWorkerOutboundMessage>(line.trim_end())
            .expect("second line should deserialize")
        {
            ConsciousWorkerOutboundMessage::ModelCallRequest(_) => {
                panic!("second unconscious worker line should be a final response")
            }
            ConsciousWorkerOutboundMessage::FinalResponse(response) => response,
        };

    assert_eq!(model_request.trace_id, request.trace_id);
    assert_eq!(model_request.execution_id, request.execution_id);
    match final_response.result {
        WorkerResult::Unconscious(result) => {
            assert!(result.summary.contains("memory_consolidation"));
            assert_eq!(result.maintenance_outputs.retrieval_updates.len(), 1);
            assert_eq!(result.maintenance_outputs.diagnostics.len(), 1);
        }
        WorkerResult::Smoke(_) => panic!("unexpected smoke response"),
        WorkerResult::Conscious(_) => panic!("unexpected conscious response"),
        WorkerResult::Error(error) => panic!("unexpected worker error: {}", error.message),
    }

    let status = child.wait().expect("worker should exit");
    assert!(status.success(), "worker command should succeed");
}

fn sample_context() -> UnconsciousContext {
    UnconsciousContext {
        context_id: uuid::Uuid::now_v7(),
        assembled_at: chrono::Utc::now(),
        job_id: uuid::Uuid::now_v7(),
        job_kind: UnconsciousJobKind::MemoryConsolidation,
        trigger: BackgroundTrigger {
            trigger_id: uuid::Uuid::now_v7(),
            trigger_kind: BackgroundTriggerKind::TimeSchedule,
            requested_at: chrono::Utc::now(),
            reason_summary: "nightly maintenance window".to_string(),
            payload_ref: None,
        },
        scope: UnconsciousScope {
            episode_ids: vec![uuid::Uuid::now_v7()],
            memory_artifact_ids: vec![uuid::Uuid::now_v7()],
            retrieval_artifact_ids: vec![uuid::Uuid::now_v7()],
            self_model_artifact_id: None,
            internal_principal_ref: Some("primary-user".to_string()),
            internal_conversation_ref: Some("telegram-primary".to_string()),
            summary: "Consolidate the latest scoped memory batch.".to_string(),
        },
        evidence: None,
        budget: BackgroundExecutionBudget {
            iteration_budget: 2,
            wall_clock_budget_ms: 120_000,
            token_budget: 6_000,
        },
    }
}
