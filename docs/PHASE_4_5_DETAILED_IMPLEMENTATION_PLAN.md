# Blue Lagoon

## Phase 4.5 Detailed Implementation Plan

Date: 2026-04-21
Status: Implemented; non-DB verification completed; DB-backed verification pending in CI or a Docker-enabled local environment
Scope: High-level plan Phase 4.5 only
Audience: LLM-assisted implementation work and human review

## Purpose

This document defines the detailed implementation plan for Phase 4.5 of Blue
Lagoon.

It translates the approved Phase 4.5 scope from
`docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md` into concrete, trackable, and
LLM-executable work items.

Phase 4.5 introduces the first durable management CLI surface behind the
completed Phase 4 foreground, continuity, and background-maintenance slices.
Its purpose is to replace raw-SQL-heavy local operator workflows with a stable
harness-mediated CLI for inspection and safe explicit control while preserving
the existing architecture, policy posture, and canonical write boundaries.

## Canonical inputs

This plan is subordinate to the following canonical documents:

- `PHILOSOPHY.md`
- `docs/REQUIREMENTS.md`
- `docs/LOOP_ARCHITECTURE.md`
- `docs/IMPLEMENTATION_DESIGN.md`
- `docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md`
- `docs/PHASE_1_DETAILED_IMPLEMENTATION_PLAN.md`
- `docs/PHASE_1_1_DETAILED_IMPLEMENTATION_PLAN.md`
- `docs/PHASE_2_DETAILED_IMPLEMENTATION_PLAN.md`
- `docs/PHASE_3_DETAILED_IMPLEMENTATION_PLAN.md`
- `docs/PHASE_4_DETAILED_IMPLEMENTATION_PLAN.md`

If this document conflicts with the canonical documents, the canonical
documents win.

## Documentation readiness review

The Phase 4.5 planning baseline is ready.

The current canonical documents agree on the core Phase 4.5 intent:

- Blue Lagoon must expose a stable management interface for operator inspection
  and explicit control
- that management interface must remain distinct from the main end-user
  conversation surface
- the first version should provide that interface primarily as a CLI
- the management surface must remain harness-mediated and must not bypass
  canonical write ownership, proposal validation, policy checks, approval
  requirements, or execution budgets
- the interface must be capability-oriented and extensible rather than a raw
  database shell or temporary verification script
- Phase 4.5 should remain intentionally narrow and should not absorb Phase 5
  approval, workspace, or governed tool-execution scope

No blocking contradiction was found between
`docs/REQUIREMENTS.md`,
`docs/IMPLEMENTATION_DESIGN.md`,
`docs/LOOP_ARCHITECTURE.md`, and
`docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md`.

The temporary note that originally lived at
`docs/wip/phase-4-5-cli-design-note.md` has now been retired. Its useful
planning decisions have been folded into this document, which remains the
authoritative Phase 4.5 execution ledger.

## CI assessment for Phase 4.5

At Phase 4.5 planning start, the repository-hosted CI posture was strong
enough for foundation, foreground runtime, canonical continuity, and
background maintenance, but it did not yet protect the management CLI surface
as a first-class product interface.

The current stable jobs remain useful:

- `workspace-verification`
- `foreground-runtime`
- `canonical-persistence`
- `background-maintenance`

Phase 4.5 should preserve those stable jobs and add one new capability-based
gate rather than folding the management CLI regressions into an unrelated
existing job.

The Phase 4.5 CI posture is locked as follows:

- Keep `workspace-verification` as the fast repository-wide baseline gate.
- Keep `foreground-runtime` focused on Telegram-first foreground execution
  regressions.
- Keep `canonical-persistence` focused on migration-sensitive and
  canonical-write-sensitive continuity regressions.
- Keep `background-maintenance` focused on scheduler, unconscious-worker, and
  wake-signal regressions.
- Add `management-cli` for management-surface parsing, routing, formatter, and
  persistence-backed operator-flow regressions.

The intended Phase 4.5 gate-to-suite mapping is:

- `workspace-verification`
  Run formatting, compile checks, clippy, and fast unit-focused verification
  that does not require PostgreSQL.
