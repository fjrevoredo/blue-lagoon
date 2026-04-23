# Blue Lagoon

## Phase 6 Detailed Implementation Plan

Date: 2026-04-23
Status: In progress; Milestone A, Milestone B, and Milestone C completed; Milestone D not started
Scope: High-level plan Phase 6 only
Audience: LLM-assisted implementation work and human review

## Purpose

This document defines the detailed implementation plan for Phase 6 of Blue
Lagoon.

It translates the approved Phase 6 scope from
`docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md` into concrete, trackable, and
LLM-executable work items.

Phase 6 turns the already functional dual-loop runtime into a release-ready v1
runtime. Its purpose is to complete harness-owned recovery, operational
diagnostics, migration discipline, and release-grade verification so the system
can run continuously without relying on operator intuition, raw SQL, or manual
cleanup after failures.

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
- `docs/PHASE_4_5_DETAILED_IMPLEMENTATION_PLAN.md`
- `docs/PHASE_5_DETAILED_IMPLEMENTATION_PLAN.md`

If this document conflicts with the canonical documents, the canonical
documents win.

## Documentation readiness review

The Phase 6 planning baseline is ready.

The current canonical documents agree on the core Phase 6 intent:

- recovery remains harness-owned rather than worker-owned
- v1 recovery is checkpoint-light, proof-based, fresh-worker based, and
  fail-closed for ambiguous side effects
- leases, heartbeats, timeout handling, and stalled-worker cleanup are
  operational control mechanisms, not optional observability extras
- recovery, health, diagnostics, and upgrade safety must become first-class
  operator workflows through the management CLI
- migration discipline, compatibility handling, and release gates are part of
  runtime safety rather than post-hoc operational hygiene
- Phase 6 is not primarily a feature-expansion phase; it is the hardening phase
  that makes the existing assistant trustworthy to run continuously

No blocking contradiction was found between
`docs/REQUIREMENTS.md`,
`docs/LOOP_ARCHITECTURE.md`,
`docs/IMPLEMENTATION_DESIGN.md`, and
`docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md`.

Several Phase 6 details are already settled in the canonical documents and
should be treated as planning inputs rather than reopened design questions:

- PostgreSQL remains the canonical system of record for recovery checkpoints,
  recovery state, approvals, wake signals, job state, workspace state, and
  audit events
- recovery is harness-led and uses fresh worker instantiation rather than
  session resurrection or worker self-recovery
- the v1 recovery taxonomy remains compact:
  - `crash`
  - `timeout_or_stall`
  - `supervisor_restart`
  - `approval_transition`
  - `integrity_or_policy_block`
- the v1 checkpoint posture remains narrow and structured rather than attempting
  full model-session serialization
- recovery continuation after side effects must be justified by explicit action
  classification and persisted evidence; ambiguous side effects fail closed
- the management CLI remains the primary operator surface and must stay
  capability-oriented rather than storage-oriented
- release confidence must come from explicit automated evidence, including
  targeted fault injection and upgrade-path coverage, not from manual
  confidence alone

## CI assessment for Phase 6

At Phase 6 planning start, the repository-hosted CI posture is already strong
for workspace verification, foreground runtime, canonical persistence,
background maintenance, management CLI coverage, and governed actions.

The current stable jobs remain useful:

- `workspace-verification`
- `foreground-runtime`
- `canonical-persistence`
- `background-maintenance`
- `management-cli`
- `governed-actions`

Phase 6 should preserve those stable job identities and add the minimum new
stage-specific gates needed to cover recovery, upgrade safety, and release
readiness.

The Phase 6 CI posture is locked as follows:

- Keep `workspace-verification` as the fast repository-wide baseline gate.
- Keep `foreground-runtime` focused on Telegram-first foreground behavior,
  including foreground recovery triggers and approval-transition recovery where
  those remain channel-adjacent.
- Keep `canonical-persistence` focused on migration-sensitive and
  canonical-write-sensitive continuity regressions, expanded for checkpoint and
  recovery-state persistence where appropriate.
- Keep `background-maintenance` focused on scheduler, unconscious-worker,
  wake-signal, and background recovery behavior.
- Keep `management-cli` focused on operator-surface parsing, formatting, and
  persistence-backed management workflows.
- Keep `governed-actions` focused on approval, policy re-check, capability
  scoping, execution, and governed-action blocking semantics.
- Add `recovery-hardening` for crash, restart, timeout, stalled-worker, lease,
  checkpoint, and recovery-decision regression coverage.
- Add `release-readiness` for upgrade-path validation, targeted fault
  injection, smoke verification, and selected test-effectiveness checks on
  critical safety modules.

