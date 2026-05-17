# Governed Actions

---

## 1. Overview

The governed action system is the conscious loop's proposal-only tool surface.
The model emits plain text plus, when needed, one fenced JSON proposal block.
Returning only an action or payload name such as `list_workspace_artifacts` is
invalid. Invented aliases such as `read_workspace_artifacts` are also invalid.
These shapes are treated as malformed governed-action proposals.
Each model response may propose at most one governed action. If another action
is needed after the harness returns an observation, the harness performs another
bounded same-turn model call and revalidates the next proposal.
The harness parses the proposal, validates the requested scope, classifies risk,
persists an auditable execution record, optionally requests approval, and only
then executes the action through harness-owned services.

This keeps the conscious worker away from direct OS, workspace, schedule,
background-job, and wake-signal mutation. Model-usable tools are represented as
governed actions unless a bounded context summary is safer than a query action.
Trace explorer causal links connect planned governed actions to their source
execution, approval request, and scheduled-task mutations when those records are
created.

---

## 2. Implementation

### Source Files

| File | Relevant symbol |
|---|---|
| `crates/contracts/src/lib.rs` | `WorkerFailureMetadata` (line 228), `ForegroundGovernedActionLoopState` (line 457), `ForegroundGovernedActionRepairGuidance` (line 467), `GovernedActionKind` (line 1422), `DEFAULT_GOVERNED_ACTION_LIST_LIMIT` (line 1472), workflow integration payload structs (line 1590), `GovernedActionPayload` (line 1787) |
| `crates/workers/src/main.rs` | `GOVERNED_ACTIONS_BLOCK_TAG` (line 26), `schema_disclosure_for_scenario()` (line 794), `governed_action_schema_message()` (line 1616), `governed_action_reminder_message()` (line 1704), `governed_action_repair_guidance_message()` (line 1711), `build_governed_action_proposals()` (line 1759), `validate_governed_action_response_shape()` (line 1777), `invalid_model_output_metadata()` (line 1819), `governed_action_kind_as_str()` (line 2133) |
| `crates/harness/src/foreground_orchestration.rs` | `execute_conscious_turn_with_governed_action_loop()` (line 1814), `extract_worker_failure_signal()` (line 2502), `recoverable_malformed_action_signal()` (line 2515), `classify_conscious_worker_failure()` (line 2530), malformed re-steer audit/diagnostic emissions (lines 1903-1936) |
| `crates/harness/src/governed_actions.rs` | `execute_governed_action()` (line 537), `execute_inspect_ingress_attachments()` (line 1555), `execute_process_ingress_attachment()` (line 1578), `execute_list_email_messages()` (line 1859), `execute_send_email_message()` (line 1919), `execute_sync_task_list()` (line 1990), `execute_run_diagnostic_action()` (line 2479), `validate_upsert_scheduled_foreground_task_action()` (line 3237), `is_one_shot_scheduled_task_key()` (line 3262), `governed_action_kind_as_str()` (line 4370), `CanonicalGovernedActionPayload` (line 4508) |
| `crates/harness/src/integrations.rs` | `CalendarIntegrationAdapter` (line 128), `EmailIntegrationAdapter` (line 178), `TaskSyncIntegrationAdapter` (line 228), provider support checks (lines 235-249), deterministic/fake adapters (calendar lines 257/347, email lines 505/600, task sync lines 625/682) |
| `crates/harness/src/policy.rs` | `classify_governed_action_risk()` (line 176), `governed_action_requires_approval()` (line 223), `evaluate_governed_action_identity_boundaries()` (line 230) |
| `crates/harness/src/recovery.rs` | `governed_action_recovery_action_classification()` (line 1355) |
| `crates/harness/src/management.rs` | `CalendarIntegrationRunSummary` (line 374), `EmailIntegrationRunSummary` (line 392), `TaskSyncRunSummary` (line 410), integration run queries (calendar line 2221, email line 2261, task sync line 2301) |
| `crates/harness/src/approval.rs` | action-kind persistence mapping for approval requests |
| `crates/harness/src/workspace.rs` | workspace artifact, script, version, and run persistence services |
| `crates/harness/src/scheduled_foreground.rs` | `upsert_task()` for scheduled foreground work |
| `crates/harness/src/background_planning.rs` | `plan_background_job()` for conscious-to-background delegation |
| `crates/harness/src/causal_links.rs` | explicit trace edges for governed-action cause/effect records |
| `crates/runtime/src/admin.rs` | `IntegrationsCommand` (line 527), integration subcommands (calendar/email/task sync lines 533-589), integration run renderers (calendar line 2185, email line 2222, task sync line 2259) |
| `migrations/0010__conscious_tool_action_kinds.sql` | reviewed constraint update for the completed conscious-loop action-kind strings |
| `migrations/0014__diagnostic_action_kind.sql` | forward constraint update for the later `run_diagnostic` action kind on existing operator databases |
| `migrations/0015__attachment_processing.sql` | attachment processing tables and forward action-kind constraint update for `inspect_ingress_attachments` / `process_ingress_attachment` |
| `migrations/0016__calendar_integration_action_kinds.sql` | forward action-kind constraint update for `list_calendar_events` / `upsert_calendar_event` |
| `migrations/0017__workflow_integration_email_task_action_kinds.sql` | forward action-kind constraint update for `list_email_messages` / `send_email_message` / `sync_task_list` |