- `foreground-runtime`
  Continue running `foreground_component` and `foreground_integration`.
- `canonical-persistence`
  Continue running `continuity_component` and `continuity_integration`.
- `background-maintenance`
  Continue running `unconscious_component` and `unconscious_integration`.
- `management-cli`
  Run the runtime CLI parsing and surface tests, beginning with a dedicated
  runtime admin CLI suite plus PostgreSQL-backed management component and
  integration suites.

Implementation status note:

- the `management-cli` gate now exists in `.github/workflows/ci.yml`
- local non-DB verification is complete
- DB-backed suite execution is still pending outside this Codex environment
  because local Docker/PostgreSQL access is unavailable here

Phase 4.5 CI expansion should avoid duplicating the same expensive suites
across multiple jobs unless a stricter later-stage gate intentionally reuses
them.

## Implementation starting point

The current repository already contains the main boundaries that Phase 4.5
should extend rather than replace.

The implementation started from the following default starting points:

- `crates/runtime/src/main.rs`
  for the current Clap entrypoint, which is still intentionally thin and
  currently exposes only `migrate`, `harness`, and `telegram`
- `crates/harness/src/runtime.rs`
  for existing one-shot runtime entrypoints that the management CLI should
  reuse or extend rather than bypass
- `crates/harness/src/config.rs`
  for typed config loading, validation, and resolved-subsystem helpers that the
  status surface must inspect safely
- `crates/harness/src/schema.rs`
  for schema compatibility evaluation, which currently fails closed for normal
  runtime execution and will need a read-only inspection posture for management
  status
- `crates/harness/src/background.rs`
  for background job, job-run, and wake-signal persistence services that the
  management surface should inspect and partially reuse for safe operator
  actions
- `crates/harness/src/background_planning.rs`
  for the harness-owned background enqueue path that Phase 4.5 should reuse
  instead of writing ad hoc SQL
- `crates/harness/src/foreground.rs`
  for pending-ingress and recoverable-conversation planning logic that the
  management surface should expose without reimplementing
- `crates/harness/src/audit.rs`
  for trace-linked audit persistence that mutating management operations should
  continue to use
- `crates/harness/src/lib.rs`
  for the public harness module surface that, at planning start, did not yet
  include a dedicated management module
- `crates/harness/tests/support/mod.rs`
  for disposable PostgreSQL test support and migration-backed test setup
- `.github/workflows/ci.yml`
  for the stable capability-based CI gate layout that Phase 4.5 needed to
  extend with a management-specific gate

At implementation start, the repository state also made several important
constraints explicit:

- the runtime crate consisted of a single `main.rs`, so the CLI
  surface will become difficult to evolve cleanly if Phase 4.5 keeps adding
  commands without introducing internal structure
- the runtime command output was predominantly `Debug`-style rather than
  stable operator-facing output
- the existing harness modules already held most of the domain logic needed for
  Phase 4.5, but they do not yet expose a coherent management-oriented service
  layer or read models
- there was no existing management CLI namespace, dedicated management test
  suite, or stable operator-facing formatter layer

## Phase 4.5 target

Phase 4.5 is complete only when Blue Lagoon proves the following:

- the runtime exposes a coherent management CLI namespace under the existing
  `runtime` binary
- the management surface remains harness-mediated, capability-oriented, and
  clearly distinct from the end-user conversation surface
- the minimum required operator tasks can be completed without raw SQL for the
  intended local verification and inspection flows
- the initial management CLI can inspect runtime readiness, pending foreground
  work, background jobs, and wake signals using stable operator-facing output
- the CLI can safely create a background job through the existing harness-owned
  planning and validation path
- the CLI can explicitly execute one due background job without introducing a
  second scheduling authority or new execution semantics
- the initial commands support concise human-readable output and structured
  machine-readable output suitable for scripting
- required Phase 4.5 unit, component, and integration tests pass
- repository CI runs the required management CLI regression suites under a
  stable capability-based gate identity

## Settled implementation clarifications

The following Phase 4.5 decisions are treated as settled for execution unless
later canonical documents intentionally change them:

- Phase 4.5 is a product-interface phase, not a temporary testing-shell phase.
  The implementation should optimize for a stable management interface shape
  that later phases can extend rather than for one-off local debugging
  shortcuts.
- The product concept is the management CLI. The initial command namespace
  should live under `runtime admin ...` so operator-facing commands stay
  grouped without introducing a separate binary or control plane.
- The runtime crate should remain thin. It may grow small CLI-definition and
  output-formatting modules in Phase 4.5, but the domain logic and persistence
  access should remain in `harness`.
- Phase 4.5 should introduce a dedicated harness management service module
  rather than scattering management reads and ad hoc SQL across `runtime` or
  unrelated harness modules.
- The initial Phase 4.5 command set should remain intentionally narrow and
  capability-oriented:
  - `runtime admin status`
  - `runtime admin foreground pending`
  - `runtime admin background list`
  - `runtime admin background enqueue`
  - `runtime admin background run-next`
  - `runtime admin wake-signals list`
- A dedicated `runtime admin telegram status` command is deferred. The initial
  Phase 4.5 requirement is Telegram readiness and binding visibility inside
  `runtime admin status`, not a separate transport-specific diagnostics tree.
- A dedicated `runtime admin verify summary` command is also deferred. The
  management CLI should prefer stable capability commands that can be composed
  by operators and future automation rather than a special verification summary
  tied to one temporary workflow.
- Read-oriented commands should support both concise text output and `--json`
  output in the initial Phase 4.5 slice. That keeps the surface scriptable
  from the start and avoids retrofitting output contracts immediately after the
  first release.
- Admin command output must be operator-facing rather than `Debug`-oriented.
  Data belongs on stdout and failures belong on stderr.
- Read-only status inspection must be able to report degraded or incomplete
  subsystem readiness without forcing the exact same fail-closed path used by
  normal runtime execution. Missing optional subsystem secrets or pending
  migrations should be representable in management status rather than only as
  immediate process errors.
- Mutating management operations must preserve the same audit, policy, planning,
  and canonical-write boundaries as the rest of the runtime.
- `runtime admin background enqueue` must call the existing harness-owned
  background planning path rather than inserting raw rows directly.
- `runtime admin background run-next` should be a focused operator surface over
  the existing one-shot background execution path rather than a new scheduler
  or execution engine.
- Read-only status commands should return success when they can render a
  degraded or incomplete runtime state correctly. Unsupported arguments,
  failed queries, or failed command execution remain process errors.
- `runtime admin background run-next` should treat "no due job" as a successful
  surfaced outcome rather than as a command failure.
- Phase 4.5 should not add arbitrary SQL execution, arbitrary storage mutation,
  an interactive shell, a TUI, or any bypass around Phase 3 or Phase 4 safety
  boundaries.
- Phase 4.5 is expected to reuse the existing schema. A new reviewed migration
  should be added only if implementation reveals a concrete read-model or
  operator-safety requirement that cannot be met cleanly with the current
  tables.
- The default new artifact names for Phase 4.5 should be:
  - harness management module: `crates/harness/src/management.rs`
  - runtime CLI surface test: `crates/runtime/tests/admin_cli.rs`
  - PostgreSQL-backed harness component suite:
    `crates/harness/tests/management_component.rs`
  - architecture-critical harness integration suite:
    `crates/harness/tests/management_integration.rs`
  - repository CI gate: `management-cli`

## Phase 4.5 scope boundary

### In scope for Phase 4.5

- a stable management CLI namespace under the existing `runtime` entrypoint
- a dedicated harness-side management service layer and read-model types
- read-only runtime readiness and status inspection covering schema, worker
  resolution, configured subsystem readiness, and high-level pending-work
  summaries
- inspection of pending or recoverable foreground work without raw database
  queries
- inspection of background jobs, recent job-run state, and wake-signal state
- a safe explicit background enqueue path that reuses background planning and
  validation
- a focused explicit path to execute one due background job through the
  existing harness behavior
- concise human-readable output plus machine-readable `--json` output for the
  initial admin commands
