CREATE TABLE IF NOT EXISTS background_jobs (
    background_job_id UUID PRIMARY KEY,
    trace_id UUID NOT NULL,
    job_kind TEXT NOT NULL CHECK (
        job_kind IN (
            'memory_consolidation',
            'retrieval_maintenance',
            'contradiction_and_drift_scan',
            'self_model_reflection'
        )
    ),
    trigger_id UUID NOT NULL,
    trigger_kind TEXT NOT NULL CHECK (
        trigger_kind IN (
            'time_schedule',
            'volume_threshold',
            'drift_or_anomaly_signal',
            'foreground_delegation',
            'external_passive_event',
            'maintenance_trigger'
        )
    ),
    trigger_requested_at TIMESTAMPTZ NOT NULL,
    trigger_reason_summary TEXT NOT NULL,
    trigger_payload_ref TEXT NULL,
    deduplication_key TEXT NOT NULL,
    scope_summary TEXT NOT NULL,
    scope_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    iteration_budget INTEGER NOT NULL CHECK (iteration_budget > 0),
    wall_clock_budget_ms BIGINT NOT NULL CHECK (wall_clock_budget_ms > 0),
    token_budget INTEGER NOT NULL CHECK (token_budget > 0),
    status TEXT NOT NULL CHECK (
        status IN (
            'planned',
            'leased',
            'running',
            'completed',
            'failed',
            'suppressed',
            'cancelled'
        )
    ),
    available_at TIMESTAMPTZ NOT NULL,
    lease_expires_at TIMESTAMPTZ NULL,
    last_started_at TIMESTAMPTZ NULL,
    last_completed_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (
        (status = 'leased' AND lease_expires_at IS NOT NULL)
        OR (status <> 'leased')
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS background_jobs_trigger_job_kind_uidx
    ON background_jobs (trigger_id, job_kind);

CREATE UNIQUE INDEX IF NOT EXISTS background_jobs_dedup_active_uidx
    ON background_jobs (deduplication_key)
    WHERE status IN ('planned', 'leased', 'running');

CREATE INDEX IF NOT EXISTS background_jobs_due_lookup_idx
    ON background_jobs (status, available_at ASC, created_at ASC)
    WHERE status = 'planned';

CREATE INDEX IF NOT EXISTS background_jobs_trace_idx
    ON background_jobs (trace_id, created_at DESC);

CREATE INDEX IF NOT EXISTS background_jobs_trigger_lookup_idx
    ON background_jobs (trigger_kind, job_kind, trigger_requested_at DESC);

CREATE TABLE IF NOT EXISTS background_job_runs (
    background_job_run_id UUID PRIMARY KEY,
    background_job_id UUID NOT NULL REFERENCES background_jobs (background_job_id) ON DELETE CASCADE,
    trace_id UUID NOT NULL,
    execution_id UUID NULL UNIQUE REFERENCES execution_records (execution_id) ON DELETE SET NULL,
    lease_token UUID NOT NULL UNIQUE,
    status TEXT NOT NULL CHECK (
        status IN (
            'leased',
            'running',
            'completed',
            'failed',
            'timed_out'
        )
    ),
    worker_pid INTEGER NULL,
    lease_acquired_at TIMESTAMPTZ NOT NULL,
    lease_expires_at TIMESTAMPTZ NOT NULL,
    started_at TIMESTAMPTZ NULL,
    completed_at TIMESTAMPTZ NULL,
    result_payload JSONB NULL,
    failure_payload JSONB NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (lease_expires_at >= lease_acquired_at),
    CHECK (
        (status = 'leased' AND started_at IS NULL AND completed_at IS NULL)
        OR (status = 'running' AND started_at IS NOT NULL AND completed_at IS NULL)
        OR (status IN ('completed', 'failed', 'timed_out') AND started_at IS NOT NULL AND completed_at IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS background_job_runs_job_created_at_idx
    ON background_job_runs (background_job_id, created_at DESC);

CREATE INDEX IF NOT EXISTS background_job_runs_active_lease_idx
    ON background_job_runs (background_job_id, status, lease_expires_at DESC)
    WHERE status IN ('leased', 'running');

CREATE INDEX IF NOT EXISTS background_job_runs_trace_idx
    ON background_job_runs (trace_id, created_at DESC);

CREATE INDEX IF NOT EXISTS background_job_runs_completed_at_idx
    ON background_job_runs (completed_at DESC)
    WHERE completed_at IS NOT NULL;

CREATE TABLE IF NOT EXISTS wake_signals (
    wake_signal_id UUID PRIMARY KEY,
    background_job_id UUID NOT NULL REFERENCES background_jobs (background_job_id) ON DELETE CASCADE,
    background_job_run_id UUID NULL REFERENCES background_job_runs (background_job_run_id) ON DELETE SET NULL,
    trace_id UUID NOT NULL,
    execution_id UUID NULL REFERENCES execution_records (execution_id) ON DELETE SET NULL,
    reason TEXT NOT NULL CHECK (
        reason IN (
            'critical_conflict',
            'proactive_briefing_ready',
            'self_state_anomaly',
            'maintenance_insight_ready'
        )
    ),
    priority TEXT NOT NULL CHECK (
        priority IN ('low', 'normal', 'high')
    ),
    reason_code TEXT NOT NULL,
    summary TEXT NOT NULL,
    payload_ref TEXT NULL,
    status TEXT NOT NULL CHECK (
        status IN ('pending_review', 'accepted', 'rejected', 'suppressed', 'deferred')
    ),
    decision_kind TEXT NULL CHECK (
        decision_kind IS NULL
        OR decision_kind IN ('accepted', 'rejected', 'suppressed', 'deferred')
    ),
    decision_reason TEXT NULL,
    requested_at TIMESTAMPTZ NOT NULL,
    reviewed_at TIMESTAMPTZ NULL,
    cooldown_until TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (
        (decision_kind IS NULL AND decision_reason IS NULL AND reviewed_at IS NULL)
        OR (decision_kind IS NOT NULL AND decision_reason IS NOT NULL AND reviewed_at IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS wake_signals_pending_review_idx
    ON wake_signals (status, requested_at ASC)
    WHERE status = 'pending_review';

CREATE INDEX IF NOT EXISTS wake_signals_job_run_idx
    ON wake_signals (background_job_run_id, requested_at DESC);

CREATE INDEX IF NOT EXISTS wake_signals_trace_idx
    ON wake_signals (trace_id, requested_at DESC);

CREATE INDEX IF NOT EXISTS wake_signals_execution_idx
    ON wake_signals (execution_id, requested_at DESC)
    WHERE execution_id IS NOT NULL;
