-- Extend governed-action action_kind constraints for the completed conscious-loop
-- tool surface. This migration is metadata-only: existing rows remain valid,
-- and rollback/correction is to reapply the prior constraint set after removing
-- rows that use the added action kinds.

ALTER TABLE governed_action_executions
    DROP CONSTRAINT governed_action_executions_action_kind_check;

ALTER TABLE governed_action_executions
    ADD CONSTRAINT governed_action_executions_action_kind_check CHECK (
        action_kind IN (
            'inspect_workspace_artifact',
            'list_workspace_artifacts',
            'create_workspace_artifact',
            'update_workspace_artifact',
            'list_workspace_scripts',
            'inspect_workspace_script',
            'create_workspace_script',
            'append_workspace_script_version',
            'list_workspace_script_runs',
            'upsert_scheduled_foreground_task',
            'request_background_job',
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
            'list_workspace_artifacts',
            'create_workspace_artifact',
            'update_workspace_artifact',
            'list_workspace_scripts',
            'inspect_workspace_script',
            'create_workspace_script',
            'append_workspace_script_version',
            'list_workspace_script_runs',
            'upsert_scheduled_foreground_task',
            'request_background_job',
            'run_subprocess',
            'run_workspace_script',
            'web_fetch'
        )
    );
