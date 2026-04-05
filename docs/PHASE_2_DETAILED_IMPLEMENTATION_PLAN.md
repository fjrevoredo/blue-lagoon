# Blue Lagoon

## Phase 2 Detailed Implementation Plan

Date: 2026-04-05
Status: Initial draft for iteration; ready for execution
Scope: High-level plan Phase 2 only
Audience: LLM-assisted implementation work and human review

## Purpose

This document defines the detailed implementation plan for Phase 2 of Blue
Lagoon.

It translates the approved Phase 2 scope from
`docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md` into concrete, trackable, and
LLM-executable work items.

Phase 2 is the first real foreground vertical slice. Its purpose is to add a
traceable user-facing path from Telegram input to persisted foreground episode
and Telegram response beyond the retained Phase 1 synthetic path, while
preserving harness sovereignty and keeping deeper memory, approval, and
background-job systems deferred to later phases.

## Canonical inputs

This plan is subordinate to the following canonical documents:

- `PHILOSOPHY.md`
- `docs/REQUIREMENTS.md`
- `docs/LOOP_ARCHITECTURE.md`
- `docs/IMPLEMENTATION_DESIGN.md`
- `docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md`
- `docs/PHASE_1_DETAILED_IMPLEMENTATION_PLAN.md`
- `docs/PHASE_1_1_DETAILED_IMPLEMENTATION_PLAN.md`

If this document conflicts with the canonical documents, the canonical
documents win.

## Phase 2 target

Phase 2 is complete only when Blue Lagoon has a minimal foreground runtime that
proves the following:

- a private 1:1 Telegram user message can enter the runtime through a
  transport-only adapter
- the harness can normalize ingress, validate the trigger, initialize budgets,
  and create a tracked conscious execution
- the harness can assemble bounded conscious context from the active trigger, a
  compact self-model seed, a current internal-state snapshot, and bounded recent
  episode history
- a conscious worker can participate in the foreground path without owning
  policy, provider logic, tool execution, or canonical writes
- a harness-owned model gateway can execute one provider-routed foreground model
  call through provider-agnostic internal contracts
- the foreground path can persist ingress, episode, output, execution, and
  audit data with trace linkage
- the Telegram adapter can deliver the resulting user-facing response back to
  the same conversation
- required Phase 2 unit, component, and integration tests pass
- the relevant foreground-path suites are wired into repository CI

## Settled implementation clarifications

The following Phase 2 decisions are treated as settled unless later canonical
documents intentionally change them:

- Phase 2 implements the first reactive foreground slice only. It does not yet
  implement approvals, tool execution, proactive notifications, approved wake
  handling, or the broader Phase 3 canonical memory-and-merge system.
- Private 1:1 Telegram chat is the only supported production conversation mode
  in Phase 2. Groups, channels, and multi-party behavior remain out of scope.
- The Telegram adapter remains transport-focused. It may receive updates,
  normalize inbound events, deliver outbound messages, and map Telegram
  identifiers to internal references, but it must not own policy, trigger
  semantics, approval logic, or business workflow logic.
- The normalized ingress contract should already contain optional fields for
  channel kind, external event and message identifiers, event kind,
  occurred-at timestamp, text body where present, reply linkage, attachment
  references, command hints, approval payloads, and raw payload references, but
  the required end-to-end Phase 2 execution path only needs plain text user
  input and plain text assistant output.
- Phase 2 remains strictly single-user first. Unsupported actors or
  conversations must fail closed and be auditable rather than being accepted by
  default.
- The first internal principal reference may be a configured single-user
  identity rather than a full multi-user identity-management system.
- Context assembly v0 is intentionally narrow. It should include the current
  trigger, a compact self-model seed, a current internal-state snapshot, and a
  bounded recent foreground history slice. Canonical long-term memory retrieval,
  retrieval artifacts, and proposal-based memory merges remain Phase 3 work.
- The initial self-model used in Phase 2 may come from versioned local config or
  another repository-local seed artifact combined with runtime internal state.
  Canonical self-model artifact storage remains Phase 3 work.