- automated unit, component, and integration coverage for CLI parsing,
  formatter, routing, and persistence-backed management semantics
- repository CI expansion for management CLI regression coverage

### Explicitly out of scope for Phase 4.5

- approval objects, approval resolution, or any Phase 5 governed tool-execution
  behavior
- workspace inspection, workspace mutation, or filesystem tool execution
- arbitrary database consoles, raw SQL execution, or broad destructive admin
  commands
- a general browser-based admin plane, dashboard stack, or TUI
- operator commands that bypass harness policy, proposal validation, merge
  logic, schema checks, or bounded execution
- broad recovery, checkpoint, retry-budget, or supervisor-control behavior from
  Phase 6
- Telegram-specific deep diagnostics trees beyond the minimum readiness and
  binding state surfaced by `runtime admin status`
- verification-specific summary commands whose shape is not clearly reusable as
  part of the long-term management interface

### Deferred by later phases

- richer Telegram diagnostics and transport-level operator commands
- audit browsing and richer trace-query management commands
- approval and workspace inspection commands from Phase 5
- recovery, replay, and continuation-management commands from Phase 6
- dashboards, local status pages, or richer observability exports

### Execution posture confirmed for Phase 4.5

- implement the first management surface behind the existing harness-centered
  control model rather than as a second control plane
- keep the initial command tree intentionally narrow and organized by
  capability
- prefer additive harness service and runtime CLI modules over new top-level
  crates
- keep the runtime binary thin and avoid embedding persistence logic in CLI
  handlers
- use disposable real PostgreSQL for persistence-critical management-command
  verification

## LLM execution rules

The plan should be executed under the following rules:

- Work one task at a time unless a task is explicitly marked as parallel-safe.
- Do not start a task until all of its dependencies are marked `DONE`.
- No core Phase 4.5 task is complete without the verification listed for it.
- Keep the management CLI harness-mediated. If command handlers start reaching
  directly into ad hoc persistence logic from `runtime`, stop and split the
  work first.
- Keep the interface capability-oriented. If a task begins exposing raw storage
  details as the primary operator surface, stop and narrow scope first.
- Prefer the lowest effective test layer.
- Use disposable real PostgreSQL for persistence-critical verification.
- Update this document immediately after finishing each task.

## Progress tracking protocol

This document becomes the progress ledger for Phase 4.5 once implementation
starts.

Each task contains:

- a stable task ID
- a `Status` field
- explicit dependencies
- concrete deliverables
- verification commands or checks
- an `Evidence` field to update when done

Use only these status values:

- `TODO`
- `IN PROGRESS`
- `BLOCKED`
- `DONE`

When a task is completed:

1. Change its `Status` to `DONE`.
2. Fill in the `Evidence` field with the relevant file paths, commands, or test
   evidence.
3. Update the `Progress snapshot` section so the completed count, active task,
   and milestone state remain current.
4. If the task changes execution order or narrows implementation scope further,
   update dependent tasks before moving on.

## Progress snapshot

- Current milestone: `Milestone D`
- Current active task: `P45-15 (DB-backed verification blocked by local Docker/PostgreSQL access in this environment)`
- Completed tasks: `14/16`
- Milestone A status: `DONE`
- Milestone B status: `DONE`
- Milestone C status: `DONE`
- Milestone D status: `BLOCKED`

Repository sequencing note:

- Phase 4 is complete, and this document is the draft execution ledger for the
  bridging Phase 4.5 management CLI slice that must land before Phase 5
  broadens the operator surface further.

## Execution refinement notes

The current task count and scope are appropriate for starting implementation,
but several tasks are interface-heavy enough that they should be treated as
likely split points if the code reveals more coupling than expected.

The current execution posture for task sizing is:

- keep the Phase 4.5 ledger at the current 16-task scale
- split only when a task stops being one coherent implementation unit
- preserve capability-based task names if a split becomes necessary

The most likely split candidates are:

- `P45-03` if the management service layer naturally separates into read-only
  status services and mutating background-control services
- `P45-04` if schema inspection and config-readiness inspection prove to need
  independent service layers