The intended Phase 6 gate-to-suite mapping is:

- `workspace-verification`
  Run formatting, compile checks, clippy, and fast unit-focused verification
  that does not require PostgreSQL.
- `foreground-runtime`
  Continue running `foreground_component` and `foreground_integration`, extended
  for recovery-trigger and degraded-runtime behavior where needed.
- `canonical-persistence`
  Continue running `continuity_component` and `continuity_integration`, plus
  checkpoint, migration, and upgrade-sensitive persistence cases that directly
  protect canonical state.
- `background-maintenance`
  Continue running `unconscious_component` and `unconscious_integration`,
  extended for stall handling, retry policy, wake-signal durability, and
  background recovery.
- `management-cli`
  Continue running `crates/runtime/tests/admin_cli.rs`, extended for recovery,
  diagnostics, and health workflows.
- `governed-actions`
  Continue running the governed-action suites added in Phase 5.
- `recovery-hardening`
  Run new PostgreSQL-backed recovery component and integration suites covering
  checkpoints, leases, heartbeats, retry policy, stalled-worker cleanup, crash
  recovery, approval-transition recovery, approval-expiry recovery, policy
  re-check failure, wake-signal routing under degraded conditions, and
  fail-closed ambiguous-side-effect handling.
- `release-readiness`
  Run upgrade-path tests, targeted fault-injection tests, selected smoke flows,
  and narrowly scoped meta-validation checks for the critical safety modules.

Phase 6 CI expansion should avoid re-running the same expensive end-to-end
scenarios in multiple jobs unless the duplication serves a deliberate stage
boundary such as pre-merge versus release gating.

## Implementation starting point

The current repository already contains several Phase 6 starting points that
should be extended rather than replaced.

The default implementation starting points are:

- `crates/contracts/src/lib.rs`
  for shared execution, trigger, approval, wake-signal, and worker-facing
  contracts that will need explicit recovery and diagnostics additions
- `crates/harness/src/runtime.rs`
  for one-shot runtime wiring and the future recovery supervisor orchestration
- `crates/harness/src/execution.rs`
  for durable execution-state handling that recovery must build on rather than
  bypass
- `crates/harness/src/foreground.rs`
  for persisted foreground execution state and backlog-aware recovery seams
- `crates/harness/src/foreground_orchestration.rs`
  for foreground orchestration paths that will need recovery-trigger handling
- `crates/harness/src/background_execution.rs`
  and `crates/harness/src/background_planning.rs`
  for current unconscious-job orchestration that will need checkpointing,
  retries, and stalled-job handling
- `crates/harness/src/governed_actions.rs`
  for action classification, execution observations, policy re-check behavior,
  and ambiguous-side-effect recovery decisions
- `crates/harness/src/policy.rs`
  for budgets, wake-signal policy, and future recovery classification logic
- `crates/harness/src/management.rs`
  for the existing harness-side operator surface that Phase 6 should extend
  instead of bypassing
- `crates/runtime/src/admin.rs`
  for the existing management CLI namespace that should gain recovery, health,
  and diagnostics subcommands without introducing a parallel operator binary
- `crates/harness/src/audit.rs`
  for durable trace-linked event history that recovery and diagnostics must
  continue to use
- `crates/harness/src/migration.rs`
  and `crates/harness/src/schema.rs`
  for schema validation, startup gating, and migration discipline
- `crates/harness/src/worker.rs`
  for isolated subprocess execution, timeout enforcement, and worker lifecycle
  boundaries that will need lease and heartbeat integration
- `crates/harness/tests/support/mod.rs`
  for disposable PostgreSQL test support and migration-backed test setup
- `.github/workflows/ci.yml`
  for staged repository gates that now need Phase 6 hardening and release
  coverage
- `config/default.toml`
  for repository-safe recovery, lease, retry, and diagnostics defaults
- `migrations/0006__workspace_and_governed_actions.sql`
  as the last reviewed migration before recovery, health, and upgrade-path
  schema additions

At Phase 6 planning start, the current repository state also makes several
important constraints explicit:

- the runtime already supports foreground continuity, bounded background jobs,
  governed actions, approvals, and a management CLI, so Phase 6 should harden
  those capabilities rather than replacing them
- some backlog-aware foreground recovery exists, but it is not yet generalized
  into one coherent recovery checkpoint and continuation model
- there is no full Phase 6 recovery supervisor surface yet for crash, restart,
  timeout, and stalled-worker handling across both loops
- lease, heartbeat, and retry behavior are not yet first-class canonical
  runtime objects or policies
