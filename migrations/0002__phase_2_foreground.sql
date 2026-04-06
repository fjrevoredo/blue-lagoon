CREATE TABLE IF NOT EXISTS conversation_bindings (
    conversation_binding_id UUID PRIMARY KEY,
    channel_kind TEXT NOT NULL,
    external_user_id TEXT NOT NULL,
    external_conversation_id TEXT NOT NULL,
    internal_principal_ref TEXT NOT NULL,
    internal_conversation_ref TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (channel_kind, external_user_id, external_conversation_id),
    UNIQUE (internal_conversation_ref)
);

CREATE INDEX IF NOT EXISTS conversation_bindings_principal_idx
    ON conversation_bindings (internal_principal_ref);

CREATE TABLE IF NOT EXISTS ingress_events (
    ingress_id UUID PRIMARY KEY,
    conversation_binding_id UUID NULL REFERENCES conversation_bindings (conversation_binding_id),
    trace_id UUID NOT NULL,
    execution_id UUID NULL REFERENCES execution_records (execution_id),
    channel_kind TEXT NOT NULL,
    external_user_id TEXT NOT NULL,
    external_conversation_id TEXT NOT NULL,
    external_event_id TEXT NOT NULL,
    external_message_id TEXT NULL,
    internal_principal_ref TEXT NULL,
    internal_conversation_ref TEXT NULL,
    event_kind TEXT NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    received_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    status TEXT NOT NULL,
    rejection_reason TEXT NULL,
    text_body TEXT NULL,
    reply_to_external_message_id TEXT NULL,
    attachment_count INTEGER NOT NULL DEFAULT 0 CHECK (attachment_count >= 0),
    attachments_json JSONB NOT NULL DEFAULT '[]'::JSONB,
    command_name TEXT NULL,
    command_args_json JSONB NOT NULL DEFAULT '[]'::JSONB,
    approval_token TEXT NULL,
    approval_callback_data TEXT NULL,
    raw_payload_ref TEXT NULL,
    UNIQUE (channel_kind, external_event_id)
);

CREATE INDEX IF NOT EXISTS ingress_events_execution_id_idx
    ON ingress_events (execution_id);
CREATE INDEX IF NOT EXISTS ingress_events_message_id_idx
    ON ingress_events (channel_kind, external_message_id);
CREATE INDEX IF NOT EXISTS ingress_events_conversation_occurred_at_idx
    ON ingress_events (internal_conversation_ref, occurred_at DESC);

CREATE TABLE IF NOT EXISTS episodes (
    episode_id UUID PRIMARY KEY,
    trace_id UUID NOT NULL,
    execution_id UUID NOT NULL REFERENCES execution_records (execution_id),
    ingress_id UUID NULL REFERENCES ingress_events (ingress_id),
    internal_principal_ref TEXT NOT NULL,
    internal_conversation_ref TEXT NOT NULL,
    trigger_kind TEXT NOT NULL,
    trigger_source TEXT NOT NULL,
    status TEXT NOT NULL,
    started_at TIMESTAMPTZ NOT NULL,
    completed_at TIMESTAMPTZ NULL,
    outcome TEXT NULL,
    summary TEXT NULL,
    UNIQUE (execution_id)
);

CREATE INDEX IF NOT EXISTS episodes_conversation_started_at_idx
    ON episodes (internal_conversation_ref, started_at DESC);
CREATE INDEX IF NOT EXISTS episodes_trace_id_idx
    ON episodes (trace_id);

CREATE TABLE IF NOT EXISTS episode_messages (
    episode_message_id UUID PRIMARY KEY,
    episode_id UUID NOT NULL REFERENCES episodes (episode_id) ON DELETE CASCADE,
    trace_id UUID NOT NULL,
    execution_id UUID NOT NULL REFERENCES execution_records (execution_id),
    message_order INTEGER NOT NULL CHECK (message_order >= 0),
    message_role TEXT NOT NULL,
    channel_kind TEXT NOT NULL,
    text_body TEXT NULL,
    external_message_id TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (episode_id, message_order)
);

CREATE INDEX IF NOT EXISTS episode_messages_execution_id_idx
    ON episode_messages (execution_id);
CREATE INDEX IF NOT EXISTS episode_messages_episode_order_idx
    ON episode_messages (episode_id, message_order);