### Model-Facing Action Kinds

The live governed-action enum contains these model-usable kinds:

| Action kind | Purpose | Default risk |
|---|---|---|
| `inspect_workspace_artifact` | Inspect one active non-script workspace artifact | Tier 0 |
| `list_workspace_artifacts` | List/search non-script workspace artifacts | Tier 0 |
| `create_workspace_artifact` | Create a note, runbook, scratchpad, or task list | Tier 1 |
| `update_workspace_artifact` | Replace a non-script artifact with optimistic conflict checking | Tier 1 |
| `list_workspace_scripts` | List/search registered workspace scripts | Tier 0 |
| `inspect_workspace_script` | Inspect script metadata and bounded version content | Tier 0 |
| `create_workspace_script` | Create a script artifact and initial append-only version | Tier 2 |
| `append_workspace_script_version` | Append an auditable script version with conflict checking | Tier 2 |
| `list_workspace_script_runs` | Inspect bounded script run history | Tier 0 |
| `inspect_ingress_attachments` | Inspect attachment metadata and processing state for one ingress | Tier 0 |
| `process_ingress_attachment` | Run bounded attachment text extraction for one ingress attachment | Tier 1 |
| `list_calendar_events` | List bounded calendar events for one principal/conversation time window | Tier 1 |
| `upsert_calendar_event` | Create or update one calendar event for one principal/conversation | Tier 2 |
| `list_email_messages` | List bounded messages in one mailbox/query scope for one principal/conversation | Tier 1 |
| `send_email_message` | Send one outbound email with explicit recipient and body fields | Tier 2 |
| `sync_task_list` | Sync one external task list into a workspace task-list artifact | Tier 2 |
| `upsert_scheduled_foreground_task` | Create or update future foreground work | Tier 2 |
| `request_background_job` | Request bounded background maintenance work | Tier 1 |
| `run_diagnostic` | Execute one harness-native read-only diagnostic query | Tier 0 |
| `run_subprocess` | Execute a bounded subprocess | Tier 1-3 by scope |
| `run_workspace_script` | Execute a registered script version | Tier 1-3 by scope |
| `web_fetch` | Fetch one HTTP/HTTPS URL with bounded response capture | Tier 2 |

Workflow integration run visibility for operator troubleshooting is available
through `runtime admin integrations calendar list`, `runtime admin integrations
email list`, and `runtime admin integrations task-sync list` (text or `--json`),
backed by `management::list_calendar_integration_runs()`,
`management::list_email_integration_runs()`, and
`management::list_task_sync_runs()`.

### Proposal Format

The worker injects governed-action instructions as a Developer message. For
explicit action requests, reminder scheduling, troubleshooting turns, approval
follow-ups, terse confirmation follow-ups such as `yes` or `well yes`, and
retry-on-last-task follow-ups after a malformed action proposal, scenario policy
sends the full schema; routine chat and plain factual turns receive only the
short reminder from `governed_action_reminder_message()`. When an action is needed, the model may
append one block tagged `blue-lagoon-governed-actions`:

````json
```blue-lagoon-governed-actions
{
  "actions": [
    {
      "proposal_id": "<uuid>",
      "title": "<one-line description>",
      "rationale": "<why needed>",
      "action_kind": "list_workspace_artifacts",
      "requested_risk_tier": null,
      "capability_scope": {
        "filesystem": { "read_roots": [], "write_roots": [] },
        "network": "disabled",
        "environment": { "allow_variables": [] },
        "execution": { "timeout_ms": 0, "max_stdout_bytes": 0, "max_stderr_bytes": 0 }
      },
      "payload": {
        "kind": "list_workspace_artifacts",
        "value": { "artifact_kind": null, "status": "active", "query": null, "limit": 10 }
      }
    }
  ]
}
```
````

`build_governed_action_proposals()` extracts the last matching tagged block. If
no tagged block is present, no governed-action proposals are parsed. Untagged
JSON payloads are rejected by `validate_governed_action_response_shape()` as
malformed governed-action output. The
foreground orchestrator may continue through multiple governed-action rounds in
the same foreground turn: the worker receives harness observations, may propose
another action if one is still needed, and the harness then decides whether the
next proposal is allowed, approval-gated, or denied under policy, remaining
budgets, and the configured per-turn action cap.

Read-only list and diagnostic payloads should include an explicit `limit`.
For parser robustness, the contracts layer applies a bounded default of `10`
when that field is omitted from list/search payloads or diagnostic list
queries. Workspace artifact and script list payloads also default omitted
`status` to `active`; recovery checkpoint diagnostics default omitted
`open_only` to `false`; recovery lease diagnostics default omitted
`soft_warning_threshold_percent` to `80`. This compatibility default is
intentionally limited to read-only discovery actions; mutating payload fields,
identifiers, and required diagnostic selectors still fail as malformed
proposals when absent.

### Strictness Guardrails (No Masking)

The governed-action parser and harness error handling follow a strict-first
policy. Guardrails are used to make malformed output recoverable, not to coerce
invalid payloads into accepted behavior.

Required behavior:

- Model-emitted action proposals MUST use the tagged
  `blue-lagoon-governed-actions` block. Any untagged or alternate shape MUST
  fail as malformed.
- Missing required proposal fields (for example `actions` or required payload
  fields like `artifact_kind` for `inspect_workspace_artifact`) MUST fail as
  malformed; they MUST NOT be defaulted.
- Bare action tokens, invented aliases, and tool-call wrapper shapes MUST fail
  as malformed.
- Malformed proposal output MUST NOT execute any governed action side effects.
- Compatibility defaults MAY be applied only to bounded read-only discovery
  fields (`limit`, selected status/filter defaults) as documented above.

Primary implementation hooks:

- Worker parse/shape validation:
  `build_governed_action_proposals()`,
  `validate_governed_action_response_shape()`,
  `detect_bare_governed_action_invocation()`,
  `looks_like_untagged_governed_action_payload()`,
  `invalid_model_output_metadata()`.
- Harness re-steer/failure classification:
  `foreground_orchestration::execute_conscious_turn_with_governed_action_loop()`,
  `foreground_orchestration::extract_worker_failure_signal()`,
  `foreground_orchestration::recoverable_malformed_action_signal()`,
  `foreground_orchestration::classify_conscious_worker_failure()`.

Regression coverage anchors:

- `build_governed_action_proposals_requires_tagged_block`
- `build_governed_action_proposals_ignores_untagged_payloads`
- `build_governed_action_proposals_rejects_missing_actions_field`
- `build_governed_action_proposals_rejects_missing_required_payload_field`
- `conscious_worker_response_rejects_bare_governed_action_token`
- `conscious_worker_response_rejects_bare_unknown_governed_action_alias`
- `conscious_worker_response_rejects_tool_call_style_governed_action_wrapper`
- `foreground_orchestration_resteers_malformed_action_and_completes`
- `foreground_orchestration_records_diagnostic_when_resteer_attempts_exhausted`
- `telegram_fixture_runtime_resteers_malformed_governed_action_and_completes_same_turn`

### Same-Turn Malformed-Action Re-Steer

Malformed proposal handling stays strict, but the harness may re-steer once or
twice in the same turn when the failure is explicitly recoverable:

