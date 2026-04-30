# Trace Explorer

---

## 1. Overview

The trace explorer is the harness-owned operator surface for inspecting one
assistant request or maintenance flow as a connected cause-and-effect graph. It
normalizes existing durable records, model-call records, causal links, and
scheduling state into one read-only management report.

The explorer is CLI-first. It supports compact text output, machine-readable
JSON, and static Mermaid graph rendering without requiring a live admin web UI.

---

## 2. Implementation

### Source Files

| File | Relevant symbol |
|---|---|
| `crates/harness/src/management.rs` | `TraceReport` (line 392), `load_trace_report()` (line 1385), `list_recent_traces()` (line 1410), `load_trace_scheduled_task_nodes()` (line 2427), `load_trace_explicit_causal_links()` (line 2559) |
| `crates/harness/src/model_calls.rs` | `ModelCallRecord` (line 16), `insert_pending_model_call_record()` (line 41), `clear_expired_model_call_payloads()` (line 258), `background_job_run_for_execution()` (line 312) |
| `crates/harness/src/causal_links.rs` | `NewCausalLink` (line 8), `insert()` (line 31), `list_for_trace()` (line 69) |
| `crates/harness/src/worker.rs` | foreground model-call persistence (line 195), background model-call persistence (line 366) |
| `crates/runtime/src/admin.rs` | `TraceCommand` (line 73), `TraceSubcommand` (line 79), `render_trace_report_text()` (line 1181), `render_trace_mermaid()` (line 1292) |
| `migrations/0011__model_call_records.sql` | durable model-call records |
| `migrations/0012__causal_links.sql` | durable causal graph edges |

### Trace Assembly

`management::load_trace_report()` resolves a `trace_id` from either a direct
trace lookup or an `execution_id`. It then builds a `TraceReport` from:

- `execution_records`
- `ingress_events` and `execution_ingress_links`
- `episodes` and `episode_messages`
- `audit_events`
- `model_call_records`
- `background_jobs`, `background_job_runs`, and `wake_signals`
- `approval_requests` and `governed_action_executions`
- `scheduled_foreground_tasks`
- `causal_links`

The report contains:

- `nodes`: stable typed timeline records.
- `edges`: graph relationships marked as `explicit` or `inferred`.
- `scheduling`: scheduling-specific projection for task status, cadence, due
  time, current execution, last execution, and last outcome.
- `notes`: missing-data or compatibility notes.

Explicit causal links replace matching inferred links when source, target, and
edge kind are identical. Inferred links remain for historical records that
predate `migrations/0012__causal_links.sql`.

### Model-Call Records

Foreground and background worker launch paths insert a pending
`model_call_records` row before provider invocation, then update it on success
or failure. Stored payloads include redacted request JSON, response JSON, system
prompt text, message array JSON, provider/model labels, token counts when
available, status, timing, and retention metadata.

If a test or one-shot worker path is intentionally running against an unmigrated
clean schema, the worker protocol continues and skips model-call persistence.
Normal migrated runtime databases are expected to have the table.

Retention-managed fields are:

- `request_payload_json`
- `response_payload_json`
- `system_prompt_text`
- `messages_json`

`clear_expired_model_call_payloads()` clears those bulky fields after the
configured retention window while preserving trace IDs, execution IDs, status,
provider/model metadata, timing, tokens, and `payload_retention_reason`.

### Causal Links

New flows write explicit rows to `causal_links` for the operationally important
relationships:

- `ingress_event -> execution_record` as `triggered_execution`
- `execution_record -> episode` as `opened_episode`
- `execution_record -> model_call_record` as `invoked_model`
- `background_job_run -> model_call_record` as `invoked_model`
- `execution_record -> governed_action_execution` as `planned_action`
- `governed_action_execution -> approval_request` as `required_approval`
- `governed_action_execution -> scheduled_foreground_task` as
  `mutated_scheduled_task`
- `background_job_run -> wake_signal` as `recorded_wake_signal`
- `wake_signal -> ingress_event` as `staged_foreground_trigger`
- `scheduled_foreground_task -> execution_record` as `triggered_execution`

Each link carries compact JSON payload context such as task keys, risk tiers,
reason codes, or model-call request summaries.

### CLI

The runtime admin CLI exposes:

- `cargo run -p runtime -- admin trace show --trace-id <uuid>`
- `cargo run -p runtime -- admin trace show --execution-id <uuid>`
- `cargo run -p runtime -- admin trace show --trace-id <uuid> --json`
- `cargo run -p runtime -- admin trace recent --limit <n>`
- `cargo run -p runtime -- admin trace render --trace-id <uuid> --format mermaid`
- `cargo run -p runtime -- admin trace cleanup-model-payloads`

Text output is compact by default. JSON output contains the full normalized
trace model, including retained model prompts/messages when still available.
Mermaid output renders the same normalized node and edge model as a static
flowchart. Cleanup clears expired bulky model-call prompt/message/request/
response payloads according to the configured retention window.

---

## 3. Configuration & Extension

| Config key | Default | Valid range | Read by |
|---|---:|---|---|
| `observability.model_call_payload_retention_days` | `30` | integer greater than zero | `config.rs:126`, `worker.rs:195`, `worker.rs:366` |

To add a new traceable relationship:

1. Write the source and target records first.
2. Insert a `causal_links` row with stable `source_kind`, `target_kind`, and
   `edge_kind` strings.
3. Add a mapping in `trace_node_kind_for_causal_kind()` if the node kind is new.
4. Extend management component tests so the trace report includes the explicit
   edge.

To add a new trace node source:

1. Add the query in `crates/harness/src/management.rs`.
2. Normalize it into a `TraceNode` with stable `node_kind`, `source_id`,
   timestamp, status, title, summary, payload, and related IDs.
3. Add inferred edges only when they remain useful for old data.
4. Add JSON shape assertions for fields intended for future UI reuse.

---

## 4. Further Reading

- `docs/LOOP_ARCHITECTURE.md` describes the foreground and background loop
  boundaries that traces connect.
- `docs/internal/conscious_loop/CONTEXT_ASSEMBLY.md` documents how model input
  is assembled before model-call persistence records it.
- `docs/internal/conscious_loop/GOVERNED_ACTIONS.md` documents the governed
  action proposals and approvals that appear in trace graphs.
- `crates/runtime/src/admin.rs` contains the operator command parser and trace
  text/Mermaid renderers.

Verified: 2026-04-30.
