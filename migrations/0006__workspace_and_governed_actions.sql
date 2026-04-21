CREATE TABLE IF NOT EXISTS workspace_artifacts (
    workspace_artifact_id UUID PRIMARY KEY,
    trace_id UUID NULL,
    execution_id UUID NULL REFERENCES execution_records (execution_id) ON DELETE SET NULL,
    artifact_kind TEXT NOT NULL CHECK (
        artifact_kind IN ('note', 'runbook', 'scratchpad', 'task_list', 'script')
    ),
    title TEXT NOT NULL,
    content_text TEXT NULL,
    status TEXT NOT NULL CHECK (
        status IN ('active', 'archived')
    ),
    metadata_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS workspace_artifacts_recent_history_idx
    ON workspace_artifacts (artifact_kind, updated_at DESC);

CREATE INDEX IF NOT EXISTS workspace_artifacts_trace_idx
    ON workspace_artifacts (trace_id, created_at DESC)
    WHERE trace_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS workspace_scripts (
    workspace_script_id UUID PRIMARY KEY,
    workspace_artifact_id UUID NOT NULL UNIQUE REFERENCES workspace_artifacts (workspace_artifact_id) ON DELETE CASCADE,
    language TEXT NOT NULL,
    entrypoint TEXT NULL,
    latest_version INTEGER NOT NULL CHECK (latest_version > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS workspace_scripts_language_idx
    ON workspace_scripts (language, updated_at DESC);

CREATE TABLE IF NOT EXISTS workspace_script_versions (
    workspace_script_version_id UUID PRIMARY KEY,
    workspace_script_id UUID NOT NULL REFERENCES workspace_scripts (workspace_script_id) ON DELETE CASCADE,
    version INTEGER NOT NULL CHECK (version > 0),
    content_text TEXT NOT NULL,
    content_sha256 TEXT NOT NULL,
    change_summary TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (workspace_script_id, version)
);

CREATE INDEX IF NOT EXISTS workspace_script_versions_lookup_idx
    ON workspace_script_versions (workspace_script_id, version DESC);

CREATE INDEX IF NOT EXISTS workspace_script_versions_sha_idx
    ON workspace_script_versions (content_sha256);

CREATE TABLE IF NOT EXISTS approval_requests (
    approval_request_id UUID PRIMARY KEY,
    trace_id UUID NOT NULL,
    execution_id UUID NULL REFERENCES execution_records (execution_id) ON DELETE SET NULL,
    action_proposal_id UUID NOT NULL,
    action_fingerprint TEXT NOT NULL,
    action_kind TEXT NOT NULL CHECK (
        action_kind IN (
            'inspect_workspace_artifact',
            'run_subprocess',
            'run_workspace_script'
        )
    ),
    risk_tier TEXT NOT NULL CHECK (
        risk_tier IN ('tier_0', 'tier_1', 'tier_2', 'tier_3')
    ),
    title TEXT NOT NULL,
    consequence_summary TEXT NOT NULL,
    capability_scope_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    status TEXT NOT NULL CHECK (
        status IN ('pending', 'approved', 'rejected', 'expired', 'invalidated')
    ),
    requested_by TEXT NOT NULL,
    token TEXT NOT NULL UNIQUE,
    requested_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    resolved_at TIMESTAMPTZ NULL,
    resolution_kind TEXT NULL CHECK (
        resolution_kind IS NULL
        OR resolution_kind IN ('approved', 'rejected', 'expired', 'invalidated')
    ),
    resolved_by TEXT NULL,
    resolution_reason TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (expires_at > requested_at),
    CHECK (
        (resolution_kind IS NULL AND resolved_by IS NULL AND resolution_reason IS NULL AND resolved_at IS NULL)
        OR (resolution_kind IS NOT NULL AND resolved_by IS NOT NULL AND resolved_at IS NOT NULL)
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS approval_requests_pending_fingerprint_uidx
    ON approval_requests (action_fingerprint)
    WHERE status = 'pending';

CREATE INDEX IF NOT EXISTS approval_requests_token_lookup_idx
    ON approval_requests (token);

CREATE INDEX IF NOT EXISTS approval_requests_pending_lookup_idx
    ON approval_requests (status, expires_at ASC, requested_at ASC)
    WHERE status = 'pending';

CREATE INDEX IF NOT EXISTS approval_requests_trace_idx
    ON approval_requests (trace_id, requested_at DESC);

CREATE TABLE IF NOT EXISTS governed_action_executions (
    governed_action_execution_id UUID PRIMARY KEY,
    trace_id UUID NOT NULL,
    execution_id UUID NULL UNIQUE REFERENCES execution_records (execution_id) ON DELETE SET NULL,
    approval_request_id UUID NULL REFERENCES approval_requests (approval_request_id) ON DELETE SET NULL,
    action_proposal_id UUID NOT NULL,
    action_fingerprint TEXT NOT NULL,
    action_kind TEXT NOT NULL CHECK (
        action_kind IN (
            'inspect_workspace_artifact',
            'run_subprocess',
            'run_workspace_script'
        )
    ),
    risk_tier TEXT NOT NULL CHECK (
        risk_tier IN ('tier_0', 'tier_1', 'tier_2', 'tier_3')
    ),
    status TEXT NOT NULL CHECK (
        status IN (
            'proposed',
            'awaiting_approval',
            'approved',
            'rejected',
            'expired',
            'invalidated',
            'blocked',
            'executed',
            'failed'
        )
    ),
    capability_scope_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    payload_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    workspace_script_id UUID NULL REFERENCES workspace_scripts (workspace_script_id) ON DELETE SET NULL,
    workspace_script_version_id UUID NULL REFERENCES workspace_script_versions (workspace_script_version_id) ON DELETE SET NULL,
    blocked_reason TEXT NULL,
    output_ref TEXT NULL,
    started_at TIMESTAMPTZ NULL,
    completed_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (
        (completed_at IS NULL)
        OR (started_at IS NOT NULL AND completed_at >= started_at)
    )
);

CREATE INDEX IF NOT EXISTS governed_action_executions_fingerprint_idx
    ON governed_action_executions (action_fingerprint, created_at DESC);

CREATE INDEX IF NOT EXISTS governed_action_executions_status_idx
    ON governed_action_executions (status, created_at DESC);

CREATE INDEX IF NOT EXISTS governed_action_executions_blocked_idx
    ON governed_action_executions (created_at DESC)
    WHERE status IN ('blocked', 'rejected', 'expired', 'invalidated', 'failed');

CREATE INDEX IF NOT EXISTS governed_action_executions_trace_idx
    ON governed_action_executions (trace_id, created_at DESC);

CREATE TABLE IF NOT EXISTS workspace_script_runs (
    workspace_script_run_id UUID PRIMARY KEY,
    workspace_script_id UUID NOT NULL REFERENCES workspace_scripts (workspace_script_id) ON DELETE CASCADE,
    workspace_script_version_id UUID NOT NULL REFERENCES workspace_script_versions (workspace_script_version_id) ON DELETE RESTRICT,
    trace_id UUID NOT NULL,
    execution_id UUID NULL UNIQUE REFERENCES execution_records (execution_id) ON DELETE SET NULL,
    governed_action_execution_id UUID NULL UNIQUE REFERENCES governed_action_executions (governed_action_execution_id) ON DELETE SET NULL,
    approval_request_id UUID NULL REFERENCES approval_requests (approval_request_id) ON DELETE SET NULL,
    status TEXT NOT NULL CHECK (
        status IN ('pending', 'running', 'completed', 'failed', 'timed_out', 'blocked')
    ),
    risk_tier TEXT NOT NULL CHECK (
        risk_tier IN ('tier_0', 'tier_1', 'tier_2', 'tier_3')
    ),
    args_json JSONB NOT NULL DEFAULT '[]'::JSONB,
    output_ref TEXT NULL,
    failure_summary TEXT NULL,
    started_at TIMESTAMPTZ NULL,
    completed_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (
        (completed_at IS NULL)
        OR (started_at IS NOT NULL AND completed_at >= started_at)
    )
);

CREATE INDEX IF NOT EXISTS workspace_script_runs_recent_idx
    ON workspace_script_runs (workspace_script_id, created_at DESC);

CREATE INDEX IF NOT EXISTS workspace_script_runs_trace_idx
    ON workspace_script_runs (trace_id, created_at DESC);

CREATE INDEX IF NOT EXISTS workspace_script_runs_status_idx
    ON workspace_script_runs (status, created_at DESC);
