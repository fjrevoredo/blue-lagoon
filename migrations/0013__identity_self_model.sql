CREATE TABLE IF NOT EXISTS identity_lifecycle (
    identity_lifecycle_id UUID PRIMARY KEY,
    status TEXT NOT NULL,
    lifecycle_state TEXT NOT NULL,
    active_self_model_artifact_id UUID NULL REFERENCES self_model_artifacts (self_model_artifact_id) ON DELETE SET NULL,
    active_interview_id UUID NULL,
    transition_reason TEXT NOT NULL,
    transitioned_by TEXT NOT NULL,
    kickstart_started_at TIMESTAMPTZ NULL,
    kickstart_completed_at TIMESTAMPTZ NULL,
    reset_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    payload_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    CHECK (status IN ('current', 'superseded')),
    CHECK (lifecycle_state IN (
        'bootstrap_seed_only',
        'identity_kickstart_in_progress',
        'complete_identity_active',
        'identity_reset_pending'
    ))
);

CREATE UNIQUE INDEX IF NOT EXISTS identity_lifecycle_single_current_idx
    ON identity_lifecycle (status)
    WHERE status = 'current';

CREATE INDEX IF NOT EXISTS identity_lifecycle_state_idx
    ON identity_lifecycle (lifecycle_state, updated_at DESC);

