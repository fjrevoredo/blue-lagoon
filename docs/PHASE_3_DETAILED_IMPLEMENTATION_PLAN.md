# Blue Lagoon

## Phase 3 Detailed Implementation Plan

Date: 2026-04-06
Status: Complete
Scope: High-level plan Phase 3 only
Audience: LLM-assisted implementation work and human review

## Purpose

This document defines the detailed implementation plan for Phase 3 of Blue
Lagoon.

It translates the approved Phase 3 scope from
`docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md` into concrete, trackable, and
LLM-executable work items.

Phase 3 is the first continuity-making phase behind the Phase 2 foreground
slice. Its purpose is to replace prompt-only continuity stubs with canonical
proposal, merge, memory, retrieval, and self-model flows while also adding the
first backlog-aware foreground recovery behavior for delayed pending messages.

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

If this document conflicts with the canonical documents, the canonical
documents win.

## Documentation readiness review

The Phase 3 planning baseline is ready.

The current canonical documents agree on the core Phase 3 intent:

- foreground execution begins emitting structured candidate memory and
  self-model-relevant outputs rather than only user-facing text
- the harness validates and merges proposal-based canonical writes
- memory stays episode-first, provenance-aware, and temporal or
  supersession-aware
- canonical self-model state becomes durable and queryable rather than remaining
  a seed-only prompt artifact
- the first recovery-aware pending-message backlog path is implemented at the
  harness layer rather than in the Telegram adapter

No blocking contradiction was found between
`docs/IMPLEMENTATION_DESIGN.md` and
`docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md`.

The required execution clarifications were resolved before implementation and
remained stable through completion. They are recorded in the next section so
the execution ledger stays tied to the design decisions that governed the
implementation.

## CI assessment for Phase 3

The current repository-hosted CI posture is sufficient as a minimum baseline
but no longer ideal for the Phase 3 risk surface.

The current `workspace-verification` job remains useful as the stable initial
required check created in Phase 1.1, but it now mixes multiple concerns:

- fast repository-wide verification
- PostgreSQL-backed component tests
- architecture-critical integration tests
- foreground-specific runtime regression checks

That mixed shape made sense as the initial bootstrap gate, but it is not the
best long-term fit for the explicit gate posture required by
`docs/IMPLEMENTATION_DESIGN.md`, which distinguishes:

- per-change gates for format, lint, unit, and fast component checks
- pre-merge or mainline gates for the full component suite, core integration
  suite, and migration-sensitive verification

Phase 3 should therefore reorganize CI by capability and risk while preserving
the stable baseline identity established earlier.

The Phase 3 CI posture is locked as follows:

- Keep `workspace-verification` as the existing stable fast baseline gate.
- Split persistence-critical and runtime-critical PostgreSQL-backed suites out
  of that baseline rather than continuing to grow one omnibus job.
- Use capability-based gate names rather than phase-labeled names.

The recommended stable gate names are:

- `workspace-verification`
  Fast repository-wide verification for formatting, compile checks, clippy, and
  fast unit-focused coverage that does not need PostgreSQL.
- `foreground-runtime`
  Foreground Telegram-first runtime regression coverage, including the
  foreground component and integration suites that prove the user-facing
  trigger-to-response path.
- `canonical-persistence`
  Persistence-critical migration, proposal, merge, retrieval, and canonical
  self-model regression coverage that requires PostgreSQL and directly protects
  harness-owned canonical writes.

The intended Phase 3 gate-to-suite mapping is:

- `workspace-verification`
  Run `cargo fmt --all --check`, `cargo check --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, and fast
  non-PostgreSQL unit-focused verification. This gate should not provision
  PostgreSQL.
- `foreground-runtime`
  Run the foreground Telegram-first PostgreSQL-backed suites, beginning with
  `foreground_component` and `foreground_integration`.
- `canonical-persistence`
  Run PostgreSQL-backed migration-sensitive and canonical-write-sensitive
  continuity suites, beginning with `continuity_component`,
  `continuity_integration`, and any still-relevant migration or foundation
  persistence checks that directly protect canonical schema or write safety.

Phase 3 CI reorganization should avoid redundant execution of the same named
suite in multiple jobs unless one stage intentionally reuses it for a stricter
mainline or release gate.

Phase 3 does not need to introduce later-phase gates yet, but the capability
name pattern above should remain the naming model for future expansion.

## Implementation starting point

The current repository already contains the main boundaries that Phase 3 should
extend rather than replace.

The default implementation starting points are:

- [crates/contracts/src/lib.rs](/mnt/d/Repos/blue-lagoon/crates/contracts/src/lib.rs)
  for shared proposal, merge, retrieval, and backlog-aware foreground contracts
- [crates/harness/src/config.rs](/mnt/d/Repos/blue-lagoon/crates/harness/src/config.rs)
  for typed runtime config and fail-closed validation
- [crates/harness/src/foreground.rs](/mnt/d/Repos/blue-lagoon/crates/harness/src/foreground.rs)
  for foreground persistence, ingress queries, and episode-linked repository
  logic
- [crates/harness/src/foreground_orchestration.rs](/mnt/d/Repos/blue-lagoon/crates/harness/src/foreground_orchestration.rs)
  for the main Telegram-first foreground orchestration path
- [crates/harness/src/context.rs](/mnt/d/Repos/blue-lagoon/crates/harness/src/context.rs)
  for context assembly and later retrieval-backed continuity context injection
- [crates/harness/src/self_model.rs](/mnt/d/Repos/blue-lagoon/crates/harness/src/self_model.rs)
  for the current seed-based self-model load path that Phase 3 should evolve
- [crates/harness/src/migration.rs](/mnt/d/Repos/blue-lagoon/crates/harness/src/migration.rs)
  for reviewed migration registration and migration-order expectations
- [crates/workers/src/main.rs](/mnt/d/Repos/blue-lagoon/crates/workers/src/main.rs)
  for conscious-worker protocol changes and Phase 3 structured outputs
- [crates/harness/tests/support/mod.rs](/mnt/d/Repos/blue-lagoon/crates/harness/tests/support/mod.rs)
  for disposable PostgreSQL test support and migration-backed test setup
- [config/default.toml](/mnt/d/Repos/blue-lagoon/config/default.toml)
  and [config/self_model_seed.toml](/mnt/d/Repos/blue-lagoon/config/self_model_seed.toml)
  for repository-safe defaults and bootstrap seed artifacts
- [migrations/0003__migration_metadata_normalization.sql](/mnt/d/Repos/blue-lagoon/migrations/0003__migration_metadata_normalization.sql)
  as the last reviewed migration before the Phase 3 continuity migration

## Phase 3 target

Phase 3 is complete only when Blue Lagoon proves the following:

- the conscious foreground path can emit structured proposals for canonical
  memory and self-model-relevant observations
- the harness can validate those proposals, accept or reject them, persist
  merge decisions, and apply canonical writes without giving write authority to
  workers
- accepted long-term memory artifacts, canonical retrieval-layer records, and
  canonical self-model artifacts exist in PostgreSQL behind reviewed migrations
- context assembly can retrieve accepted episodic and memory material into later
  foreground execution using a bounded canonical retrieval baseline
- the self-model used by conscious context is loaded from canonical storage when
  available, with the repository seed artifact retained only as a bootstrap path
- per-message ingress durability is preserved while the harness can switch a
  stale or degraded pending-message backlog into one recovery-aware foreground
  analysis flow
- required Phase 3 unit, component, and integration tests pass
- repository CI runs the required persistence-critical Phase 3 regression suites
  under stable gate identities

## Settled implementation clarifications

The following Phase 3 decisions are treated as settled for execution unless
later canonical documents intentionally change them:

- Phase 3 remains a foreground-centered phase. It adds canonical write and
  continuity flows behind the existing Telegram-first foreground slice, but it
  does not yet introduce the general unconscious job scheduler or broader
  background-maintenance loop from Phase 4.
- The Phase 3 proposal flow is intentionally narrow.
- The required foreground worker outputs are episodic-summary enrichment
  suitable for canonical episode completion, candidate long-term memory
  artifact proposals, and self-model-relevant observation proposals.
- User-facing response generation remains part of the same foreground episode.
- The existing `episodes` and `episode_messages` tables remain the canonical
  episodic baseline introduced in Phase 2. Phase 3 should extend or relate to
  them where needed rather than replacing them with a parallel episodic store.
- Canonical write ownership remains entirely harness-owned. Workers may emit
  structured proposals, but they must not emit direct SQL-ready mutations or
  bypass harness validation.
- Phase 3 naming should follow existing repository capability naming. The
  default canonical table names to plan around are:
  - `proposals`
  - `merge_decisions`
  - `memory_artifacts`
  - `self_model_artifacts`
  - `retrieval_artifacts`
- If implementation reveals a materially better domain-specific name, that
  change should be made deliberately and consistently before code lands rather
  than ad hoc during individual task execution.
- The canonical self-model read path changes in Phase 3. The repository-local
  seed artifact remains the bootstrap source for first initialization and local
  recovery when no canonical self-model artifact exists yet, but conscious
  context should read from PostgreSQL once canonical self-model state has been
  established.
- Phase 3 retrieval is intentionally minimal, real, and conservative. The
  required baseline is bounded harness retrieval over accepted episodic and
  memory artifacts plus the canonical retrieval-layer records needed to support
  that assembly.
- The conservative retrieval posture for Phase 3 means:
  - accepted artifacts only
  - active, non-superseded artifacts only
  - small deterministic selection bounds
  - strong relevance preference for recency, same-conversation continuity, and
    explicit lexical or identifier linkage to the active trigger
- Phase 3 retrieval should optimize first for safety, determinism, and
  stale-fact resistance rather than for maximum recall. Re-derivable embedding
  tables, vector projections, looser ranking heuristics, and richer retrieval
  maintenance remain out of scope until later phases.
- Memory artifacts introduced in Phase 3 must already support provenance,
  confidence, and explicit temporal or supersession posture. Outdated facts must
  not be overwritten destructively by default.
- The first backlog-aware foreground recovery slice is policy-driven and
  harness-owned. It activates when one conversation has multiple pending ingress
  events spanning a configured time gap or when pending work is resumed in a
  degraded or recovery condition. Channel adapters only provide normalized
  ingress data and timestamps.
- Phase 3 does not yet need the full generalized recovery-checkpoint model that
  later phases will broaden. It only needs the first recovery-aware pending
  backlog selection and foreground analysis flow described by the implementation
  design.
- CI expansion in Phase 3 should preserve the stable `workspace-verification`
  identity and add adjacent stable gate names using the locked capability-based
  names `foreground-runtime` and `canonical-persistence`.

## Phase 3 scope boundary

### In scope for Phase 3

- canonical proposal and merge flow for foreground-produced memory and
  self-model-relevant outputs
- reviewed SQL migrations for proposals, merge decisions, memory artifacts,
  self-model artifacts, and canonical retrieval-layer records
- harness validation, acceptance, rejection, and audit logging for proposal
  outcomes
- canonical self-model bootstrap and load path
- bounded retrieval assembly from accepted episodic and memory state into later
  foreground context
- pending-ingress backlog detection and one recovery-aware foreground reply path
- persistence-critical tests and CI gate expansion for canonical-write safety

### Explicitly out of scope for Phase 3

- general unconscious job scheduling, dream or reflection workers, or retrieval
  maintenance jobs
- autonomous background consolidation or contradiction scanning as a standalone
  execution domain
- approval objects, approval-resolution continuation, or side-effecting tool
  execution
- semantic embedding pipelines, vector projections, or separate retrieval
  infrastructure beyond the minimal canonical retrieval baseline
- generalized recovery checkpoints, leases, heartbeats, and broad crash-resume
  policy outside the first backlog-aware foreground slice
- multi-channel or multi-user expansion beyond the existing Telegram-first
  single-user runtime

### Deferred by later phases

- Phase 4: unconscious jobs, consolidation, contradiction detection, retrieval
  maintenance, and self-model delta generation in the background
- Phase 5: approvals, workspace, tool execution, and risk-tiered side effects
- Phase 6: generalized recovery, checkpointing, leases, broad fault handling,
  and release-grade hardening

### Execution posture confirmed for Phase 3

- foreground-first continuity implementation
- harness-sovereign canonical writes
- PostgreSQL-first canonical persistence
- episode-first memory model
- bootstrap-capable but database-backed self-model
- recovery-aware backlog handling without replaying stale replies one by one

## LLM execution rules

The plan should be executed under the following rules:

- Work one task at a time unless a task is explicitly marked as parallel-safe.
- Do not start a task until all of its dependencies are marked `DONE`.
- No core Phase 3 task is complete without the verification listed for it.
- Keep canonical data relational-first and JSONB-minimal unless a tightly scoped
  payload is materially clearer as structured JSON.
- Keep retrieval intentionally narrow. If a task begins expanding into full
  background maintenance, embeddings infrastructure, or broad recovery
  checkpointing, stop and split the work first.
- Prefer the lowest effective test layer.
- Use disposable real PostgreSQL for persistence-critical verification.
- Update this document immediately after finishing each task.

## Progress tracking protocol

This document becomes the progress ledger for Phase 3 once implementation
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
- Current active task: `none (Phase 3 complete; Phase 4 planning is next)`
- Completed tasks: `18/18`
- Milestone A status: `DONE`
- Milestone B status: `DONE`
- Milestone C status: `DONE`
- Milestone D status: `DONE`

Repository sequencing note:

- Phase 2 is complete, and this document now serves as the completed Phase 3
  execution ledger and evidence record.

## Execution refinement notes

The current task count and scope are appropriate for starting implementation,
but several tasks are integration-heavy enough that they should be treated as
likely split points if the code reveals more coupling than expected.

The current execution posture for task sizing is:

- keep the Phase 3 ledger at the current 18-task scale
- split only when a task stops being one coherent implementation unit
- preserve capability-based task names if a split becomes necessary

The most likely split candidates are:

- `P3-04` if canonical persistence services for proposals, merge history,
  memory artifacts, self-model artifacts, and retrieval-layer records do not
  remain one coherent storage pass
- `P3-07` if proposal validation logic and merge-decision recording plus audit
  plumbing stop fitting one reviewable implementation slice
- `P3-10` if retrieval selection logic and context-assembly integration start
  evolving at different speeds
- `P3-11` if normal foreground proposal persistence and canonical merge
  application need to land separately to keep regressions controlled
- `P3-13` if backlog-aware context shaping and batched reply orchestration need
  to be decoupled for safer iteration

The default new artifact names for Phase 3 implementation are:

- reviewed migration file: `migrations/0004__canonical_continuity.sql`
- PostgreSQL-backed harness component suite: `continuity_component`
- architecture-critical harness integration suite: `continuity_integration`

These names are intentionally capability-based and aligned with the existing
`foundation_*` and `foreground_*` suite pattern.

## Expected Phase 3 verification commands

These are the intended recurring verification commands for this phase. Some
will become available only after earlier tasks are complete.

- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo test -p harness --test continuity_component -- --nocapture`
- `cargo test -p harness --test foreground_component -- --nocapture`
- `cargo test -p harness --test foreground_integration -- --nocapture`
- `cargo test -p harness --test continuity_integration -- --nocapture`
- `cargo run -p runtime -- migrate`
- `cargo run -p runtime -- harness --once --idle`
- `cargo run -p runtime -- telegram --fixture <fixture-path>`
- `cargo run -p runtime -- telegram --poll-once`
- manual review that the Phase 3 migration, canonical-write, retrieval, and CI
  gate names match the documented domain language