- health and diagnostics workflows are not yet exposed as a complete
  capability-oriented operator surface
- release-stage CI and upgrade-path verification are not yet defined as
  explicit repository-hosted gates

## Phase 6 target

Phase 6 is complete only when Blue Lagoon proves the following:

- the harness can recover foreground and background work safely after crash,
  restart, timeout, stall, and approval-transition events without violating
  canonical-write or side-effect safety rules
- checkpoints exist as structured, harness-owned recovery records that preserve
  the minimum durable state needed for safe continuation, retry, deferment,
  clarification, re-approval, or graceful abandonment
- active work is supervised with leases and heartbeats, and stalled workers are
  detected, terminated, audited, and routed into one coherent recovery
  evaluation path
- retry policy and recovery budgets are explicit, class-based, bounded, and
  fail closed when exhausted
- ambiguous side effects are never resumed or replayed blindly; continuation is
  allowed only when action classification and persisted evidence prove it is
  safe
- approval expiry, policy re-check failure, and wake-signal routing under
  degraded runtime conditions are covered by explicit recovery behavior rather
  than incidental subsystem-specific fallbacks
- health, diagnostics, recovery inspection, and the minimum required explicit
  recovery controls are available through the management CLI rather than raw SQL
  or ad hoc scripts
- migration operational conventions, upgrade-path validation, and persisted
  artifact compatibility rules are implemented and enforced at runtime
- required Phase 6 unit, component, integration, fault-injection, and
  release-critical suites pass
- repository CI runs the required recovery and release-readiness gates under
  stable capability-based identities

## Settled implementation clarifications

The following Phase 6 decisions are treated as settled for execution unless
later canonical documents intentionally change them:

- Phase 6 is a hardening phase, not a broad feature-expansion phase. Any new
  operator surface or runtime behavior must exist to make the current
  architecture reliable, diagnosable, and releasable.
- Recovery remains harness-owned. Workers must not own retries, checkpoint
  policy, ambiguous-side-effect decisions, or recovery-trigger issuance.
- V1 recovery remains checkpoint-light and fresh-worker based. The system must
  not attempt full hidden-session serialization or restoration.
- Recovery must be proof-based. The harness should resume or retry only when it
  can justify doing so from persisted state, action classification, and policy.
- Ambiguous side effects fail closed. The fallback paths are clarification,
  explicit re-approval, deferment, or graceful abandonment, not optimistic
  continuation.
- Lease and heartbeat handling should remain class-based and configurable:
  - foreground work
  - normal background work
  - sensitive or long-running governed actions
- Retry policy should remain class-based rather than one global retry limit.
- The management CLI must stay capability-oriented. Phase 6 should add recovery,
  health, diagnostics, and upgrade workflows through the existing management
  surface rather than introducing a new admin shell.
- Migration safety remains part of the control architecture. Runtime startup
  must continue to fail closed on unsupported schema versions or incompatible
  persisted state.
- Phase 6 may add the minimum new canonical tables needed for recovery and
  operator safety. The default table names to plan around are:
  - `recovery_checkpoints`
  - `worker_leases`
  - `operational_diagnostics`
  - any migration-safe upgrade metadata or compatibility aids required by the
    reviewed schema design

## Execution refinement notes

This document is a draft execution ledger for Phase 6.

Execution rules for this phase:

- Work one task at a time unless a task is explicitly marked as parallel-safe.
- Do not start a task until all of its dependencies are marked `DONE`.
- No core Phase 6 task is complete without the verification listed for it.
- Keep recovery harness-owned. If workers begin selecting their own retry,
  checkpoint, or continuation policy, stop and split the work first.
- Keep recovery structured and narrow. If a task begins drifting toward full
  session serialization, pause and re-scope it.
- Keep Phase 6 release-focused. If a task introduces unrelated feature breadth,
  stop and defer it unless it directly improves recovery, diagnostics, migration
  safety, or release verification.
- Keep the management CLI as the operator surface. If a workflow still depends
  on raw SQL or temporary scripts after implementation, the task is not done.

Current execution status:

- Current active task: `D1`
- Completed tasks: `15/20`
- Milestone A status: `DONE`
- Milestone B status: `DONE`
- Milestone C status: `DONE`
- Milestone D status: `NOT STARTED`

Repository sequencing note:

- Phase 5 is complete, and this document is the draft execution ledger for the
  Phase 6 recovery, diagnostics, migration-safety, and release-readiness slice.

## Milestone A quality gate

Milestone A is green only if:

- the Phase 6 scope boundary and deferred behavior are explicit
- the recovery model is translated into settled implementation decisions without
  reopening already-settled design posture
- the checkpoint, lease, heartbeat, retry, and ambiguous-side-effect recovery
  boundaries are explicit before schema or orchestration work begins
- the operator workflows that must land in the management CLI are identified
  before implementation starts
- the CI gate strategy for recovery hardening and release readiness is explicit
  before new verification work is spread across the repository

## Milestone B quality gate

Milestone B is green only if:

- canonical persistence exists for recovery checkpoints and the minimum required
  recovery-supervision state
- the harness can record, read, and update recovery state without ad hoc SQL
- checkpoint and recovery persistence remain clearly separated from ephemeral
  in-process worker state
- migration and schema-safety rules for the new persisted artifacts are explicit
  and compatible with the repository migration posture
- component tests prove the new recovery state persists and rehydrates
  correctly against disposable PostgreSQL

## Milestone C quality gate

Milestone C is green only if:

- the harness can supervise active work with leases and heartbeat-aware timeout
  handling
- foreground, background, and governed-action recovery flows share one coherent
  recovery decision path rather than ad hoc per-subsystem logic
- retry, deferment, re-approval, and graceful-abandonment behavior are explicit,
  bounded, and audited
- ambiguous side-effect cases are provably blocked from unsafe automatic
  continuation
- recovery, health, and diagnostics workflows are accessible through the
  management CLI

## Milestone D quality gate

Milestone D is green only if:

- required unit coverage exists for recovery decision logic, lease handling,
  retry classification, schema-compatibility validation, and CLI validation or
  formatter logic
- PostgreSQL-backed component coverage exists for checkpoints, leases,
  diagnostics, migration-sensitive persistence, and operator workflows
- architecture-critical integration coverage exists for crash, restart, timeout,
  stalled-worker, approval-transition, approval-expiry, policy re-check failure,
  and wake-signal-routing recovery paths
- targeted fault-injection and upgrade-path coverage exists for the critical
  safety modules
- repository CI runs the required Phase 6 suites under stable gate identities,
  including release-readiness gates
- the Phase 6 exit criteria can be justified by explicit automated evidence

## Milestone A: Scope lock and recovery model translation

### Task A1: Lock the Phase 6 scope and explicit deferrals

Status: `DONE`

Deliverables:

- explicit statement of what Phase 6 must harden now
- explicit list of what remains deferred past Phase 6
- explicit rule that Phase 6 is not a broad new feature phase

Dependencies:

- none

Verification:

- document self-check against
  `docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md`,
  `docs/IMPLEMENTATION_DESIGN.md`, and
  `docs/REQUIREMENTS.md`

Implementation evidence:

- Phase 6 hardening scope, non-goals, and target outcomes are locked in this
  document.
- The plan explicitly constrains Phase 6 to recovery, diagnostics,
  migration-safety, and release-readiness work.

### Task A2: Translate the settled recovery model into implementation constraints

Status: `DONE`

Deliverables:

- explicit implementation rules for:
  - checkpoints
  - recovery reason taxonomy
  - continuation versus abandonment
  - ambiguous side-effect handling
  - lease and heartbeat supervision
  - retry budgets
- explicit ownership boundaries for harness versus workers

Dependencies:

- Task A1

Verification:

- document self-check against the recovery sections of
  `docs/IMPLEMENTATION_DESIGN.md`

Implementation evidence:

- The recovery model is fixed as harness-owned, checkpoint-light, proof-based,
  fresh-worker based, and fail-closed for ambiguous side effects.
- The plan now fixes the recovery taxonomy, checkpoint posture, lease posture,
  and test-effectiveness targets before implementation continues.

### Task A3: Define the Phase 6 operator workflows and CLI expansion boundary

Status: `DONE`

Deliverables:

- explicit list of required Phase 6 management capabilities, including:
  - recovery checkpoint inspection
  - stalled or interrupted work inspection
  - health summaries
  - diagnostics and recent operational anomalies
  - bounded explicit recovery actions where justified
  - upgrade or schema-compatibility inspection
- explicit list of workflows still deferred

Dependencies:

- Task A1
- Task A2

Verification:

- document self-check against Section 19 of `docs/REQUIREMENTS.md`

Implementation evidence:

- Required Phase 6 management CLI capabilities are fixed under
  "Pre-implementation decisions from plan self-check".
- The plan keeps management capability-oriented and rejects raw SQL or temporary
  scripts as the normal recovery operator workflow.

### Task A4: Define the Phase 6 test and CI gate strategy

Status: `DONE`

Deliverables:

- explicit suite plan for:
  - unit
  - PostgreSQL-backed component
  - integration
  - targeted fault injection
  - upgrade-path
  - smoke
  - selected test-effectiveness checks
- explicit mapping from suites to CI jobs

Dependencies:

- Task A1
- Task A2
- Task A3

Verification:

- document self-check against the testing and release-gate sections of
  `docs/IMPLEMENTATION_DESIGN.md`

Implementation evidence:

- The plan defines `recovery-hardening` and `release-readiness` gates and maps
  them to local recovery, upgrade-path, fault-injection, smoke, and
  test-effectiveness verification.

## Milestone B: Canonical recovery state and migration safety

### Task B1: Add reviewed SQL migration for recovery and diagnostics persistence

Status: `DONE`

Deliverables:

- reviewed migration introducing the minimum Phase 6 canonical state, expected
  to include:
  - `recovery_checkpoints`
  - lease or supervision state
  - operational diagnostics or alert persistence where justified
- schema choices aligned with additive-first and expand-contract posture

Dependencies:

- Milestone A

Verification:

- `cargo test --workspace --lib -- --nocapture`
- targeted migration-sensitive component tests after implementation

Implementation evidence:

- Added `migrations/0007__recovery_hardening.sql` with canonical
  `recovery_checkpoints`, `worker_leases`, and `operational_diagnostics`
  tables.
- Verified migration discovery with:
  - `cargo test -p harness --lib migration::tests::load_migrations_discovers_reviewed_files_in_order_with_canonical_names -- --nocapture`

### Task B2: Implement harness recovery-checkpoint persistence services

Status: `DONE`

Deliverables:

- typed read and write models for recovery checkpoints
- harness APIs for creating, refreshing, finalizing, and loading recovery
  checkpoints
- explicit support for foreground, background, and governed-action recovery
  contexts where required by the settled model

Dependencies:

- Task B1

Verification:

- `cargo test -p harness --test recovery_component -- --nocapture`

Implementation evidence:

- Added `crates/harness/src/recovery.rs` with typed recovery checkpoint create,
  fetch, list-open, and resolution APIs.
- Added `recovery_component` coverage for checkpoint persistence, rehydration,
  listing, and resolution.

### Task B3: Implement worker supervision persistence and lease state

Status: `DONE`

Deliverables:

- typed lease and heartbeat models
- harness APIs for lease acquisition, refresh, expiry, and cleanup
- durable traceability linking supervision state to executions, jobs, and
  governed actions where applicable

Dependencies:

- Task B1

Verification:

- `cargo test -p harness --test recovery_component -- --nocapture`

Implementation evidence:

- Added typed worker lease create, fetch, refresh, release, and expire-due APIs.
- Added `recovery_component` coverage for lease persistence, heartbeat refresh,
  release, and expiry behavior.

### Task B4: Implement migration compatibility and upgrade-path helpers

Status: `DONE`

Deliverables:

- schema or harness helpers needed for compatibility validation of persisted
  cross-process and cross-recovery artifacts
- startup-safe compatibility checks where the runtime must refuse mixed or
  ambiguous versions
- explicit upgrade-path validation seams for automated testing

Dependencies:

- Task B1

Verification:

- `cargo test -p harness --test migration_component -- --nocapture`

Implementation evidence:

- Added `schema::assess_upgrade_path` as the reusable schema upgrade and
  compatibility assessment seam.
- Added `migration_component` coverage for missing-schema and supported-schema
  upgrade-path assessment.

### Task B5: Add PostgreSQL-backed component tests for recovery persistence

Status: `DONE`

Deliverables:

- component tests proving:
  - checkpoint persistence and rehydration
  - lease creation, refresh, and expiry handling
  - compatibility or upgrade-state validation
  - diagnostics persistence where implemented

Dependencies:

- Task B2
- Task B3
- Task B4

Verification:

- `cargo test -p harness --test recovery_component -- --nocapture`
- `cargo test -p harness --test migration_component -- --nocapture`

Implementation evidence:

- Added `recovery_component` coverage for checkpoint, lease, and diagnostic
  persistence against disposable PostgreSQL.
- Added `migration_component` coverage for upgrade-path assessment against both
  clean and migrated disposable PostgreSQL databases.

## Milestone C: Recovery orchestration, diagnostics, and operator surface

### Task C1: Implement a unified recovery decision service

Status: `DONE`

Deliverables:

- one harness-owned recovery decision layer covering:
  - foreground recovery
  - unconscious-job recovery
  - governed-action recovery classification
  - approval-transition recovery
  - approval-expiry recovery
  - integrity or policy-block recovery
