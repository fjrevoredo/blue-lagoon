use contracts::{
    ChannelKind, ConsciousContext, ConsciousWorkerInboundMessage, ConsciousWorkerOutboundMessage,
    ForegroundBudget, ForegroundTrigger, ForegroundTriggerKind, IngressEventKind,
    InternalStateSnapshot, ModelCallResponse, ModelOutput, ModelProviderKind, ModelUsage,
    NormalizedIngress, SelfModelSnapshot, WorkerRequest, WorkerResult,
};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
fn conscious_worker_cli_emits_model_request_and_final_response() {
    let request =
        WorkerRequest::conscious(uuid::Uuid::now_v7(), uuid::Uuid::now_v7(), sample_context());
    let request_json = serde_json::to_string(&request).expect("request should serialize");

    let mut child = Command::new(env!("CARGO_BIN_EXE_workers"))
        .arg("conscious-worker")
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
            panic!("first conscious worker line should be a model call request")
        }
    };

    let model_response = ModelCallResponse {
        request_id: model_request.request_id,
        trace_id: request.trace_id,
        execution_id: request.execution_id,
        provider: ModelProviderKind::ZAi,
        model: "z-ai-foreground".to_string(),
        received_at: chrono::Utc::now(),
        output: ModelOutput {
            text: "hello from model".to_string(),
            json: None,
            finish_reason: "stop".to_string(),
        },
        usage: ModelUsage {
            input_tokens: 10,
            output_tokens: 4,
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
                panic!("second conscious worker line should be a final response")
            }
            ConsciousWorkerOutboundMessage::FinalResponse(response) => response,
        };

    assert_eq!(model_request.trace_id, request.trace_id);
    assert_eq!(model_request.execution_id, request.execution_id);
    match final_response.result {
        WorkerResult::Conscious(result) => {
            assert_eq!(result.assistant_output.text, "hello from model");
            assert_eq!(
                result.assistant_output.internal_conversation_ref,
                "telegram-primary"
            );
            assert_eq!(result.episode_summary.outcome, "completed");
            assert_eq!(result.candidate_proposals.len(), 1);
        }
        WorkerResult::Smoke(_) => panic!("unexpected smoke response"),
        WorkerResult::Unconscious(_) => panic!("unexpected unconscious response"),
        WorkerResult::Error(error) => panic!("unexpected worker error: {}", error.message),
    }

    let status = child.wait().expect("worker should exit");
    assert!(status.success(), "worker command should succeed");
}

fn sample_context() -> ConsciousContext {
    ConsciousContext {
        context_id: uuid::Uuid::now_v7(),
        assembled_at: chrono::Utc::now(),
        trigger: ForegroundTrigger {
            trigger_id: uuid::Uuid::now_v7(),
            trace_id: uuid::Uuid::now_v7(),
            execution_id: uuid::Uuid::now_v7(),
            trigger_kind: ForegroundTriggerKind::UserIngress,
            ingress: NormalizedIngress {
                ingress_id: uuid::Uuid::now_v7(),
                channel_kind: ChannelKind::Telegram,
                external_user_id: "42".to_string(),
                external_conversation_id: "42".to_string(),
                external_event_id: "update-42".to_string(),
                external_message_id: Some("message-42".to_string()),
                internal_principal_ref: "primary-user".to_string(),
                internal_conversation_ref: "telegram-primary".to_string(),
                event_kind: IngressEventKind::MessageCreated,
                occurred_at: chrono::Utc::now(),
                text_body: Some("remember that I prefer concise replies".to_string()),
                reply_to: None,
                attachments: Vec::new(),
                command_hint: None,
                approval_payload: None,
                raw_payload_ref: None,
            },
            received_at: chrono::Utc::now(),
            deduplication_key: "telegram:update-42".to_string(),
            budget: ForegroundBudget {
                iteration_budget: 1,
                wall_clock_budget_ms: 30_000,
                token_budget: 4_000,
            },
        },
        self_model: SelfModelSnapshot {
            stable_identity: "blue-lagoon".to_string(),
            role: "personal_assistant".to_string(),
            communication_style: "direct".to_string(),
            capabilities: vec!["conversation".to_string()],
            constraints: vec!["respect_harness_policy".to_string()],
            preferences: vec!["concise".to_string()],
            current_goals: vec!["support_the_user".to_string()],
            current_subgoals: vec!["reply_to_current_message".to_string()],
            identity: None,
            identity_lifecycle: Default::default(),
        },
        internal_state: InternalStateSnapshot {
            load_pct: 15,
            health_pct: 100,
            reliability_pct: 100,
            resource_pressure_pct: 10,
            confidence_pct: 80,
            connection_quality_pct: 95,
            active_conditions: Vec::new(),
        },
        recent_history: vec![],
        retrieved_context: contracts::RetrievedContext::default(),
        governed_action_observations: Vec::new(),
        recovery_context: contracts::ForegroundRecoveryContext::default(),
    }
}
