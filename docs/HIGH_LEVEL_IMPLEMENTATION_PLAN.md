# Blue Lagoon

## High-Level Implementation Plan

Date: 2026-04-05
Status: Phase 1 completed, Phase 2 planning baseline
Audience: Project planning before the detailed implementation plan

## Purpose

This document defines the first high-level implementation plan for Blue Lagoon.

It translates the approved philosophy, formal requirements, and implementation
design into a phased implementation sequence that can be reviewed, refined, and
later expanded into a detailed task-level plan.

This document is intentionally high level. It defines what should be built first,
what each phase must prove, and what remains deliberately deferred. It is not yet
the detailed work breakdown.

## Source baseline

This plan is derived from the current canonical project documents:

- `PHILOSOPHY.md`
- `docs/REQUIREMENTS.md`
- `docs/LOOP_ARCHITECTURE.md`
- `docs/IMPLEMENTATION_DESIGN.md`

If this plan conflicts with those documents, the canonical design and
requirements documents win.

## Planning posture

The implementation sequence should optimize for early proof of the core
architecture rather than feature breadth.

The first objective is not to ship every subsystem at once. The first objective
is to establish a runnable harness-centered system that proves the following hard
properties as early as possible:

- The harness is sovereign.
- The conscious and unconscious domains are isolated.
- Canonical writes are proposal-based and harness-owned.
- PostgreSQL is the canonical system of record.
- The assistant can preserve continuity through episodes, self-model state, and
  traceable execution history.
- The runtime can support both reactive foreground work and bounded background
  maintenance.

The plan therefore follows three sequencing rules:

1. Build the authority boundaries first.
2. Deliver an end-to-end foreground slice before broad subsystem expansion.
3. Add background maintenance, tooling, recovery, and hardening only after the
   core control plane is real.

This sequencing does not mean safety, policy, or recovery can be absent until
late phases. Minimal versions of those controls should exist from the beginning.
Later phases deepen and harden them rather than introducing them for the first
time.

## Implementation principles

Every phase should preserve the following project posture:

- Keep the harness as the only control plane and canonical write owner.
- Keep workers isolated, disposable, and bounded.
- Keep memory episode-first and proposal-validated.
- Keep the self-model operational and injected into foreground reasoning.
- Keep tool execution policy-driven and outside worker authority.
- Keep the architecture provider-agnostic even if one provider is used first.
- Keep the first implementation single-user first and Telegram-first.
- Keep tests production-runnability-oriented from the beginning.
- Treat tests as deliverables for every phase, not as final hardening cleanup.
- Treat no core architectural code path as complete until it has appropriate
  automated tests.

## Testing posture

Testing is a first-class implementation requirement in this plan, not a
secondary validation activity.

Each phase must deliver the tests needed to prove the behavior introduced in
that phase at the lowest effective layer in the test pyramid. Fast deterministic
unit tests should dominate. Component, integration, smoke, and targeted
fault-injection coverage should be added where architectural risk justifies it.

The plan should be interpreted with the following testing rules:

- Every phase must produce working code and the automated tests that justify it.
- Persistence-critical behavior must be tested against disposable real
  PostgreSQL, not only mocks.
- Architecture-critical flows must gain integration coverage as soon as they
  become real.
- Recovery, approvals, policy enforcement, canonical writes, and migration
  safety must accumulate regression tests as they are implemented.
- Release readiness depends on explicit test gates, not on informal confidence.

## Phase structure

The implementation should proceed through six major phases.

## Current status

- Phase 1 status: `COMPLETE`
- Phase 2 status: `NEXT`
- Implementation evidence for Phase 1 lives in
  `docs/PHASE_1_DETAILED_IMPLEMENTATION_PLAN.md`
- The current repository state includes a runnable Rust workspace under
  `crates/`, reviewed SQL migrations, PostgreSQL-backed persistence, schema
  gating, worker subprocess execution, and a verified synthetic trigger path

### Phase 1: Runtime foundation and authority boundaries

#### Phase 1 implementation status

Phase 1 is complete.

#### Phase 1 goal

Establish the codebase skeleton, canonical persistence baseline, process
boundaries, and startup safety rules that every later phase depends on.

#### Phase 1 primary outcomes

- Create the initial Rust workspace and the first-class boundaries for `runtime`,
  `harness`, `contracts`, and `workers`.
- Establish local development runtime shape with PostgreSQL and reviewed SQL
  migrations.
- Establish the default single-node, harness-centered, multi-process runtime
  posture used by local development and early production-like environments.
- Implement schema-version checks, config loading, secret loading posture, and
  basic startup gating.