## Phase 3 milestones

- Milestone A: canonical schema, contracts, and scope baseline
- Milestone B: proposal validation, merge, and canonical continuity writes
- Milestone C: retrieval assembly and backlog-aware foreground recovery
- Milestone D: tests, CI, docs, and completion gate

## Milestone A quality gate

Milestone A is green only if:

- the Phase 3 scope boundary and deferred behavior are explicit
- runtime config can represent the Phase 3 retrieval and backlog thresholds that
  must be operator-configurable
- reviewed SQL migrations exist for proposals, merge decisions, memory
  artifacts, self-model artifacts, and canonical retrieval-layer records
- the minimum cross-process and harness contracts exist for Phase 3 proposal
  emission, merge outcomes, retrieval assembly, and backlog-aware foreground
  triggers
- the Phase 3 CI gate split and stable capability-based gate names are defined
  before heavier persistence suites are added
- the schema and contract design preserve Phase 4 and Phase 6 expansion paths
  without requiring Phase 3 rework

## Milestone B quality gate

Milestone B is green only if:

- the conscious worker can emit structured candidate-memory and
  self-model-relevant outputs without gaining write authority
- the harness can validate, accept, reject, and audit proposal outcomes
- accepted long-term memory artifacts and self-model artifacts can be written
  canonically with provenance and explicit supersession or validity posture
