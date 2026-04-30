CREATE TABLE IF NOT EXISTS causal_links (
    causal_link_id UUID PRIMARY KEY,
    trace_id UUID NOT NULL,
    source_kind TEXT NOT NULL,
    source_id UUID NOT NULL,
    target_kind TEXT NOT NULL,
    target_id UUID NOT NULL,
    edge_kind TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    payload_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    UNIQUE (source_kind, source_id, target_kind, target_id, edge_kind)
);

CREATE INDEX IF NOT EXISTS causal_links_trace_idx
    ON causal_links (trace_id, created_at ASC);

CREATE INDEX IF NOT EXISTS causal_links_source_idx
    ON causal_links (source_kind, source_id);

CREATE INDEX IF NOT EXISTS causal_links_target_idx
    ON causal_links (target_kind, target_id);

CREATE INDEX IF NOT EXISTS causal_links_edge_kind_idx
    ON causal_links (edge_kind, created_at DESC);
