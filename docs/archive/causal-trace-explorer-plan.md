# Causal Trace Explorer Plan

## Metadata

- Plan Status: COMPLETED
- Created: 2026-04-29
- Last Updated: 2026-04-30
- Owner: Coding agent
- Approval: APPROVED

## Status Legend

- Plan Status values: DRAFT, QUESTIONS PENDING, READY FOR APPROVAL, APPROVED, IN PROGRESS, COMPLETED, BLOCKED
- Task/Milestone Status values: TO BE DONE, IN PROGRESS, COMPLETED, BLOCKED, SKIPPED

## Goal

Build a first-class causal trace explorer that lets an operator inspect one assistant request, scheduled task run, wake signal, or governed action as a connected cause-and-effect flow without manually combining Docker logs, ad hoc CLI commands, and database queries.

The end state is an auditable management surface that can answer: what triggered this work, what context and model input were used, what the model proposed, how the harness evaluated it, what actions were approved, blocked, or executed, what follow-up work was created, and where the flow failed or succeeded.

## Scope

- Add a read-only management CLI surface for trace lookup, timeline rendering, and machine-readable JSON output.
- Build the initial trace view from existing durable tables before adding new persistence.
- Persist exact model-call records for foreground and background model invocations, including prompt/message payloads and provider response metadata.
- Add explicit causal links for important cross-table cause-and-effect relationships that are currently implicit or convention-based.
- Cover Telegram foreground requests, scheduled foreground tasks, wake-signal foreground conversion, governed actions, approvals, and model follow-up calls.
- Add focused automated tests at the lowest effective layer, then integration tests for the runtime admin command surface.
- Update internal implementation documentation when behavior or source references change.

## Non-Goals

- Do not build a full authenticated web admin panel in the first implementation pass.
- Do not expose hidden model chain-of-thought. The trace explorer should expose model inputs, model outputs, structured action proposals, model-provided rationale fields, harness policy rationale, tool observations, and decision reasons.
- Do not replace existing tracing/logging infrastructure.
- Do not change assistant behavior, governed-action policy, scheduling semantics, or Telegram delivery logic except where needed to attach durable observability records.
- Do not persist secrets, raw environment variables, Telegram tokens, database URLs, or other local operator secrets in trace records.

## Assumptions

- The first operator experience should be CLI-first because the repository already has a broad `admin` management surface and tests for it.
- HTML or Mermaid export is useful after the normalized trace data exists, but it should not block the first useful CLI increment.
- Existing identifiers are enough for a useful first pass: `trace_id`, `execution_id`, ingress IDs, episode IDs, approval request IDs, governed action IDs, wake signal IDs, scheduled task IDs, and background job run IDs.
- Exact model-call input is not currently durable enough for the requested troubleshooting experience, so schema and worker/gateway changes are required.
- Sensitive value redaction must happen before persistence, not only at render time.
- Bulky model-call text payloads should have a default retention window of one month, and operators should be able to configure that window without code changes.
- Trace visual exports may start as local files, but the rendering boundary should be kept clean enough that a future read-only web UI can reuse the normalized trace model.
- Trace text output may include full Telegram message text by default for now, while still applying secret redaction and preserving JSON output for exact structured inspection.
- The current latest reviewed migration is `0010__conscious_tool_action_kinds.sql`; new schema work should use the next reviewed migration number unless another migration lands first.

## Open Questions

- None.

## Milestones

### Milestone 1: Existing-Data Trace CLI

- Status: COMPLETED
- Purpose: Deliver a useful troubleshooting tool quickly by normalizing existing durable records into one timeline, without waiting for schema changes.
- Exit Criteria: An operator can run trace commands against existing data and see a coherent timeline for foreground ingress, execution records, episodes, audit events, governed actions, approvals, wake signals, and scheduled task runs when those records exist.

#### Task 1.1: Define Trace Domain Types

- Status: COMPLETED
- Objective: Add a typed in-memory trace representation that can support text, JSON, and future graph rendering.
- Steps:
  1. Add trace domain structs in `crates/harness/src/management.rs` or a focused `crates/harness/src/management_trace.rs` module following existing harness management patterns.
  2. Model trace nodes with stable fields: node kind, ID, timestamp, status, title, summary, payload reference, and related IDs.
  3. Model trace edges with stable fields: source node, target node, edge kind, timestamp, and optional detail.
  4. Add serialization support only for management output types that need `--json`.
- Validation: `cargo check -p harness`.
- Notes: Keep control-plane logic in `crates/harness`; keep `crates/runtime` as thin CLI wiring.