- merge decisions are durably queryable and linked to proposals, traces, and
  executions
- context assembly no longer depends only on the repository seed artifact for
  self-model state when canonical state exists

## Milestone C quality gate

Milestone C is green only if:

- later foreground runs can retrieve accepted episodic and memory material
  through a bounded canonical retrieval baseline
- the retrieval layer remains distinct from re-derivable projections
- pending-message backlog detection is harness-owned and policy-aware
- one recovery-aware foreground execution can analyze an ordered delayed backlog
  without losing per-message ingress durability
- the Telegram-first runtime paths can exercise the backlog-aware foreground flow
  without pushing recovery logic down into the Telegram adapter

## Milestone D quality gate

Milestone D is green only if:

- required unit tests pass for proposal validation, merge rules, retrieval
  selection, and backlog-mode routing
- required component tests with real PostgreSQL pass for canonical memory,
  self-model, retrieval, and merge persistence
- architecture-critical integration tests pass for proposal-to-merge-to-later
  retrieval and for backlog-aware foreground recovery
- repository CI runs the required Phase 3 persistence-critical suites under
  the stable gate identities `workspace-verification`, `foreground-runtime`,
  and `canonical-persistence`
- canonical docs reflect the implemented Phase 3 boundaries and verification
  commands
- this document reflects the final task status and evidence

## Task list

### Task P3-01: Lock the Phase 3 continuity slice boundary

- Status: `DONE`
- Depends on: none
- Parallel-safe: no
- Deliverables:
  - explicit Phase 3 scope boundary covering what is in and out
  - documented clarification of the minimal Phase 3 retrieval baseline
  - documented clarification of canonical self-model bootstrap versus canonical
    read precedence
  - documented clarification of backlog-aware recovery activation rules and
    Phase 6 deferrals
- Verification:
  - manual review against `docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md`,
    `docs/IMPLEMENTATION_DESIGN.md`, `docs/REQUIREMENTS.md`, and
    `docs/LOOP_ARCHITECTURE.md`
- Evidence:
  - confirmed the settled Phase 3 scope, retrieval baseline, self-model
    precedence, and backlog-aware recovery activation rules already recorded in
    this document against `docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md`,
    `docs/IMPLEMENTATION_DESIGN.md`, `docs/REQUIREMENTS.md`, and
    `docs/LOOP_ARCHITECTURE.md` on 2026-04-06 before implementation started

### Task P3-02: Extend runtime config for retrieval and backlog-aware recovery

- Status: `DONE`
- Depends on: `P3-01`
- Parallel-safe: no
- Deliverables:
  - typed config for Phase 3 retrieval limits and selection bounds
  - typed config for backlog-threshold and degraded-recovery foreground intake
  - fail-closed validation for required new Phase 3 settings
  - operator-safe defaults in repository config
  - config posture aligned with the conservative Phase 3 retrieval baseline
- Verification:
  - unit tests for config parsing and validation
  - failed startup on invalid Phase 3 settings
- Evidence:
  - updated `crates/harness/src/config.rs`, `crates/harness/src/policy.rs`,
    `crates/harness/src/self_model.rs`,
    `crates/harness/tests/support/mod.rs`, and `config/default.toml`
  - `cmd.exe /c cargo test -p harness config:: -- --nocapture`
  - `cmd.exe /c cargo fmt --all --check`

### Task P3-03: Add reviewed SQL migration for canonical continuity tables

- Status: `DONE`
- Depends on: `P3-01`
- Parallel-safe: no
- Deliverables:
  - reviewed migration file
    `migrations/0004__canonical_continuity.sql`
  - canonical tables for proposals, merge decisions, long-term memory
    artifacts, self-model artifacts, and retrieval-layer records
  - indexes and constraints for provenance references, active-record lookup,
    supersession linkage, temporal validity, and merge lookup
  - any required pending-ingress indexes or schema refinements needed for
    backlog-aware foreground recovery
- Verification:
  - migration discovery and ordering behave correctly
  - migration applies cleanly to disposable PostgreSQL
- Execution note:
  - reuse the existing `episodes`, `episode_messages`, and `ingress_events`
    baseline where possible rather than introducing duplicate continuity tables
- Evidence:
  - added `migrations/0004__canonical_continuity.sql`
  - updated `crates/harness/src/migration.rs` and
    `crates/harness/tests/foundation_component.rs`
  - `cmd.exe /c cargo test -p harness migration:: -- --nocapture`
  - `cmd.exe /c cargo test -p harness --test foundation_component migration_application_creates_foundation_and_foreground_tables -- --nocapture`

### Task P3-04: Implement harness persistence services for canonical continuity state

- Status: `DONE`
- Depends on: `P3-03`
- Parallel-safe: no
- Deliverables:
  - harness persistence services for proposals, merge decisions, long-term
    memory artifacts, self-model artifacts, and retrieval-layer records
  - typed read and write models aligned with the new canonical tables
  - repository-facing query paths for active artifacts, superseded artifacts,
    and merge history lookup
- Verification:
  - component tests against disposable PostgreSQL for each new persistence
    service
- Execution note:
  - likely split point if proposal and merge persistence, canonical artifact
    persistence, and retrieval-layer persistence stop fitting one coherent
    storage pass
- Evidence:
  - added `crates/harness/src/continuity.rs`,
    `crates/harness/tests/continuity_component.rs`, and
    `crates/harness/src/lib.rs`
  - `cmd.exe /c cargo test -p harness continuity:: -- --nocapture`
  - `cmd.exe /c cargo test -p harness --test continuity_component -- --nocapture`
  - `cmd.exe /c cargo fmt --all --check`

### Task P3-05: Define canonical Phase 3 contracts

