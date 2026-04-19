# Blue Lagoon

## Phase 4 Detailed Implementation Plan

Date: 2026-04-19
Status: Planned
Scope: High-level plan Phase 4 only
Audience: LLM-assisted implementation work and human review

## Purpose

This document defines the detailed implementation plan for Phase 4 of Blue
Lagoon.

It translates the approved Phase 4 scope from
`docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md` into concrete, trackable, and
LLM-executable work items.

Phase 4 introduces the second execution domain behind the completed Phase 3
foreground continuity slice. Its purpose is to make bounded background
maintenance real through harness-owned scheduling, scoped unconscious workers,
proposal-based maintenance outputs, and policy-gated wake-signal handling.

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

If this document conflicts with the canonical documents, the canonical
documents win.

## Documentation readiness review

The Phase 4 planning baseline is ready.

The current canonical documents agree on the core Phase 4 intent:

- the unconscious loop is a harness-managed background maintenance domain, not
  an autonomous second control plane
- unconscious workers remain isolated, bounded, and structured-output-only
- background maintenance must reuse harness-owned proposal and merge boundaries
  rather than introducing direct worker mutation paths
- wake signals are typed maintenance outputs evaluated by harness policy rather
  than direct wake-ups
- the first scheduler and maintenance slice should remain intentionally narrow
  and should not pull in broader Phase 5 tooling or Phase 6 recovery scope

No blocking contradiction was found between
`docs/IMPLEMENTATION_DESIGN.md`,
`docs/LOOP_ARCHITECTURE.md`,
`docs/REQUIREMENTS.md`, and
`docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md`.

The required execution clarifications for Phase 4 are recorded in the next
sections so implementation stays tied to the dual-loop design decisions that
govern the unconscious path.

## CI assessment for Phase 4

The current repository-hosted CI posture is strong enough for the completed
foreground and canonical-persistence surface, but it does not yet cover the
new scheduler, unconscious-worker, or wake-signal risk surface introduced in
Phase 4.

The current stable jobs remain useful:

- `workspace-verification`
- `foreground-runtime`
- `canonical-persistence`

Phase 4 should preserve those stable gate names and add one additional
capability-based gate rather than expanding existing jobs into another omnibus
pipeline.

The Phase 4 CI posture is locked as follows:

- Keep `workspace-verification` as the fast repository-wide baseline gate.
- Keep `foreground-runtime` focused on user-facing Telegram-first foreground
  regressions.
- Keep `canonical-persistence` focused on migration-sensitive and
  canonical-write-sensitive persistence regressions.
- Add `background-maintenance` for scheduler, unconscious-worker, bounded
  maintenance, and wake-signal regression coverage.

The intended Phase 4 gate-to-suite mapping is:

- `workspace-verification`
  Run formatting, compile checks, clippy, and fast unit-focused verification
  that does not require PostgreSQL.
- `foreground-runtime`
  Continue running `foreground_component` and `foreground_integration`.
- `canonical-persistence`
  Continue running `continuity_component` and `continuity_integration`, plus
  any Phase 4 persistence-sensitive cases that directly protect canonical
  proposal and merge behavior.
- `background-maintenance`
  Run the new PostgreSQL-backed unconscious-loop suites, beginning with
  `unconscious_component` and `unconscious_integration`, and any targeted
  scheduler or wake-signal regression suites that prove bounded background
  execution.

Phase 4 CI expansion should avoid duplicating the same expensive suites across
multiple jobs unless a stricter later-stage gate intentionally reuses them.

## Implementation starting point

The current repository already contains the main boundaries that Phase 4 should
extend rather than replace.

The default implementation starting points are:

- `crates/contracts/src/lib.rs`
  for shared loop, proposal, model, and worker-facing contracts that can grow
  to represent unconscious-job requests, results, and wake signals
- `crates/harness/src/config.rs`
  for typed runtime config and fail-closed validation
- `crates/harness/src/runtime.rs`
  for thin harness entrypoints and one-shot runtime execution wiring
