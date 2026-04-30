CREATE TABLE IF NOT EXISTS model_call_records (
    model_call_id UUID PRIMARY KEY,
    trace_id UUID NOT NULL,
    execution_id UUID NULL REFERENCES execution_records (execution_id) ON DELETE SET NULL,
    loop_kind TEXT NOT NULL CHECK (
        loop_kind IN ('foreground', 'background', 'management', 'unknown')
    ),
    purpose TEXT NOT NULL,
    task_class TEXT NULL,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    request_payload_json JSONB NULL,
    response_payload_json JSONB NULL,
    system_prompt_text TEXT NULL,
    messages_json JSONB NULL,
    input_tokens INTEGER NULL CHECK (input_tokens IS NULL OR input_tokens >= 0),
    output_tokens INTEGER NULL CHECK (output_tokens IS NULL OR output_tokens >= 0),
    finish_reason TEXT NULL,
    status TEXT NOT NULL CHECK (
        status IN ('pending', 'succeeded', 'failed')
    ),
    error_summary TEXT NULL,
    started_at TIMESTAMPTZ NOT NULL,
    completed_at TIMESTAMPTZ NULL,
    payload_retention_expires_at TIMESTAMPTZ NULL,
    payload_cleared_at TIMESTAMPTZ NULL,
    payload_retention_reason TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (
        completed_at IS NULL
        OR completed_at >= started_at
    ),
    CHECK (
        (payload_cleared_at IS NULL AND payload_retention_reason IS NULL)
        OR (payload_cleared_at IS NOT NULL AND payload_retention_reason IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS model_call_records_trace_idx
    ON model_call_records (trace_id, started_at DESC);

CREATE INDEX IF NOT EXISTS model_call_records_execution_idx
    ON model_call_records (execution_id, started_at DESC)
    WHERE execution_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS model_call_records_started_idx
    ON model_call_records (started_at DESC);

CREATE INDEX IF NOT EXISTS model_call_records_loop_kind_idx
    ON model_call_records (loop_kind, started_at DESC);

CREATE INDEX IF NOT EXISTS model_call_records_status_idx
    ON model_call_records (status, started_at DESC);

CREATE INDEX IF NOT EXISTS model_call_records_retention_idx
    ON model_call_records (payload_retention_expires_at ASC)
    WHERE payload_retention_expires_at IS NOT NULL
      AND payload_cleared_at IS NULL;