- explicit continuation, retry, deferment, re-approval, clarification, and
  abandonment outcomes

Dependencies:

- Milestone B

Verification:

- unit coverage for recovery classification logic
- `cargo test -p harness --test recovery_component -- --nocapture`

Completion evidence:

- Added `recovery::evaluate_recovery_decision` as the harness-owned decision
  layer for foreground, background, governed-action, approval-transition,
  approval-expiry, and integrity or policy-block recovery classification.
- Added explicit recovery action classification, evidence, approval, and policy
  state inputs so continuation, retry, deferment, re-approval, clarification,
  and abandonment outcomes are produced by one shared path.
- Added unit coverage for all explicit C1 outcomes, including fail-closed
  ambiguous side effects, exhausted recovery budget, expired approval,
  pending approval, and policy-block abandonment.
- Verified with
  `cargo test -p harness recovery::tests -- --nocapture` and
  `cargo test -p harness --test recovery_component -- --nocapture`.

### Task C2: Integrate leases, heartbeats, and stalled-worker handling into runtime orchestration

Status: `DONE`

Deliverables:

- runtime supervision of active work with class-based lease defaults
- heartbeat refresh integration where progress is observable
- soft-warning and hard-expiry handling
- stalled-worker cleanup that routes into the unified recovery path

Dependencies:

- Task B3
- Task C1

Verification:

- `cargo test -p harness --test recovery_integration -- --nocapture`

Completion evidence:

- Added lease supervision classification for healthy, soft-warning, and
  hard-expired active worker leases.
- Added stale active-lease recovery routing through
  `recovery::recover_expired_worker_leases`, including lease expiry,
  checkpoint creation, unified recovery-decision evaluation, checkpoint
  resolution, and operational diagnostic recording.
- Added startup/runtime hooks in harness and Telegram one-shot paths so expired
  leases are routed through recovery before new work is processed.
- Added persisted foreground, background, and governed-action lease
  creation/release around conscious, unconscious, and governed-action execution
  paths.
- Added soft-warning diagnostic recording with duplicate suppression per active
  worker lease.
- Added `recovery_integration` coverage for expired worker lease recovery
  routing and soft-warning recording, and verified existing foreground,
  unconscious, and governed-action integration suites.
- Added progress-boundary lease refresh through
  `recovery::refresh_worker_lease_progress`, with runtime calls from
  foreground, background, and governed-action execution paths.
- Added observed timeout recovery routing through
  `recovery::recover_observed_worker_timeout`, including active lease
  termination, checkpoint creation, unified recovery-decision evaluation,
  checkpoint resolution, and operational diagnostics.
- Wired observed worker timeout recovery into foreground, background, and
  governed-action orchestration so direct timeout handling no longer bypasses
  the unified recovery path.
- Added recovery integration coverage for observed active-lease timeout
  routing and progress-boundary heartbeat refresh.
- Extended the background timeout component test to assert that the direct
  timeout path emits recovery diagnostics.
- Verified with `cargo test -p harness --test recovery_integration -- --nocapture`,
  `cargo test -p harness --test unconscious_component -- --nocapture`,
  `cargo test -p harness --test foreground_integration -- --nocapture`,
  `cargo test -p harness --test governed_actions_component -- --nocapture`,
  `cargo test -p harness --test governed_actions_integration -- --nocapture`,
  and `cargo clippy --workspace --all-targets -- -D warnings`.

### Task C3: Generalize foreground and background recovery triggers

Status: `DONE`

Deliverables:

- coherent recovery-trigger issuance for:
  - crash
  - timeout or stall
  - supervisor restart
  - approval transition
  - approval expiry
  - integrity or policy block
- wake-signal routing behavior under degraded-runtime and recovery conditions
- reuse of the same checkpoint and recovery decision infrastructure across
  foreground and background work

Dependencies:

- Task B2
- Task C1
- Task C2

Verification:

- `cargo test -p harness --test recovery_integration -- --nocapture`

### Task C4: Implement fail-closed ambiguous-side-effect handling for governed actions

Status: `DONE`

Deliverables:

- explicit recovery classification for governed actions and side-effecting work
- persisted evidence sufficient to justify safe continuation only when allowed
- fail-closed handling for ambiguous completion state, including re-approval or
  clarification paths where applicable
- explicit policy re-check failure recovery behavior at execution time

Dependencies:

- Task C1
- Task C3

Verification:

- `cargo test -p harness --test governed_actions_integration -- --nocapture`
- targeted recovery integration coverage

### Task C5: Implement health summaries, diagnostics, and operator-facing recovery visibility