- `crates/harness/src/execution.rs`
  for durable execution record handling that Phase 4 should reuse where
  possible for cross-loop traceability
- `crates/harness/src/worker.rs`
  for isolated subprocess execution, timeout enforcement, and worker lifecycle
  boundaries
- `crates/harness/src/policy.rs`
  for budgets, trigger validation, and the future wake-signal evaluation path
- `crates/harness/src/audit.rs`
  for durable audit writing and trace-linked event history
- `crates/harness/src/continuity.rs`
  for canonical proposal, merge, memory, retrieval, and self-model persistence
  services introduced in Phase 3
- `crates/harness/src/proposal.rs`
  for harness-owned proposal validation and merge-decision recording
- `crates/harness/src/memory.rs`
  for long-term memory merge rules and artifact persistence
- `crates/harness/src/retrieval.rs`
  for retrieval maintenance and retrieval-artifact handling
- `crates/harness/src/self_model.rs`
  for canonical self-model reads and writes
- `crates/workers/src/main.rs`
  for worker protocol changes and the first unconscious-worker runtime mode
- `crates/harness/tests/support/mod.rs`
  for disposable PostgreSQL test support and migration-backed test setup
- `config/default.toml`
  for repository-safe scheduler, budget, and wake-signal defaults
- `migrations/0004__canonical_continuity.sql`
  as the last reviewed migration before the Phase 4 background-maintenance
  migration

## Phase 4 target

Phase 4 is complete only when Blue Lagoon proves the following:

- the harness can schedule and execute bounded unconscious jobs from approved
  maintenance triggers without giving scheduling authority to workers
- unconscious workers can return structured maintenance outputs only, including
  memory deltas, retrieval updates, self-model deltas, diagnostics, and
  optional wake signals
- accepted unconscious outputs flow through the existing harness-owned
  proposal, merge, and canonical-write path rather than bypassing Phase 3
  boundaries
- the system can demonstrate a closed background-maintenance loop with scoped
  inputs, explicit budgets, durable audit history, and bounded worker
  termination
- wake signals are durably recorded and evaluated by harness policy before any
  foreground conversion occurs
- required Phase 4 unit, component, and integration tests pass
- repository CI runs the required unconscious-loop regression suites under
  stable capability-based gate identities

## Settled implementation clarifications

The following Phase 4 decisions are treated as settled for execution unless
later canonical documents intentionally change them:

- Phase 4 remains a maintenance-centered phase. It introduces the first
  unconscious scheduler and worker path, but it does not yet introduce Phase 5
  tool execution, approvals, or workspace mutation.
- The unconscious loop remains entirely harness-owned. Workers may emit
  structured maintenance outputs, but they must not select their own scope,
  own their own retries, or directly mutate canonical state.
- Phase 4 should reuse the Phase 3 proposal and merge pipeline instead of
  creating a parallel background-specific canonical-write mechanism.
- All canonical unconscious trigger kinds should be represented in contracts
  and harness policy so the Phase 4 scheduler does not silently narrow the
  allowed trigger model defined by the canonical documents.
- The first trigger paths that must be fully exercised end to end in Phase 4
  are:
  - time-based schedule
  - volume or backlog threshold
  - foreground delegation
  - maintenance trigger
- Drift or anomaly signals and external passive events may land first as
  recognized contract or policy inputs with explicit fail-closed or deferred
  handling if fuller production trigger plumbing is not yet required to land
  one coherent Phase 4 slice.
- The first supported unconscious job kinds should remain narrow and
  capability-based:
  - `memory_consolidation`
  - `retrieval_maintenance`
  - `contradiction_and_drift_scan`
  - `self_model_reflection`
- Phase 4 should reuse `execution_records` and `audit_events` where possible
  for cross-loop traceability instead of creating duplicate execution-history
  stores.
- Phase 4 may add the minimum scheduler-specific canonical tables needed for
  due-job selection, job-run state, and wake-signal persistence. The default
  table names to plan around are:
  - `background_jobs`
  - `background_job_runs`
  - `wake_signals`