- Status: `DONE`
- Depends on: `P3-01`
- Parallel-safe: yes
- Deliverables:
  - shared contracts for foreground proposal emission
  - shared contracts for proposal-validation outcomes and merge decisions
  - shared contracts for retrieved memory context included in conscious
    execution
  - shared contracts for backlog-aware foreground trigger or equivalent
    recovery-analysis input shape
- Verification:
  - contract round-trip tests in `contracts`
  - manual review that contract names are capability-based rather than
    phase-labeled
- Evidence:
  - updated `crates/contracts/src/lib.rs`,
    `crates/harness/src/context.rs`,
    `crates/workers/src/main.rs`,
    `crates/workers/tests/conscious_worker_cli.rs`, and
    `crates/harness/tests/foreground_component.rs`
  - `cmd.exe /c cargo test -p contracts -- --nocapture`
  - `cmd.exe /c cargo test -p workers conscious_worker -- --nocapture`
  - `cmd.exe /c cargo fmt --all --check`

### Task P3-06: Extend the conscious worker protocol for Phase 3 proposal outputs

- Status: `DONE`
- Depends on: `P3-05`
- Parallel-safe: no
- Deliverables:
  - conscious worker result shape that can return candidate memory and
    self-model-relevant outputs in addition to the user-facing response
  - recovery-aware input shape so one foreground execution can distinguish
    normal single-message mode from delayed-backlog analysis mode
  - worker-side validation that rejects malformed Phase 3 structured outputs
- Verification:
  - worker protocol tests
  - fakeable conscious-worker round-trip tests
- Evidence:
  - updated `crates/contracts/src/lib.rs`,
    `crates/workers/src/main.rs`, and
    `crates/workers/tests/conscious_worker_cli.rs`
  - `cmd.exe /c cargo test -p workers conscious_worker -- --nocapture`
  - `cmd.exe /c cargo test -p contracts -- --nocapture`
  - `cmd.exe /c cargo fmt --all --check`

### Task P3-07: Implement harness-side proposal validation and merge decision recording

- Status: `DONE`
- Depends on: `P3-04`, `P3-05`
- Parallel-safe: no
- Deliverables:
  - proposal validation rules covering proposal kind, provenance, confidence,
    conflict posture, and allowed canonical targets
  - merge decision recording for accepted and rejected proposals
  - trace-linked audit events for proposal evaluation and merge outcomes
  - fail-closed handling for invalid or conflicting proposal payloads
- Verification:
  - unit tests for validation and merge-rule logic
  - component tests proving accepted and rejected outcomes are durably recorded
- Execution note:
  - likely split point if pure validation logic and merge-recording plus audit
    integration need to land separately
- Evidence:
  - added `crates/harness/src/proposal.rs`
  - updated `crates/harness/src/lib.rs`,
    `crates/harness/src/continuity.rs`, and
    `crates/harness/tests/continuity_component.rs`
  - `cmd.exe /c cargo test -p harness proposal:: -- --nocapture`
  - `cmd.exe /c cargo test -p harness --test continuity_component -- --nocapture`
  - `cmd.exe /c cargo fmt --all --check`

### Task P3-08: Implement canonical self-model bootstrap and read path

- Status: `DONE`
- Depends on: `P3-04`, `P3-07`
- Parallel-safe: no
- Deliverables:
  - canonical self-model artifact persistence model
  - bootstrap path that can seed canonical self-model state from the repository
    seed artifact when canonical state is absent
  - self-model read logic that prefers canonical PostgreSQL state for conscious
    context assembly once available
  - clear fail-closed behavior for invalid or missing canonical self-model state
- Verification:
  - unit tests for bootstrap precedence and invalid-state handling
  - component tests with real PostgreSQL for seeded and already-canonical cases
- Evidence:
  - updated `crates/harness/src/self_model.rs`,
    `crates/harness/src/context.rs`,
    `crates/harness/src/foreground_orchestration.rs`,
    `crates/harness/src/continuity.rs`,
    `crates/harness/tests/continuity_component.rs`, and
    `crates/harness/tests/foreground_component.rs`
  - `cmd.exe /c cargo test -p harness self_model:: -- --nocapture`
  - `cmd.exe /c cargo test -p harness context:: -- --nocapture`
  - `cmd.exe /c cargo test -p harness --test continuity_component -- --nocapture`
  - `cmd.exe /c cargo fmt --all --check`

### Task P3-09: Implement canonical long-term memory artifact merge rules

- Status: `DONE`
- Depends on: `P3-04`, `P3-07`
- Parallel-safe: no
- Deliverables:
  - accepted long-term memory artifact model with provenance, confidence,
    canonical status, and validity or supersession fields
  - merge rules for add, revise, supersede, and reject behavior
  - non-destructive handling for stale or conflicting facts
  - artifact queries that support later foreground retrieval
- Verification:
  - unit tests for supersession and temporal-validity decisions
  - component tests proving canonical active-artifact selection works
- Evidence:
  - added `crates/harness/src/memory.rs`
  - updated `crates/harness/src/continuity.rs`,
    `crates/harness/src/lib.rs`, and
    `crates/harness/tests/continuity_component.rs`
  - `cmd.exe /c cargo test -p harness memory:: -- --nocapture`
  - `cmd.exe /c cargo test -p harness --test continuity_component -- --nocapture`
  - `cmd.exe /c cargo fmt --all --check`

### Task P3-10: Implement the minimal canonical retrieval baseline for context assembly

