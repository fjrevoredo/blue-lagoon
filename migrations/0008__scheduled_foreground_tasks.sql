CREATE TABLE IF NOT EXISTS scheduled_foreground_tasks (
    scheduled_foreground_task_id UUID PRIMARY KEY,
    task_key TEXT NOT NULL UNIQUE,
    channel_kind TEXT NOT NULL CHECK (channel_kind IN ('telegram')),
    status TEXT NOT NULL CHECK (status IN ('active', 'paused', 'disabled')),
    internal_principal_ref TEXT NOT NULL,
    internal_conversation_ref TEXT NOT NULL,
    message_text TEXT NOT NULL,
    cadence_seconds BIGINT NOT NULL CHECK (cadence_seconds >= 1),
    cooldown_seconds BIGINT NOT NULL CHECK (cooldown_seconds >= 0),
    next_due_at TIMESTAMPTZ NOT NULL,
    current_execution_id UUID NULL REFERENCES execution_records (execution_id) ON DELETE SET NULL,
    current_run_started_at TIMESTAMPTZ NULL,
    last_execution_id UUID NULL REFERENCES execution_records (execution_id) ON DELETE SET NULL,
    last_run_started_at TIMESTAMPTZ NULL,
    last_run_completed_at TIMESTAMPTZ NULL,
    last_outcome TEXT NULL CHECK (
        last_outcome IS NULL
        OR last_outcome IN ('completed', 'suppressed', 'failed')
    ),
    last_outcome_reason TEXT NULL,
    last_outcome_summary TEXT NULL,
    created_by TEXT NOT NULL,
    updated_by TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (
        (current_execution_id IS NULL AND current_run_started_at IS NULL)
        OR (current_execution_id IS NOT NULL AND current_run_started_at IS NOT NULL)
    ),
    CHECK (
        (last_execution_id IS NULL AND last_run_started_at IS NULL AND last_run_completed_at IS NULL)
        OR (last_execution_id IS NOT NULL AND last_run_started_at IS NOT NULL)
    ),
    CHECK (
        last_run_completed_at IS NULL
        OR last_run_started_at IS NOT NULL
    ),
    CHECK (
        last_run_completed_at IS NULL
        OR last_run_completed_at >= last_run_started_at
    )
);

CREATE INDEX IF NOT EXISTS scheduled_foreground_tasks_status_due_idx
    ON scheduled_foreground_tasks (status, next_due_at ASC);

CREATE INDEX IF NOT EXISTS scheduled_foreground_tasks_conversation_idx
    ON scheduled_foreground_tasks (internal_conversation_ref, next_due_at ASC);

CREATE INDEX IF NOT EXISTS scheduled_foreground_tasks_current_execution_idx
    ON scheduled_foreground_tasks (current_execution_id)
    WHERE current_execution_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS scheduled_foreground_tasks_last_execution_idx
    ON scheduled_foreground_tasks (last_execution_id)
    WHERE last_execution_id IS NOT NULL;