- If implementation reveals a materially better domain-specific name, that
  change should be made deliberately and consistently before code lands rather
  than ad hoc during individual task execution.
- Wake signals are policy inputs, not autonomous wake-ups. They must be
  persisted, evaluated, throttled or dropped if required, and converted into
  foreground triggers only by harness logic.
- Background maintenance should stay relational-first and JSONB-minimal.
  Job-run progress and scoped-input payloads may use tightly bounded JSONB
  where that is materially clearer than proliferating one-off columns.
- Phase 4 does not yet need the full generalized recovery-checkpoint model from
  Phase 6. The minimum required durability is enough job state and audit
  history to support bounded retries, fail-closed crash handling, and explicit
  wake-signal evaluation.
- The stable capability-based gate name for Phase 4 should be
  `background-maintenance`.

## Phase 4 scope boundary

### In scope for Phase 4

- harness-owned scheduling, scoping, and budget assignment for unconscious jobs
- typed config for scheduler cadence, thresholds, background budgets, and
  wake-signal policy
- reviewed SQL migration for the minimum durable background job and wake-signal
  state
- shared contracts for unconscious-job requests, outputs, diagnostics, and
  wake signals
- the first isolated unconscious-worker runtime mode in `workers`
- the first background maintenance jobs for memory consolidation, retrieval
  maintenance, contradiction and drift scanning, and self-model reflection
- reuse of Phase 3 proposal and merge services for accepted unconscious outputs
- durable audit history for job scheduling, execution, proposal handling, and
  wake-signal evaluation
- wake-signal persistence, policy evaluation, and guarded wake-to-foreground
  conversion
- automated unit, component, and integration coverage for scheduler and
  unconscious-loop behavior
- repository CI expansion for the unconscious-loop regression surface

### Explicitly out of scope for Phase 4

- direct user-facing proactive messaging beyond the narrow harness-owned
  wake-signal conversion path
- side-effecting tool execution, workspace mutation, or approval handling from
  unconscious jobs
- distributed scheduling, external message brokers, or multi-node background
  worker pools
- broad external passive-event ingestion pipelines
- a generalized Phase 6 checkpoint, continuation, and recovery-budget model
- richer retrieval infrastructure such as vector databases, embedding rebuild
  fleets, or graph backfills
- admin UIs, dashboards, or operator control planes
- multi-user or multi-tenant scheduler behavior

### Deferred by later phases

- Phase 5: governed tool execution, workspace, and approval model
- Phase 6: generalized recovery, retry-budget policy, and broader operational
  hardening for interrupted background work
- later proactive behavior beyond the narrow approved wake-signal path
- richer anomaly-trigger plumbing, external passive-event ingestion, and
  heavier retrieval infrastructure

### Execution posture confirmed for Phase 4

- implement the second execution domain behind the existing harness-centered
  control model rather than as a new subsystem with parallel authority
- keep scheduler and unconscious-worker scope intentionally narrow in the first
  landing
- prefer additive schema and reuse of Phase 3 canonical write posture
- keep new code in `harness` and `workers` before considering additional
  top-level crates
- prefer the lowest effective test layer, but use disposable real PostgreSQL
  for persistence-critical scheduler and wake-signal semantics

## LLM execution rules

The plan should be executed under the following rules:

- Work one task at a time unless a task is explicitly marked as parallel-safe.
- Do not start a task until all of its dependencies are marked `DONE`.
- No core Phase 4 task is complete without the verification listed for it.
- Keep background scheduling harness-owned. If scheduling authority starts
  drifting into workers, stop and split the work first.
- Keep unconscious outputs structured-only and proposal-based. If a task begins
  producing direct user-facing output or direct side effects, stop and narrow
  scope first.
- Prefer the lowest effective test layer.
- Use disposable real PostgreSQL for persistence-critical verification.
- Update this document immediately after finishing each task.

