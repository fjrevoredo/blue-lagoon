# Context Assembly

---

## 1. Overview

Context assembly is the process that produces the `ModelInput` — a system prompt plus an ordered message array — sent to the conscious worker at the start of each foreground turn.

It is the translation layer between the harness's stored state (self-model,
identity lifecycle, episode history, retrieved context, recovery context) and
what the model actually receives. Every detail the agent can reason about - its
identity, its capabilities, recent conversation history, retrieved memories,
pending actions - enters through this pipeline. Nothing else reaches the model.

---

## 2. Implementation

### Source Files

| File | Relevant symbol |
|---|---|
| `crates/harness/src/context.rs` | `assemble_foreground_context()` (line 77), `apply_identity_lifecycle_context()` (line 205), assembly limit constants (lines 18-20) |
| `crates/workers/src/main.rs` | `build_model_input()` (line 512), `build_model_call_request()` (line 419), `identity_kickstart_schema_message()` (line 671), `build_identity_kickstart_proposals()` (line 840) |
| `crates/contracts/src/lib.rs` | `SelfModelSnapshot` (line 410), `predefined_identity_templates()` (line 616), `predefined_identity_delta()` (line 642) |
| `config/self_model_seed.toml` | Bootstrap self-model and seed identity values |
| `config/default.toml` | `harness.default_foreground_token_budget` |

### Assembly Pipeline

Steps execute in order inside `assemble_foreground_context()`:

1. Self-model loaded via `self_model::load_self_model_snapshot()` - from the seed TOML or from a canonical artifact in the DB (see Self-Model Seed below).
2. Identity lifecycle context applied via `apply_identity_lifecycle_context()`.
   Bootstrap-only state exposes the one-time kickstart context with predefined
   template summaries; complete identity state reconstructs a compact identity
   snapshot from active `identity_items`.
3. Internal state snapshot built from `InternalStateSeed` + active conditions.
4. Trigger text truncated to `trigger_text_char_limit` characters.
5. Recent episode history fetched - up to `recent_history_limit` episodes before the trigger timestamp; each message truncated to `history_message_char_limit` characters.
6. Retrieved context assembled via `retrieval::assemble_retrieved_context()`.

### `ModelInput` Structure

Two fields passed to the model gateway:

1. **`system_prompt`** — single formatted string (see System Prompt Template below).
2. **`messages`** — ordered `Vec<ModelInputMessage { role, content }>` (see Message Array Ordering below).

The harness now persists the exact `ModelCallRequest` material at the worker
gateway boundary before provider invocation. Recent retained trace reports can
therefore show the system prompt and message array that were available to the
model for a foreground or background turn. See
`docs/internal/harness/TRACE_EXPLORER.md` for retention and trace lookup
details.

### Token Budget

Computed in `build_model_call_request()` (`main.rs:419`):

| Field | Value | Source |
|---|---|---|
| `token_budget` | `4_000` (default) | `config/default.toml: harness.default_foreground_token_budget` |
| `max_output_tokens` | `min(token_budget, 800)` | `main.rs:424` |
| `max_input_tokens` | `max(1, token_budget - max_output_tokens)` | `main.rs:425` |

The unconscious loop uses the same pattern but caps `max_output_tokens` at `1_200` (`main.rs:453`).

### System Prompt Template

Constructed in `build_model_input()` at `main.rs:512`. Exact format:

```
You are {stable_identity}, a harness-governed personal AI assistant. You communicate with a single privileged user via Telegram.

Role: {role}. Communication style: {communication_style}. Behavioral preferences: {preferences}.[ Identity formation is available when the user asks to shape the assistant's first complete identity.][ Identity: {identity_summary}. Self-description: {self_description}. Values: {values}. Boundaries: {boundaries}.]

Capabilities: {capabilities}.
Active constraints: {constraints}.
Goals: {current_goals}.[ Active subgoals: {current_subgoals}.][ Active conditions: {active_conditions}.]

Current time: {current_time}.

Runtime state: load={load_pct}%, health={health_pct}%, confidence={confidence_pct}%, mode={mode}.

You have governed actions available for executing commands and running workspace scripts. Network access is disabled by default; any proposal with network enabled is automatically routed for approval. See the developer message for the full action schema. Never tell the user you have no tools — use the governed action system when needed.
```