- Worker marks recoverable malformed-output failures through
  `WorkerFailureMetadata` with:
  `failure_kind=malformed_action_proposal`,
  `side_effect_status=none_executed`,
  `retry_recommended=true`.
- Harness retries only when this metadata is present and retry budget remains.
- Retry attempts inject a targeted Developer message from
  `governed_action_repair_guidance_message()` so the worker can correct the
  exact schema failure.
- Each retry emits `foreground_malformed_action_resteer_attempt` audit events.
- Retry exhaustion emits one operational diagnostic with
  `reason_code=foreground_malformed_action_resteer_exhausted`.
- Non-recoverable invalid outputs still fail explicitly; no parser coercion is
  applied.

### Model-Output Parsing Inventory (Anti-Masking Audit)

| Parsing path | Current posture | Disposition |
|---|---|---|
| `build_governed_action_proposals()` + `validate_governed_action_response_shape()` | Strict tagged-block-only parsing; malformed/untagged shapes fail | High-risk masking path remediated; guarded by strict regression tests and same-turn re-steer |
| `build_identity_kickstart_proposals()` | Optional control block; malformed identity block is ignored with no governed-action execution | Retained as bounded low-risk tolerance for bootstrap UX; no side effects |
| `parse_identity_reflection_output()` | Structured JSON required for identity reflection; invalid JSON downgraded to diagnostic and ignored | Retained fail-closed posture for writes; invalid output never becomes canonical proposal |
| `build_memory_consolidation_proposals()` | Free-text summarization path (no governed-action parsing) | Not a governed-action parser; no coercive schema fallback path |

### Payload Families

Workspace artifact payloads:

```json
{ "kind": "inspect_workspace_artifact", "value": { "artifact_id": "<uuid>", "artifact_kind": "scratchpad" } }
{ "kind": "list_workspace_artifacts", "value": { "artifact_kind": null, "status": "active", "query": null, "limit": 10 } }
{ "kind": "create_workspace_artifact", "value": { "artifact_kind": "note", "title": "...", "content_text": "...", "provenance": "conversation" } }
{ "kind": "update_workspace_artifact", "value": { "artifact_id": "<uuid>", "expected_updated_at": "2026-04-29T10:00:00Z", "title": null, "content_text": "...", "change_summary": "..." } }
```

Workspace script payloads:

```json
{ "kind": "list_workspace_scripts", "value": { "status": "active", "language": null, "query": null, "limit": 10 } }
{ "kind": "inspect_workspace_script", "value": { "script_id": "<uuid>", "script_version_id": null } }
{ "kind": "create_workspace_script", "value": { "title": "...", "language": "python", "content_text": "...", "description": "...", "requested_capabilities": [] } }
{ "kind": "append_workspace_script_version", "value": { "script_id": "<uuid>", "expected_latest_version_id": "<uuid>", "expected_content_sha256": null, "language": "python", "content_text": "...", "change_summary": "..." } }
{ "kind": "list_workspace_script_runs", "value": { "script_id": "<uuid>", "status": null, "limit": 10 } }
{ "kind": "run_workspace_script", "value": { "script_id": "<uuid>", "script_version_id": null, "args": [] } }
```

Attachment payloads:

```json
{ "kind": "inspect_ingress_attachments", "value": { "ingress_id": "<uuid>" } }
{ "kind": "process_ingress_attachment", "value": { "ingress_id": "<uuid>", "attachment_id": "<attachment-id>" } }
```

Workflow integration payloads:

```json
{ "kind": "list_calendar_events", "value": { "internal_principal_ref": "primary-user", "internal_conversation_ref": "telegram-primary", "start_at": "2026-05-20T09:00:00Z", "end_at": "2026-05-20T18:00:00Z", "max_results": 10 } }
{ "kind": "upsert_calendar_event", "value": { "internal_principal_ref": "primary-user", "internal_conversation_ref": "telegram-primary", "title": "Project sync", "starts_at": "2026-05-20T13:00:00Z", "ends_at": "2026-05-20T14:00:00Z", "location": "Room A", "details": "Discuss milestone 3", "external_event_id": null } }
{ "kind": "list_email_messages", "value": { "internal_principal_ref": "primary-user", "internal_conversation_ref": "telegram-primary", "mailbox": "inbox", "query": "subject:\"project\" newer_than:7d", "max_results": 10 } }
{ "kind": "send_email_message", "value": { "internal_principal_ref": "primary-user", "internal_conversation_ref": "telegram-primary", "to": ["teammate@example.com"], "cc": [], "subject": "Project update", "body_text": "Quick update on Milestone 3...", "reply_to_external_message_id": null } }
{ "kind": "sync_task_list", "value": { "internal_principal_ref": "primary-user", "internal_conversation_ref": "telegram-primary", "task_list_title": "Milestone 4 Tracker", "items": ["Design planner stage", "Implement trigger producers"], "external_list_id": null, "workspace_artifact_id": null } }
```

Schedule, background, subprocess, and fetch payloads:

```json
{ "kind": "upsert_scheduled_foreground_task", "value": { "task_key": "check_in", "title": "Check in", "user_facing_prompt": "...", "next_due_at_utc": "2026-04-29T10:00:00Z", "cadence_seconds": 86400, "cooldown_seconds": 3600, "internal_principal_ref": "primary-user", "internal_conversation_ref": "telegram-primary", "active": true } }
{ "kind": "upsert_scheduled_foreground_task", "value": { "task_key": "oneoff_check_in_20260429", "title": "One-time check in", "user_facing_prompt": "...", "next_due_at_utc": "2026-04-29T10:00:00Z", "cadence_seconds": 0, "cooldown_seconds": 3600, "internal_principal_ref": "primary-user", "internal_conversation_ref": "telegram-primary", "active": true } }
{ "kind": "request_background_job", "value": { "job_kind": "memory_consolidation", "rationale": "...", "input_scope_ref": null, "urgency": "normal", "wake_preference": null, "internal_conversation_ref": "telegram-primary" } }
{ "kind": "run_subprocess", "value": { "command": "<executable>", "args": [], "working_directory": "<absolute path or null>" } }
{ "kind": "web_fetch", "value": { "url": "https://example.com", "timeout_ms": 10000, "max_response_bytes": 524288 } }
```

Scheduled foreground tasks are recurring by default. Recurring tasks must use a
positive `cadence_seconds` that passes `scheduled_foreground` validation.
One-shot tasks are represented without a schema migration by using a task key
with the `oneoff_` or `one_shot_` prefix and `cadence_seconds: 0`. The harness
stores a bounded placeholder cadence internally because the database column is
non-null, but terminal outcomes disable prefixed one-shot tasks after success,
suppression, or failure. This prevents reminders from turning into retry loops
unless a future action explicitly creates or reactivates a recurring task.

### Observation Feedback

Execution produces a `GovernedActionObservation` and feeds it into the next
model call as a bounded Developer message. Workspace artifact and script
inspection previews are capped at 2,000 characters. Web fetch previews are
capped at 1,500 characters after content formatting. Full raw fetch bodies and
harness-native payload details are stored in execution records, not injected
unbounded into conscious context.

When the harness is in a same-turn continuation path, the observation message
also carries the current `ForegroundGovernedActionLoopState`: actions already
used in the turn, remaining cap, and configured cap-exceeded posture. This
keeps the worker aware of bounded continuation state without making the worker
the authority for execution decisions.

Approval-triggered action execution persists the model follow-up text first,
then appends the harness observation to durable assistant history. Telegram
delivery sends only user-facing text.

### Recovery Posture

Read-only harness-native actions are replay-safe after a worker interruption:
`inspect_workspace_artifact`, `list_workspace_artifacts`,
`list_workspace_scripts`, `inspect_workspace_script`,
`list_workspace_script_runs`, `inspect_ingress_attachments`, and `run_diagnostic`.

Actions that can create, update, schedule, delegate, execute, fetch, or otherwise
produce side effects are classified as ambiguous or nonrepeatable during
governed-action recovery. Workflow integration actions
(`list_calendar_events`, `upsert_calendar_event`, `list_email_messages`,
`send_email_message`, `sync_task_list`) are also treated as nonrepeatable
because they depend on external integration state. Recovery must not
automatically retry these actions unless durable completion evidence proves the
original action already reached a terminal state.

