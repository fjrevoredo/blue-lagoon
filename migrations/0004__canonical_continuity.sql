ALTER TABLE ingress_events
ADD COLUMN IF NOT EXISTS foreground_status TEXT;

ALTER TABLE ingress_events
ADD COLUMN IF NOT EXISTS last_processed_at TIMESTAMPTZ NULL;

UPDATE ingress_events
SET foreground_status = CASE
    WHEN status = 'rejected' THEN 'rejected'
    WHEN execution_id IS NOT NULL THEN 'processed'
    ELSE 'pending'
END
WHERE foreground_status IS NULL;

ALTER TABLE ingress_events
ALTER COLUMN foreground_status SET DEFAULT 'pending';

ALTER TABLE ingress_events
ALTER COLUMN foreground_status SET NOT NULL;

CREATE INDEX IF NOT EXISTS ingress_events_backlog_lookup_idx
    ON ingress_events (internal_conversation_ref, foreground_status, occurred_at ASC)
    WHERE internal_conversation_ref IS NOT NULL;

CREATE INDEX IF NOT EXISTS ingress_events_pending_received_at_idx
    ON ingress_events (foreground_status, received_at ASC);

CREATE TABLE IF NOT EXISTS proposals (
    proposal_id UUID PRIMARY KEY,
    trace_id UUID NOT NULL,
    execution_id UUID NOT NULL REFERENCES execution_records (execution_id),
    episode_id UUID NULL REFERENCES episodes (episode_id) ON DELETE SET NULL,
    source_ingress_id UUID NULL REFERENCES ingress_events (ingress_id) ON DELETE SET NULL,
    source_loop_kind TEXT NOT NULL,
    proposal_kind TEXT NOT NULL,
    canonical_target TEXT NOT NULL,
    status TEXT NOT NULL,
    confidence DOUBLE PRECISION NOT NULL CHECK (confidence >= 0.0 AND confidence <= 1.0),
    conflict_posture TEXT NOT NULL,
    subject_ref TEXT NOT NULL,
    content_text TEXT NOT NULL,
    rationale TEXT NULL,
    valid_from TIMESTAMPTZ NULL,
    valid_to TIMESTAMPTZ NULL,
    supersedes_artifact_id UUID NULL,
    supersedes_artifact_kind TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    payload_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    CHECK (
        (supersedes_artifact_id IS NULL AND supersedes_artifact_kind IS NULL)
        OR (supersedes_artifact_id IS NOT NULL AND supersedes_artifact_kind IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS proposals_execution_created_at_idx
    ON proposals (execution_id, created_at DESC);

CREATE INDEX IF NOT EXISTS proposals_target_status_idx
    ON proposals (canonical_target, status, created_at DESC);

CREATE INDEX IF NOT EXISTS proposals_episode_idx
    ON proposals (episode_id);

CREATE INDEX IF NOT EXISTS proposals_trace_idx
    ON proposals (trace_id);

CREATE TABLE IF NOT EXISTS memory_artifacts (
    memory_artifact_id UUID PRIMARY KEY,
    proposal_id UUID NOT NULL UNIQUE REFERENCES proposals (proposal_id),
    trace_id UUID NOT NULL,
    execution_id UUID NOT NULL REFERENCES execution_records (execution_id),
    episode_id UUID NULL REFERENCES episodes (episode_id) ON DELETE SET NULL,
    source_ingress_id UUID NULL REFERENCES ingress_events (ingress_id) ON DELETE SET NULL,
    artifact_kind TEXT NOT NULL,
    subject_ref TEXT NOT NULL,
    content_text TEXT NOT NULL,
    confidence DOUBLE PRECISION NOT NULL CHECK (confidence >= 0.0 AND confidence <= 1.0),
    provenance_kind TEXT NOT NULL,
    status TEXT NOT NULL,
    valid_from TIMESTAMPTZ NULL,
    valid_to TIMESTAMPTZ NULL,
    superseded_at TIMESTAMPTZ NULL,
    superseded_by_artifact_id UUID NULL REFERENCES memory_artifacts (memory_artifact_id),
    supersedes_artifact_id UUID NULL REFERENCES memory_artifacts (memory_artifact_id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    payload_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    CHECK (
        superseded_by_artifact_id IS NULL
        OR superseded_by_artifact_id <> memory_artifact_id
    ),
    CHECK (
        supersedes_artifact_id IS NULL
        OR supersedes_artifact_id <> memory_artifact_id
    )
);

CREATE INDEX IF NOT EXISTS memory_artifacts_active_lookup_idx
    ON memory_artifacts (status, artifact_kind, created_at DESC);

CREATE INDEX IF NOT EXISTS memory_artifacts_subject_idx
    ON memory_artifacts (subject_ref, created_at DESC);

CREATE INDEX IF NOT EXISTS memory_artifacts_episode_idx
    ON memory_artifacts (episode_id);

CREATE INDEX IF NOT EXISTS memory_artifacts_supersession_idx
    ON memory_artifacts (supersedes_artifact_id, superseded_by_artifact_id);

CREATE TABLE IF NOT EXISTS self_model_artifacts (
    self_model_artifact_id UUID PRIMARY KEY,
    proposal_id UUID NULL UNIQUE REFERENCES proposals (proposal_id) ON DELETE SET NULL,
    trace_id UUID NULL,
    execution_id UUID NULL REFERENCES execution_records (execution_id),
    episode_id UUID NULL REFERENCES episodes (episode_id) ON DELETE SET NULL,
    artifact_origin TEXT NOT NULL,
    status TEXT NOT NULL,
    stable_identity TEXT NOT NULL,
    role TEXT NOT NULL,
    communication_style TEXT NOT NULL,
    capabilities_json JSONB NOT NULL DEFAULT '[]'::JSONB,
    constraints_json JSONB NOT NULL DEFAULT '[]'::JSONB,
    preferences_json JSONB NOT NULL DEFAULT '[]'::JSONB,
    current_goals_json JSONB NOT NULL DEFAULT '[]'::JSONB,
    current_subgoals_json JSONB NOT NULL DEFAULT '[]'::JSONB,
    superseded_at TIMESTAMPTZ NULL,
    superseded_by_artifact_id UUID NULL REFERENCES self_model_artifacts (self_model_artifact_id),
    supersedes_artifact_id UUID NULL REFERENCES self_model_artifacts (self_model_artifact_id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    payload_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    CHECK (
        superseded_by_artifact_id IS NULL
        OR superseded_by_artifact_id <> self_model_artifact_id
    ),
    CHECK (
        supersedes_artifact_id IS NULL
        OR supersedes_artifact_id <> self_model_artifact_id
    )
);

CREATE INDEX IF NOT EXISTS self_model_artifacts_status_created_at_idx
    ON self_model_artifacts (status, created_at DESC);

CREATE INDEX IF NOT EXISTS self_model_artifacts_execution_idx
    ON self_model_artifacts (execution_id);

CREATE TABLE IF NOT EXISTS retrieval_artifacts (
    retrieval_artifact_id UUID PRIMARY KEY,
    source_kind TEXT NOT NULL,
    source_episode_id UUID NULL REFERENCES episodes (episode_id) ON DELETE CASCADE,
    source_memory_artifact_id UUID NULL REFERENCES memory_artifacts (memory_artifact_id) ON DELETE CASCADE,
    internal_conversation_ref TEXT NULL,
    lexical_document TEXT NOT NULL,
    relevance_timestamp TIMESTAMPTZ NOT NULL,
    status TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    payload_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    CHECK (
        (
            source_episode_id IS NOT NULL
            AND source_memory_artifact_id IS NULL
            AND source_kind = 'episode'
        )
        OR (
            source_episode_id IS NULL
            AND source_memory_artifact_id IS NOT NULL
            AND source_kind = 'memory_artifact'
        )
    )
);

CREATE INDEX IF NOT EXISTS retrieval_artifacts_status_recency_idx
    ON retrieval_artifacts (status, relevance_timestamp DESC);

CREATE INDEX IF NOT EXISTS retrieval_artifacts_conversation_idx
    ON retrieval_artifacts (internal_conversation_ref, relevance_timestamp DESC);

CREATE INDEX IF NOT EXISTS retrieval_artifacts_episode_idx
    ON retrieval_artifacts (source_episode_id);

CREATE INDEX IF NOT EXISTS retrieval_artifacts_memory_idx
    ON retrieval_artifacts (source_memory_artifact_id);

CREATE TABLE IF NOT EXISTS merge_decisions (
    merge_decision_id UUID PRIMARY KEY,
    proposal_id UUID NOT NULL UNIQUE REFERENCES proposals (proposal_id) ON DELETE CASCADE,
    trace_id UUID NOT NULL,
    execution_id UUID NOT NULL REFERENCES execution_records (execution_id),
    episode_id UUID NULL REFERENCES episodes (episode_id) ON DELETE SET NULL,
    decision_kind TEXT NOT NULL,
    decision_reason TEXT NOT NULL,
    accepted_memory_artifact_id UUID NULL REFERENCES memory_artifacts (memory_artifact_id),
    accepted_self_model_artifact_id UUID NULL REFERENCES self_model_artifacts (self_model_artifact_id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    payload_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    CHECK (
        (accepted_memory_artifact_id IS NULL AND accepted_self_model_artifact_id IS NULL)
        OR (accepted_memory_artifact_id IS NOT NULL AND accepted_self_model_artifact_id IS NULL)
        OR (accepted_memory_artifact_id IS NULL AND accepted_self_model_artifact_id IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS merge_decisions_execution_idx
    ON merge_decisions (execution_id, created_at DESC);

CREATE INDEX IF NOT EXISTS merge_decisions_trace_idx
    ON merge_decisions (trace_id, created_at DESC);

CREATE TABLE IF NOT EXISTS execution_ingress_links (
    execution_ingress_link_id UUID PRIMARY KEY,
    execution_id UUID NOT NULL REFERENCES execution_records (execution_id) ON DELETE CASCADE,
    ingress_id UUID NOT NULL REFERENCES ingress_events (ingress_id) ON DELETE CASCADE,
    link_role TEXT NOT NULL,
    sequence_index INTEGER NOT NULL CHECK (sequence_index >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (execution_id, ingress_id),
    UNIQUE (execution_id, sequence_index)
);

CREATE INDEX IF NOT EXISTS execution_ingress_links_ingress_idx
    ON execution_ingress_links (ingress_id);
