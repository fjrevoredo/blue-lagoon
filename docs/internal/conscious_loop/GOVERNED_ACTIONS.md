# Governed Actions

---

## 1. Overview

The governed action system is the conscious loop's proposal-only tool surface.
The model emits plain text plus, when needed, one fenced JSON proposal block.
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
| `crates/contracts/src/lib.rs` | `GovernedActionKind` (line 714), payload structs (line 739), `GovernedActionPayload` (line 867) |
| `crates/workers/src/main.rs` | `GOVERNED_ACTIONS_BLOCK_TAG` (line 24), `governed_action_schema_message()` (line 592), `build_governed_action_proposals()` (line 676), `governed_action_kind_as_str()` (line 718) |
| `crates/harness/src/governed_actions.rs` | `execute_governed_action()` (line 442), `execute_inspect_workspace_artifact()` (line 773), `execute_create_workspace_script()` (line 1090), `execute_request_background_job()` (line 1290), `validate_capability_scope()` (line 1370), `governed_action_kind_as_str()` (line 2795), `CanonicalGovernedActionPayload` (line 2917) |
| `crates/harness/src/policy.rs` | `classify_governed_action_risk()` (line 168), `governed_action_requires_approval()` (line 207) |
| `crates/harness/src/recovery.rs` | `governed_action_recovery_action_classification()` (line 1314) |
| `crates/harness/src/approval.rs` | action-kind persistence mapping for approval requests |
| `crates/harness/src/workspace.rs` | workspace artifact, script, version, and run persistence services |
| `crates/harness/src/scheduled_foreground.rs` | `upsert_task()` for scheduled foreground work |
| `crates/harness/src/background_planning.rs` | `plan_background_job()` for conscious-to-background delegation |
| `crates/harness/src/causal_links.rs` | explicit trace edges for governed-action cause/effect records |
| `migrations/0010__conscious_tool_action_kinds.sql` | reviewed constraint update for new action-kind strings |

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
| `upsert_scheduled_foreground_task` | Create or update future foreground work | Tier 2 |
| `request_background_job` | Request bounded background maintenance work | Tier 1 |
| `run_subprocess` | Execute a bounded subprocess | Tier 1-3 by scope |
| `run_workspace_script` | Execute a registered script version | Tier 1-3 by scope |
| `web_fetch` | Fetch one HTTP/HTTPS URL with bounded response capture | Tier 2 |

### Proposal Format

The worker injects the schema as a Developer message. The model may append one
block tagged `blue-lagoon-governed-actions`:

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

`build_governed_action_proposals()` extracts the last matching block. The
orchestrator currently processes at most one action per immediate model turn.

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

Schedule, background, subprocess, and fetch payloads:

```json
{ "kind": "upsert_scheduled_foreground_task", "value": { "task_key": "check_in", "title": "Check in", "user_facing_prompt": "...", "next_due_at_utc": "2026-04-29T10:00:00Z", "cadence_seconds": 86400, "cooldown_seconds": 3600, "internal_principal_ref": "primary-user", "internal_conversation_ref": "telegram-primary", "active": true } }
{ "kind": "request_background_job", "value": { "job_kind": "memory_consolidation", "rationale": "...", "input_scope_ref": null, "urgency": "normal", "wake_preference": null, "internal_conversation_ref": "telegram-primary" } }
{ "kind": "run_subprocess", "value": { "command": "<executable>", "args": [], "working_directory": "<absolute path or null>" } }
{ "kind": "web_fetch", "value": { "url": "https://example.com", "timeout_ms": 10000, "max_response_bytes": 524288 } }
```

### Observation Feedback

Execution produces a `GovernedActionObservation` and feeds it into the next
model call as a bounded Developer message. Workspace artifact and script
inspection previews are capped at 2,000 characters. Web fetch previews are
capped at 1,500 characters after content formatting. Full raw fetch bodies and
harness-native payload details are stored in execution records, not injected
unbounded into conscious context.

Approval-triggered action execution persists the model follow-up text first,
then appends the harness observation to durable assistant history. Telegram
delivery sends only user-facing text.

### Recovery Posture

Read-only harness-native actions are replay-safe after a worker interruption:
`inspect_workspace_artifact`, `list_workspace_artifacts`,
`list_workspace_scripts`, `inspect_workspace_script`, and
`list_workspace_script_runs`.

Actions that can create, update, schedule, delegate, execute, fetch, or otherwise
produce side effects are classified as ambiguous or nonrepeatable during
governed-action recovery. Recovery must not automatically retry them unless
durable completion evidence proves the original action already reached a
terminal state.

---

## 3. Configuration & Extension

### Capability Scope Rules

| Rule | Applies to |
|---|---|
| Empty filesystem, environment, and disabled network | harness-native workspace, script authoring/discovery, schedule, and background request actions |
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
| `max_filesystem_roots_per_action` | `4` |
| `default_network_access` | `"disabled"` |
| `allowlisted_environment_variables` | `["BLUE_LAGOON_DATABASE_URL"]` |
| `max_environment_variables_per_action` | `8` |
| `max_captured_output_bytes` | `65536` |
| `max_web_fetch_timeout_ms` | `15000` |
| `max_web_fetch_response_bytes` | `524288` |

`requested_risk_tier` may raise the final tier, but it cannot lower the
intrinsic tier computed by `policy::classify_governed_action_risk()`.

### Extension Checklist

Use `docs/internal/harness/TOOL_IMPLEMENTATION.md` for the full E2E workflow.
The short version is:

- Add contract enum and payload variants.
- Add the migration for constrained `action_kind` columns.
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

*Last verified: branch `codex/causal-trace-explorer`, session 2026-04-30.*