- The model gateway remains harness-owned from the first real foreground path.
  Workers must not own provider-specific logic, direct provider network access,
  or routing authority.
- The first concrete provider adapter implemented for Phase 2 will target
  `z.ai`, but it must sit behind provider-agnostic harness-owned model-gateway
  contracts so later provider additions do not require architectural rework.
- The minimal conscious execution path may use a single model-request cycle per
  foreground episode. Multi-step tool-mediated execution remains later work.
- Phase 2 episode persistence is a foreground continuity baseline, not the full
  canonical proposal-and-merge memory system. Episode records should be durable
  and useful for later context assembly, but candidate-memory and self-model
  proposal flows remain deferred.
- Required automated tests must not require live Telegram or live model-provider
  networks. Telegram transport and provider behavior should normally be stubbed
  or faked in required suites, with real-network checks remaining optional
  operator verification.
- Phase 2 CI expansion should extend the stable Phase 1.1 baseline or add
  adjacent stable gates without renaming or replacing the existing
  `workspace-verification` identity.

## Phase 2 scope boundary

### In scope for Phase 2

- one reactive foreground path only
- private 1:1 Telegram text ingress and plain-text Telegram egress
- single configured principal and conversation binding for the initial user
- normalized ingress, trigger validation, deduplication, policy checks, and
  foreground budget initialization
- compact self-model seeding plus internal-state snapshot injection into
  conscious context
- bounded recent foreground episode retrieval for context assembly v0
- one conscious worker cycle that can request one harness-mediated model call
- one harness-owned provider-agnostic model gateway with one concrete `z.ai`
  adapter
- persistence for conversation bindings, ingress events, episodes, episode
  messages, execution linkage, and trace-linked audit events
- fakeable Telegram and provider boundaries for required automated tests

### Explicitly out of scope for Phase 2

- canonical long-term memory artifacts, retrieval artifacts, merge workflows, or
  self-model artifact persistence beyond the initial runtime seed path
- background jobs, wake-signal conversion, scheduled foreground tasks, or other
  proactive behavior beyond what is already documented as deferred
- approval objects, approval rendering, approval-resolution execution, or any
  user-authorized side-effect flow
- tool execution, tool-call approval, or tool-result observation beyond model
  output generation
- group chats, Telegram channels, multi-party semantics, or multi-user identity
  management
- rich attachment processing beyond normalized attachment references with basic
  metadata fields
- multi-step conscious orchestration, multi-turn tool mediation, or autonomous
  replanning loops beyond the single foreground model cycle

### Deferred by later phases

- Phase 3: canonical memory artifacts, retrieval structures, self-model
  artifacts, and proposal-and-merge flows
- Phase 4: bounded background jobs, maintenance triggers, consolidation, and
  wake-signal production or handling
- Phase 5: approval flows, risk-tiered tool execution, and richer policy-driven
  external actions

### Execution posture confirmed for Phase 2

- reactive only
- single-user first
- Telegram-first
- harness-sovereign
- provider-agnostic internally even though `z.ai` is first
- fakeable at transport and provider boundaries

## LLM execution rules

The plan should be executed under the following rules:

- Work one task at a time unless a task is explicitly marked as parallel-safe.
- Do not start a task until all of its dependencies are marked `DONE`.
- No core task is complete without the verification listed for it.
- Keep the Phase 2 implementation narrow. If a task starts pulling in Phase 3,
  Phase 4, or Phase 5 behavior, split the work and update this document first.
- Prefer the lowest effective test layer.
- Use disposable real PostgreSQL for persistence-critical verification.
- Keep Telegram and provider integrations fakeable by design.
- Update this document immediately after finishing each task.

## Progress tracking protocol

This document is the progress ledger for Phase 2.

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
4. If the task changed execution order or narrowed scope further, update
   dependent tasks before moving on.

## Progress snapshot

- Current milestone: `MILESTONE C`
- Current active task: `P2-12`
- Completed tasks: `11/18`
- Milestone A status: `DONE`
- Milestone B status: `DONE`
- Milestone C status: `IN PROGRESS`
- Milestone D status: `TODO`