- Status: `DONE`
- Depends on: `P3-02`, `P3-04`, `P3-08`, `P3-09`
- Parallel-safe: no
- Deliverables:
  - retrieval selection logic over accepted episodic and memory artifacts
  - canonical retrieval-layer records needed to support deterministic bounded
    foreground retrieval
  - context-assembly changes that inject retrieved memory material alongside the
    canonical self-model snapshot and recent episode history
  - conservative ranking and filtering behavior that prioritizes active
    non-superseded artifacts, recency, same-conversation continuity, and strong
    explicit relevance over wide recall
  - explicit distinction between canonical retrieval state and re-derivable
    projections
- Verification:
  - unit tests for retrieval ranking and bounds
  - component tests for retrieval-backed context assembly against real PostgreSQL
- Execution note:
  - likely split point if retrieval selection and ranking logic should land
    before the context-assembly integration
- Evidence:
  - added `crates/harness/src/retrieval.rs`
  - updated `crates/harness/src/context.rs`,
    `crates/harness/src/lib.rs`, and
    `crates/harness/tests/foreground_component.rs`
  - `cmd.exe /c cargo test -p harness retrieval:: -- --nocapture`
  - `cmd.exe /c cargo test -p harness --test foreground_component context_assembly_v0_loads_seed_and_bounded_recent_history -- --nocapture`
  - `cmd.exe /c cargo test -p harness --test foreground_component context_assembly_injects_retrieved_episode_and_memory_context -- --nocapture`
  - `cmd.exe /c cargo fmt --all --check`

### Task P3-11: Integrate proposal emission and merge flow into normal foreground orchestration

- Status: `DONE`
- Depends on: `P3-06`, `P3-07`, `P3-08`, `P3-09`
- Parallel-safe: no
- Deliverables:
  - normal foreground orchestration path that persists worker-emitted proposals
  - canonical merge execution during or immediately after foreground completion
  - episode completion updates linked to proposal and merge outcomes
  - audit emission for proposal creation, merge decision, and canonical write
    application
- Verification:
  - component tests for the normal foreground proposal-to-merge flow
- Execution note:
  - likely split point if foreground proposal persistence should land before
    merge application and episode-finalization updates
- Evidence:
  - updated [crates/harness/src/foreground_orchestration.rs](/mnt/d/Repos/blue-lagoon/crates/harness/src/foreground_orchestration.rs),
    [crates/harness/src/memory.rs](/mnt/d/Repos/blue-lagoon/crates/harness/src/memory.rs),
    [crates/harness/src/self_model.rs](/mnt/d/Repos/blue-lagoon/crates/harness/src/self_model.rs), and
    [crates/harness/tests/foreground_component.rs](/mnt/d/Repos/blue-lagoon/crates/harness/tests/foreground_component.rs)
    to persist worker-emitted proposals, execute canonical merges, update
    episode completion summaries with proposal outcomes, and emit canonical
    write audit events only for accepted writes
  - `cmd.exe /c cargo test -p harness --test foreground_component foreground_orchestration_runs_from_ingress_to_delivery -- --nocapture`
  - `cmd.exe /c cargo test -p harness --test foreground_component foreground_persistence_writes_bindings_and_ingress_events -- --nocapture`
  - `cmd.exe /c cargo test -p harness --test continuity_component -- --nocapture`
  - `cmd.exe /c cargo fmt --all --check`

### Task P3-12: Implement pending-ingress backlog detection and recovery trigger shaping

- Status: `DONE`
- Depends on: `P3-02`, `P3-03`, `P3-05`
- Parallel-safe: no
- Deliverables:
  - pending-ingress selection for one conversation using ordered timestamps
  - backlog-threshold evaluation and degraded-runtime or recovery-mode trigger
    shaping
  - durable linkage between the recovery-aware execution and the individual
    ingress events it analyzes
  - audit emission for backlog-mode activation decisions
- Verification:
  - unit tests for threshold and routing decisions
  - component tests with real PostgreSQL for pending-ingress selection
- Evidence:
  - updated [crates/harness/src/foreground.rs](/mnt/d/Repos/blue-lagoon/crates/harness/src/foreground.rs),
    [crates/harness/src/foreground_orchestration.rs](/mnt/d/Repos/blue-lagoon/crates/harness/src/foreground_orchestration.rs), and
    [crates/harness/tests/foreground_component.rs](/mnt/d/Repos/blue-lagoon/crates/harness/tests/foreground_component.rs)
    with backlog-aware pending-ingress planning, threshold evaluation,
    `execution_ingress_links` persistence, ingress foreground-state transitions,
    and recovery-mode audit emission
  - `cmd.exe /c cargo test -p harness foreground:: -- --nocapture`
  - `cmd.exe /c cargo test -p harness --test foreground_component pending_foreground_execution_switches_to_backlog_recovery_and_links_selected_ingress -- --nocapture`
  - `cmd.exe /c cargo test -p harness --test foreground_component pending_foreground_execution_stays_single_when_backlog_threshold_is_not_met -- --nocapture`
  - `cmd.exe /c cargo test -p harness --test foreground_component foreground_orchestration_runs_from_ingress_to_delivery -- --nocapture`
  - `cmd.exe /c cargo test -p harness --test foreground_component accepted_foreground_trigger_persists_execution_budget_and_audit -- --nocapture`
  - `cmd.exe /c cargo test -p harness --test foreground_component rejected_foreground_trigger_persists_rejection_and_audit -- --nocapture`
  - `cmd.exe /c cargo fmt --all --check`

### Task P3-13: Implement recovery-aware foreground context assembly and batched reply flow

- Status: `DONE`
- Depends on: `P3-06`, `P3-10`, `P3-12`
- Parallel-safe: no
- Deliverables:
  - context assembly that can include ordered delayed-ingress backlog material
  - conscious worker prompt or structured context markers that distinguish
    backlog recovery analysis from normal foreground execution
  - one recovery-aware foreground completion path that analyzes the backlog as a
    batch and emits one policy-appropriate reply or clarification outcome
  - preservation of per-message ingress durability without naive sequential
    replay