CREATE TABLE IF NOT EXISTS identity_items (
    identity_item_id UUID PRIMARY KEY,
    self_model_artifact_id UUID NULL REFERENCES self_model_artifacts (self_model_artifact_id) ON DELETE SET NULL,
    proposal_id UUID NULL REFERENCES proposals (proposal_id) ON DELETE SET NULL,
    trace_id UUID NULL,
    stability_class TEXT NOT NULL,
    category TEXT NOT NULL,
    item_key TEXT NOT NULL,
    value_text TEXT NOT NULL,
    confidence DOUBLE PRECISION NOT NULL CHECK (confidence >= 0.0 AND confidence <= 1.0),
    weight DOUBLE PRECISION NULL CHECK (weight IS NULL OR (weight >= 0.0 AND weight <= 1.0)),
    provenance_kind TEXT NOT NULL,
    source_kind TEXT NOT NULL,
    merge_policy TEXT NOT NULL,
    status TEXT NOT NULL,
    evidence_refs_json JSONB NOT NULL DEFAULT '[]'::JSONB,
    valid_from TIMESTAMPTZ NULL,
    valid_to TIMESTAMPTZ NULL,
    superseded_at TIMESTAMPTZ NULL,
    supersedes_item_id UUID NULL REFERENCES identity_items (identity_item_id),
    superseded_by_item_id UUID NULL REFERENCES identity_items (identity_item_id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    payload_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    CHECK (stability_class IN ('stable', 'evolving', 'transient_projection')),
    CHECK (status IN ('active', 'superseded', 'expired', 'rejected', 'deferred')),
    CHECK (evidence_refs_json IS NULL OR jsonb_typeof(evidence_refs_json) = 'array'),
    CHECK (
        superseded_by_item_id IS NULL
        OR superseded_by_item_id <> identity_item_id
    ),
    CHECK (
        supersedes_item_id IS NULL
        OR supersedes_item_id <> identity_item_id
    ),
    CHECK (
        valid_to IS NULL
        OR valid_from IS NULL
        OR valid_to >= valid_from
    )
);

CREATE INDEX IF NOT EXISTS identity_items_active_lookup_idx
    ON identity_items (status, stability_class, category, item_key, updated_at DESC);

CREATE INDEX IF NOT EXISTS identity_items_category_idx
    ON identity_items (category, status, updated_at DESC);

CREATE INDEX IF NOT EXISTS identity_items_self_model_idx
    ON identity_items (self_model_artifact_id, status, category);

CREATE INDEX IF NOT EXISTS identity_items_proposal_idx
    ON identity_items (proposal_id);

CREATE INDEX IF NOT EXISTS identity_items_supersession_idx
    ON identity_items (supersedes_item_id, superseded_by_item_id);

CREATE TABLE IF NOT EXISTS identity_templates (
    identity_template_id UUID PRIMARY KEY,
    template_key TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    description TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    payload_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    CHECK (status IN ('active', 'retired'))
);

CREATE INDEX IF NOT EXISTS identity_templates_status_idx
    ON identity_templates (status, template_key);

CREATE TABLE IF NOT EXISTS identity_template_items (
    identity_template_item_id UUID PRIMARY KEY,
    identity_template_id UUID NOT NULL REFERENCES identity_templates (identity_template_id) ON DELETE CASCADE,
    stability_class TEXT NOT NULL,
    category TEXT NOT NULL,
    item_key TEXT NOT NULL,
    value_text TEXT NOT NULL,
    confidence DOUBLE PRECISION NOT NULL CHECK (confidence >= 0.0 AND confidence <= 1.0),
    weight DOUBLE PRECISION NULL CHECK (weight IS NULL OR (weight >= 0.0 AND weight <= 1.0)),
    merge_policy TEXT NOT NULL,
    payload_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    CHECK (stability_class IN ('stable', 'evolving', 'transient_projection')),
    UNIQUE (identity_template_id, stability_class, category, item_key)
);

CREATE INDEX IF NOT EXISTS identity_template_items_template_idx
    ON identity_template_items (identity_template_id, stability_class, category);

CREATE TABLE IF NOT EXISTS identity_kickstart_interviews (
    identity_interview_id UUID PRIMARY KEY,
    status TEXT NOT NULL,
    current_step TEXT NOT NULL,
    answered_fields_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    required_fields_json JSONB NOT NULL DEFAULT '[]'::JSONB,
    last_prompt_text TEXT NULL,
    selected_template_id UUID NULL REFERENCES identity_templates (identity_template_id) ON DELETE SET NULL,
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ NULL,
    cancelled_at TIMESTAMPTZ NULL,
    payload_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    CHECK (status IN ('in_progress', 'completed', 'cancelled')),
    CHECK (jsonb_typeof(answered_fields_json) = 'object'),
    CHECK (jsonb_typeof(required_fields_json) = 'array')
);

CREATE INDEX IF NOT EXISTS identity_kickstart_interviews_status_idx
    ON identity_kickstart_interviews (status, updated_at DESC);

CREATE INDEX IF NOT EXISTS identity_kickstart_interviews_template_idx
    ON identity_kickstart_interviews (selected_template_id);

ALTER TABLE identity_lifecycle
ADD CONSTRAINT identity_lifecycle_active_interview_fk
FOREIGN KEY (active_interview_id)
REFERENCES identity_kickstart_interviews (identity_interview_id)
ON DELETE SET NULL;

CREATE TABLE IF NOT EXISTS identity_diagnostics (
    identity_diagnostic_id UUID PRIMARY KEY,
    diagnostic_kind TEXT NOT NULL,
    severity TEXT NOT NULL,
    status TEXT NOT NULL,
    identity_item_id UUID NULL REFERENCES identity_items (identity_item_id) ON DELETE SET NULL,
    proposal_id UUID NULL REFERENCES proposals (proposal_id) ON DELETE SET NULL,
    trace_id UUID NULL,
    message TEXT NOT NULL,
    evidence_refs_json JSONB NOT NULL DEFAULT '[]'::JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    resolved_at TIMESTAMPTZ NULL,
    payload_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    CHECK (severity IN ('info', 'warning', 'error')),
    CHECK (status IN ('open', 'resolved', 'deferred')),
    CHECK (jsonb_typeof(evidence_refs_json) = 'array')
);

CREATE INDEX IF NOT EXISTS identity_diagnostics_status_idx
    ON identity_diagnostics (status, severity, created_at DESC);

CREATE INDEX IF NOT EXISTS identity_diagnostics_kind_idx
    ON identity_diagnostics (diagnostic_kind, status, created_at DESC);

CREATE INDEX IF NOT EXISTS identity_diagnostics_item_idx
    ON identity_diagnostics (identity_item_id);

CREATE INDEX IF NOT EXISTS identity_diagnostics_proposal_idx
    ON identity_diagnostics (proposal_id);
