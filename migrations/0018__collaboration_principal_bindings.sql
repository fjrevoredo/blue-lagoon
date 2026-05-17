ALTER TABLE conversation_bindings
    ADD COLUMN IF NOT EXISTS principal_role TEXT NOT NULL DEFAULT 'owner';

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'conversation_bindings_principal_role_check'
          AND conrelid = 'conversation_bindings'::regclass
    ) THEN
        ALTER TABLE conversation_bindings
            ADD CONSTRAINT conversation_bindings_principal_role_check
            CHECK (principal_role IN ('owner', 'delegate'));
    END IF;
END
$$;

ALTER TABLE conversation_bindings
    DROP CONSTRAINT IF EXISTS conversation_bindings_internal_conversation_ref_key;

CREATE UNIQUE INDEX IF NOT EXISTS conversation_bindings_internal_conversation_principal_uidx
    ON conversation_bindings (channel_kind, internal_conversation_ref, internal_principal_ref);

CREATE INDEX IF NOT EXISTS conversation_bindings_internal_conversation_idx
    ON conversation_bindings (channel_kind, internal_conversation_ref);
