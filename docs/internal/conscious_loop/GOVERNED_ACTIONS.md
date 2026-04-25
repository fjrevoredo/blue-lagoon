# Governed Actions

---

## 1. Overview

The governed action system is the mechanism by which the conscious agent proposes side-effecting operations — running shell commands, executing workspace scripts — without using native API tool-use.

Instead of emitting a tool-call in the model response format, the agent appends a structured JSON block to its text output. The harness extracts this block, validates the capability scope, classifies the risk tier, optionally routes it through the approval workflow, and then executes it. Observations from execution are fed back to the agent in the next model call within the same episode, allowing multi-step turns.

This design keeps the agent's output plain text and makes all side-effects auditable and policy-governed before they reach the OS.

---

## 2. Implementation

### Source Files

| File | Relevant symbol |
|---|---|
| `crates/workers/src/main.rs` | `governed_action_schema_message()` (line 586), `build_governed_action_proposals()` (line 642), `GOVERNED_ACTIONS_BLOCK_TAG` (line 24) |
| `crates/harness/src/governed_actions.rs` | `validate_capability_scope()` (line 667), `execute_governed_action()` |
| `crates/harness/src/policy.rs` | `classify_governed_action_risk()`, `governed_action_requires_approval()` |
| `crates/contracts/src/lib.rs` | `GovernedActionProposal` (line 721), `CapabilityScope` (line 705), `GovernedActionPayload` (line 753) |
| `config/default.toml` | `[governed_actions]` section |

### Tool Policy

The conscious worker always uses `ToolPolicy::ProposalOnly` (`main.rs:441`). The agent cannot invoke tools natively through Claude/API tool-use. The governed action JSON block is the only way for the agent to trigger side effects.

### Block Format and Parsing

The agent appends a fenced code block tagged `blue-lagoon-governed-actions` after its user-visible response text. The schema message (injected as the last Developer-role message — see `CONTEXT_ASSEMBLY.md`) defines this format for the agent.

````
```blue-lagoon-governed-actions
{
  "actions": [...]
}
```
````

`build_governed_action_proposals()` extracts the block using `rfind` on the full response text (`main.rs:673`), so the **last** occurrence wins if the model emits multiple blocks. `strip_governed_action_block()` removes the block from the text before the assistant turn is stored.

### Full Proposal JSON Schema

Contract type: `GovernedActionProposal` (`contracts/src/lib.rs:721`).

```json
{
  "actions": [
    {
      "proposal_id": "<UUID v4>",
      "title": "<one-line description>",
      "rationale": "<why needed, or null>",
      "action_kind": "run_subprocess | run_workspace_script | web_fetch",
      "requested_risk_tier": null,
      "capability_scope": {
        "filesystem": {
          "read_roots": ["<absolute path>"],
          "write_roots": []
        },
        "network": "disabled | allowlisted | enabled",
        "environment": {
          "allow_variables": []
        },
        "execution": {
          "timeout_ms": 30000,
          "max_stdout_bytes": 16384,
          "max_stderr_bytes": 8192
        }
      },
      "payload": { ... }
    }
  ]
}
```

Notes:
- `rationale` is `Option<String>` — may be `null`.
- `requested_risk_tier` is advisory only; the harness re-classifies using `classify_governed_action_risk()`.
- The schema message instructs the agent to propose at most one action per turn.

### Payload Shapes

**`run_subprocess`:**
```json
{
  "kind": "run_subprocess",
  "value": {
    "command": "<executable>",
    "args": ["<arg1>", "<arg2>"],
    "working_directory": "<absolute path or null>"
  }
}
```

**`run_workspace_script`:**
```json
{
  "kind": "run_workspace_script",
  "value": {
    "script_id": "<uuid>",
    "script_version_id": null,
    "args": []
  }
}
```

**`web_fetch`:**
```json
{
  "kind": "web_fetch",
  "value": {
    "url": "https://...",
    "timeout_ms": 10000,
    "max_response_bytes": 524288
  }
}
```

For `web_fetch`: set `capability_scope.filesystem` to `{"read_roots": [], "write_roots": []}`, `capability_scope.network` to `"enabled"` (required — validation rejects any other value), and `capability_scope.execution` to `{"timeout_ms": 0, "max_stdout_bytes": 0, "max_stderr_bytes": 0}` (ignored for web_fetch; limits live in the payload). Every web_fetch proposal is routed for approval (always Tier 2).

> **NOT IMPLEMENTED:** `InspectWorkspaceArtifact` exists as a `GovernedActionKind` enum variant and in the contracts but always returns `GovernedActionStatus::Blocked` at execution time with summary `"workspace inspection execution is not implemented in the first governed backend"` (`governed_actions.rs:523–526`). Do not expose this action kind to the agent.

### Observation Feedback

After execution, the harness feeds results back to the agent in the next model call within the same episode as a Developer-role message (instead of the schema):

```
Harness governed-action observations: {kind}:{status}:{summary} | ...
Continue the foreground turn using these outcomes. Do not repeat the same action proposal unless the previous action failed and a materially different retry is required.
```

