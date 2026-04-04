# Blue Lagoon

## Phase 1 Detailed Implementation Plan

Date: 2026-04-05
Status: Initial draft for iteration
Scope: High-level plan Phase 1 only
Audience: LLM-assisted implementation work and human review

## Purpose

This document defines the detailed implementation plan for Phase 1 of Blue
Lagoon.

It translates the approved Phase 1 scope from
`docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md` into concrete, trackable, and
LLM-executable work items.

This plan is intentionally optimized for execution by an LLM agent. That means:

- tasks should be small
- tasks should have explicit dependencies
- each task should produce concrete artifacts
- each task should have objective verification steps
- progress should be written back into this document immediately after task
  completion

## Canonical inputs

This plan is subordinate to the following canonical documents:

- `PHILOSOPHY.md`
- `docs/REQUIREMENTS.md`
- `docs/LOOP_ARCHITECTURE.md`
- `docs/IMPLEMENTATION_DESIGN.md`
- `docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md`

If this document conflicts with those documents, the canonical documents win.

## Phase 1 target

Phase 1 is complete only when Blue Lagoon has a runnable foundation that proves
the following:

- the Rust workspace and crate boundaries exist
- the harness can boot safely
- PostgreSQL-backed reviewed migrations exist
- startup fails closed on schema incompatibility
- workers run as isolated subprocesses
- the harness can process a synthetic trigger end to end
- execution state and audit history are durably recorded
- minimal policy and budget scaffolding exists
- required Phase 1 tests pass

## Settled implementation clarifications

The following Phase 1 decisions are now treated as settled for execution unless
later canonical documents intentionally change them:

- Migration application is an explicit harness-owned command path, not an
  implicit normal-startup side effect.
- Normal harness startup must verify schema compatibility and fail closed on
  unsupported or incomplete schema state.
- The Phase 1 synthetic trigger path uses a generic smoke worker subprocess
  under harness control rather than prematurely modeling a full conscious
  worker.
- The Phase 1 worker contract should remain evolvable into future conscious and
  unconscious worker shapes without changing the core subprocess-control model.
- The canonical local Compose topology for Phase 1 includes `postgres` and the
  runtime service that hosts the harness.
- Workers are not separate Compose services by default.
- JSON over `stdin` and `stdout` is the Phase 1 cross-process serialization
  format.

## LLM execution rules

The plan should be executed under the following rules:

- Work one task at a time unless a task is explicitly marked as parallel-safe.
- Do not start a task until all of its dependencies are marked `DONE`.
- Do not expand a task mid-flight. If it grows too large, split it into new
  task IDs and update this document first.
- No core task is complete without the tests listed in its verification steps.
- Prefer the lowest effective test layer.
- Use disposable real PostgreSQL for persistence-critical verification.
- Update this document immediately after finishing a task, before starting the
  next one.
- If execution stops unexpectedly, the next session should resume by reading
  this document, finding the first `TODO` task whose dependencies are `DONE`,
  and continuing from there.

## Progress tracking protocol

This document is the progress ledger for Phase 1.

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
4. If the task changed the execution order, update dependent tasks before moving
   on.

## Progress snapshot

- Current milestone: `NOT STARTED`
- Current active task: `none`
- Completed tasks: `0/21`
- Milestone A status: `TODO`
- Milestone B status: `TODO`
- Milestone C status: `TODO`
- Milestone D status: `TODO`

## Expected Phase 1 verification commands

These are the intended recurring verification commands for this phase. Some will
become available only after earlier tasks are complete.

- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo test --workspace`
- `docker compose config`
- `docker compose up -d postgres`
- `cargo run -p app -- migrate`
- `cargo run -p app -- --help`
- `cargo run -p app -- harness --once --idle`
- `cargo run -p app -- harness --once --synthetic-trigger smoke`

## Phase 1 milestones

- Milestone A: workspace and runtime skeleton
- Milestone B: persistence and startup safety
- Milestone C: harness control-path baseline
- Milestone D: tests, docs, and completion gate

## Milestone A quality gate

Milestone A is green only if:

- the Cargo workspace resolves
- the main crates compile
- local runtime scaffolding exists
- the planned repository boundaries are reflected in code layout

## Milestone B quality gate

Milestone B is green only if:

- migrations exist and apply cleanly
- schema compatibility checks exist
- startup fails closed on unsupported schema state
- phase-1 database tables exist for migration history, audit events, and minimal
  execution records

## Milestone C quality gate

Milestone C is green only if:

- the harness can create a minimal execution record
- the harness can spawn a worker subprocess
- the worker can return a structured result
- the harness can record an audit event for the path
- the flow is bounded by minimal policy and budget checks

## Milestone D quality gate

Milestone D is green only if:

- required unit tests pass
- required component tests with real PostgreSQL pass
- the no-trigger harness boot or idle path works
- the synthetic trigger demo path works
- Phase 1 documentation is updated with the runnable commands

## Task list

### Task P1-01: Create root Rust workspace

- Status: `TODO`
- Depends on: none
- Parallel-safe: no
- Deliverables:
  - root `Cargo.toml` workspace manifest
  - root crate membership for `app`, `harness`, `contracts`, and `workers`
  - initial directory structure for those crates
- Verification:
  - `cargo metadata --format-version 1 >/dev/null`
- Evidence:
  - pending

### Task P1-02: Scaffold crate entrypoints and compile baseline

- Status: `TODO`
- Depends on: `P1-01`
- Parallel-safe: no
- Deliverables:
  - `app` crate with thin executable entrypoint
  - `harness` crate with public bootstrap surface
  - `contracts` crate for shared cross-process types
  - `workers` crate with worker executable entrypoint
- Verification:
  - `cargo check --workspace`
- Evidence:
  - pending

### Task P1-03: Add repository runtime files for local development

- Status: `TODO`
- Depends on: `P1-02`
- Parallel-safe: no
- Deliverables:
  - `compose.yaml` or equivalent local Compose file
  - non-secret config template
  - environment example file for secrets and connection values
  - initial ignore or local-dev hygiene updates if needed
  - Compose posture aligned with Phase 1 runtime shape, including `postgres`
    and the runtime service, with no separate worker service by default
- Verification:
  - `docker compose config`
- Evidence:
  - pending

### Task P1-04: Document initial Phase 1 command surface

- Status: `TODO`
- Depends on: `P1-03`
- Parallel-safe: yes
- Deliverables:
  - initial command documentation in `README.md` or another canonical doc
  - Phase 1 developer commands for boot, test, and local database startup
- Verification:
  - manual review that the documented commands match the scaffolded files
- Evidence:
  - pending

### Task P1-05: Add reviewed SQL migration layout

- Status: `TODO`
- Depends on: `P1-03`
- Parallel-safe: no
- Deliverables:
  - `migrations/` directory
  - migration naming convention `NNNN__short_snake_case.sql`
  - first migration files needed for Phase 1
  - migration runner integration point in code
  - explicit migration command path separate from normal harness startup
- Verification:
  - migration files exist with the agreed naming convention
  - migration runner can discover them
- Evidence:
  - pending

### Task P1-06: Create Phase 1 canonical database tables

- Status: `TODO`
- Depends on: `P1-05`
- Parallel-safe: no
- Deliverables:
  - `schema_migrations` table support
  - `audit_events` table support
  - minimal execution-record table for tracked synthetic runs
  - only the minimum indexes and constraints needed for Phase 1
- Verification:
  - migrations apply cleanly to disposable PostgreSQL
  - tables are visible after migration
- Evidence:
  - pending

### Task P1-07: Implement config loading and startup inputs

- Status: `TODO`
- Depends on: `P1-03`
- Parallel-safe: no
- Deliverables:
  - typed runtime config loading
  - environment-variable loading for secrets
  - validation for required startup inputs
  - fail-closed behavior for missing critical config
- Verification:
  - unit tests for config parsing and validation
  - failed startup on missing required settings
- Evidence:
  - pending

### Task P1-08: Implement schema-version compatibility gating

- Status: `TODO`
- Depends on: `P1-06`, `P1-07`
- Parallel-safe: no
- Deliverables:
  - startup schema-version check
  - supported-version rules in code
  - fail-closed startup path on unsupported schema state
  - startup verification path that does not auto-apply pending migrations during
    normal harness boot
- Verification:
  - unit tests for compatibility decisions
  - component test against disposable PostgreSQL for supported and unsupported
    schema states
- Evidence:
  - pending

### Task P1-09: Add tracing bootstrap and trace correlation primitives

- Status: `TODO`
- Depends on: `P1-02`, `P1-07`
- Parallel-safe: yes
- Deliverables:
  - tracing initialization in the app or harness path
  - basic trace ID creation strategy
  - structured logging surface aligned with harness ownership
- Verification:
  - startup emits structured logs
  - unit tests for trace-context helpers where applicable
- Evidence:
  - pending

### Task P1-10: Add durable audit-event write path

- Status: `TODO`
- Depends on: `P1-06`, `P1-09`
- Parallel-safe: no
- Deliverables:
  - harness-owned audit-event write path
  - minimal Phase 1 event envelope
  - non-inline or bounded write approach if feasible in Phase 1
- Verification:
  - component test writes and reads an audit event from real PostgreSQL
- Evidence:
  - pending

### Task P1-11: Add minimal execution-record persistence

- Status: `TODO`
- Depends on: `P1-06`, `P1-07`
- Parallel-safe: yes
- Deliverables:
  - harness-owned write path for execution records
  - minimal execution states for synthetic trigger handling
  - enough durable data to avoid silent loss on interrupted work
- Verification:
  - component test writes and reads a synthetic execution record
- Evidence:
  - pending

### Task P1-12: Add minimal policy and budget scaffolding

- Status: `TODO`
- Depends on: `P1-02`, `P1-07`
- Parallel-safe: yes
- Deliverables:
  - minimal policy-check interface in harness
  - minimal budget structure for bounded execution
  - default allow or deny behavior explicitly encoded for Phase 1
- Verification:
  - unit tests for policy and budget decisions
- Evidence:
  - pending

### Task P1-13: Define initial cross-process worker contract

- Status: `TODO`
- Depends on: `P1-02`
- Parallel-safe: yes
- Deliverables:
  - structured request type in `contracts`
  - structured response type in `contracts`
  - minimal error shape for worker execution
  - explicit serialization format choice for Phase 1: JSON over `stdin` and
    `stdout`
- Verification:
  - unit tests for contract serialization and validation
- Evidence:
  - pending

### Task P1-14: Implement isolated worker subprocess launcher

- Status: `TODO`
- Depends on: `P1-11`, `P1-12`, `P1-13`
- Parallel-safe: no
- Deliverables:
  - harness-side worker launch path
  - subprocess invocation under harness control
  - bounded timeout or termination behavior for the synthetic path
  - no in-process worker shortcut
- Verification:
  - component or integration test proving a subprocess is spawned and handled
- Evidence:
  - pending

### Task P1-15: Implement stub worker runtime

- Status: `TODO`
- Depends on: `P1-13`
- Parallel-safe: yes
- Deliverables:
  - generic smoke-worker executable that accepts the Phase 1 request
  - deterministic structured response for the synthetic flow
  - explicit non-goals preventing the worker from owning canonical writes
  - contract shape that can evolve into future conscious and unconscious worker
    paths without redesigning the subprocess boundary
- Verification:
  - direct worker test for request in and structured response out
- Evidence:
  - pending

### Task P1-16A: Implement no-trigger harness boot and idle path

- Status: `TODO`
- Depends on: `P1-08`, `P1-12`
- Parallel-safe: no
- Deliverables:
  - harness command path that boots with valid config and schema state
  - no-trigger safe idle behavior for Phase 1
  - explicit exit behavior for one-shot idle verification
  - explicit separation between migration command execution and normal harness
    boot
- Verification:
  - `cargo run -p app -- harness --once --idle`
- Evidence:
  - pending

### Task P1-16: Implement synthetic trigger end-to-end harness flow

- Status: `TODO`
- Depends on: `P1-10`, `P1-11`, `P1-12`, `P1-14`, `P1-15`, `P1-16A`
- Parallel-safe: no
- Deliverables:
  - synthetic trigger intake path
  - execution-record creation
  - policy and budget check invocation
  - generic smoke-worker dispatch and response handling
  - audit-event emission for the flow
  - idle or completion return path
- Verification:
  - `cargo run -p app -- harness --once --synthetic-trigger smoke`
  - integration test for trigger to worker to persisted outputs
- Evidence:
  - pending

### Task P1-17: Add Phase 1 unit-test baseline

- Status: `TODO`
- Depends on: `P1-07`, `P1-08`, `P1-12`, `P1-13`
- Parallel-safe: yes
- Deliverables:
  - unit tests for config logic
  - unit tests for schema compatibility logic
  - unit tests for policy and budget scaffolding
  - unit tests for contract serialization helpers
- Verification:
  - `cargo test --workspace`
- Evidence:
  - pending

### Task P1-18: Add Phase 1 real-PostgreSQL component tests

- Status: `TODO`
- Depends on: `P1-06`, `P1-08`, `P1-10`, `P1-11`
- Parallel-safe: no
- Deliverables:
  - migration application test
  - startup compatibility test
  - audit-event persistence test
  - execution-record persistence test
- Verification:
  - component-test command against disposable PostgreSQL
- Evidence:
  - pending

### Task P1-19: Add Phase 1 smoke and regression gate

- Status: `TODO`
- Depends on: `P1-16`, `P1-17`, `P1-18`
- Parallel-safe: no
- Deliverables:
  - one repeatable smoke path for the synthetic trigger
  - required Phase 1 verification command set
  - at least one regression-oriented test proving a fail-closed behavior
- Verification:
  - all recurring Phase 1 verification commands pass
- Evidence:
  - pending

### Task P1-20: Update Phase 1 completion notes and progress ledger

- Status: `TODO`
- Depends on: `P1-19`
- Parallel-safe: no
- Deliverables:
  - this document updated with final statuses
  - Phase 1 completion summary added to canonical docs if needed
  - any follow-on notes required for Phase 2 planning
- Verification:
  - manual review that all completed tasks contain evidence
- Evidence:
  - pending

## Recommended execution order

Execute Phase 1 in this order unless a justified change is written into this
document first:

1. `P1-01`
2. `P1-02`
3. `P1-03`
4. `P1-04`
5. `P1-05`
6. `P1-06`
7. `P1-07`
8. `P1-08`
9. `P1-09`
10. `P1-10`
11. `P1-11`
12. `P1-12`
13. `P1-13`
14. `P1-15`
15. `P1-14`
16. `P1-16A`
17. `P1-16`
18. `P1-17`
19. `P1-18`
20. `P1-19`
21. `P1-20`

## Phase 1 definition of done

Phase 1 is done only when all of the following are true:

- all tasks required for Milestones A through D are marked `DONE`
- all milestone quality gates are green
- the no-trigger harness boot path works
- the synthetic trigger path works end to end
- required automated tests pass
- the progress ledger in this document is up to date
- the repository state is good enough to begin Phase 2 without re-opening Phase
  1 architecture decisions

## Next document after this phase

Once Phase 1 is complete and the progress ledger is current, the next planning
document should be the detailed implementation plan for Phase 2.