Field sources:

| Template field | Source |
|---|---|
| `{stable_identity}` | `self_model.stable_identity` |
| `{role}` | `self_model.role` |
| `{communication_style}` | `self_model.communication_style` |
| `{preferences}` | `self_model.preferences` joined |
| identity formation fragment | Present when `self_model.identity_lifecycle.kickstart_available` is true |
| active identity fragment | Present when `self_model.identity` is populated from active identity items |
| `{capabilities}` | `self_model.capabilities` joined |
| `{constraints}` | `self_model.constraints` joined |
| `{current_goals}` | `self_model.current_goals` joined |
| `{current_subgoals}` | `self_model.current_subgoals` joined — **fragment omitted when empty** |
| `{active_conditions}` | `internal_state.active_conditions` joined — **fragment omitted when empty** |
| `{current_time}` | `context.assembled_at` formatted as `"%Y-%m-%d %H:%M UTC"` |
| `{load_pct}` | `internal_state.load_pct` (u8, 0–100) |
| `{health_pct}` | `internal_state.health_pct` (u8, 0–100) |
| `{confidence_pct}` | `internal_state.confidence_pct` (u8, 0–100) |
| `{mode}` | `"single_ingress"` or `"backlog_recovery"` |

`InternalStateSnapshot` also tracks `reliability_pct`, `resource_pressure_pct`, and `connection_quality_pct` — these are intentionally omitted from the prompt for brevity.

### Message Array Ordering

Messages are appended in this order by `build_model_input()`:

| # | Role | Content | Condition |
|---|---|---|---|
| 1..N | User / Assistant | Recent episode excerpts, oldest first (reversed from DB fetch) | Always |
| N+1 | User | Current trigger `text_body` | Only if `text_body` is `Some` |
| N+2 | Developer | Backlog recovery notice with ordered ingress batch | Only in `BacklogRecovery` mode with non-empty `ordered_ingress` |
| N+3 | Developer | `"Retrieved canonical context: ..."` summary | Only if `retrieved_context.items` is non-empty |
| N+4 | Developer | Governed action observations | If `governed_action_observations` is non-empty |
| N+4 (alt) | Developer | Full governed action schema | If `governed_action_observations` is empty |
| N+5 (alt) | Developer | Identity kickstart action block schema and predefined template summaries | If governed action observations are empty and identity kickstart is available |

`ModelMessageRole::Developer` maps to `"system"` in the API request body (`crates/harness/src/model_gateway.rs:474`). Multiple system-role messages in the messages array are valid in the ZAi/OpenAI-compatible API format used.

Approval-triggered governed actions add one more persistence rule: after an approved action executes, `approval_follow_up_episode_text()` in `crates/harness/src/foreground_orchestration.rs:1928` stores the model follow-up text first, then appends the harness observation. That persisted message is then available to later context assembly through normal `recent_history`, independent of the transient `governed_action_observations` field used for the immediate follow-up call. The model text comes first because `history_message_char_limit` truncates from the start of each message; user-visible commitments such as follow-up actions must survive even when a long fetched preview is appended. Telegram delivery uses `approval_follow_up_delivery_text()` in `crates/harness/src/foreground_orchestration.rs:1945`, so the user sees only the model-facing follow-up text while the harness observation remains in durable context. For `web_fetch`, the observation text contains the formatter kind and a bounded model-facing preview produced by `FetchedContentFormatter` (`crates/harness/src/fetched_content.rs:20`), including terminal-style `<pre>` extraction for HTML responses when present, while the full raw body remains in the execution record payload.

### Self-Model Seed

Location: `config/self_model_seed.toml`. Loaded by
`self_model::load_self_model_snapshot()`. Flat seed fields preserve the legacy
bootstrap self-model, while `[identity]` seed fields provide initial rich
identity context until a complete identity is selected. `SelfModelSnapshot` is
defined in `crates/contracts/src/lib.rs:410`.

