# Trace Explorer

---

## 1. Overview

The trace explorer is the harness-owned operator surface for inspecting one
assistant request or maintenance flow as a connected cause-and-effect graph. It
normalizes existing durable records, model-call records, causal links, and
scheduling state into one read-only management report.

The explorer is CLI-first. It now supports:

- diagnosis-first text and JSON output for one trace
- detailed timeline output with a prepended diagnosis summary
- focused failing-node inspection for the most relevant payload
- static Mermaid graph rendering for architecture inspection

Mermaid is no longer the recommended first-line troubleshooting path. Operators
should start with `trace explain` or `trace show`, then use Mermaid only when
they need causal-graph inspection.

---

## 2. Implementation

### Source Files

| File | Relevant symbol |
|---|---|
| `crates/harness/src/management.rs` | `TraceReport` (line 398), `TraceFailureClass` (line 482), `TraceDiagnosisSummary` (line 542), `TraceExplanationReport` (line 584), `load_trace_report()` (line 2165), `diagnose_trace_report()` (line 2201), `classify_failure_text()` (line 3719), `derive_next_steps()` (line 3913), `trace_failure_class_label()` (line 4049) |
| `crates/harness/src/model_calls.rs` | `ModelCallRecord` (line 16), `insert_pending_model_call_record()` (line 41), `clear_expired_model_call_payloads()` (line 258), `background_job_run_for_execution()` (line 312) |
| `crates/harness/src/causal_links.rs` | `NewCausalLink` (line 8), `insert()` (line 31), `list_for_trace()` (line 69) |
| `crates/harness/src/worker.rs` | `launch_conscious_worker_with_timeout()` (line 136), `launch_unconscious_worker_with_timeout()` (line 344), `collect_worker_protocol_failure_context()` (line 620), `stderr_excerpt()` (line 645) |
| `crates/runtime/src/admin.rs` | `TraceSubcommand` (line 85), `TraceExplainCommand` (line 119), `TraceShowCommand` (line 129), `render_trace_explanation_text()` (line 1431), `render_trace_report_text()` (line 1518), `render_trace_mermaid()` (line 1680), `format_trace_failure_class()` (line 1739) |
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

The diagnosis layer derives a second, operator-facing view from `TraceReport`:

- `TraceDiagnosisSummary`: verdict, failure class, first failing step, last
  successful step, side-effect status, user-reply status, retry safety, likely
  cause, next-step hints, and notes. Failure classes include transport,
  persistence, approval, governed-action blocking, malformed governed-action
  proposal, worker protocol, and scheduled foreground validation cases.
- `TraceFocusReport`: read-only inspection of the focused node payload,
  including retained-payload availability and retention/missing-data notes.

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

Worker protocol errors are annotated before they reach trace classification.
The launchers attach a `worker_protocol_phase` marker for spawn, request write,
model-call read, model-call persistence, provider call, model response write,
final response read, child wait, and stderr cleanup phases. When the child exits
early or closes its pipe, the harness closes stdin, waits for the child with a
bounded cleanup timeout, and appends the child exit status plus a short stderr
excerpt when available. `trace explain` classifies those errors as
`worker_protocol_failure` and tells the operator to inspect the worker
binary/configuration before retrying.

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

- `cargo run -p runtime -- admin trace explain --trace-id <uuid>`
- `cargo run -p runtime -- admin trace explain --execution-id <uuid>`
- `cargo run -p runtime -- admin trace explain --trace-id <uuid> --focus failing-node`
- `cargo run -p runtime -- admin trace explain --trace-id <uuid> --json`
- `cargo run -p runtime -- admin trace show --trace-id <uuid>`
- `cargo run -p runtime -- admin trace show --execution-id <uuid>`
- `cargo run -p runtime -- admin trace show --trace-id <uuid> --json`
- `cargo run -p runtime -- admin trace recent --limit <n>`
- `cargo run -p runtime -- admin trace render --trace-id <uuid> --format mermaid`
- `cargo run -p runtime -- admin trace cleanup-model-payloads`

`trace explain` is the primary troubleshooting entrypoint. It renders a
failure-first diagnosis summary and can optionally attach a focused failing-node
inspection. `trace show` renders the same diagnosis summary first, then the full
timeline, edges, scheduling projection, and trace notes. `trace show --json`
continues to emit the normalized `TraceReport`. `trace explain --json` emits a
`TraceExplanationReport` with `diagnosis` and optional `focus`.

Focused inspection is conservative:

- if the focused node exists, the payload is emitted together with an
  availability classification
- if retained bulky model-call payloads were cleared by retention, the report
  marks that explicitly and keeps only retained metadata
- if no failing node exists, focused failing-node inspection reports
  `unavailable` rather than guessing

`trace render --format mermaid` renders the normalized node and edge model as a
static flowchart. Use it for architecture inspection, not as the default
operator troubleshooting path.

Cleanup clears expired bulky model-call prompt/message/request/response
payloads according to the configured retention window while preserving the
metadata required for trace correlation and conservative diagnosis.

---

## 3. Configuration & Extension

| Config key | Default | Valid range | Read by |
|---|---:|---|---|
| `observability.model_call_payload_retention_days` | `30` | integer greater than zero | `config.rs:130`, `worker.rs:200`, `worker.rs:380` |

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

To extend diagnosis:

1. Add the durable evidence to `TraceReport` first if the current node set
   cannot prove the new verdict safely.
2. Extend `diagnose_trace_report()` with a deterministic rule that prefers
   structured payload fields over title or summary text.
3. Keep retry-safety and side-effect classification fail-closed when evidence is
   missing or ambiguous.
4. Extend management component tests and runtime CLI tests together.

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

Verified: 2026-05-07.