- `P45-10` if background list summaries and job-run detail views diverge
  materially
- `P45-15` if runtime CLI tests and PostgreSQL-backed management suites need
  separate stabilization passes

These names are intentionally capability-based and aligned with the existing
`foundation_*`, `foreground_*`, `continuity_*`, and `unconscious_*` suite
pattern.

## Expected Phase 4.5 verification commands

These are the intended recurring verification commands for this phase. Some
will become available only after earlier tasks are complete.

- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo test -p runtime --test admin_cli -- --nocapture`
- `cargo test -p harness --test management_component -- --nocapture`
- `cargo test -p harness --test management_integration -- --nocapture`
- `cargo test -p harness --test foreground_component -- --nocapture`
- `cargo test -p harness --test continuity_component -- --nocapture`
- `cargo test -p harness --test unconscious_component -- --nocapture`
- `cargo run -p runtime -- admin status`
- `cargo run -p runtime -- admin status --json`
- `cargo run -p runtime -- admin foreground pending`
- `cargo run -p runtime -- admin background list`
- `cargo run -p runtime -- admin background enqueue ...`
- `cargo run -p runtime -- admin background run-next`
- `cargo run -p runtime -- admin wake-signals list`
- manual review that admin commands render stable text output and structured
  `--json` output without leaking secret values
- manual review that background enqueue and run-next preserve the documented
  harness-owned planning, audit, and bounded-execution posture

## Phase 4.5 milestones

- Milestone A: scope lock, surface architecture, and management service baseline
- Milestone B: read-only inspection commands
- Milestone C: safe background control commands
- Milestone D: tests, CI, docs, and completion gate

## Milestone A quality gate

Milestone A is green only if:

- the Phase 4.5 scope boundary and deferred behavior are explicit
- the command namespace, output posture, and harness-versus-runtime boundaries
  are settled
- the new harness management service module and test/CI posture are defined
  before heavier command implementation starts
- the schema-readiness inspection posture is defined in a way that does not
  weaken fail-closed normal runtime execution
- the Phase 4.5 surface preserves later extension paths for approvals,
  workspace inspection, recovery, and richer observability

## Milestone B quality gate

Milestone B is green only if:

- the runtime can expose a coherent read-only management surface for status,
  pending foreground work, background jobs, and wake signals
- those commands rely on harness services and read models rather than ad hoc
  runtime-side SQL
- the status surface can represent degraded or missing readiness without
  leaking secrets or collapsing into `Debug` output
- the first operator-facing text and `--json` output shapes are stable enough
  to test

## Milestone C quality gate

Milestone C is green only if:

- the CLI can safely enqueue a background job through the existing planning and
  validation path
- the CLI can explicitly execute one due background job without introducing new
  scheduling authority or altered execution semantics
- mutating commands emit or reuse the required audit trail and do not bypass
  Phase 3 or Phase 4 safety boundaries
- the management surface demonstrates a closed operator flow for the raw-SQL
  background verification cases that motivated Phase 4.5

## Milestone D quality gate

Milestone D is green only if:

- required unit coverage exists for parsing, output formatting, schema-status
  evaluation, and management service logic
- PostgreSQL-backed component coverage exists for management queries and
  mutating operator flows where persistence semantics matter
- architecture-critical integration coverage exists for runtime admin routing
  and explicit background control flows
- repository CI runs the required Phase 4.5 suites under stable
  capability-based gate names
- canonical docs and operator-facing docs reflect the implemented Phase 4.5
  command surface and verification commands
- this document reflects the final task status and evidence

## Task list

### Task P45-01: Lock the Phase 4.5 management CLI slice boundary

- Status: `DONE`
- Depends on: none
- Parallel-safe: no
- Deliverables:
  - explicit Phase 4.5 scope boundary covering what is in and out
  - documented clarification that the management CLI is a durable product
    interface rather than a temporary test shell
  - documented clarification of the initial command set and explicit deferrals
    for `telegram status` and `verify summary`
  - documented clarification of the `runtime admin` namespace posture
- Verification:
  - manual review against `docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md`,
    `docs/IMPLEMENTATION_DESIGN.md`, `docs/REQUIREMENTS.md`, and
    `docs/LOOP_ARCHITECTURE.md`
- Evidence:
  - confirmed and implemented through the settled scope and deferral language
    already recorded in this document plus the aligned forward-management
    wording in `docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md`

### Task P45-02: Define the runtime CLI architecture and output posture

- Status: `DONE`
- Depends on: `P45-01`
- Parallel-safe: no
- Deliverables:
  - settled internal runtime CLI structure that keeps `main.rs` thin as the
    command tree grows
  - explicit command and option model for the initial `runtime admin ...`
    surface
  - explicit text-output and `--json` posture for the initial command set
  - explicit rule that data goes to stdout and failures go to stderr
- Verification:
  - code review that the proposed file and module structure keeps runtime thin
  - `cargo check --workspace`
- Evidence:
  - added `crates/runtime/src/admin.rs` and updated
    `crates/runtime/src/main.rs` so the runtime binary now exposes a structured
    `admin` namespace with explicit text and `--json` output posture
  - verification: `cargo check --workspace`

### Task P45-03: Add a dedicated harness-side management service layer

- Status: `DONE`
- Depends on: `P45-02`
- Parallel-safe: no
- Deliverables:
  - a new `harness` management module with read-model types and service entry
    points for the Phase 4.5 command set
  - public harness APIs that the runtime CLI can call without embedding ad hoc
    SQL or cross-module orchestration in CLI handlers
  - management result types designed for both text rendering and `--json`
    serialization
- Verification:
  - `cargo check --workspace`
  - unit tests for management read-model encoding or validation as applicable
- Evidence:
  - added `crates/harness/src/management.rs` and exported it from
    `crates/harness/src/lib.rs`
  - verification: `cargo check --workspace`

### Task P45-04: Implement runtime readiness and schema inspection services

- Status: `DONE`
- Depends on: `P45-03`
- Parallel-safe: no
- Deliverables:
  - read-only schema inspection that can report compatible, missing, pending,
    too-old, or too-new schema states without weakening normal runtime
    fail-closed behavior
  - management readiness inspection for worker resolution, Telegram binding
    presence, model-route summary, and required secret presence by name without
    printing secret values
  - high-level pending-work summary counts needed by `runtime admin status`
- Verification:
  - unit tests for schema-status mapping and readiness summaries
  - `cargo test -p harness --test management_component -- --nocapture`
- Evidence:
  - implemented runtime status, schema compatibility inspection, worker
    resolution reporting, optional subsystem readiness, and pending-work
    summary logic in `crates/harness/src/management.rs`
  - verification: `cargo check --workspace`,
    `cargo test --workspace --lib -- --nocapture`

### Task P45-05: Implement foreground pending-work inspection services

- Status: `DONE`
- Depends on: `P45-03`
- Parallel-safe: yes
- Deliverables:
  - management queries for pending foreground work grouped by internal
    conversation
  - visibility into whether a conversation currently qualifies for backlog
    recovery or single-ingress handling
  - read models suitable for both text and `--json` output
- Verification:
  - `cargo test -p harness --test management_component -- --nocapture`
- Evidence:
  - implemented pending foreground conversation summaries and backlog-recovery
    classification in `crates/harness/src/management.rs`
  - verification: `cargo check --workspace`,
    `cargo test --workspace --lib -- --nocapture`

### Task P45-06: Implement background job and job-run inspection services

- Status: `DONE`
- Depends on: `P45-03`
- Parallel-safe: yes
- Deliverables:
  - management queries for recent background jobs with status, trigger kind,
    availability time, and latest run summary
  - any new persistence helpers required to inspect jobs beyond the existing
    due-only and by-id APIs
  - read models suitable for both text and `--json` output
- Verification:
  - `cargo test -p harness --test management_component -- --nocapture`
- Evidence:
  - implemented recent background job inspection with latest-run summaries in
    `crates/harness/src/management.rs`
  - verification: `cargo check --workspace`

### Task P45-07: Implement wake-signal inspection services

- Status: `DONE`
- Depends on: `P45-03`
- Parallel-safe: yes
- Deliverables:
  - management queries for recent wake signals across relevant statuses rather
    than only pending-review state
  - read models that expose reason code, priority, decision state, and review
    timing without exposing raw storage internals as the primary interface
  - any new persistence helpers needed to support recent-signal listing and
    filtering semantics
- Verification:
  - `cargo test -p harness --test management_component -- --nocapture`
- Evidence:
  - implemented recent wake-signal inspection in
    `crates/harness/src/management.rs`
  - verification: `cargo check --workspace`

### Task P45-08: Implement `runtime admin status`

- Status: `DONE`
- Depends on: `P45-04`
- Parallel-safe: no
- Deliverables:
  - `runtime admin status` command handler and formatter
  - stable text output for runtime readiness and pending-work summary
  - `--json` output for the same status shape
- Verification:
  - `cargo test -p runtime --test admin_cli -- --nocapture`
  - `cargo run -p runtime -- admin status`
  - `cargo run -p runtime -- admin status --json`
- Evidence:
  - implemented `runtime admin status` in `crates/runtime/src/admin.rs`
  - verification: `cargo run -p runtime -- admin --help`

### Task P45-09: Implement `runtime admin foreground pending`

- Status: `DONE`
- Depends on: `P45-05`
- Parallel-safe: yes
- Deliverables:
  - `runtime admin foreground pending` command handler and formatter
  - stable text output for pending and recoverable foreground conversations
  - `--json` output for the same inspection shape
- Verification:
  - `cargo test -p runtime --test admin_cli -- --nocapture`
  - `cargo run -p runtime -- admin foreground pending`
- Evidence:
  - implemented `runtime admin foreground pending` in
    `crates/runtime/src/admin.rs`
  - verification: `cargo check --workspace`

### Task P45-10: Implement `runtime admin background list`

- Status: `DONE`
- Depends on: `P45-06`
- Parallel-safe: yes
- Deliverables:
  - `runtime admin background list` command handler and formatter
  - stable text output for background jobs with latest run visibility
  - `--json` output for the same inspection shape
- Verification:
  - `cargo test -p runtime --test admin_cli -- --nocapture`
  - `cargo run -p runtime -- admin background list`
- Evidence:
  - implemented `runtime admin background list` in
    `crates/runtime/src/admin.rs`
  - verification: `cargo check --workspace`

### Task P45-11: Implement safe `runtime admin background enqueue`

- Status: `DONE`
- Depends on: `P45-03`
- Parallel-safe: no
- Deliverables:
  - command handler for explicit background enqueue with typed operator-facing
    arguments
  - routing through `background_planning::plan_background_job` or an equivalent
    harness-owned management entrypoint rather than direct SQL writes
  - stable text and `--json` result reporting covering planned, duplicate, and
    rejected outcomes
- Verification:
  - `cargo test -p runtime --test admin_cli -- --nocapture`
  - `cargo test -p harness --test management_integration -- --nocapture`
  - `cargo run -p runtime -- admin background enqueue ...`
- Evidence:
  - implemented typed enqueue routing through the harness-owned background
    planning path in `crates/harness/src/management.rs` and
    `crates/runtime/src/admin.rs`
  - verification: `cargo test -p runtime --test admin_cli admin_background_enqueue_help_lists_operator_arguments -- --nocapture`

### Task P45-12: Implement `runtime admin background run-next`

- Status: `DONE`
- Depends on: `P45-11`
- Parallel-safe: no
- Deliverables:
  - command handler for explicit one-shot due-job execution
  - focused operator result reporting over the existing background one-shot
    harness path
  - stable text and `--json` output without introducing new scheduler semantics
- Verification:
  - `cargo test -p runtime --test admin_cli -- --nocapture`
  - `cargo test -p harness --test management_integration -- --nocapture`
  - `cargo run -p runtime -- admin background run-next`
- Evidence:
  - implemented focused one-shot due-job execution in
    `crates/harness/src/management.rs` and `crates/runtime/src/admin.rs`
  - verification: `cargo check --workspace`

### Task P45-13: Implement `runtime admin wake-signals list`

- Status: `DONE`
- Depends on: `P45-07`
- Parallel-safe: yes
- Deliverables:
  - `runtime admin wake-signals list` command handler and formatter
  - stable text output for pending, deferred, accepted, rejected, or suppressed
    wake-signal summaries as implemented
  - `--json` output for the same inspection shape
- Verification:
  - `cargo test -p runtime --test admin_cli -- --nocapture`
  - `cargo run -p runtime -- admin wake-signals list`
- Evidence:
  - implemented `runtime admin wake-signals list` in
    `crates/runtime/src/admin.rs`
  - verification: `cargo check --workspace`

### Task P45-14: Add unit coverage for parsing, status evaluation, and formatters

- Status: `DONE`
- Depends on: `P45-08`, `P45-09`, `P45-10`, `P45-11`, `P45-12`, `P45-13`
- Parallel-safe: no
- Deliverables:
  - runtime CLI parsing tests for the initial admin command tree
  - harness or runtime unit tests for output-model validation and formatter
    behavior
  - unit tests for schema-readiness inspection and any command-specific
    validation logic
- Verification:
  - `cargo test -p runtime --test admin_cli -- --nocapture`
  - `cargo test --workspace --lib -- --nocapture`
- Evidence:
  - added runtime CLI surface tests in `crates/runtime/tests/admin_cli.rs`
  - added management unit coverage in `crates/harness/src/management.rs`
  - verification: `cargo test --workspace --lib -- --nocapture`,
    `cargo test -p runtime --test admin_cli admin_help_lists_management_subcommands -- --nocapture`,
    `cargo test -p runtime --test admin_cli admin_background_enqueue_help_lists_operator_arguments -- --nocapture`

### Task P45-15: Add PostgreSQL-backed component and integration coverage for management flows

- Status: `BLOCKED`
- Depends on: `P45-11`, `P45-12`, `P45-13`, `P45-14`
- Parallel-safe: no
- Deliverables:
  - `management_component` coverage for persistence-backed read models and
    management-service semantics
  - `management_integration` coverage for explicit enqueue and run-next flows
  - regression coverage proving management commands do not bypass planning,
    audit, or bounded execution rules
- Verification:
  - `cargo test -p harness --test management_component -- --nocapture`
  - `cargo test -p harness --test management_integration -- --nocapture`
- Evidence:
  - added `crates/harness/tests/management_component.rs` and
    `crates/harness/tests/management_integration.rs`
  - local execution attempted with:
    `cargo test -p harness --test management_component -- --nocapture`,
    `cargo test -p harness --test management_integration -- --nocapture`
  - blocked in this environment because test support could not access Docker to
    start PostgreSQL (`permission denied while trying to connect to the docker
    API`) and the runtime DB-backed CLI test could not connect to a local
    PostgreSQL admin port without that service

### Task P45-16: Extend repository CI and operator docs for the Phase 4.5 gate

- Status: `BLOCKED`
- Depends on: `P45-15`
- Parallel-safe: no
- Deliverables:
  - repository-hosted CI updates for the `management-cli` gate
  - command-surface documentation updates in the appropriate operator-facing
    docs
  - cleanup or replacement of any stale verification guidance that the new
    management CLI supersedes
  - cleanup or archival of `docs/wip/phase-4-5-cli-design-note.md` once its
    remaining value has been fully folded into canonical planning or operator
    docs
  - final Phase 4.5 plan status updates and evidence completion in this
    document
- Verification:
  - manual review of `.github/workflows/ci.yml`
  - `cargo test --workspace`
  - manual review that updated operator docs match the implemented command
    surface
- Evidence:
  - added the `management-cli` job to `.github/workflows/ci.yml`
  - updated operator-facing workflow guidance in `AGENTS.md`
  - updated repository command guidance in `README.md`
  - retired the temporary pre-plan note at
    `docs/wip/phase-4-5-cli-design-note.md` after folding its decisions into
    this execution ledger
  - remaining completion condition depends on `P45-15` running successfully in
    CI or another Docker-enabled environment
