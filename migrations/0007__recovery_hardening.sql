CREATE TABLE IF NOT EXISTS recovery_checkpoints (
    recovery_checkpoint_id UUID PRIMARY KEY,
    trace_id UUID NOT NULL,
    execution_id UUID NULL REFERENCES execution_records (execution_id) ON DELETE SET NULL,
    background_job_id UUID NULL REFERENCES background_jobs (background_job_id) ON DELETE SET NULL,
    background_job_run_id UUID NULL REFERENCES background_job_runs (background_job_run_id) ON DELETE SET NULL,
    governed_action_execution_id UUID NULL REFERENCES governed_action_executions (governed_action_execution_id) ON DELETE SET NULL,
    approval_request_id UUID NULL REFERENCES approval_requests (approval_request_id) ON DELETE SET NULL,
    checkpoint_kind TEXT NOT NULL CHECK (
        checkpoint_kind IN ('foreground', 'background', 'governed_action')
    ),
    recovery_reason_code TEXT NOT NULL CHECK (
        recovery_reason_code IN (
            'crash',
            'timeout_or_stall',
            'supervisor_restart',
            'approval_transition',
            'integrity_or_policy_block'
        )
    ),
    status TEXT NOT NULL CHECK (
        status IN ('open', 'resolved', 'abandoned', 'invalidated')
    ),
    recovery_decision TEXT NULL CHECK (
        recovery_decision IS NULL
        OR recovery_decision IN (
            'continue',
            'retry',
            'defer',
            'reapprove',
            'clarify',
            'abandon'
        )
    ),
    recovery_budget_remaining INTEGER NOT NULL CHECK (recovery_budget_remaining >= 0),
    checkpoint_payload_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    resolved_summary TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    resolved_at TIMESTAMPTZ NULL,
    CHECK (
        (status = 'open' AND recovery_decision IS NULL AND resolved_at IS NULL)
        OR (status <> 'open' AND resolved_at IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS recovery_checkpoints_open_idx
    ON recovery_checkpoints (created_at ASC)
    WHERE status = 'open';

CREATE INDEX IF NOT EXISTS recovery_checkpoints_trace_idx
    ON recovery_checkpoints (trace_id, created_at DESC);

CREATE INDEX IF NOT EXISTS recovery_checkpoints_execution_idx
    ON recovery_checkpoints (execution_id, created_at DESC)
    WHERE execution_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS recovery_checkpoints_reason_idx
    ON recovery_checkpoints (recovery_reason_code, created_at DESC);

CREATE TABLE IF NOT EXISTS worker_leases (
    worker_lease_id UUID PRIMARY KEY,
    trace_id UUID NOT NULL,
    execution_id UUID NULL REFERENCES execution_records (execution_id) ON DELETE SET NULL,
    background_job_id UUID NULL REFERENCES background_jobs (background_job_id) ON DELETE SET NULL,
    background_job_run_id UUID NULL REFERENCES background_job_runs (background_job_run_id) ON DELETE SET NULL,
    governed_action_execution_id UUID NULL REFERENCES governed_action_executions (governed_action_execution_id) ON DELETE SET NULL,
    worker_kind TEXT NOT NULL CHECK (
        worker_kind IN ('foreground', 'background', 'governed_action')
    ),
    status TEXT NOT NULL CHECK (
        status IN ('active', 'released', 'expired', 'terminated')
    ),
    lease_token UUID NOT NULL UNIQUE,
    worker_pid INTEGER NULL,
    lease_acquired_at TIMESTAMPTZ NOT NULL,
    lease_expires_at TIMESTAMPTZ NOT NULL,
    last_heartbeat_at TIMESTAMPTZ NOT NULL,
    released_at TIMESTAMPTZ NULL,
    metadata_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (lease_expires_at >= lease_acquired_at),
    CHECK (last_heartbeat_at >= lease_acquired_at),
    CHECK (
        (status = 'active' AND released_at IS NULL)
        OR (status <> 'active' AND released_at IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS worker_leases_active_expiry_idx
    ON worker_leases (lease_expires_at ASC)
    WHERE status = 'active';

CREATE INDEX IF NOT EXISTS worker_leases_trace_idx
    ON worker_leases (trace_id, created_at DESC);

CREATE INDEX IF NOT EXISTS worker_leases_execution_idx
    ON worker_leases (execution_id, created_at DESC)
    WHERE execution_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS worker_leases_background_job_run_idx
    ON worker_leases (background_job_run_id, created_at DESC)
    WHERE background_job_run_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS operational_diagnostics (
    operational_diagnostic_id UUID PRIMARY KEY,
    trace_id UUID NULL,
    execution_id UUID NULL REFERENCES execution_records (execution_id) ON DELETE SET NULL,
    subsystem TEXT NOT NULL,
    severity TEXT NOT NULL CHECK (
        severity IN ('info', 'warn', 'error', 'critical')
    ),
    reason_code TEXT NOT NULL,
    summary TEXT NOT NULL,
    diagnostic_payload_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS operational_diagnostics_recent_idx
    ON operational_diagnostics (created_at DESC);

CREATE INDEX IF NOT EXISTS operational_diagnostics_subsystem_idx
    ON operational_diagnostics (subsystem, created_at DESC);

CREATE INDEX IF NOT EXISTS operational_diagnostics_severity_idx
    ON operational_diagnostics (severity, created_at DESC);

CREATE INDEX IF NOT EXISTS operational_diagnostics_trace_idx
    ON operational_diagnostics (trace_id, created_at DESC)
    WHERE trace_id IS NOT NULL;
