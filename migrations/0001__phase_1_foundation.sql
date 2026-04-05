CREATE TABLE IF NOT EXISTS audit_events (
    event_id UUID PRIMARY KEY,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    loop_kind TEXT NOT NULL,
    subsystem TEXT NOT NULL,
    event_kind TEXT NOT NULL,
    severity TEXT NOT NULL,
    trace_id UUID NOT NULL,
    span_id TEXT NULL,
    parent_span_id TEXT NULL,
    execution_id UUID NULL,
    worker_pid INTEGER NULL,
    model_tier TEXT NULL,
    payload JSONB NOT NULL DEFAULT '{}'::JSONB
);

CREATE INDEX IF NOT EXISTS audit_events_occurred_at_idx ON audit_events (occurred_at);
CREATE INDEX IF NOT EXISTS audit_events_trace_id_idx ON audit_events (trace_id);
CREATE INDEX IF NOT EXISTS audit_events_execution_id_idx ON audit_events (execution_id);

CREATE TABLE IF NOT EXISTS execution_records (
    execution_id UUID PRIMARY KEY,
    trace_id UUID NOT NULL,
    trigger_kind TEXT NOT NULL,
    synthetic_trigger TEXT NULL,
    status TEXT NOT NULL,
    worker_kind TEXT NULL,
    worker_pid INTEGER NULL,
    request_payload JSONB NOT NULL DEFAULT '{}'::JSONB,
    response_payload JSONB NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ NULL
);

CREATE INDEX IF NOT EXISTS execution_records_trace_id_idx ON execution_records (trace_id);
CREATE INDEX IF NOT EXISTS execution_records_status_idx ON execution_records (status);