Status: `DONE`

Deliverables:

- harness services for health summaries and recent operational diagnostics
- trace-linked operational anomaly recording for repeated recovery conditions,
  failures, and pressure signals where justified
- operational metrics or rollups suitable for both human operators and future
  internal-state feeds

Dependencies:

- Task B2
- Task B3
- Task C1

Verification:

- `cargo test -p harness --test management_component -- --nocapture`

### Task C6: Extend the management CLI for recovery, health, and diagnostics workflows

Status: `DONE`

Deliverables:

- new or extended `admin` commands for:
  - recovery status or checkpoint inspection
  - health summaries
  - diagnostics or recent anomaly listing
  - bounded explicit recovery controls where justified by the settled model
  - schema or upgrade-safety inspection where needed
- text and JSON output paths
- parser and formatter tests

Dependencies:

- Task A3
- Task C5
- Task B4

Verification:

- `cargo test -p runtime --bin runtime -- --nocapture`
- `cargo test -p runtime --test admin_cli -- --nocapture`
- `cargo test -p harness --test management_component -- --nocapture`
- `cargo test -p harness --test management_integration -- --nocapture`

Completion evidence:

- Added capability-oriented `runtime admin` namespaces for:
  - `health summary`
  - `diagnostics list`
  - `recovery checkpoints list`
  - `recovery leases list`
  - `recovery supervise`
  - `schema status`
  - `schema upgrade-path`
- Added harness-side management services for schema upgrade assessment,
  interrupted or stalled work inspection through `worker_leases`, and bounded
  worker-lease supervision so the CLI reuses harness-owned recovery and schema
  logic instead of bypassing it.
- Added audit-backed operator traceability for `recovery supervise`, including
  per-invocation `trace_id`, `actor_ref`, `reason`, and completion/failure
  events in `audit_events`.
- Added parser, formatter, and help coverage in:
  - `crates/runtime/src/admin.rs`
  - `crates/runtime/tests/admin_cli.rs`
- Added `management_integration` coverage proving the bounded recovery control
  path can supervise and recover expired worker leases through the management
  surface.
- Verified with:
  - `cargo fmt --all --check`
  - `cargo test -p runtime --bin runtime -- --nocapture`
  - `cargo test -p runtime --test admin_cli -- --nocapture`
  - `cargo test -p harness --test management_component -- --nocapture`
  - `cargo test -p harness --test management_integration -- --nocapture`

## Milestone D: Release-grade verification and CI hardening

### Task D1: Add unit coverage for critical Phase 6 safety logic

Status: `TODO`

Deliverables:

- unit tests for:
  - recovery decision logic
  - lease and heartbeat rules
  - retry ceilings and recovery budgets
  - schema compatibility validation
  - CLI validation and formatting logic

Dependencies:

- Milestone C

Verification:

- `cargo test --workspace --lib -- --nocapture`

### Task D2: Add component and integration recovery suites

Status: `TODO`

Deliverables:

- dedicated PostgreSQL-backed recovery suites, expected to include:
  - `recovery_component`
  - `recovery_integration`
- architecture-critical coverage for:
  - worker crash
  - worker stall or lease expiry
  - supervisor restart
  - approval expiry
  - approval-transition recovery
  - policy re-check failure
  - policy-block recovery
  - wake-signal routing under degraded conditions

Dependencies:

- Task D1

Verification:

- `cargo test -p harness --test recovery_component -- --nocapture`
- `cargo test -p harness --test recovery_integration -- --nocapture`

### Task D3: Add upgrade-path, fault-injection, and smoke verification

Status: `TODO`

Deliverables:

- upgrade-path tests for reviewed migrations and compatibility windows
- targeted fault-injection coverage for critical failure modes
- a narrow smoke layer for release confidence
- selected test-effectiveness checks for critical safety modules
- controlled packaging or release automation where justified by v1 readiness

Dependencies:

- Task D2

Verification:

- dedicated Phase 6 upgrade, fault-injection, and smoke commands as implemented
- `./scripts/pre-commit.sh` or `./scripts/pre-commit.ps1`

### Task D4: Extend repository CI/CD to cover recovery hardening and release gates

Status: `TODO`

Deliverables:

- CI job updates implementing the Phase 6 gate strategy
- stable gate names for:
  - `recovery-hardening`
  - `release-readiness`
- explicit mapping from local verification commands to repository-hosted gates
- controlled release workflow support where justified by the implemented
  release-readiness surface

Dependencies:

- Task A4
- Task D2
- Task D3

Verification:

- CI workflow self-check against implemented suites
- local rerun of the lowest practical command subset before merge