Foreground replay follows the same proof-based rule. If an interrupted
foreground execution already linked one or more governed actions and every
linked action is not both replay-safe and approval-free, stale foreground
recovery fails closed instead of blindly replaying the turn.

---

## 3. Configuration & Extension

### Capability Scope Rules

| Rule | Applies to |
|---|---|
| Empty filesystem, environment, and disabled network | harness-native workspace, script authoring/discovery, attachment inspection/processing, workflow integrations (calendar/email/task sync), schedule, and background request actions |
| At least one filesystem root | `run_subprocess`, `run_workspace_script` |
| Non-empty execution limits within configured maxima | `run_subprocess`, `run_workspace_script` |
| Network must be enabled; execution budget fields may be zero | `web_fetch` |
| Environment variables must be allowlisted | actions that request environment access |

Config defaults live in `config/default.toml` under `[governed_actions]`:

| Key | Default |
|---|---|
| `approval_required_min_risk_tier` | `"tier_2"` |
| `default_subprocess_timeout_ms` | `30000` |
| `max_subprocess_timeout_ms` | `120000` |
| `max_actions_per_foreground_turn` | `10` |
| `malformed_action_resteer_max_attempts` | `2` |
| `malformed_action_resteer_timeout_ms` | `10000` |
| `cap_exceeded_behavior` | `"escalate"` |
| `max_filesystem_roots_per_action` | `4` |
| `default_network_access` | `"disabled"` |
| `allowlisted_environment_variables` | `["BLUE_LAGOON_DATABASE_URL"]` |
| `max_environment_variables_per_action` | `8` |
| `max_captured_output_bytes` | `65536` |
| `max_web_fetch_timeout_ms` | `15000` |
| `max_web_fetch_response_bytes` | `524288` |

`requested_risk_tier` may raise the final tier, but it cannot lower the
intrinsic tier computed by `policy::classify_governed_action_risk()`.

Workflow integration action config lives in `config/default.toml` under
`[integrations.calendar]`, `[integrations.email]`, and
`[integrations.task_sync]`:

| Key | Default | Validation | Source |
|---|---|---|---|
| `integrations.calendar.enabled` | `false` | `true`/`false`; when `true`, provider and credential env are required | `config/default.toml:41`, `crates/harness/src/config.rs:112`, `crates/harness/src/config.rs:1265` |
| `integrations.calendar.provider` | `""` | non-empty when enabled; supported values are `deterministic_fake` and `fake` | `config/default.toml:42`, `crates/harness/src/config.rs:112`, `crates/harness/src/config.rs:1268`, `crates/harness/src/integrations.rs:235` |
| `integrations.calendar.credential_env` | `""` | non-empty env-var name when enabled; resolved fail-closed | `config/default.toml:43`, `crates/harness/src/config.rs:112`, `crates/harness/src/config.rs:1275`, `crates/harness/src/config.rs:774` |
| `integrations.calendar.api_base_url` | `""` | optional; if set, must be non-empty when enabled | `config/default.toml:45`, `crates/harness/src/config.rs:112`, `crates/harness/src/config.rs:1280` |
| `integrations.email.enabled` | `false` | `true`/`false`; when `true`, provider and credential env are required | `config/default.toml:48`, `crates/harness/src/config.rs:124`, `crates/harness/src/config.rs:1292` |
| `integrations.email.provider` | `""` | non-empty when enabled; supported values are `deterministic_fake` and `fake` | `config/default.toml:49`, `crates/harness/src/config.rs:124`, `crates/harness/src/config.rs:1295`, `crates/harness/src/integrations.rs:242` |
| `integrations.email.credential_env` | `""` | non-empty env-var name when enabled; resolved fail-closed | `config/default.toml:50`, `crates/harness/src/config.rs:124`, `crates/harness/src/config.rs:1302`, `crates/harness/src/config.rs:774` |
| `integrations.email.api_base_url` | `""` | optional; if set, must be non-empty when enabled | `config/default.toml:52`, `crates/harness/src/config.rs:124`, `crates/harness/src/config.rs:1307` |
| `integrations.task_sync.enabled` | `false` | `true`/`false`; when `true`, provider and credential env are required | `config/default.toml:55`, `crates/harness/src/config.rs:136`, `crates/harness/src/config.rs:1319` |
| `integrations.task_sync.provider` | `""` | non-empty when enabled; supported values are `deterministic_fake` and `fake` | `config/default.toml:56`, `crates/harness/src/config.rs:136`, `crates/harness/src/config.rs:1322`, `crates/harness/src/integrations.rs:249` |
| `integrations.task_sync.credential_env` | `""` | non-empty env-var name when enabled; resolved fail-closed | `config/default.toml:57`, `crates/harness/src/config.rs:136`, `crates/harness/src/config.rs:1329`, `crates/harness/src/config.rs:774` |
| `integrations.task_sync.api_base_url` | `""` | optional; if set, must be non-empty when enabled | `config/default.toml:59`, `crates/harness/src/config.rs:136`, `crates/harness/src/config.rs:1334` |

