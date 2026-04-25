-- Extend action_kind check constraints to include 'web_fetch'.
-- Both governed_action_executions and approval_requests use inline CHECK constraints
-- that were created without explicit names, so PostgreSQL named them automatically.

ALTER TABLE governed_action_executions
    DROP CONSTRAINT governed_action_executions_action_kind_check;

ALTER TABLE governed_action_executions
    ADD CONSTRAINT governed_action_executions_action_kind_check CHECK (
        action_kind IN (
            'inspect_workspace_artifact',
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
            'run_subprocess',
            'run_workspace_script',
            'web_fetch'
        )
    );