#### Task 1.2: Implement Existing-Table Trace Query

- Status: COMPLETED
- Objective: Build a management query that assembles trace nodes and inferred edges from current tables.
- Steps:
  1. Query `execution_records` by `trace_id` or `execution_id`.
  2. Join or separately fetch linked `ingress_events` through `execution_ingress_links`.
  3. Fetch `episodes` and `episode_messages` by episode ID or execution relationship where available.
  4. Fetch relevant `audit_events` by `trace_id`, `execution_id`, and domain IDs found during assembly.
  5. Fetch related `approval_requests`, `governed_action_executions`, `wake_signals`, `background_job_runs`, and `scheduled_foreground_tasks` using direct IDs and known payload conventions.
  6. Sort nodes into a stable chronological timeline and infer edges for known relationships.
- Validation: Add harness management component tests with migrated disposable PostgreSQL and run `cargo test -p harness --test management_component -- --nocapture`.
- Notes: This pass should gracefully report missing optional relationships instead of failing the entire trace.

#### Task 1.3: Add Admin Trace CLI Commands

- Status: COMPLETED
- Objective: Expose trace lookup through the runtime admin CLI.
- Steps:
  1. Add `admin trace show --trace-id <uuid>`.
  2. Add `admin trace show --execution-id <uuid>`.
  3. Add `admin trace recent --limit <n>` with optional source filters if existing management command patterns support them cleanly.
  4. Add `--json` output for `trace show`.
  5. Keep text output compact: summary header, timeline, inferred edges, missing-data notes.
- Validation: `cargo test -p runtime --test admin_cli -- --nocapture` and `cargo test -p runtime --bin runtime -- --nocapture`.
- Notes: Match existing `admin` parser and formatter style.

### Milestone 2: Durable Model-Call Records

- Status: COMPLETED
- Purpose: Persist exact model-call inputs and outputs so operators can inspect what the model saw and what it returned without relying on provider logs or Docker logs, while bounding storage growth with configurable retention.
- Exit Criteria: Foreground and background model invocations create redacted durable model-call records linked to trace and execution IDs, trace output includes them, and bulky text payload retention defaults to one month with operator configuration.

#### Task 2.1: Add Model Call Migration

- Status: COMPLETED
- Objective: Add reviewed SQL schema for durable model invocation records.
- Steps:
  1. Create a migration after the current latest reviewed migration, currently `0010__conscious_tool_action_kinds.sql`.
  2. Add a `model_call_records` table with fields for call ID, trace ID, execution ID, loop kind, purpose, provider, model, request payload JSON, response payload JSON, system prompt text, messages JSON, token counts where available, status, error summary, started timestamp, completed timestamp, payload retention expiry, payload cleared timestamp, and payload retention reason.
  3. Add indexes for `trace_id`, `execution_id`, `started_at`, `loop_kind`, and `status`.
  4. Ensure schema names and constraints match existing migration conventions.
- Validation: `cargo test -p harness --test migration_component -- --nocapture`.
- Notes: Include only redacted request/response material suitable for operator debugging.

#### Task 2.2: Persist Model Calls At The Gateway Boundary

- Status: COMPLETED
- Objective: Capture model-call records around actual provider invocation.
- Steps:
  1. Extend contracts or harness/worker request types as needed to carry a model-call ID or trace metadata to the gateway boundary.
  2. Insert a pending model-call record before provider invocation.
  3. Update the record on success with response metadata and parsed response payload.
  4. Update the record on failure with status and error summary.
  5. Add redaction before persistence for secrets and overly large or unsafe payloads.
- Validation: Add focused unit/component tests for successful and failed model-call persistence; run `cargo test -p harness --test management_component -- --nocapture` and `cargo test -p workers -- --nocapture` if worker-facing tests are affected.
- Notes: Review `crates/workers/src/main.rs` and `crates/harness/src/model_gateway.rs` during implementation to choose the narrowest persistence boundary.

#### Task 2.3: Link Model Calls Into Trace Output

- Status: COMPLETED
- Objective: Show model invocation records in trace timelines.
- Steps:
  1. Fetch `model_call_records` during trace assembly by trace ID and execution ID.
  2. Add nodes for model call started, completed, and failed states.
  3. Add edges from execution records to model calls and from model calls to detected governed-action proposal records when IDs are available.
  4. Add text output fields for provider, model, purpose, timing, status, token counts, and prompt/message availability.
  5. Add JSON output fields for exact redacted prompt/message payloads.
- Validation: Extend trace management component tests and runtime admin CLI tests; run `cargo test -p runtime --test admin_cli -- --nocapture`.
- Notes: Text output may include full Telegram message text, but should avoid dumping full model prompts by default unless an explicit verbose mode is added.