Approval collaboration policy config (Telegram foreground binding):

| Key | Default | Validation | Source |
|---|---|---|---|
| `telegram.foreground_binding.delegates` | `[]` | each entry must define non-zero `allowed_user_id` and non-empty `internal_principal_ref`; duplicates are rejected across owner + delegates | `crates/harness/src/config.rs:234`, `crates/harness/src/config.rs:1144` |
| `telegram.foreground_binding.approval_resolution_policy` | `"delegate_allowed"` | enum: `delegate_allowed` or `owner_only`; resolved approval actor policy is enforced during callback/command resolution | `crates/harness/src/config.rs:236`, `crates/harness/src/config.rs:247`, `crates/harness/src/foreground_orchestration.rs:504`, `crates/harness/src/approval.rs:432` |

Resolved integration config entrypoints:

- `resolve_calendar_integration_config()` (`crates/harness/src/config.rs:701`)
- `resolve_email_integration_config()` (`crates/harness/src/config.rs:725`)
- `resolve_task_sync_integration_config()` (`crates/harness/src/config.rs:749`)
- shared resolver helper: `resolve_workflow_integration_config_fields()`
  (`crates/harness/src/config.rs:774`)

Provider behavior in the current harness path for calendar/email/task sync:

- `deterministic_fake`: deterministic success-path adapter for predictable tests.
- `fake`: deterministic failure-path adapter that returns temporary failures.

### Identity Boundary Rules

After capability-scope validation, planning and execution both load the compact
identity snapshot and evaluate active identity boundaries through
`policy::evaluate_governed_action_identity_boundaries()`. A matching enduring
boundary can deterministically block network access, subprocess execution, or
workspace write effects before approval or execution proceeds. This is a
harness policy decision, not a model preference: the model can propose an
action, but identity boundaries are rechecked by the harness at planning time
and again immediately before execution.
`list_calendar_events`, `upsert_calendar_event`, `list_email_messages`,
`send_email_message`, and `sync_task_list` are treated as network-dependent
actions for this boundary evaluation even though they execute through
harness-owned integration adapters instead of direct worker networking.

### Extension Checklist

Use `docs/internal/harness/TOOL_IMPLEMENTATION.md` for the full E2E workflow.
The short version is:

- Add contract enum and payload variants.
- Add the migration for constrained `action_kind` columns.
- When adding action kinds after an operator database may already have applied
  earlier migrations, add a new forward migration rather than editing an
  already-applied migration file.
- Add shape validation, scope validation, risk classification, canonical
  fingerprinting, execution dispatch, observation formatting, and audit payloads.
- Update worker schema text and every exhaustive action-kind match.
- Add component tests for planning, validation, execution, and DB constraints.
- Update this document and restamp the verified date.

---

## 4. Further Reading

- `docs/internal/conscious_loop/CONTEXT_ASSEMBLY.md`: how observations and schema
  messages are positioned in conscious context.
- `docs/internal/harness/TOOL_IMPLEMENTATION.md`: exact implementation workflow
  for adding a governed tool.
- `docs/internal/harness/TRACE_EXPLORER.md`: operator trace graph and causal
  links for governed-action planning, approvals, and scheduled-task changes.
- `docs/LOOP_ARCHITECTURE.md`: canonical conscious/unconscious boundary.
- `docs/IMPLEMENTATION_DESIGN.md`: canonical runtime design constraints.

---

*Last verified: 2026-05-17.*
