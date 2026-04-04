use contracts::{WorkerRequest, WorkerResponse, WorkerResult};

#[test]
fn smoke_worker_cli_reads_json_and_writes_json() {
    let request = WorkerRequest::smoke(uuid::Uuid::now_v7(), uuid::Uuid::now_v7(), "smoke");
    let input = serde_json::to_string(&request).expect("request should serialize");

    let output = assert_cmd::Command::cargo_bin("workers")
        .expect("workers binary should build")
        .arg("smoke-worker")
        .write_stdin(input)
        .output()
        .expect("workers command should run");

    assert!(output.status.success(), "worker command should succeed");
    let response: WorkerResponse =
        serde_json::from_slice(&output.stdout).expect("response should deserialize");
    match response.result {
        WorkerResult::Smoke(result) => {
            assert_eq!(result.status, "completed");
            assert!(result.summary.contains("smoke"));
        }
        WorkerResult::Error(error) => panic!("unexpected worker error: {}", error.message),
    }
}