- Verification:
  - component tests for backlog-aware context shaping and single-reply outcome
- Execution note:
  - likely split point if backlog-aware context shaping should land before the
    final batched reply orchestration path
- Evidence:
  - updated [crates/harness/src/context.rs](/mnt/d/Repos/blue-lagoon/crates/harness/src/context.rs),
    [crates/harness/src/foreground.rs](/mnt/d/Repos/blue-lagoon/crates/harness/src/foreground.rs),
    [crates/harness/src/foreground_orchestration.rs](/mnt/d/Repos/blue-lagoon/crates/harness/src/foreground_orchestration.rs), and
    [crates/harness/tests/foreground_component.rs](/mnt/d/Repos/blue-lagoon/crates/harness/tests/foreground_component.rs)
    to carry recovery context through context assembly, load persisted ingress
    into planned execution, run one batched backlog-aware foreground episode,
    and mark every selected ingress processed after one reply
  - `cmd.exe /c cargo test -p harness context:: -- --nocapture`
  - `cmd.exe /c cargo test -p harness --test foreground_component planned_foreground_orchestration_processes_backlog_batch_with_single_reply -- --nocapture`
  - `cmd.exe /c cargo test -p harness --test foreground_component foreground_orchestration_runs_from_ingress_to_delivery -- --nocapture`
  - `cmd.exe /c cargo fmt --all --check`

### Task P3-14: Extend runtime Telegram execution paths for backlog-aware processing

- Status: `DONE`
- Depends on: `P3-02`, `P3-11`, `P3-12`, `P3-13`
- Parallel-safe: no
- Deliverables:
  - runtime fixture and poll-once execution paths that can exercise backlog-aware
    foreground recovery behavior
  - operator-visible summaries or audit evidence showing when backlog mode was
    activated
  - channel-adapter boundaries kept transport-only while harness orchestration
    owns recovery-mode decisions
- Verification:
  - fixture-driven runtime checks using a multi-message backlog scenario
  - manual review that Telegram adapter files do not own Phase 3 recovery policy
- Evidence:
  - updated [crates/harness/src/runtime.rs](/mnt/d/Repos/blue-lagoon/crates/harness/src/runtime.rs),
    [crates/harness/src/foreground.rs](/mnt/d/Repos/blue-lagoon/crates/harness/src/foreground.rs),
    [crates/harness/tests/foreground_integration.rs](/mnt/d/Repos/blue-lagoon/crates/harness/tests/foreground_integration.rs), and
    [crates/harness/tests/fixtures/telegram/private_text_backlog_batch.json](/mnt/d/Repos/blue-lagoon/crates/harness/tests/fixtures/telegram/private_text_backlog_batch.json)
    to stage accepted Telegram ingress, plan one execution per conversation,
    route fixture/runtime batches through the planned foreground path, and
    expose backlog recovery activation in runtime summaries
  - `cmd.exe /c cargo test -p harness --test foreground_integration telegram_fixture_runtime_run_persists_response_and_trace_linked_audit -- --nocapture`
  - `cmd.exe /c cargo test -p harness --test foreground_integration telegram_fixture_runtime_batch_activates_backlog_recovery -- --nocapture`
  - `cmd.exe /c cargo test -p harness --test foreground_integration telegram_fixture_runtime_duplicate_ingress_is_idempotent_and_audited -- --nocapture`
  - manual review confirmed the Telegram adapter in [crates/harness/src/telegram.rs](/mnt/d/Repos/blue-lagoon/crates/harness/src/telegram.rs) remains transport-only while recovery decisions live in [crates/harness/src/runtime.rs](/mnt/d/Repos/blue-lagoon/crates/harness/src/runtime.rs) and [crates/harness/src/foreground.rs](/mnt/d/Repos/blue-lagoon/crates/harness/src/foreground.rs)
  - `cmd.exe /c cargo fmt --all --check`

### Task P3-15: Add unit coverage for proposal, merge, retrieval, and backlog policy logic

- Status: `DONE`
- Depends on: `P3-07`, `P3-09`, `P3-10`, `P3-12`
- Parallel-safe: no
- Deliverables:
  - deterministic unit tests for proposal validation
  - deterministic unit tests for merge decisions and supersession handling
  - deterministic unit tests for retrieval scoring or selection bounds
  - deterministic unit tests for backlog-threshold activation and routing
- Verification:
  - `cargo test` at the smallest relevant crate or module scope
- Evidence:
  - unit coverage is present in [crates/harness/src/proposal.rs](/mnt/d/Repos/blue-lagoon/crates/harness/src/proposal.rs),
    [crates/harness/src/memory.rs](/mnt/d/Repos/blue-lagoon/crates/harness/src/memory.rs),
    [crates/harness/src/retrieval.rs](/mnt/d/Repos/blue-lagoon/crates/harness/src/retrieval.rs), and
    [crates/harness/src/foreground.rs](/mnt/d/Repos/blue-lagoon/crates/harness/src/foreground.rs)
  - `cmd.exe /c cargo test -p harness proposal:: -- --nocapture`
  - `cmd.exe /c cargo test -p harness memory:: -- --nocapture`
  - `cmd.exe /c cargo test -p harness retrieval:: -- --nocapture`
  - `cmd.exe /c cargo test -p harness foreground:: -- --nocapture`

### Task P3-16: Add PostgreSQL-backed component coverage for canonical continuity services