## Progress tracking protocol

This document becomes the progress ledger for Phase 4 once implementation
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

- Current milestone: `Milestone B`
- Current active task: `P4-09`
- Completed tasks: `8/18`
- Milestone A status: `DONE`
- Milestone B status: `IN PROGRESS`
- Milestone C status: `TODO`
- Milestone D status: `TODO`

Repository sequencing note:

- Phase 3 is complete, and this document now serves as the planned Phase 4
  execution ledger for the next implementation cycle.

## Execution refinement notes

The current task count and scope are appropriate for starting implementation,
but several tasks are integration-heavy enough that they should be treated as
likely split points if the code reveals more coupling than expected.

The current execution posture for task sizing is:

- keep the Phase 4 ledger at the current 18-task scale
- split only when a task stops being one coherent implementation unit
- preserve capability-based task names if a split becomes necessary

The most likely split candidates are:

- `P4-04` if scheduler state persistence and wake-signal persistence stop
  fitting one coherent storage pass
- `P4-08` if due-job selection, leasing, and bounded execution coordination
  evolve at different speeds
- `P4-09` through `P4-12` if one maintenance job type proves materially more
  complex than the others and needs its own milestone slice
- `P4-13` if wake-signal persistence and policy-gated conversion need to land
  separately to keep regressions controlled

The default new artifact names for Phase 4 implementation are:

- reviewed migration file: `migrations/0005__unconscious_loop.sql`
- PostgreSQL-backed harness component suite: `unconscious_component`
- architecture-critical harness integration suite: `unconscious_integration`
- repository CI gate: `background-maintenance`

These names are intentionally capability-based and aligned with the existing
`foundation_*`, `foreground_*`, and `continuity_*` suite pattern.

## Expected Phase 4 verification commands

These are the intended recurring verification commands for this phase. Some
will become available only after earlier tasks are complete.

- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo test -p harness --test foreground_component -- --nocapture`
- `cargo test -p harness --test foreground_integration -- --nocapture`
- `cargo test -p harness --test continuity_component -- --nocapture`
- `cargo test -p harness --test continuity_integration -- --nocapture`
- `cargo test -p harness --test unconscious_component -- --nocapture`
- `cargo test -p harness --test unconscious_integration -- --nocapture`
- `cargo run -p runtime -- migrate`
- `cargo run -p runtime -- harness --once --idle`
- manual review that the one-shot harness runtime can select and execute one due
  unconscious job once the Phase 4 runtime path is implemented
- manual review that wake-signal persistence, policy evaluation, and
  foreground-conversion behavior match the documented domain language

## Phase 4 milestones

- Milestone A: scheduler baseline, schema, contracts, and scope lock
- Milestone B: bounded unconscious execution path
- Milestone C: first maintenance jobs and wake-signal conversion
- Milestone D: tests, CI, docs, and completion gate

## Milestone A quality gate

Milestone A is green only if:

- the Phase 4 scope boundary and deferred behavior are explicit
- runtime config can represent background budgets, scheduling thresholds, and
  wake-signal policy posture
- reviewed SQL migrations exist for the minimum durable background scheduling
  and wake-signal state
- the minimum cross-process and harness contracts exist for unconscious-job
  requests, outputs, diagnostics, and wake signals
- the Phase 4 CI expansion and stable capability-based gate name are defined
  before heavier scheduler and maintenance suites are added
- the schema and contract design preserve Phase 5 and Phase 6 expansion paths
  without requiring Phase 4 rework

## Milestone B quality gate

Milestone B is green only if:

- the harness can persist, select, scope, and launch due unconscious jobs
  without giving scheduling authority to workers
- unconscious workers can run in isolated bounded mode and return structured
  outputs only
- accepted background outputs can enter the existing proposal and merge flow
  without bypassing Phase 3 boundaries
- bounded termination, timeout handling, and fail-closed worker-failure
  handling exist for the unconscious path

## Milestone C quality gate

Milestone C is green only if:

- the first maintenance job kinds can run end to end with explicit scoped
  inputs and bounded budgets
- at least one time-based or threshold-based trigger can produce a due
  unconscious job through the scheduler
- wake signals are durably recorded and policy-gated before any foreground
  conversion occurs
- the background loop demonstrates a closed harness-owned maintenance cycle
  with trace-linked audit history

## Milestone D quality gate

Milestone D is green only if:

- required unit coverage exists for scheduling, scoping, budgets, wake-signal
  evaluation, and worker-protocol validation
- PostgreSQL-backed component coverage exists for background scheduling state,
  maintenance outputs, and wake-signal persistence
- architecture-critical integration coverage exists for unconscious job to
  proposal to merge flow and wake-signal to foreground-trigger flow
- repository CI runs the required Phase 4 suites under stable capability-based
  gate names
- canonical docs reflect the implemented Phase 4 boundaries and verification
  commands
- this document reflects the final task status and evidence

## Task list

### Task P4-01: Lock the Phase 4 unconscious-loop slice boundary

- Status: `DONE`
- Depends on: none
- Parallel-safe: no
- Deliverables:
  - explicit Phase 4 scope boundary covering what is in and out
  - documented clarification of the first supported trigger paths and job kinds
  - documented clarification of wake-signal posture versus direct proactive
    behavior
  - documented clarification of Phase 4 scheduler durability versus Phase 6
    recovery deferrals
- Verification:
  - manual review against `docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md`,
    `docs/IMPLEMENTATION_DESIGN.md`, `docs/REQUIREMENTS.md`, and
    `docs/LOOP_ARCHITECTURE.md`
- Evidence:
  - confirmed the settled Phase 4 scope, trigger posture, wake-signal posture,
    and Phase 6 recovery deferrals already recorded in this document against
    `docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md`,
    `docs/IMPLEMENTATION_DESIGN.md`, `docs/REQUIREMENTS.md`, and
    `docs/LOOP_ARCHITECTURE.md` on 2026-04-19 before implementation started

### Task P4-02: Extend runtime config for background scheduling and wake policy

- Status: `DONE`
- Depends on: `P4-01`
- Parallel-safe: no
- Deliverables:
  - typed config for background job budgets and default worker timeouts
  - typed config for schedule cadence and threshold-trigger settings
  - typed config for wake-signal evaluation and throttle posture
  - fail-closed validation for required new Phase 4 settings
  - operator-safe defaults in repository config
- Verification:
  - unit tests for config parsing and validation
  - failed startup on invalid Phase 4 settings
- Evidence:
  - updated `crates/harness/src/config.rs`,
    `crates/harness/src/policy.rs`,
    `crates/harness/src/self_model.rs`,
    `crates/harness/tests/support/mod.rs`, and `config/default.toml`
  - `cargo fmt --all --check`
  - `cargo test -p harness --lib -- --nocapture`
  - `cargo check --workspace`

### Task P4-03: Add reviewed SQL migration for background scheduling state

- Status: `DONE`
- Depends on: `P4-01`
- Parallel-safe: no
- Deliverables:
  - reviewed migration file `migrations/0005__unconscious_loop.sql`
  - canonical tables for background jobs, background job runs, and wake signals
  - indexes and constraints for due-job lookup, lease-safe active-run lookup,
    trace linkage, trigger metadata, and wake-signal evaluation history
  - any required schema refinements needed to link Phase 4 job outcomes back to
    Phase 3 canonical proposals and merges
- Verification:
  - migration discovery and ordering behave correctly
  - migration applies cleanly to disposable PostgreSQL
- Execution note:
  - reuse `execution_records` and `audit_events` where possible rather than
    introducing duplicate orchestration history stores
- Evidence:
  - added `migrations/0005__unconscious_loop.sql`
  - updated `crates/harness/tests/foundation_component.rs` and
    `crates/harness/tests/unconscious_component.rs`
  - `cargo fmt --all --check`
  - `cargo test -p harness --test foundation_component -- --nocapture`
  - `cargo test -p harness --test unconscious_component -- --nocapture`
  - `cargo check --workspace`

### Task P4-04: Implement harness persistence services for scheduler and wake state

- Status: `DONE`
- Depends on: `P4-03`
- Parallel-safe: no
- Deliverables:
  - harness persistence services for background jobs, job runs, and wake
    signals
  - typed read and write models aligned with the new Phase 4 tables
  - repository-facing query paths for due-job selection, active-run lookup,
    completed-run history, and wake-signal review
- Verification:
  - component tests against disposable PostgreSQL for each new persistence
    service
- Execution note:
  - likely split point if scheduler persistence and wake-signal persistence stop
    fitting one coherent storage pass
- Evidence:
  - added `crates/harness/src/background.rs`
  - updated `crates/harness/src/lib.rs` and
    `crates/harness/tests/unconscious_component.rs`
  - `cargo fmt --all --check`
  - `cargo test -p harness --test unconscious_component -- --nocapture`
  - `cargo check --workspace`

### Task P4-05: Define canonical Phase 4 contracts

- Status: `DONE`
- Depends on: `P4-01`
- Parallel-safe: yes
- Deliverables:
  - shared contracts for unconscious-job requests and scoped inputs
  - shared contracts for structured maintenance outputs, diagnostics, and
    wake signals
  - shared contracts for all canonical background trigger kinds and job kinds
  - shared contracts for harness-evaluated wake-signal outcomes
- Verification:
  - contract round-trip tests in `contracts`
  - manual review that contract names are capability-based rather than
    phase-labeled
- Evidence:
  - updated `crates/contracts/src/lib.rs`
  - `cargo fmt --all --check`
  - `cargo test -p contracts -- --nocapture`
  - `cargo check --workspace`

### Task P4-06: Extend the worker protocol for unconscious execution

- Status: `DONE`
- Depends on: `P4-05`
- Parallel-safe: no
- Deliverables:
  - worker request and result shapes that can represent unconscious jobs in
    addition to existing conscious flows
  - worker-side validation that rejects malformed Phase 4 structured outputs
  - initial unconscious-worker runtime mode in `workers`
  - explicit bounded output posture for diagnostics and optional wake signals
- Verification:
  - worker protocol tests
  - fakeable unconscious-worker round-trip tests
- Evidence:
  - updated `crates/workers/src/main.rs`,
    `crates/workers/tests/conscious_worker_cli.rs`,
    `crates/workers/tests/smoke_worker_cli.rs`, and
    `crates/workers/tests/unconscious_worker_cli.rs`
  - `cargo fmt --all --check`
  - `cargo test -p workers -- --nocapture`
  - `cargo check --workspace`

### Task P4-07: Implement harness-side background trigger validation and job planning

- Status: `DONE`
- Depends on: `P4-02`, `P4-05`
- Parallel-safe: no
- Deliverables:
  - trigger validation rules for all canonical unconscious trigger kinds
  - full planning and due-job creation for schedule, threshold, delegation, and
    maintenance-trigger paths
  - explicit recognized or fail-closed handling for drift or anomaly signals
    and external passive events until fuller trigger plumbing is required
  - background job planning and deduplication rules
  - explicit budget assignment and scoped-input planning for due jobs
  - trace-linked audit events for job creation, suppression, or rejection
- Verification:
  - unit tests for trigger validation, deduplication, and budget planning
  - component tests proving planned jobs are durably recorded
- Evidence:
  - added `crates/harness/src/background_planning.rs`
  - updated `crates/harness/src/background.rs`,
    `crates/harness/src/policy.rs`,
    `crates/harness/src/migration.rs`,
    `crates/harness/src/lib.rs`, and
    `crates/harness/tests/unconscious_component.rs`
  - `cargo fmt --all --check`
  - `cargo test -p harness --lib -- --nocapture`
  - `cargo test -p harness --test unconscious_component -- --nocapture`
  - `cargo check --workspace`

### Task P4-08: Implement due-job selection, leasing, and bounded execution coordination

- Status: `DONE`
- Depends on: `P4-04`, `P4-06`, `P4-07`
- Parallel-safe: no
- Deliverables:
  - due-job selection and active-run lease handling
  - harness-owned unconscious-worker launch path with bounded termination
  - fail-closed handling for worker crash, timeout, or malformed result payloads
  - trace-linked audit history for job start, completion, timeout, and failure
- Verification:
  - unit tests for selection and lease logic
  - component tests with real PostgreSQL for active-run coordination
  - targeted failure-path tests for crash and timeout handling
- Execution note:
  - likely split point if due-job selection and worker coordination evolve at
    different speeds
- Evidence:
  - added `crates/harness/src/background_execution.rs`
  - updated `crates/harness/src/background.rs`,
    `crates/harness/src/lib.rs`,
    `crates/harness/src/model_gateway.rs`,
    `crates/harness/src/worker.rs`,
    `crates/harness/tests/unconscious_component.rs`, and
    `crates/workers/src/main.rs`
  - `cargo fmt --all --check`
  - `cargo test -p harness --lib -- --nocapture`
  - `cargo test -p workers -- --nocapture`
  - `cargo test -p harness --test unconscious_component -- --nocapture`
  - `cargo check --workspace`

### Task P4-09: Implement the first memory consolidation maintenance job

- Status: `TODO`
- Depends on: `P4-04`, `P4-05`, `P4-06`, `P4-08`
- Parallel-safe: yes
- Deliverables:
  - scoped memory-consolidation input shape for unconscious execution
  - structured memory delta proposals suitable for the existing merge pipeline
  - harness integration that validates and applies accepted consolidation
    outputs through Phase 3 proposal and merge services
- Verification:
  - unit tests for consolidation planning and validation
  - component or integration tests proving accepted consolidation outputs reach
    canonical persistence safely
- Evidence:
  - not started

### Task P4-10: Implement the first retrieval-maintenance job

- Status: `TODO`
- Depends on: `P4-04`, `P4-05`, `P4-06`, `P4-08`
- Parallel-safe: yes
- Deliverables:
  - scoped retrieval-maintenance input shape for unconscious execution
  - structured retrieval update proposals that preserve the conservative Phase 3
    retrieval baseline
  - harness integration that applies accepted retrieval-maintenance outputs
    without introducing broader retrieval infrastructure
- Verification:
  - unit tests for retrieval-maintenance planning and validation
  - component or integration tests proving accepted retrieval updates are
    durably queryable
- Evidence:
  - not started

### Task P4-11: Implement contradiction and drift diagnostic scanning

- Status: `TODO`
- Depends on: `P4-04`, `P4-05`, `P4-06`, `P4-08`
- Parallel-safe: yes
- Deliverables:
  - contradiction-scan and drift-diagnostic input shape for unconscious
    execution
  - structured diagnostic outputs and any resulting proposal or alert posture
  - bounded harness handling for diagnostics that do not warrant wake-signal
    conversion
- Verification:
  - unit tests for contradiction and drift classification logic
  - component or integration tests proving diagnostics are durably recorded and
    remain non-mutating unless promoted through accepted proposals
- Evidence:
  - not started

### Task P4-12: Implement self-model reflection and delta proposal generation

- Status: `TODO`
- Depends on: `P4-04`, `P4-05`, `P4-06`, `P4-08`
- Parallel-safe: yes
- Deliverables:
  - scoped self-model-reflection input shape for unconscious execution
  - structured self-model delta proposals suitable for the existing Phase 3
    self-model merge path
  - harness integration that validates and applies accepted self-model deltas
    canonically
- Verification:
  - unit tests for self-model-reflection validation and merge planning
  - component or integration tests proving accepted deltas update canonical
    self-model state safely
- Evidence:
  - not started

### Task P4-13: Implement wake-signal persistence, policy evaluation, and conversion

- Status: `TODO`
- Depends on: `P4-04`, `P4-05`, `P4-07`, `P4-08`
- Parallel-safe: no
- Deliverables:
  - durable wake-signal persistence model
  - typed wake-signal reason and priority handling
  - harness policy evaluation, throttling, suppression, and approval posture
  - guarded conversion of accepted wake signals into foreground triggers or
    equivalent queued foreground work
- Verification:
  - unit tests for wake-signal evaluation and throttling logic
  - component tests proving accepted and rejected wake-signal outcomes are
    durably recorded
- Execution note:
  - likely split point if persistence and conversion need to land separately to
    keep regressions controlled
- Evidence:
  - not started

### Task P4-14: Extend runtime harness execution paths for background maintenance

- Status: `TODO`
- Depends on: `P4-08`, `P4-13`
- Parallel-safe: no
- Deliverables:
  - runtime path that can execute one due unconscious job through the harness
    one-shot model
  - manual-verification-friendly harness entrypoint behavior for Phase 4 local
    checks
  - clear runtime audit trail for job selection, execution, and wake-signal
    outcomes
- Verification:
  - component or integration tests for one-shot runtime execution of due jobs
  - manual review that the runtime surface remains thin and harness-owned
- Evidence:
  - not started

### Task P4-15: Add unit coverage for scheduler, scoping, and wake-policy logic

- Status: `TODO`
- Depends on: `P4-02`, `P4-05`, `P4-07`, `P4-08`, `P4-13`
- Parallel-safe: yes
- Deliverables:
  - unit tests for scheduler planning, due-job selection, and deduplication
  - unit tests for scoped-input shaping and budget handling
  - unit tests for wake-signal validation, throttling, and conversion policy
  - targeted failure-path tests for malformed outputs and timeout handling
- Verification:
  - `cargo test --workspace --lib -- --nocapture`
  - focused harness module test runs while implementation is in progress
- Evidence:
  - not started

### Task P4-16: Add PostgreSQL-backed component coverage for scheduler and maintenance services

- Status: `TODO`
- Depends on: `P4-03`, `P4-04`, `P4-09`, `P4-10`, `P4-11`, `P4-12`, `P4-13`
- Parallel-safe: no
- Deliverables:
  - `crates/harness/tests/unconscious_component.rs`
  - component coverage for scheduler persistence, active-run coordination,
    maintenance-output persistence, and wake-signal handling
  - component coverage for accepted and rejected maintenance outputs against
    disposable PostgreSQL
- Verification:
  - `cargo test -p harness --test unconscious_component -- --nocapture`
- Evidence:
  - not started

### Task P4-17: Add architecture-critical integration tests for Phase 4 unconscious flows

- Status: `TODO`
- Depends on: `P4-08`, `P4-09`, `P4-10`, `P4-11`, `P4-12`, `P4-13`, `P4-14`
- Parallel-safe: no
- Deliverables:
  - `crates/harness/tests/unconscious_integration.rs`
  - integration coverage for due unconscious job to proposal to merge flow
  - integration coverage for wake signal to policy evaluation to foreground
    conversion flow
  - integration coverage for timeout or crash handling on the unconscious path
- Verification:
  - `cargo test -p harness --test unconscious_integration -- --nocapture`
  - manual review that the Phase 4 integration suite remains architecture
    critical rather than becoming an omnibus duplicate of component coverage
- Evidence:
  - not started

### Task P4-18: Extend repository CI and canonical docs for the Phase 4 gate

- Status: `TODO`
- Depends on: `P4-15`, `P4-16`, `P4-17`
- Parallel-safe: no
- Deliverables:
  - repository-hosted CI updates for the `background-maintenance` gate
  - canonical doc updates for Phase 4 verification commands and current
    sequencing references
  - this document updated as the Phase 4 execution ledger and evidence record
- Verification:
  - successful local verification of the final Phase 4 command set
  - manual review that CI gate names and document language remain
    capability-based
- Evidence:
  - not started
