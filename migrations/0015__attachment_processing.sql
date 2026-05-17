CREATE TABLE IF NOT EXISTS ingress_attachments (
    ingress_attachment_id UUID PRIMARY KEY,
    ingress_id UUID NOT NULL REFERENCES ingress_events (ingress_id) ON DELETE CASCADE,
    trace_id UUID NOT NULL,
    execution_id UUID NULL REFERENCES execution_records (execution_id),
    internal_principal_ref TEXT NOT NULL,
    internal_conversation_ref TEXT NOT NULL,
    channel_kind TEXT NOT NULL,
    attachment_id TEXT NOT NULL,
    media_type TEXT NULL,
    file_name TEXT NULL,
    size_bytes BIGINT NULL CHECK (size_bytes IS NULL OR size_bytes >= 0),
    raw_payload_ref TEXT NULL,
    processing_status TEXT NOT NULL,
    latest_processing_attempt_id UUID NULL,
    latest_extracted_artifact_id UUID NULL,
    last_failure_reason TEXT NULL,
    last_processed_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (ingress_id, attachment_id)
);

CREATE INDEX IF NOT EXISTS ingress_attachments_ingress_idx
    ON ingress_attachments (ingress_id, created_at DESC);

CREATE INDEX IF NOT EXISTS ingress_attachments_processing_status_idx
    ON ingress_attachments (processing_status, updated_at DESC);

CREATE INDEX IF NOT EXISTS ingress_attachments_conversation_idx
    ON ingress_attachments (internal_conversation_ref, updated_at DESC);

CREATE TABLE IF NOT EXISTS ingress_attachment_processing_attempts (
    ingress_attachment_processing_attempt_id UUID PRIMARY KEY,
    ingress_attachment_id UUID NOT NULL REFERENCES ingress_attachments (ingress_attachment_id) ON DELETE CASCADE,
    trace_id UUID NOT NULL,
    execution_id UUID NULL REFERENCES execution_records (execution_id),
    requested_by TEXT NOT NULL,
    request_kind TEXT NOT NULL,
    status TEXT NOT NULL,
    extractor_kind TEXT NULL,
    detail TEXT NULL,
    bytes_processed BIGINT NULL CHECK (bytes_processed IS NULL OR bytes_processed >= 0),
    extracted_chars INTEGER NULL CHECK (extracted_chars IS NULL OR extracted_chars >= 0),
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ NULL
);

CREATE INDEX IF NOT EXISTS ingress_attachment_processing_attempts_attachment_idx
    ON ingress_attachment_processing_attempts (ingress_attachment_id, started_at DESC);

CREATE INDEX IF NOT EXISTS ingress_attachment_processing_attempts_status_idx
    ON ingress_attachment_processing_attempts (status, started_at DESC);

CREATE TABLE IF NOT EXISTS ingress_attachment_extracted_artifacts (
    ingress_attachment_extracted_artifact_id UUID PRIMARY KEY,
    ingress_attachment_id UUID NOT NULL REFERENCES ingress_attachments (ingress_attachment_id) ON DELETE CASCADE,
    ingress_attachment_processing_attempt_id UUID NOT NULL REFERENCES ingress_attachment_processing_attempts (ingress_attachment_processing_attempt_id) ON DELETE CASCADE,
    trace_id UUID NOT NULL,
    execution_id UUID NULL REFERENCES execution_records (execution_id),
    extractor_kind TEXT NOT NULL,
    content_format TEXT NOT NULL,
    content_text TEXT NOT NULL,
    summary_text TEXT NOT NULL,
    content_sha256 TEXT NOT NULL,
    content_chars INTEGER NOT NULL CHECK (content_chars >= 0),
    metadata_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS ingress_attachment_extracted_artifacts_attachment_idx
    ON ingress_attachment_extracted_artifacts (ingress_attachment_id, created_at DESC);

CREATE INDEX IF NOT EXISTS ingress_attachment_extracted_artifacts_attempt_idx
    ON ingress_attachment_extracted_artifacts (ingress_attachment_processing_attempt_id);

ALTER TABLE governed_action_executions
    DROP CONSTRAINT governed_action_executions_action_kind_check;

ALTER TABLE governed_action_executions
    ADD CONSTRAINT governed_action_executions_action_kind_check CHECK (
        action_kind IN (
            'inspect_workspace_artifact',
            'list_workspace_artifacts',
            'create_workspace_artifact',
            'update_workspace_artifact',
            'list_workspace_scripts',
            'inspect_workspace_script',
            'create_workspace_script',
            'append_workspace_script_version',
            'list_workspace_script_runs',
            'inspect_ingress_attachments',
            'process_ingress_attachment',
            'upsert_scheduled_foreground_task',
            'request_background_job',
            'run_diagnostic',
            'run_subprocess',
            'run_workspace_script',
            'web_fetch'
        )
    );

ALTER TABLE approval_requests
    DROP CONSTRAINT approval_requests_action_kind_check;

ALTER TABLE approval_requests
    ADD CONSTRAINT approval_requests_action_kind_check CHECK (
        action_kind IN (
            'inspect_workspace_artifact',
            'list_workspace_artifacts',
            'create_workspace_artifact',
            'update_workspace_artifact',
            'list_workspace_scripts',
            'inspect_workspace_script',
            'create_workspace_script',
            'append_workspace_script_version',
            'list_workspace_script_runs',
            'inspect_ingress_attachments',
            'process_ingress_attachment',
            'upsert_scheduled_foreground_task',
            'request_background_job',
            'run_diagnostic',
            'run_subprocess',
            'run_workspace_script',
            'web_fetch'
        )
    );
