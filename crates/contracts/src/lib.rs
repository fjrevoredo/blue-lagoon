use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerKind {
    Smoke,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerRequest {
    pub request_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Uuid,
    pub sent_at: DateTime<Utc>,
    pub worker_kind: WorkerKind,
    pub payload: WorkerPayload,
}

impl WorkerRequest {
    pub fn smoke(trace_id: Uuid, execution_id: Uuid, synthetic_trigger: impl Into<String>) -> Self {
        Self {
            request_id: Uuid::now_v7(),
            trace_id,
            execution_id,
            sent_at: Utc::now(),
            worker_kind: WorkerKind::Smoke,
            payload: WorkerPayload::Smoke(SmokeWorkerRequest {
                synthetic_trigger: synthetic_trigger.into(),
            }),
        }
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        match (&self.worker_kind, &self.payload) {
            (WorkerKind::Smoke, WorkerPayload::Smoke(_)) => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum WorkerPayload {
    Smoke(SmokeWorkerRequest),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmokeWorkerRequest {
    pub synthetic_trigger: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerResponse {
    pub request_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Uuid,
    pub finished_at: DateTime<Utc>,
    pub worker_pid: u32,
    pub result: WorkerResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum WorkerResult {
    Smoke(SmokeWorkerResult),
    Error(WorkerFailure),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmokeWorkerResult {
    pub status: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerFailure {
    pub code: WorkerErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerErrorCode {
    InvalidRequest,
    UnsupportedWorker,
    InternalFailure,
}

#[derive(Debug, Error)]
pub enum ContractError {
    #[error("worker request kind does not match payload kind")]
    WorkerPayloadMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke_request_round_trips() {
        let request = WorkerRequest::smoke(Uuid::now_v7(), Uuid::now_v7(), "smoke");
        let json = serde_json::to_string(&request).expect("request should serialize");
        let decoded: WorkerRequest =
            serde_json::from_str(&json).expect("request should deserialize");
        assert_eq!(request, decoded);
        decoded.validate().expect("request should be valid");
    }

    #[test]
    fn worker_payload_validation_rejects_mismatch() {
        let request = WorkerRequest {
            request_id: Uuid::now_v7(),
            trace_id: Uuid::now_v7(),
            execution_id: Uuid::now_v7(),
            sent_at: Utc::now(),
            worker_kind: WorkerKind::Smoke,
            payload: WorkerPayload::Smoke(SmokeWorkerRequest {
                synthetic_trigger: "smoke".to_string(),
            }),
        };

        request.validate().expect("request should be valid");
    }
}