#### Task 2.4: Add Configurable Model Payload Retention

- Status: COMPLETED
- Objective: Bound storage growth for exact model request/response text while keeping recent traces richly inspectable.
- Steps:
  1. Add operator configuration for model-call payload retention with a default of one month.
  2. Define which fields are retention-managed, such as full system prompt text, message arrays, request payload JSON, and response payload JSON.
  3. Add a harness-owned cleanup path that clears or compacts expired bulky payload fields while preserving model-call metadata, status, timing, token counts, trace IDs, execution IDs, and summaries.
  4. Expose retention behavior through management diagnostics or documentation so operators know when payloads may be absent due to retention.
  5. Ensure trace rendering distinguishes payloads missing due to retention from payloads that were never captured.
  6. Add the new configuration knob to `config/default.toml` and `config/local.example.toml` if it follows existing operator-config conventions.
- Validation: Add component tests for default retention, custom retention, and trace output after payload cleanup; run `cargo test -p harness --test management_component -- --nocapture` and `cargo test -p harness --test unconscious_component -- --nocapture` if retention is implemented through background maintenance.
- Notes: Prefer reusing the existing background-maintenance or management cleanup patterns instead of adding an unrelated cleanup mechanism. Update the relevant internal doc configuration section for the new knob.

### Milestone 3: Explicit Causal Links

- Status: COMPLETED
- Purpose: Replace fragile inference with durable graph edges for relationships that matter operationally.
- Exit Criteria: New flows write causal links as they run, and the trace explorer prefers explicit links while retaining inferred links for historical records.

#### Task 3.1: Add Causal Links Migration

- Status: COMPLETED
- Objective: Add reviewed SQL schema for durable causal graph edges.
- Steps:
  1. Add a migration after the latest reviewed migration at implementation time.
  2. Add a `causal_links` table with link ID, trace ID, source kind, source ID, target kind, target ID, edge kind, created timestamp, and payload JSON.
  3. Add indexes for `trace_id`, `(source_kind, source_id)`, `(target_kind, target_id)`, and `edge_kind`.
  4. Use constrained text or enum-like check constraints where consistent with existing migration style.
- Validation: `cargo test -p harness --test migration_component -- --nocapture`.
- Notes: Keep the schema generic enough for future traceable flows without making it an untyped event log replacement.

#### Task 3.2: Write Links In Foreground Flow

- Status: COMPLETED
- Objective: Persist explicit causal links for Telegram and scheduled foreground execution.
- Steps:
  1. Link `ingress_event -> execution_record` as `triggered_execution`.
  2. Link `execution_record -> episode` as `opened_episode` or `continued_episode`.
  3. Link `execution_record -> model_call_record` as `invoked_model`.
  4. Link `model_call_record -> governed_action_execution` as `proposed_action` when applicable.
  5. Link `governed_action_execution -> approval_request` as `required_approval` when applicable.
  6. Link governed action execution to scheduled task records for create/update/delete scheduling actions.
- Validation: `cargo test -p harness --test foreground_component -- --nocapture` and `cargo test -p harness --test governed_actions_component -- --nocapture`.
- Notes: Relevant implementation areas include foreground orchestration, governed action proposal handling, and scheduled foreground task handling.

#### Task 3.3: Write Links In Background And Wake-Signal Flow

- Status: COMPLETED
- Objective: Persist explicit causal links for background maintenance and wake-signal conversion into foreground work.
- Steps:
  1. Link `background_job_run -> model_call_record` as `invoked_model`.
  2. Link `background_job_run -> wake_signal` as `recorded_wake_signal`.
  3. Link `wake_signal -> ingress_event` as `staged_foreground_trigger` when policy accepts the wake signal.
  4. Link `wake_signal -> audit_event` or equivalent reviewed decision record as `reviewed_by_policy` if a durable target is available.
- Validation: `cargo test -p harness --test unconscious_component -- --nocapture` and `cargo test -p harness --test unconscious_integration -- --nocapture`.
- Notes: If audit event IDs are not durable or accessible enough, record policy review detail in the causal link payload instead.

#### Task 3.4: Prefer Explicit Links In Trace Assembly

- Status: COMPLETED
- Objective: Use durable causal links as the primary graph source while preserving historical trace usefulness.
- Steps:
  1. Fetch explicit causal links by trace ID.
  2. Merge explicit links with inferred links from Milestone 1.
  3. De-duplicate links using source, target, and edge kind.
  4. Mark links as `explicit` or `inferred` in JSON output and text output where useful.
  5. Add missing-data notes when trace records predate causal link persistence.