| Field | Type | Semantic meaning |
|---|---|---|
| `stable_identity` | `String` | Agent name/handle - surfaced first in system prompt |
| `role` | `String` | Functional role label |
| `communication_style` | `String` | Default interaction tone |
| `capabilities` | `Vec<String>` | What the agent can do - surfaced in system prompt |
| `constraints` | `Vec<String>` | Policy-level restrictions - surfaced in system prompt |
| `preferences` | `Vec<String>` | Behavioral defaults - surfaced in system prompt |
| `current_goals` | `Vec<String>` | High-level goals - surfaced in system prompt |
| `current_subgoals` | `Vec<String>` | Active sub-objectives — surfaced only if non-empty |
| `[identity]` fields | TOML table | Rich seed identity values and lifecycle bootstrap defaults |

`SelfModelSourceKind` (`crates/harness/src/self_model.rs:104`) records which source was used at runtime: `BootstrapSeed` or `CanonicalArtifact`. If a canonical artifact exists in the DB (written by the background loop), it takes precedence over the seed.

### Identity Kickstart

Bootstrap-only identity state exposes a harness-native kickstart block tagged
`blue-lagoon-identity-kickstart`. A model can emit
`select_predefined_identity` with one of the template keys returned by
`predefined_identity_templates()`. The worker strips the block from user-visible
assistant text, converts it to an `identity_delta` canonical proposal using
`predefined_identity_delta()`, and the harness merge path persists identity
items plus a `complete_identity_active` lifecycle transition.

The same block also supports `start_custom_identity_interview`,
`answer_custom_identity_question`, and `cancel_identity_formation`. During a
custom interview, context assembly loads the active interview and sets
`kickstart.next_step` to the next required missing field so an interrupted
conversation can resume deterministically. Each answer is persisted in
`identity_kickstart_interviews`; the final required answer is converted by the
harness into canonical identity items and a complete lifecycle transition.

---

## 3. Configuration & Extension

### Tunable Limits

All three assembly limits live as constants in `crates/harness/src/context.rs:18-20` and can be overridden at the call site via `ContextAssemblyLimits`:

| Constant | Default | What it controls |
|---|---|---|
| `DEFAULT_RECENT_HISTORY_LIMIT` | `8` | Max episodes fetched from DB per turn |
| `DEFAULT_TRIGGER_TEXT_CHAR_LIMIT` | `2_000` | Max chars of incoming trigger text |
| `DEFAULT_HISTORY_MESSAGE_CHAR_LIMIT` | `400` | Max chars per episode message (user and assistant independently) |

### Token Budget

Change `harness.default_foreground_token_budget` in `config/local.toml` to override the default of `4_000`. The `max_output_tokens` cap of `800` is hardcoded in `main.rs:424` - raise it there if longer responses are needed (re-run the component test suite afterwards).

### Self-Model Seed

Edit `config/self_model_seed.toml` to change the bootstrap values for identity,
capabilities, constraints, goals, etc. These take effect immediately on next
boot if no canonical artifact exists in the DB. Complete identity selection is
durable in the identity tables. Operators can reopen first identity formation
with `cargo run -p runtime -- admin identity reset --force`, inspect identity
with `admin identity status` and `admin identity show`, and propose controlled
post-kickstart edits with `admin identity edit propose`.

### Adding a New Context Source

To feed a new data source into the model input:
1. Add the data to `ConsciousContext` in `crates/contracts/src/lib.rs`.
2. Populate it in `assemble_foreground_context()` (`context.rs`).
3. Consume it in `build_model_input()` (`main.rs`) — append a `Developer`-role message.
4. Add a test in the foreground component suite.

---

## 4. Further Reading

- `docs/internal/conscious_loop/GOVERNED_ACTIONS.md` — how the governed action schema message (the last Developer message in the array) is constructed and parsed.
- `docs/internal/harness/TRACE_EXPLORER.md` — how retained model-call inputs and outputs are surfaced for operator debugging.
- `docs/LOOP_ARCHITECTURE.md` — canonical description of the conscious loop and its relationship to the harness.
- `docs/IMPLEMENTATION_DESIGN.md` — design rationale for the two-process model and the worker protocol.
- `crates/harness/src/retrieval.rs` — retrieval ranking algorithm that produces `retrieved_context`.

---

*Last verified: branch `codex/identity-self-model`, session 2026-05-01.*