- Status: `DONE`
- Depends on: `P3-11`, `P3-13`
- Parallel-safe: no
- Deliverables:
  - harness component suite
    `crates/harness/tests/continuity_component.rs`
  - component tests for proposal persistence and merge history
  - component tests for canonical self-model bootstrap and read behavior
  - component tests for accepted memory artifact queries and retrieval-backed
    context assembly
  - component tests for pending-ingress backlog selection and linkage
- Verification:
  - disposable PostgreSQL component suites under the harness test support
- Evidence:
  - component coverage is present in [crates/harness/tests/continuity_component.rs](/mnt/d/Repos/blue-lagoon/crates/harness/tests/continuity_component.rs) and
    [crates/harness/tests/foreground_component.rs](/mnt/d/Repos/blue-lagoon/crates/harness/tests/foreground_component.rs)
    for proposal persistence, merge history, canonical self-model bootstrap/read,
    retrieval-backed context assembly, and pending-ingress backlog linkage
  - `cmd.exe /c cargo test -p harness --test continuity_component -- --nocapture`
  - `cmd.exe /c cargo test -p harness --test foreground_component pending_foreground_execution_switches_to_backlog_recovery_and_links_selected_ingress -- --nocapture`
  - `cmd.exe /c cargo test -p harness --test foreground_component planned_foreground_orchestration_processes_backlog_batch_with_single_reply -- --nocapture`

### Task P3-17: Add architecture-critical integration tests for Phase 3 continuity flows

- Status: `DONE`
- Depends on: `P3-14`, `P3-16`
- Parallel-safe: no
- Deliverables:
  - harness integration suite
    `crates/harness/tests/continuity_integration.rs`
  - integration test for normal foreground proposal emission to merge to later
    retrieval in a subsequent episode
  - integration test for canonical self-model persistence affecting later
    conscious context
  - integration test for backlog-aware delayed-ingress foreground recovery with
    per-message durability preserved
  - migration-sensitive regression coverage for the new canonical tables
- Verification:
  - `cargo test -p harness --test continuity_integration -- --nocapture`
- Evidence:
  - added [crates/harness/tests/continuity_integration.rs](/mnt/d/Repos/blue-lagoon/crates/harness/tests/continuity_integration.rs)
    plus the supporting fixtures
    [crates/harness/tests/fixtures/telegram/private_preference_message.json](/mnt/d/Repos/blue-lagoon/crates/harness/tests/fixtures/telegram/private_preference_message.json),
    [crates/harness/tests/fixtures/telegram/private_preference_followup.json](/mnt/d/Repos/blue-lagoon/crates/harness/tests/fixtures/telegram/private_preference_followup.json), and
    [crates/harness/tests/fixtures/telegram/private_text_backlog_batch.json](/mnt/d/Repos/blue-lagoon/crates/harness/tests/fixtures/telegram/private_text_backlog_batch.json)
    for proposal-to-merge-to-later-retrieval, self-model carry-forward, and
    backlog-aware recovery integration coverage
  - `cmd.exe /c cargo test -p harness --test continuity_integration -- --nocapture`
  - `cmd.exe /c cargo test -p harness --test foreground_integration -- --nocapture`

### Task P3-18: Extend repository CI and canonical docs for the Phase 3 gate

- Status: `DONE`
- Depends on: `P3-15`, `P3-16`, `P3-17`
- Parallel-safe: no
- Deliverables:
  - repository-hosted CI updates for the required Phase 3 persistence-critical
    suites
  - stable gate naming that extends rather than replaces the current baseline,
    using `workspace-verification`, `foreground-runtime`, and
    `canonical-persistence`
  - canonical doc updates for Phase 3 verification commands, active next phase
    status, and any new operator checks
  - this document updated as the Phase 3 execution ledger during implementation
- Verification:
  - manual review of workflow trigger and job names
  - manual review that the gate-to-suite mapping avoids unnecessary duplicate
    execution across `workspace-verification`, `foreground-runtime`, and
    `canonical-persistence`
  - successful local verification of the documented Phase 3 command set
- Evidence:
  - updated [`.github/workflows/ci.yml`](/mnt/d/Repos/blue-lagoon/.github/workflows/ci.yml)
    to preserve `workspace-verification` as the fast baseline gate and add the
    PostgreSQL-backed `foreground-runtime` and `canonical-persistence` jobs
    with the Phase 3 suite split defined by this plan
  - updated
    [`docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md`](/mnt/d/Repos/blue-lagoon/docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md)
    to mark Phase 3 complete and Phase 4 as the active next phase
  - updated
    [`docs/PHASE_3_DETAILED_IMPLEMENTATION_PLAN.md`](/mnt/d/Repos/blue-lagoon/docs/PHASE_3_DETAILED_IMPLEMENTATION_PLAN.md)
    to record the completed Phase 3 ledger, evidence, and milestone state
  - manual review confirmed the workflow exposes the intended stable gate names
    `workspace-verification`, `foreground-runtime`, and
    `canonical-persistence`
  - manual review confirmed the gate-to-suite mapping avoids unnecessary
    duplicate execution by keeping format, check, clippy, and library-unit
    coverage in `workspace-verification`, foreground runtime regression suites
    in `foreground-runtime`, and canonical continuity suites in
    `canonical-persistence`
  - `cmd.exe /c cargo fmt --all --check`
  - `cmd.exe /c cargo check --workspace`
  - `cmd.exe /c cargo clippy --workspace --all-targets -- -D warnings`
  - `cmd.exe /c cargo test --workspace --lib -- --nocapture`
  - `cmd.exe /c cargo test -p harness --test foreground_component -- --nocapture`
  - `cmd.exe /c cargo test -p harness --test foreground_integration -- --nocapture`
  - `cmd.exe /c cargo test -p harness --test continuity_component -- --nocapture`
  - `cmd.exe /c cargo test -p harness --test continuity_integration -- --nocapture`