- Validation: Extend management component trace tests to cover mixed explicit and inferred links.
- Notes: This prevents old records from becoming unreadable after the schema enhancement.

### Milestone 4: Scheduling Troubleshooting View

- Status: COMPLETED
- Purpose: Address the immediate scheduling pain point with a focused trace projection that explains why scheduling did or did not happen.
- Exit Criteria: An operator can inspect a scheduling-related request or scheduled task run and see proposal, policy, approval, task mutation, scheduler claim, foreground execution, and delivery outcome.

#### Task 4.1: Add Scheduling Trace Projection

- Status: COMPLETED
- Objective: Add scheduling-specific summaries on top of the generic trace graph.
- Steps:
  1. Detect scheduling governed actions and scheduled foreground task records in trace assembly.
  2. Summarize requested schedule, policy tier, approval state, final task state, next due time, last execution ID, last outcome, and last outcome reason.
  3. For scheduler-created synthetic ingress, show the original scheduled task key and execution ID.
  4. Include suppression and failure audit events from scheduled foreground processing.
- Validation: Add component tests with scheduled task create/update and scheduler-claim fixtures; run `cargo test -p harness --test management_component -- --nocapture`.
- Notes: This is a projection, not a new source of truth.

#### Task 4.2: Add Operator-Facing Examples

- Status: COMPLETED
- Objective: Document concrete commands for diagnosing scheduling failures.
- Steps:
  1. Add examples to the relevant internal documentation or management CLI docs.
  2. Include examples for tracing from an execution ID, trace ID, scheduled task ID, and recent trace list.
  3. Explain where full prompt/message payloads are available and what is redacted.
- Validation: Manual documentation inspection for consistency with `docs/REQUIREMENTS.md`, `docs/LOOP_ARCHITECTURE.md`, and the implemented CLI help.
- Notes: Keep examples stable and avoid handoff-style prose.

### Milestone 5: Visual Export

- Status: COMPLETED
- Purpose: Make causal flows easier to scan visually after the underlying trace model is reliable.
- Exit Criteria: A trace can be exported as a local HTML or Mermaid artifact that shows timeline and graph relationships without needing a running web service, and the renderer consumes the same normalized trace model that a future read-only web UI could reuse.

#### Task 5.1: Add Mermaid Or HTML Trace Export

- Status: COMPLETED
- Objective: Render a local visual representation of a trace.
- Steps:
  1. Add `admin trace render --trace-id <uuid> --format mermaid` or `--format html`, choosing the smallest option that fits existing CLI conventions.
  2. Render nodes by kind and status with stable labels.
  3. Render edges by causal relationship.
  4. Include a timeline table with timestamps, durations, status, and summaries.
  5. Avoid embedding unredacted full prompts or message payloads in HTML by default.
- Validation: Add runtime formatter tests for deterministic output and inspect one generated artifact manually.
- Notes: Prefer static output over a live web server in this milestone.
- Notes: Keep rendering code separated from CLI argument handling so a future read-only web UI can reuse the trace model and renderer with minimal reshaping.

#### Task 5.2: Add JSON Contract Stability Tests

- Status: COMPLETED
- Objective: Keep machine-readable trace output stable enough for future UI reuse.
- Steps:
  1. Add snapshot-like tests or explicit JSON shape assertions for representative traces.
  2. Cover foreground request, scheduling request, scheduled task execution, governed action approval, wake-signal conversion, and model-call failure.
  3. Document intentional breaking-change process if existing management JSON output has a local convention.
- Validation: `cargo test -p runtime --test admin_cli -- --nocapture` and relevant harness management tests.
- Notes: Avoid brittle ordering except where chronological ordering is part of the contract.

### Milestone 6: Documentation And Final Verification

- Status: COMPLETED
- Purpose: Ensure implementation, docs, tests, and repository hygiene are complete.
- Exit Criteria: Internal docs reflect live behavior, temporary artifacts are removed, and final verification passes or any blocker is explicitly recorded.

#### Task 6.1: Update Internal Documentation

- Status: COMPLETED
- Objective: Document the new trace explorer, model-call persistence, and causal link behavior.
- Steps:
  1. Update `docs/internal/INTERNAL_DOCUMENTATION.md` if a new internal doc is added.
  2. Update relevant conscious loop, governed action, foreground, scheduled foreground, and management CLI internal documentation.
  3. Verify every changed source line reference resolves to the correct symbol.
  4. Re-stamp verified dates on affected internal docs.
  5. Remove any `NOT IMPLEMENTED` callouts that become implemented.
