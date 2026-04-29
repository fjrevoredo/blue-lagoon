# Governed Actions

---

## 1. Overview

The governed action system is the mechanism by which the conscious agent proposes side-effecting operations — running shell commands, executing workspace scripts — without using native API tool-use.

Instead of emitting a tool-call in the model response format, the agent appends a structured JSON block to its text output. The harness extracts this block, validates the capability scope, classifies the risk tier, optionally routes it through the approval workflow, and then executes it. Observations from execution are fed back to the agent in the next model call within the same episode, allowing multi-step turns. Approval-triggered follow-up episodes also persist the harness observation with the assistant follow-up text so later foreground turns can recover the approved action result from recent history.

This design keeps the agent's output plain text and makes all side-effects auditable and policy-governed before they reach the OS.

---

## 2. Implementation

### Source Files

| File | Relevant symbol |
|---|---|
| `crates/workers/src/main.rs` | `governed_action_schema_message()` (line 592), `build_governed_action_proposals()` (line 656), `governed_action_observation_summary()` (line 641), `GOVERNED_ACTIONS_BLOCK_TAG` (line 24) |
| `crates/harness/src/governed_actions.rs` | `validate_capability_scope()` (line 696), `execute_governed_action()` (line 435), `execute_web_fetch_governed_action()` (line 1398), `web_fetch_execution_summary()` (line 1560) |
| `crates/harness/src/fetched_content.rs` | `FetchedContentInput` (line 4), `FetchedContentFormatter` (line 20), `DefaultFetchedContentFormatter` (line 25), `HtmlMarkdownFormatter` (line 38), `remove_non_content_html_blocks()` (line 133), `extract_first_tag_content()` (line 161) |
| `crates/harness/src/tool_execution.rs` | `WebFetchOutcome` (line 275), `execute_web_fetch()` (line 281) |
| `crates/harness/src/foreground_orchestration.rs` | `orchestrate_telegram_approval_resolution_trigger()` (line 244), `orchestrate_telegram_foreground_trigger()` (line 791), `approval_resolution_message()` (line 1882), `approval_follow_up_episode_text()` (line 1899), `approval_follow_up_delivery_text()` (line 1916), `foreground_assistant_delivery_text()` (line 1925), `emit_typing_chat_action()` (line 1948) |
| `crates/harness/src/telegram.rs` | `TelegramChatAction` (line 111), `TelegramDelivery::send_chat_action()` (line 162), `ReqwestTelegramDelivery::send_chat_action()` (line 402), `render_telegram_html_message()` (line 550) |
| `crates/harness/src/policy.rs` | `classify_governed_action_risk()`, `governed_action_requires_approval()` |
| `crates/contracts/src/lib.rs` | `GovernedActionProposal` (line 722), `CapabilityScope` (line 705), `GovernedActionPayload` (line 761) |
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
Continue the foreground turn using these outcomes. This immediate follow-up cannot execute another governed action; if another fetch or command is still required, say exactly what is missing and ask the user to request it in the next turn. Do not claim that you will perform another action now.
```

Format produced by `governed_action_observation_summary()` (`crates/workers/src/main.rs:641`). Multiple observations are joined with ` | `.

For `web_fetch`, the execution summary includes the target URL, response
content type, formatter kind, and a formatter-produced preview capped at 1,500
characters (`crates/harness/src/governed_actions.rs:1560`). The raw fetched
body is still stored in the execution record payload together with formatter
metadata: `formatted_preview`, `formatter_kind`, `preview_truncated`, and
`content_type`.

Formatter selection is isolated behind `FetchedContentFormatter` in
`crates/harness/src/fetched_content.rs:20`. The default implementation routes
HTML content to `HtmlMarkdownFormatter`, which first removes non-content
`script`, `style`, `noscript`, and `svg` blocks. If a `<pre>` block is present,
the formatter treats the page as terminal-style HTML, extracts that text, strips
tags, decodes common HTML entities, removes terminal escape sequences, replaces
box-drawing table rules, normalizes whitespace by line, and preserves readable
line breaks. Other HTML is converted through `html2md`. Non-HTML content uses
the same plain-text sanitizer. If the response was byte-truncated by the
configured `max_response_bytes`, or if the model-facing preview was
character-truncated, the summary explicitly says so.

For approval-triggered action execution, `approval_resolution_message()` (`crates/harness/src/foreground_orchestration.rs:1882`) sends only the approval decision acknowledgement and request title. After an approved action executes, `approval_follow_up_episode_text()` (`crates/harness/src/foreground_orchestration.rs:1899`) stores the model follow-up text first, then appends `Harness governed-action observation: {kind}:{summary}` to the stored assistant follow-up message. This keeps the model's user-facing commitment visible when later context assembly truncates recent-history messages, while still making the action result visible in `recent_history` on subsequent foreground turns. Telegram delivery uses `approval_follow_up_delivery_text()` (`crates/harness/src/foreground_orchestration.rs:1916`), which sends only the model's user-facing follow-up text and falls back to `"Approved action completed."` when the model text is empty.

Normal foreground Telegram delivery also passes through `foreground_assistant_delivery_text()` (`crates/harness/src/foreground_orchestration.rs:1925`). If the model text is empty after one or more approval prompts were created, the delivered and stored assistant text becomes an explicit approval-pending fallback instead of an empty string. This prevents Telegram `sendMessage` HTTP 400 failures from leaving the original ingress in `processing` state and replaying it during restart recovery.

Foreground processing and approval resolution emit best-effort Telegram `typing` chat actions through `emit_typing_chat_action()` (`crates/harness/src/foreground_orchestration.rs:1948`) and `TelegramDelivery::send_chat_action()` (`crates/harness/src/telegram.rs:162`). Chat action failures are logged and do not fail the foreground turn; the actual `sendMessage` path remains authoritative for delivery.

---

## 3. Configuration & Extension

### `capability_scope` Validation Rules

Enforced by `validate_capability_scope()` in `governed_actions.rs:692`. All limits are read from `RuntimeConfig` (`config/default.toml: [governed_actions]`).

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

*Last verified: branch `usage-improvements`, session 2026-04-28.*