Latest verification state:

- Phase 2 state has been self-checked after `P2-06` and `P2-11`
  implementation.
- `cmd.exe /c cargo fmt --all --check` is green.
- `cmd.exe /c cargo check --workspace` is green.
- `cmd.exe /c cargo test -p contracts -- --nocapture` is green.
- `cmd.exe /c cargo test -p harness -- --nocapture` is green.
- `cmd.exe /c cargo test -p workers -- --nocapture` is green.
- `cmd.exe /c cargo test --workspace` is green.

Latest review corrections already applied:

- invalid Telegram timestamps now fail closed rather than being rewritten
- normalized ingress persistence now retains attachment metadata, command args,
  and approval callback payloads
- conscious worker requests now reuse one consistent request ID and timestamp
  across wrapper and payload
- task status and evidence in this document have been reconciled with the code
  state

Repository sequencing note:

- Phase 1.1 is complete, so this document is now the active next execution
  ledger for implementation work.

## Expected Phase 2 verification commands

These are the intended recurring verification commands for this phase. Some
will become available only after earlier tasks are complete.

- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo run -p runtime -- migrate`
- `cargo run -p runtime -- harness --once --idle`
- `cargo run -p runtime -- telegram --fixture <fixture-path>`
- `cargo run -p runtime -- telegram --poll-once`
- manual review that the Telegram adapter, provider adapter, and CI gate names
  match the documented Phase 2 surface

## Phase 2 milestones

- Milestone A: foreground contracts, config, and schema baseline
- Milestone B: Telegram ingress and trigger intake
- Milestone C: conscious orchestration and model gateway
- Milestone D: tests, CI, docs, and completion gate

## Milestone A quality gate

Milestone A is green only if:

- the Phase 2 foreground scope and deferred boundaries are explicit
- the runtime config surface can represent Telegram and model-gateway settings
- reviewed SQL migration files exist for the minimum Phase 2 foreground
  persistence baseline
- the minimum Phase 2 contracts exist for normalized ingress, foreground
  triggers, conscious context, model calls, and conscious worker results
- persistence-critical Phase 2 schema additions compile against the current
  harness code and can be exercised in tests

## Milestone B quality gate

Milestone B is green only if:

- Telegram payloads can be normalized into the canonical ingress contract
- unsupported Telegram events, users, or conversation modes fail closed
- identifier mapping and ingress deduplication exist for the minimum foreground
  path
- accepted Telegram ingress becomes a harness-owned foreground trigger with
  minimal policy evaluation, budgets, and trace linkage
- the adapter remains transport-focused rather than accumulating core business
  logic

## Milestone C quality gate

Milestone C is green only if:

- the harness can assemble bounded foreground context v0
- the conscious worker path remains isolated and structured
- the harness-owned model gateway can serve the conscious path through one
  provider adapter
- the foreground path persists ingress, episode, message, output, execution,
  and audit artifacts with linked identifiers
- the worker does not own tool execution, provider routing, policy authority, or
  canonical writes

## Milestone D quality gate

Milestone D is green only if:

- required unit tests pass for normalization, validation, budgeting, context
  assembly, and model-gateway logic
- required component tests with real PostgreSQL pass for Phase 2 persistence and
  orchestration paths
- architecture-critical foreground integration tests pass for normalized
  Telegram input to persisted response and audit trail
- local command-surface documentation matches the implemented runtime
  entrypoints
- repository CI runs the relevant Phase 2 foreground regression suites under
  stable check identities
- this document reflects the final task status and evidence

## Task list

### Task P2-01: Lock the Phase 2 foreground slice boundary

- Status: `DONE`
- Depends on: none
- Parallel-safe: no
- Deliverables:
  - explicit Phase 2 scope boundary covering what is in and out
  - documented deferrals for Phase 3 memory merges, Phase 4 background jobs, and
    Phase 5 approvals or tool execution
  - confirmation that Phase 2 remains reactive, single-user first, and
    Telegram-first
- Verification:
  - manual review against `docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md`,
    `docs/IMPLEMENTATION_DESIGN.md`, and `docs/REQUIREMENTS.md`
- Evidence:
  - `docs/PHASE_2_DETAILED_IMPLEMENTATION_PLAN.md` now includes an explicit
    `Phase 2 scope boundary` section covering in-scope behavior, out-of-scope
    behavior, and deferrals to Phases 3, 4, and 5; settled clarifications now
    record `z.ai` as the first concrete provider behind a provider-agnostic
    harness-owned model gateway. Manual review completed against
    `docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md`,
    `docs/IMPLEMENTATION_DESIGN.md`, and `docs/REQUIREMENTS.md`.

### Task P2-02: Extend runtime config and secret-loading surface for foreground execution

- Status: `DONE`
- Depends on: `P2-01`
- Parallel-safe: no
- Deliverables:
  - typed config for Telegram adapter settings
  - typed config for the initial model-gateway routing settings
  - typed config or seed artifact loading for the initial self-model seed
  - fail-closed validation for required foreground secrets and identifiers
- Verification:
  - unit tests for config parsing and validation
  - failed startup on missing critical Phase 2 settings
- Evidence:
  - Added typed optional Phase 2 config sections and fail-closed foreground
    accessors in [crates/harness/src/config.rs](/mnt/d/Repos/blue-lagoon/crates/harness/src/config.rs)
    for Telegram transport settings, `z.ai` model-gateway routing settings, and
    self-model seed artifact resolution; added versioned non-secret config
    surface in [config/default.toml](/mnt/d/Repos/blue-lagoon/config/default.toml)
    and initial seed artifact in [config/self_model_seed.toml](/mnt/d/Repos/blue-lagoon/config/self_model_seed.toml).
    Verified with `cmd.exe /c cargo test -p harness config -- --nocapture`,
    `cmd.exe /c cargo fmt --all --check`, and
    `cmd.exe /c cargo check --workspace`.

### Task P2-03: Add reviewed SQL migration for Phase 2 foreground persistence tables

- Status: `DONE`
- Depends on: `P2-01`
- Parallel-safe: no
- Deliverables:
  - next reviewed migration file under `migrations/`
  - minimum tables for `conversation_bindings`, `ingress_events`, `episodes`,
    and `episode_messages` or equivalent approved names
  - minimum unique constraints and indexes for Telegram external identifiers,
    execution linkage, and per-episode ordering
  - episode-storage fields sufficient for the Phase 2 foreground slice,
    including trigger source, timestamps, trace or execution linkage, message
    ordering, and outcome-oriented foreground records
  - schema additions kept intentionally narrow so Phase 3 can add canonical
    memory artifacts without reworking the Phase 2 vertical slice
- Verification:
  - migration discovery and ordering behave correctly
  - migration applies cleanly to disposable PostgreSQL
- Evidence:
  - Added reviewed foreground migration
    `migrations/0002__phase_2_foreground.sql` with `conversation_bindings`,
    `ingress_events`, `episodes`, and `episode_messages` plus unique
    constraints and indexes for external identifiers, execution linkage, and
    per-episode ordering. The reviewed Phase 2 ingress schema now retains
    normalized attachment metadata, command args, and approval callback payload
    fields in addition to the baseline identifier, status, and text fields.
    Updated migration expectations in
    `crates/harness/src/migration.rs`, reset support in
    `crates/harness/tests/support/mod.rs`, and PostgreSQL-backed migration
    verification in `crates/harness/tests/phase1_component.rs`. Verified with
    `cmd.exe /c cargo test -p harness migration -- --nocapture`,
    `cmd.exe /c cargo test -p harness phase1_component -- --nocapture`,
    `cmd.exe /c cargo fmt --all --check`, and
    `cmd.exe /c cargo check --workspace`.

### Task P2-04: Implement harness persistence services for the new foreground tables

- Status: `DONE`
- Depends on: `P2-03`
- Parallel-safe: no
- Deliverables:
  - repository or service-layer writes for conversation bindings
  - repository or service-layer writes for ingress-event persistence
  - repository or service-layer writes for episode and message persistence
  - read helpers needed for bounded recent-episode retrieval
- Verification:
  - component tests against disposable PostgreSQL
- Evidence:
  - Added foreground persistence services in
    `crates/harness/src/foreground.rs` for conversation-binding upsert,
    ingress-event persistence and retrieval, episode and episode-message
    persistence, and bounded recent-history reads for context assembly.
    PostgreSQL-backed coverage lives in
    `crates/harness/tests/phase2_component.rs`, including retention of
    normalized attachment metadata, command args, and approval callback
    payloads. Verified with
    `cmd.exe /c cargo test -p harness --test phase2_component -- --nocapture`,
    `cmd.exe /c cargo fmt --all --check`, and
    `cmd.exe /c cargo check --workspace`; broader regression confirmed with
    `cmd.exe /c cargo test --workspace`.

### Task P2-05: Define canonical Phase 2 foreground contracts

- Status: `DONE`
- Depends on: `P2-01`
- Parallel-safe: yes
- Deliverables:
  - normalized ingress contract
  - foreground-trigger contract
  - conscious-context contract
  - model-call request and response contracts
  - conscious-worker request and response contracts
  - explicit support in the normalized ingress contract for channel kind,
    Telegram external user, conversation, event, and message identifiers,
    internal principal and conversation references, event kind, occurred-at
    timestamp, text body where present, reply references, attachment metadata,
    approval payloads, command hints, and raw payload references
  - compatibility-preserving evolution of existing cross-process contracts where
    needed
- Verification:
  - contract round-trip tests
  - manual review that Telegram-specific business semantics do not leak into the
    normalized ingress contract
- Evidence:
  - Added canonical Phase 2 foreground contracts in
    `crates/contracts/src/lib.rs` for normalized ingress, foreground triggers,
    conscious context, model calls, and conscious-worker request or result
    shapes; evolved worker contracts compatibly to add a conscious-worker kind
    while preserving the Phase 1 smoke path. Updated
    `crates/workers/src/main.rs`, `crates/workers/tests/smoke_worker_cli.rs`,
    `crates/harness/src/runtime.rs`, and `crates/harness/src/config.rs` to
    compile against the expanded shared contract surface. Post-review contract
    correction: conscious worker requests now use one consistent request ID and
    timestamp across the outer worker request and inner conscious payload.
    Verified with
    `cmd.exe /c cargo test -p contracts -- --nocapture`,
    `cmd.exe /c cargo fmt --all --check`, and
    `cmd.exe /c cargo check --workspace`; later confirmed by
    `cmd.exe /c cargo test --workspace`. Manual review completed to confirm the
    normalized ingress contract remains transport-level rather than
    Telegram-business-specific.

### Task P2-06: Extend worker kinds and subprocess protocol for conscious execution

- Status: `DONE`
- Depends on: `P2-05`
- Parallel-safe: no
- Deliverables:
  - new worker kind for the Phase 2 conscious foreground path
  - worker CLI entrypoint for the conscious worker
  - subprocess protocol changes that preserve the Phase 1 smoke-worker path
  - a clear structured exchange pattern for harness-mediated model requests so
    conscious workers can request model execution without owning provider access
  - structured conscious-worker result shape that can return user-facing output
    and episode metadata without direct canonical writes
- Verification:
  - worker unit tests for request validation and structured response behavior
  - existing smoke-worker tests remain green
- Evidence:
  - Added conscious-worker subprocess protocol messages in
    `crates/contracts/src/lib.rs` for worker-to-harness model-call requests and
    harness-to-worker model-call responses, while preserving the existing smoke
    worker request or response path. Implemented the `conscious-worker`
    CLI entrypoint in `crates/workers/src/main.rs`, including a line-oriented
    protocol that emits one `ModelCallRequest`, accepts one
    `ModelCallResponse`, and returns a structured `WorkerResponse` carrying the
    canonical `ConsciousWorkerResult` shape without any provider-specific or
    canonical-write authority in the worker. Added worker unit coverage for
    model-request shaping and final response wrapping, plus CLI integration
    coverage in `crates/workers/tests/conscious_worker_cli.rs`; the existing
    smoke-worker tests remain green in `crates/workers/tests/smoke_worker_cli.rs`.
    Verified with `cmd.exe /c cargo test -p contracts -- --nocapture`,
    `cmd.exe /c cargo test -p workers -- --nocapture`,
    `cmd.exe /c cargo fmt --all --check`,
    `cmd.exe /c cargo check --workspace`, and
    `cmd.exe /c cargo test --workspace`.

### Task P2-07: Implement the Telegram adapter boundary and fake transport fixtures

- Status: `DONE`
- Depends on: `P2-02`, `P2-05`
- Parallel-safe: yes
- Deliverables:
  - Telegram adapter interface and implementation boundary
  - fake or stub transport support suitable for unit, component, and integration
    testing
  - canonical fixture payloads for supported and rejected Telegram cases
  - outbound message delivery abstraction kept separate from harness business
    logic
- Verification:
  - unit tests using fake Telegram payloads and fake delivery transport
- Evidence:
  - Added transport-only Telegram boundary in
    `crates/harness/src/telegram.rs` with raw update parsing, fixture loading,
    one-shot polling abstraction, and outbound delivery abstraction plus fake
    implementations for tests; added canonical Telegram fixtures under
    `crates/harness/tests/fixtures/telegram/`, including private text,
    rejected group, private batch, private command-with-document, and approval
    callback fixtures. Verified with
    `cmd.exe /c cargo test -p harness telegram -- --nocapture`,
    `cmd.exe /c cargo fmt --all --check`, and
    `cmd.exe /c cargo check --workspace`; later confirmed by
    `cmd.exe /c cargo test --workspace`.

### Task P2-08: Implement inbound Telegram normalization and identifier mapping

- Status: `DONE`
- Depends on: `P2-04`, `P2-05`, `P2-07`
- Parallel-safe: no
- Deliverables:
  - normalization from Telegram update payloads into the canonical ingress
    contract
  - mapping from Telegram user and conversation identifiers to the configured
    internal principal and conversation references
  - normalization of event kind, text body, and external event or message
    identifiers needed for trigger intake and deduplication
  - support for reply references and attachment metadata fields in the contract
    even when deeper attachment handling is deferred
  - graceful rejection or ignore behavior for unsupported event shapes
- Verification:
  - unit tests for accepted and rejected normalization cases
  - component tests that persisted ingress records retain the normalized fields
- Evidence:
  - Added harness-owned Telegram normalization and identifier mapping in
    `crates/harness/src/ingress.rs`, including fail-closed private-chat and
    configured-actor checks, reply-link normalization, attachment-reference
    normalization, command-hint extraction, callback-payload support, and
    graceful ignore handling for unsupported raw update shapes. Post-review
    correction: invalid Telegram timestamps now fail closed instead of being
    rewritten to runtime-local time. Expanded
    ingress persistence reads in `crates/harness/src/foreground.rs` and updated
    `crates/harness/tests/phase2_component.rs` to verify persisted normalized
    fields. Verified with
    `cmd.exe /c cargo test -p harness ingress -- --nocapture`,
    `cmd.exe /c cargo test -p harness --test phase2_component -- --nocapture`,
    `cmd.exe /c cargo fmt --all --check`, and
    `cmd.exe /c cargo check --workspace`; later confirmed by
    `cmd.exe /c cargo test --workspace`.

### Task P2-09: Implement foreground trigger validation, deduplication, and budget initialization

- Status: `DONE`
- Depends on: `P2-04`, `P2-05`, `P2-08`
- Parallel-safe: no
- Deliverables:
  - trigger validation for supported Telegram ingress
  - deduplication or idempotence handling for repeated external updates
  - minimal foreground policy evaluation for actor, conversation mode, and any
    configured Telegram-boundary restrictions
  - budget initialization for conscious foreground episodes, including explicit
    iteration, wall-clock, and compute or token budgets
  - audit-event emission for accepted and rejected trigger paths
- Verification:
  - unit tests for trigger validation, duplicate handling, and policy decisions
  - component tests for persisted trigger and audit behavior
- Evidence:
  - Added explicit foreground budget config fields in
    `config/default.toml` and `crates/harness/src/config.rs` for iteration,
    wall-clock, and token limits, with fail-closed validation and updated test
    support defaults. Extended `crates/harness/src/policy.rs` with minimal
    Telegram foreground trigger policy evaluation plus explicit foreground
    budget initialization and validation. Implemented harness-owned Telegram
    trigger intake in `crates/harness/src/foreground.rs`, including
    deduplication by external event identifier, idempotent duplicate handling,
    accepted and rejected ingress persistence, execution-record creation for
    accepted triggers, and audit-event emission for accepted, rejected, and
    duplicate paths. Added `audit::list_for_trace` in
    `crates/harness/src/audit.rs` and expanded
    `crates/harness/tests/phase2_component.rs` with PostgreSQL-backed tests for
    accepted, rejected, and duplicate trigger paths. Verified with
    `cmd.exe /c cargo test -p harness -- --nocapture`,
    `cmd.exe /c cargo fmt --all --check`,
    `cmd.exe /c cargo check --workspace`, and
    `cmd.exe /c cargo test --workspace`.

### Task P2-10: Implement the compact self-model seed and internal-state snapshot path

- Status: `DONE`
- Depends on: `P2-02`
- Parallel-safe: yes
- Deliverables:
  - typed self-model seed loading for the initial foreground runtime
  - self-model seed structure covering at least stable identity, capabilities,
    role, constraints, preferences, current goals, and current subgoals where
    applicable
  - internal-state snapshot builder covering the minimum required operational
    signals
  - compact serialization of self-model and internal state for conscious context
- Verification:
  - unit tests for seed loading, validation, and snapshot derivation
- Evidence:
  - Added self-model seed loading, validation, internal-state snapshot
    building, and compact serialization helpers in
    `crates/harness/src/self_model.rs` using the repo-local seed artifact from
    `config/self_model_seed.toml`. Verified with
    `cmd.exe /c cargo test -p harness self_model -- --nocapture`,
    `cmd.exe /c cargo fmt --all --check`, and
    `cmd.exe /c cargo check --workspace`; later confirmed by
    `cmd.exe /c cargo test --workspace`.

### Task P2-11: Implement context assembly v0 for conscious foreground episodes

- Status: `DONE`
- Depends on: `P2-04`, `P2-05`, `P2-10`
- Parallel-safe: no
- Deliverables:
  - bounded context assembly from active trigger, self-model seed, internal
    state, and recent episode history
  - explicit context-size or selection limits suitable for deterministic testing
  - context-assembly metadata suitable for auditability
- Verification:
  - unit tests for selection limits and context-shaping logic
  - component tests against disposable PostgreSQL for recent-history retrieval
- Evidence:
  - Added harness-owned context assembly in
    `crates/harness/src/context.rs`, including deterministic limits for recent
    history count, trigger-text truncation, and historical message truncation,
    plus returned context-assembly metadata covering selected episode IDs,
    truncation counts, and self-model seed provenance for later audit use.
    Extended `crates/harness/src/foreground.rs` with
    `list_recent_episode_excerpts_before` so context assembly reads only
    bounded history that predates the active trigger. Exported the new module in
    `crates/harness/src/lib.rs` and expanded
    `crates/harness/tests/phase2_component.rs` with PostgreSQL-backed coverage
    proving self-model loading, internal-state injection, explicit selection
    limits, message shaping, and exclusion of future episodes from recent
    history. Verified with
    `cmd.exe /c cargo test -p harness -- --nocapture`,
    `cmd.exe /c cargo fmt --all --check`,
    `cmd.exe /c cargo check --workspace`, and
    `cmd.exe /c cargo test --workspace`.

### Task P2-12: Implement the harness-owned model gateway and one provider adapter

- Status: `TODO`
- Depends on: `P2-02`, `P2-05`
- Parallel-safe: yes
- Deliverables:
  - provider-agnostic gateway interface owned by the harness
  - one initial provider adapter behind that gateway
  - budget-aware request validation, tracing, and error shaping for model calls
  - fake provider support for deterministic testing
- Verification:
  - unit tests for routing, validation, and provider-error handling
  - component tests using the fake provider
- Evidence:
  - pending

### Task P2-13: Implement the conscious worker path for one harness-mediated model cycle

- Status: `TODO`
- Depends on: `P2-05`, `P2-06`, `P2-11`, `P2-12`
- Parallel-safe: no
- Deliverables:
  - conscious worker logic that consumes the bounded context
  - structured worker-to-harness model request for the single Phase 2 foreground
    cycle
  - worker completion result containing assistant output and episode metadata
  - explicit proof in code structure that the worker does not own provider
    adapters, tool execution, or canonical writes
- Verification:
  - worker unit tests
  - component tests with fake provider and fake transport
- Evidence:
  - pending

### Task P2-14: Implement end-to-end foreground orchestration from ingress to response

- Status: `TODO`
- Depends on: `P2-04`, `P2-07`, `P2-09`, `P2-13`
- Parallel-safe: no
- Deliverables:
  - harness orchestration from accepted ingress to conscious execution
  - execution-record creation and status transitions for real foreground runs
  - persistence of ingress, episode, assistant output, and trace-linked audit
    events
  - outbound Telegram delivery for the resulting assistant response
- Verification:
  - component tests for foreground orchestration with real PostgreSQL and fake
    provider or Telegram transport boundaries
- Evidence:
  - pending

### Task P2-15: Add thin runtime entrypoints for fixture-driven and poll-once Telegram execution

- Status: `TODO`
- Depends on: `P2-07`, `P2-14`
- Parallel-safe: no
- Deliverables:
  - `runtime` CLI entrypoint for one-shot fixture-driven Telegram ingestion
  - `runtime` CLI entrypoint for one-shot Telegram polling or equivalent live
    adapter verification path
  - runtime wiring kept thin, with control-plane behavior remaining in
    `crates/harness`
- Verification:
  - `cargo run -p runtime -- telegram --fixture <fixture-path>`
  - local manual check of the implemented poll-once path with safe fail-closed
    behavior when Telegram configuration is absent
- Evidence:
  - pending

### Task P2-16: Add unit and component coverage for the Phase 2 foreground subsystems

- Status: `TODO`
- Depends on: `P2-04`, `P2-07`, `P2-09`, `P2-10`, `P2-11`, `P2-12`, `P2-13`
- Parallel-safe: yes
- Deliverables:
  - unit tests for normalization, validation, budgeting, self-model seeding,
    internal-state snapshots, and context assembly
  - component tests for persistence services, model gateway, and conscious-path
    orchestration with real PostgreSQL where required
  - regression tests for the most likely fail-closed cases on the Phase 2 path
- Verification:
  - `cargo test --workspace`
- Evidence:
  - pending

### Task P2-17: Add architecture-critical foreground integration tests

- Status: `TODO`
- Depends on: `P2-14`, `P2-15`, `P2-16`
- Parallel-safe: no
- Deliverables:
  - integration test for normalized Telegram input to persisted episode and
    assistant response
  - integration test for rejected or duplicate Telegram ingress on the
    foreground path
  - integration test proving trace-linked audit emission for a real Phase 2
    foreground run
- Verification:
  - targeted integration-test execution
  - `cargo test --workspace`
- Evidence:
  - pending

### Task P2-18: Extend repository CI and canonical docs for the Phase 2 foreground gate

- Status: `TODO`
- Depends on: `P2-15`, `P2-16`, `P2-17`
- Parallel-safe: no
- Deliverables:
  - CI workflow changes so the relevant Phase 2 foreground suites run
    automatically under stable gate names
  - `README.md` updates for the new foreground commands and verification posture
  - status-document updates needed to reflect Phase 2 execution and evidence
  - documentation of any intentionally deferred live-network checks
- Verification:
  - manual review that local commands, workflow steps, and stable gate names are
    aligned
  - review of at least one successful repository-hosted run if the environment
    can record it, otherwise explicit blocker documentation
- Evidence:
  - pending