- Validation: Manual inspection plus `git diff -- docs/ PHILOSOPHY.md README.md AGENTS.md`.
- Notes: Internal documentation must not contradict canonical docs.

#### Task 6.2: Cleanup Intermediate Artifacts

- Status: COMPLETED
- Objective: Remove artifacts created only to support implementation.
- Steps:
  1. Inspect the worktree for temporary documentation, scratch scripts, generated trace exports, debug logs, one-off fixtures, and obsolete plan fragments.
  2. Remove only artifacts that are not part of the intended final repository state.
  3. Keep maintainable tests, fixtures, docs, and generated files that are part of the repository contract.
- Validation: `cmd.exe /c git status --short` and `cmd.exe /c git diff --name-only` show only intended final changes.
- Notes: Windows Git is the source of truth for worktree status in this repository.

#### Task 6.3: Final Verification

- Status: COMPLETED
- Objective: Validate the integrated change after cleanup.
- Steps:
  1. Run `cargo fmt --all --check`.
  2. Run `cargo check --workspace`.
  3. Run `cargo test -p runtime --test admin_cli -- --nocapture`.
  4. Run `cargo test -p runtime --bin runtime -- --nocapture`.
  5. Run `cargo test -p harness --test management_component -- --nocapture`.
  6. Run `cargo test -p harness --test management_integration -- --nocapture`.
  7. Run `cargo test -p harness --test migration_component -- --nocapture`.
  8. Run additional foreground, governed action, unconscious, runtime, or use-case suites touched by implementation.
  9. Record any environment-specific failures and the exact command output summary in this plan.
- Validation: All listed commands pass, or blockers are recorded with concrete failure details.
- Notes: Use the lowest effective test layer while developing, then rerun this final verification set before completion.

## Approval Gate

Implementation started after user approval on 2026-04-30.

Approval was given explicitly: `the plan is approved, you can implement it`.

## Plan Self-Check

- [x] Plan location follows the default location rule.
- [x] Plan status is `READY FOR APPROVAL`.
- [x] Scope, non-goals, assumptions, and open questions are explicit.
- [x] Tasks are grouped into milestones because the plan has more than 10 tasks.
- [x] Every task has concrete steps and validation.
- [x] Every milestone has exit criteria.
- [x] Cleanup and final verification are included.
- [x] The plan avoids vague actions without concrete targets.
- [x] The plan can be executed by a coding agent without reading the original conversation.
- [x] 2026-04-30 review verified the current latest migration is `0010__conscious_tool_action_kinds.sql`.
- [x] 2026-04-30 review verified named existing tables against `migrations/`.
- [x] 2026-04-30 review verified referenced harness and runtime test files exist.
- [x] 2026-04-30 review added explicit retention metadata so expired payloads can be distinguished from never-captured payloads.

## Execution Notes

- Update milestone and task status before starting and after validation.
- Update each task to COMPLETED immediately after its validation passes.
- Mark tasks or milestones BLOCKED with a short reason when progress cannot continue.
- Implementation is in progress after approval.
- Final verification completed on 2026-04-30:
  - `cargo fmt --all --check`
  - `cargo check --workspace`
  - `cargo test -p runtime --test admin_cli -- --nocapture`
  - `cargo test -p runtime --bin runtime -- --nocapture`
  - `cargo test -p harness --test management_component -- --nocapture`
  - `cargo test -p harness --test management_integration -- --nocapture`
  - `cargo test -p harness --test migration_component -- --nocapture`
  - `cargo test -p harness --test foreground_component -- --nocapture`
  - `cargo test -p harness --test governed_actions_component -- --nocapture`
  - `cargo test -p harness --test governed_actions_integration -- --nocapture`
  - `cargo test -p harness --test unconscious_component -- --nocapture`
  - `cargo test -p harness --test unconscious_integration -- --nocapture`
  - `cargo test -p harness config::tests:: --lib -- --nocapture`
  - `cargo test -p workers -- --nocapture`
- Post-implementation self-check completed on 2026-04-30:
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - Rechecked trace assembly for causal-link-only scheduled task nodes.
  - Fixed `load_trace_scheduled_task_nodes()` so governed-action schedule mutations are visible even before a task has a current or last execution.
  - Added `admin trace cleanup-model-payloads` so the model-call payload retention policy has an operator-reachable cleanup path.
  - Reran `cargo fmt --all --check`, `cargo check --workspace`, `cargo test -p harness --test management_component -- --nocapture`, and `cargo test -p runtime --test admin_cli -- --nocapture`.