### Task D5: Update operator and repository documentation for the hardened v1 posture

Status: `TODO`

Deliverables:

- updates to operator-facing command guidance in `AGENTS.md`
- updates to roadmap status in `docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md` when
  Phase 6 implementation actually completes
- any required manual-verification or release-readiness notes that remain
  useful after automation is maximized

Dependencies:

- Task D4

Verification:

- documentation self-check against implemented command surface and CI

## Phase 6 task ledger

Phase 6 contains 20 planned tasks:

- Milestone A: 4 tasks
- Milestone B: 5 tasks
- Milestone C: 6 tasks
- Milestone D: 5 tasks

This phase should be implemented in the following order:

1. Milestone A
2. Milestone B
3. Milestone C
4. Milestone D

The implementation order matters:

- recovery scope and operator workflows must be locked before schema work begins
- schema and persistence foundations must exist before orchestration hardening
- unified recovery decisions must exist before CLI control surfaces or release
  gates can be considered correct
- release-readiness evidence must be built on top of implemented recovery
  behavior rather than on placeholder test scaffolding

## Definition of done

Phase 6 is done only when all of the following are true:

- all Milestone A through D tasks are marked `DONE`
- the Phase 6 quality gates are satisfied
- the Phase 6 exit criteria from
  `docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md`
  are satisfied by explicit implementation and verification evidence
- the management CLI exposes the minimum required recovery, health, and
  diagnostics workflows without raw SQL dependency
- required automated evidence exists for crash, restart, timeout, stall,
  approval expiry, policy re-check failure, wake-signal routing, upgrade, and
  release-critical safety paths
- the repository CI posture matches the implemented local verification surface

## Phase 6 exit-criteria compliance matrix

This plan satisfies the Phase 6 exit criteria from
`docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md` as follows:

- Runtime recovery from crash, restart, timeout, and interrupted execution is
  covered by Milestones B and C, especially Tasks B2, B3, C1, C2, C3, and C4.
- Canonical-write and side-effect safety during recovery is covered by the
  proof-based recovery rules, ambiguous-side-effect fail-closed requirements,
  governed-action recovery classification, and policy re-check failure coverage
  in Tasks C1, C4, D1, D2, and D3.
- Recovery, health, and diagnostics workflows through the management CLI are
  covered by Tasks A3, C5, C6, and D5.
- Unit, component, integration, and release-critical gates are covered by Tasks
  A4, D1, D2, D3, and D4.
- Coherent first runnable v1 posture is covered by the Phase 6 target, the
  explicit non-goals, and the release-readiness Definition of Done.
- Release readiness based on automated evidence is covered by the
  `release-readiness` CI gate, upgrade-path tests, fault-injection tests, smoke
  verification, and selected test-effectiveness checks in Tasks D3 and D4.
- CI/CD support for staged gates and controlled release workflows is covered by
  the CI assessment and Task D4.

## Explicit non-goals for Phase 6

The following are explicitly out of scope unless a later canonical decision
changes them:

- adding major new user-facing features unrelated to recovery or hardening
- introducing a browser-based admin surface
- adding new primary channels beyond the existing product posture
- broad chaos engineering or fleet-scale operations work
- distributed-worker or broker-first deployment redesign
- enterprise RBAC or multi-tenant operational domains
- full session serialization or worker self-recovery
- turning the management CLI into a raw arbitrary admin shell

## Pre-implementation decisions from plan self-check

The following decisions are now fixed for the Phase 6 implementation plan:

- Required management CLI capabilities are recovery status, checkpoint
  inspection, interrupted or stalled work inspection, health summary,
  diagnostics listing, schema or upgrade-safety inspection, and bounded recovery
  evaluation or control operations where the recovery model can prove they are
  safe.
- Lease state should use a dedicated canonical supervision shape, expected to be
  `worker_leases`, linked to executions, background jobs, and governed actions
  as applicable. It should not be hidden only inside subsystem-specific rows.
- Pre-merge CI must cover normal migration safety, recovery component behavior,
  and recovery integration behavior. Release-readiness gates may additionally
  run heavier upgrade-path, fault-injection, smoke, and test-effectiveness
  checks.
- Recovery checkpoints, lease state, and safety-relevant diagnostics are
  canonical. Health rollups and dashboards may be re-derivable operational
  projections as long as their source events remain durable and auditable.
- Test-effectiveness checks are required for recovery decision logic, migration
  compatibility checks, lease expiry handling, approval expiry handling, policy
  re-check failure handling, and ambiguous-side-effect fail-closed behavior.