- Establish minimal policy and budget enforcement scaffolding at the harness
  layer, even before higher-risk tools and approvals exist.
- Establish structured tracing, durable audit-event writing, and trace
  correlation at the harness layer.
- Establish minimal durable execution-state recording so interrupted work fails
  closed rather than disappearing silently.
- Prove that workers are launched as isolated subprocesses under harness control
  rather than as in-process helpers.
- Establish the initial automated test harness covering startup safety, schema
  gating, worker spawning, and basic harness control flow.

#### Phase 1 exit criteria

- The harness can boot, validate schema compatibility, and enter an idle state.
- The harness can accept a synthetic trigger, create a tracked execution record,
  spawn a worker, receive a structured result, and persist audit history.
- The project has an executable local development environment and an initial
  automated test baseline.
- Phase 1 behavior is covered by required unit and component tests rather than
  manual verification alone.

#### Phase 1 completion note

Phase 1 has been implemented and verified. The completed foundation includes:

- a Rust workspace with `runtime`, `harness`, `contracts`, and `workers`
- reviewed SQL migrations and migration command wiring
- startup schema compatibility checks that fail closed on incompatible history
- durable audit-event and execution-record persistence
- isolated worker subprocess execution with timeout termination behavior
- unit, component, integration, and smoke-style verification coverage

### Phase 2: Minimal foreground vertical slice

#### Phase 2 goal

Deliver the first real user-facing assistant path from Telegram input to
traceable harness-mediated response.

#### Phase 2 primary outcomes

- Implement the Telegram adapter with strict transport-only normalization.
- Define the first canonical ingress and trigger contracts.
- Implement conscious episode orchestration, budget initialization, context
  assembly v0, and worker response handling.
- Enforce minimal trigger validation, policy checks, and budget checks on the
  foreground path.
- Introduce a minimal model gateway with provider-agnostic internal contracts and
  one initial provider adapter.
- Create the first compact self-model seed and internal-state snapshot path used
  during conscious reasoning.
- Persist episodes, user interactions, outputs, and trace metadata for each
  foreground run.
- Add the first architecture-critical foreground integration tests for normalized
  ingress, conscious execution, persisted episode output, and audit emission.

#### Phase 2 exit criteria

- A Telegram message can trigger a conscious episode end to end.
- The harness can assemble bounded foreground context, route a model call,
  return a user-facing response, and persist an episodic record plus audit trail.
- The foreground path proves that the worker does not own tool execution,
  canonical writes, or policy authority.
- The foreground slice is backed by automated unit, component, and integration
  tests appropriate to the implemented path.

### Phase 3: Canonical memory and self-model baseline

#### Phase 3 goal

Make continuity real by implementing the first canonical proposal, merge, memory,
and self-model flows behind the foreground loop.

#### Phase 3 primary outcomes

- Implement canonical tables and contracts for episodes, proposals, merge
  decisions, memory artifacts, self-model artifacts, and basic retrieval-layer
  records.
- Implement conscious-loop emission of episodic records, candidate memories, and
  self-model-relevant observations.
- Implement harness-side validation and merge rules for proposal-based canonical
  writes.
- Establish provenance, supersession posture, contradiction-handling posture, and
  initial temporal validity handling.
- Implement a minimal retrieval baseline sufficient for harness context assembly,
  keeping canonical retrieval artifacts distinct from re-derivable projections.
- Add persistence-critical tests for proposal validation, merge behavior,
  retrieval assembly, and canonical memory or self-model writes using real
  PostgreSQL where semantics matter.

#### Phase 3 exit criteria

- Foreground execution can create proposals and the harness can accept or reject
  them with logged merge decisions.
- The system can retrieve prior episodic and memory material into later
  foreground context.
- The self-model exists as a canonical artifact rather than as a prompt-only
  stub.
- Proposal, merge, and persistence behavior are covered by automated tests that
  would fail on unsafe canonical write regressions.

### Phase 4: Unconscious loop and bounded background maintenance

#### Phase 4 goal

Introduce the second execution domain and prove that Blue Lagoon can maintain its
memory and self-model in the background without breaking isolation or control.

#### Phase 4 primary outcomes

- Implement harness-owned background job scheduling, scoping, and budget
  assignment.
- Implement the first bounded unconscious workers for consolidation, retrieval
  maintenance, contradiction detection, and reflection or self-model delta
  proposal generation.
- Implement wake-signal contracts, typed reason codes, and policy-gated
  wake-to-foreground conversion.
- Add thresholds and scheduled triggers for the first maintenance jobs.
- Prove that unconscious outputs remain structured and proposal-based only.
- Add automated coverage for background scheduling, bounded worker execution,
  wake-signal evaluation, and failure or timeout handling for unconscious jobs.