Format produced by `governed_action_observation_summary()` (`main.rs:627`). Multiple observations are joined with ` | `.

---

## 3. Configuration & Extension

### `capability_scope` Validation Rules

Enforced by `validate_capability_scope()` in `governed_actions.rs:667`. All limits are read from `RuntimeConfig` (`config/default.toml: [governed_actions]`).

| Rule | Applies to | Default limit | Config key |
|---|---|---|---|
| At least one filesystem root (`read_roots` + `write_roots`) | subprocess, workspace_script | — | — |
| Max filesystem roots total | all | 4 | `max_filesystem_roots_per_action` |
| No empty root strings | all | — | — |
| `timeout_ms > 0` | subprocess, workspace_script | — | — |
| `timeout_ms ≤ max` | subprocess, workspace_script | 120,000 ms | `max_subprocess_timeout_ms` |
| `max_stdout_bytes > 0` | subprocess, workspace_script | — | — |
| `max_stderr_bytes > 0` | subprocess, workspace_script | — | — |
| Output byte limits (both) | subprocess, workspace_script | 65,536 bytes | `max_captured_output_bytes` |
| Each env variable must be allowlisted | all | `["BLUE_LAGOON_DATABASE_URL"]` | `allowlisted_environment_variables` |
| Max env variables | all | 8 | `max_environment_variables_per_action` |
| URL must be non-empty and http/https | web_fetch | — | — |
| `payload.timeout_ms > 0` and `≤ max` | web_fetch | 15,000 ms | `max_web_fetch_timeout_ms` |
| `payload.max_response_bytes > 0` and `≤ max` | web_fetch | 524,288 bytes | `max_web_fetch_response_bytes` |
| `capability_scope.network` must be `"enabled"` | web_fetch | — | — |

Filesystem root and subprocess execution budget checks are **skipped** for `web_fetch` — limits are in the payload instead. To raise or lower any limit, edit `config/local.toml` under `[governed_actions]`.

### Risk Tiers and Approval

Config key: `governed_actions.approval_required_min_risk_tier` (default `"tier_2"`).

| Tier | Value | Requires approval (default) |
|---|---|---|
| Tier 0 | `"tier_0"` | No |
| Tier 1 | `"tier_1"` | No |
| Tier 2 | `"tier_2"` | Yes |
| Tier 3 | `"tier_3"` | Yes |

Risk tier is classified by `policy::classify_governed_action_risk()` — the agent's `requested_risk_tier` field does not override this. To change the approval threshold, update `approval_required_min_risk_tier` in `config/local.toml`.

### Adding a New Action Kind

1. Add a new variant to `GovernedActionKind` and `GovernedActionPayload` in `crates/contracts/src/lib.rs`.
2. Add a parsing arm in `parse_governed_action_kind()` (`governed_actions.rs`).
3. Add a `WebFetch`-style variant to `CanonicalGovernedActionPayload` and its `From` impl in `governed_actions.rs`.
4. Add an execution arm in the `execute_governed_action()` dispatch and implement the backend function.
5. Add a risk-classification arm in `policy::classify_governed_action_risk()`.
6. Update validation in `validate_capability_scope()` and `validate_proposal_shape()` in `governed_actions.rs`.
7. Update `governed_action_schema_message()` in `workers/src/main.rs` to expose the new kind to the agent. **The alternate payload description must show the complete `capability_scope` object, not a diff — every field (`filesystem`, `network`, `environment`, `execution`) must be present, or the model will omit fields and deserialization will fail.**
8. Add WebFetch arms to all other `GovernedActionKind` match expressions (currently: `approval.rs`, `management.rs`, `recovery.rs`, `workers/src/main.rs`).
9. Add tests in `governed_actions_component` and `governed_actions_integration` test suites.
10. Update all `GovernedActionsConfig` test constructors with any new config fields.
11. **Write a new migration** (`migrations/NNNN__<name>.sql`) that drops and recreates the `action_kind` check constraints on `governed_action_executions` and `approval_requests` to include the new kind string. `action_kind` is a constrained `TEXT` column in the DB — the Rust enum alone is not enough. Verify the exact constraint names against the existing migration before writing the `DROP CONSTRAINT` statement.

---

## 4. Further Reading

- `docs/internal/conscious_loop/CONTEXT_ASSEMBLY.md` — how the schema message is injected into the message array, and how observation feedback is positioned relative to other Developer messages.
- `docs/LOOP_ARCHITECTURE.md` — canonical description of the foreground turn lifecycle and the harness's role as executor and auditor.
- `docs/IMPLEMENTATION_DESIGN.md` — design rationale for the proposal-only tool policy and the two-process model.
- `crates/harness/src/policy.rs` — risk classification logic and approval routing.
- `crates/harness/src/approval.rs` — approval request lifecycle.

---

*Last verified: commit `6752e2c` (branch `usage-improvements`), session 2026-04-25.*