#### Phase 4 exit criteria

- The harness can run scheduled or threshold-based unconscious jobs end to end.
- Unconscious workers can return memory deltas, retrieval updates, diagnostics,
  self-model deltas, and optional wake signals without direct canonical mutation.
- The system can demonstrate a closed background-maintenance loop with audit
  history and bounded termination.
- The unconscious path is protected by automated tests for isolation, structured
  outputs, and bounded execution behavior.

### Phase 5: Tool execution, workspace, and approval model

#### Phase 5 goal

Add governed action-taking capability without weakening harness sovereignty or
the safety model.

#### Phase 5 primary outcomes

- Implement the workspace subsystem for notes, task artifacts, scripts, script
  versions, and script run history.
- Implement the risk-tiered tool model and the first bounded subprocess execution
  path.
- Implement approval objects, Telegram approval rendering, approval-resolution
  events, and policy re-check before execution for higher-risk or side-effecting
  actions.
- Enforce capability scoping for filesystem reach, network access, environment
  exposure, and execution budgets.
- Establish clear boundaries between script creation or editing permission and
  script execution permission.
- Add automated coverage for tool-risk classification, approval validation,
  policy re-checks, capability scoping, and blocked execution paths.

#### Phase 5 exit criteria

- The conscious loop can propose tool use and the harness can validate, approve
  where required, execute, observe, and audit that action end to end.
- Workspace artifacts are stored separately from autobiographical memory.
- Sensitive or side-effecting actions are provably blocked unless policy and
  approval conditions are satisfied.
- High-risk action paths have regression tests that prove policy and approval
  failures block execution.

### Phase 6: Recovery, operational hardening, and v1 readiness

#### Phase 6 goal

Make the runtime durable enough to run continuously by completing recovery,
fault-handling, migration discipline, and release-grade verification.

#### Phase 6 primary outcomes

- Implement recovery checkpoints, recovery triggers, lease and heartbeat logic,
  retry policies, and fail-closed handling for ambiguous side effects.
- Implement health surfaces, operator diagnostics, and operational metrics that
  feed both humans and internal-state modeling.
- Complete migration operational conventions, upgrade-path validation, and
  compatibility handling for persisted cross-process artifacts.
- Expand the automated test suite to cover the architecture-critical paths
  defined in the implementation design.
- Validate end-to-end behavior for crash recovery, stalled workers, approval
  expiry, policy re-check failure, migration safety, and wake-signal routing.
- Add targeted fault-injection, upgrade-path, smoke, and test-effectiveness
  checks for the critical safety modules.

#### Phase 6 exit criteria

- The runtime can recover safely from crash, restart, timeout, and interrupted
  execution scenarios without violating canonical write or side-effect safety
  rules.
- Required automated gates are green at the unit, component, integration, and
  release-critical layers.
- The project has a coherent first runnable implementation that matches the
  agreed v1 architecture posture.
- Release readiness is justified by explicit automated evidence rather than by
  manual confidence alone.

## Cross-cutting work that starts on day one

The following work should not be postponed to the end of the project. It should
begin in the first phase and deepen throughout the plan:

- Migration discipline and schema review.
- Production-runnability-oriented automated testing.
- Real PostgreSQL automated testing for persistence-critical behavior.
- Per-phase test planning and definition of the lowest effective test layer for
  each new capability.
- Structured audit and trace instrumentation.
- Clear contracts for cross-process types.
- Minimal fail-closed recovery posture for interrupted execution.
- Deployment simplicity aligned with the agreed single-node v1 topology.
- Documentation updates when implementation changes clarify or constrain the
  design.

## Deliberate deferrals

The first implementation should explicitly defer the following unless a later
review changes priorities:

- Multi-tenant architecture.
- Enterprise policy domains and RBAC.
- Additional primary user channels beyond Telegram.
- Distributed worker pools or message brokers.
- Kubernetes-first deployment assumptions.
- Separate vector databases or graph databases for v1.
- A heavy browser-based admin control plane.
- Broad workflow-engine adoption.

These are not rejected forever. They are deferred because they do not improve the
core single-user v1 runtime enough to justify the added complexity now.

## How to use this document next

This document should be used as the agreement point for implementation order and
milestone definition.

With Phase 1 complete, the next planning step should be the detailed
implementation plan for Phase 2. That document should break the next phase into:

- concrete subsystems
- major schema work
- contract definitions
- milestone-sized deliverables
- test requirements
- dependency ordering

That next document should stay subordinate to this one. It should add execution
detail, not re-open the high-level architectural sequence without an explicit
decision.
