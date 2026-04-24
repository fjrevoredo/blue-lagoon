# Blue Lagoon

## High-Level Implementation Plan

Date: 2026-04-06
Status: Phase 1, Phase 1.1, Phase 2, Phase 3, Phase 4, Phase 4.5, Phase 5, Phase 6, and Phase 7 completed
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
- Treat the management CLI as the default operator surface from Phase 4.5
  onward, extending it deliberately when later phases introduce operator-facing
  inspection or safe explicit control needs.
- Prefer stable management CLI workflows over raw SQL, log archaeology, or ad
  hoc verification scripts whenever the operator task fits the product's
  durable management surface.

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

The plan should also be interpreted with the following operator-surface rules
from Phase 4.5 onward:

- Each later phase must explicitly assess whether the capability it introduces
  requires management CLI inspection, safe explicit control, or both.
- If a repeated operator workflow is needed for local verification, diagnosis,
  or bounded runtime control, the default assumption should be that it belongs
  in the management CLI unless there is a clear reason to defer it.
- Raw SQL, one-off scripts, or temporary verification documents may still exist
  as narrow implementation aids, but they should not remain the primary
  operator workflow when the behavior belongs in the durable product surface.

## Phase structure

The implementation should proceed through seven major product-capability phases,
with one bridging Phase 1.1 dedicated to establishing the minimum repository
CI/CD baseline and one bridging Phase 4.5 dedicated to the initial management
CLI surface before Phase 5 broadens the governed action surface.

## Current status

- Phase 1 status: `COMPLETE`
- Phase 1.1 status: `COMPLETE`
- Phase 2 status: `COMPLETE`
- Phase 3 status: `COMPLETE`
- Phase 4 status: `COMPLETE`
- Phase 4.5 status: `COMPLETE`
- Phase 5 status: `COMPLETE`
- Phase 6 status: `COMPLETE`
- Phase 7 status: `DONE`
- Implementation evidence for Phase 1 lives in
  `docs/PHASE_1_DETAILED_IMPLEMENTATION_PLAN.md`
- Detailed planning for Phase 1.1 lives in
  `docs/PHASE_1_1_DETAILED_IMPLEMENTATION_PLAN.md`
- Detailed planning for Phase 2 lives in
  `docs/PHASE_2_DETAILED_IMPLEMENTATION_PLAN.md`
- Detailed planning and execution evidence for Phase 3 lives in
  `docs/PHASE_3_DETAILED_IMPLEMENTATION_PLAN.md`
- Detailed planning and execution evidence for Phase 4 now lives in
  `docs/PHASE_4_DETAILED_IMPLEMENTATION_PLAN.md`
- Detailed planning and execution evidence for Phase 5 now lives in
  `docs/PHASE_5_DETAILED_IMPLEMENTATION_PLAN.md`
- The current repository state includes a runnable Rust workspace under
  `crates/`, reviewed SQL migrations, PostgreSQL-backed persistence, schema
  gating, worker subprocess execution, and a verified synthetic trigger path
- The current repository state now includes `.github/workflows/ci.yml` as the
  minimum repository-hosted CI baseline for Phase 1.1
- The current repository state now includes a completed Telegram-first
  foreground slice plus canonical continuity, retrieval, self-model, and
  backlog-aware recovery coverage, so Phase 3 is complete
- The current repository state now includes the bounded unconscious loop,
  background maintenance persistence, wake-signal handling, and the associated
  automated verification coverage, so Phase 4 is complete
- The current repository state now also includes the initial management CLI
  surface, harness-side management services, management CLI docs, and the
  dedicated `management-cli` CI gate, and the Phase 4.5 DB-backed verification
  surface has now passed in repository-hosted CI, so Phase 5 was able to start
- The current repository state now includes governed action planning, bounded
  subprocess and workspace-script execution, canonical approval handling,
  Phase 5 management CLI inspection and bounded resolution commands, and the
  dedicated `governed-actions` CI gate. Phase 5 is complete, and the repository
  has now also completed the Phase 6 recovery, diagnostics, migration-safety,
  and release-readiness hardening slice.

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

### Phase 1.1: Minimal CI/CD baseline

#### Phase 1.1 goal

Establish the first permanent GitHub Actions baseline that runs the required
Phase 1.1 automated verification subset and becomes the foundation that later
phases extend with broader repository-hosted change gates.

#### Phase 1.1 primary outcomes

- Add the first GitHub Actions workflow for pull requests and pushes to the
  default integration branch.
- Run the minimum required Rust verification surface in repository-hosted
  automation, with `cargo test --workspace` mandatory and `cargo fmt --all
  --check` plus `cargo check --workspace` included in the default baseline.
- Use stable workflow and check identities suitable for long-term branch
  protection rather than phase-specific naming that would need to be renamed
  later.
- Provision disposable PostgreSQL in GitHub Actions so persistence-critical
  Phase 1 tests run there rather than being silently skipped.
- Define the first named CI gate and document which Phase 1 verification
  commands are automated in Phase 1.1 versus still local-only.
- Record at least one successful repository-hosted workflow run as evidence
  that the minimum CI gate actually works.
- Keep the Phase 1.1 scope intentionally narrow: no release publishing,
  environment promotion, or production deployment automation yet.

#### Phase 1.1 exit criteria

- A pull request automatically triggers the minimum GitHub Actions workflow and
  reports a clear pass or fail status.
- The workflow runs the minimum Phase 1.1 automated verification subset without
  manual operator intervention.
- A failure in formatting, compilation, or tests produces a blocking CI signal
  in the repository-hosted checks.
- At least one successful repository-hosted workflow run is captured as phase
  evidence.
- The CI/CD baseline is documented clearly enough that later phases can extend
  it instead of redesigning or renaming it.

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
- Extend the Phase 1.1 CI baseline so foreground-path unit, component, and
  integration checks run automatically at the appropriate repository gate.

#### Phase 2 exit criteria

- A Telegram message can trigger a conscious episode end to end.
- The harness can assemble bounded foreground context, route a model call,
  return a user-facing response, and persist an episodic record plus audit trail.
- The foreground path proves that the worker does not own tool execution,
  canonical writes, or policy authority.
- The foreground slice is backed by automated unit, component, and integration
  tests appropriate to the implemented path.
- The relevant foreground-path suites are wired into repository CI so new
  regressions are surfaced automatically on pull requests or mainline gates.

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
- Implement backlog-aware foreground recovery intake so the harness can detect
  multiple pending conversation messages spanning a configurable time gap or a
  degraded-restart condition and route them into one recovery-aware foreground
  analysis instead of naively replying message by message.
- Establish provenance, supersession posture, contradiction-handling posture, and
  initial temporal validity handling.
- Implement a minimal retrieval baseline sufficient for harness context assembly,
  keeping canonical retrieval artifacts distinct from re-derivable projections.
- Preserve per-message ingress durability while allowing one foreground recovery
  execution to analyze a backlog as a timestamped batch when policy says the
  conversation should be recovered holistically.
- Add persistence-critical tests for proposal validation, merge behavior,
  retrieval assembly, backlog-aware recovery behavior, and canonical memory or
  self-model writes using real PostgreSQL where semantics matter.
- Extend CI/CD to run persistence-critical proposal, merge, retrieval, and
  migration-sensitive checks in the correct repository-hosted gate.

#### Phase 3 exit criteria

- Foreground execution can create proposals and the harness can accept or reject
  them with logged merge decisions.
- The system can retrieve prior episodic and memory material into later
  foreground context.
- The self-model exists as a canonical artifact rather than as a prompt-only
  stub.
- The foreground path can detect a stale or outage-like pending-message backlog
  and switch to one recovery-aware analysis flow without losing per-message
  durability or blindly replaying delayed replies in sequence.
- Proposal, merge, and persistence behavior are covered by automated tests that
  would fail on unsafe canonical write regressions.
- Repository CI executes the required persistence-critical gates automatically so
  canonical-write regressions are blocked before merge.

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
- Extend CI/CD with the background-maintenance suites needed to keep scheduling,
  timeout, and wake-signal regressions visible at the right gate.

#### Phase 4 exit criteria

- The harness can run scheduled or threshold-based unconscious jobs end to end.
- Unconscious workers can return memory deltas, retrieval updates, diagnostics,
  self-model deltas, and optional wake signals without direct canonical mutation.
- The system can demonstrate a closed background-maintenance loop with audit
  history and bounded termination.
- The unconscious path is protected by automated tests for isolation, structured
  outputs, and bounded execution behavior.
- Repository CI covers the required unconscious-loop regression suites for the
  implemented maintenance paths.

### Phase 4.5: Management CLI

#### Phase 4.5 goal

Introduce the first durable management CLI surface so inspection,
verification, and safe local control of the runtime become easier before Phase
5 adds more complexity and governed action-taking paths.

#### Phase 4.5 primary outcomes

- Define a small management CLI surface under the existing runtime entrypoint
  rather than introducing a separate control plane.
- Replace the current ad hoc SQL-heavy verification steps with explicit CLI
  commands for common inspection and safe operator actions.
- Add commands for status inspection across schema, worker resolution, Telegram
  binding state, foreground backlog state, background job state, and wake
  signals.
- Add a safe explicit path to create and inspect background-maintenance jobs for
  local verification without raw SQL seeding.
- Prefer machine-readable and concise human-readable output modes so the same
  commands work for both operators and scripted local checks.
- Keep the Phase 4.5 scope intentionally narrow: no interactive TUI, no broad
  arbitrary database console, and no expansion into full Phase 5 governed tool
  execution.
- Add automated coverage for CLI parsing, command routing, and the first
  persistence-backed management flows that the operator surface exposes.

#### Phase 4.5 exit criteria

- The runtime exposes a coherent management CLI for the minimum required local
  inspection and verification tasks.
- Background job creation and inspection for local verification no longer depend
  on raw SQL edits.
- The new CLI surface is covered by automated tests appropriate to its parsing
  and persistence-backed behavior.
- The management surface remains clearly separated from canonical runtime
  behavior and does not bypass harness-owned validation and policy boundaries.

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
- Extend the management CLI where needed so approvals, governed-action state,
  workspace inspection, and blocked-action diagnostics do not depend on raw SQL
  or ad hoc operator-only workflows.
- Add automated coverage for tool-risk classification, approval validation,
  policy re-checks, capability scoping, and blocked execution paths.
- Extend CI/CD so approval, policy, and blocked-action regressions are exercised
  automatically in the required repository-hosted gates.

#### Phase 5 exit criteria

- The conscious loop can propose tool use and the harness can validate, approve
  where required, execute, observe, and audit that action end to end.
- Workspace artifacts are stored separately from autobiographical memory.
- Sensitive or side-effecting actions are provably blocked unless policy and
  approval conditions are satisfied.
- Operator inspection and the minimum required explicit control flows for the
  Phase 5 governed-action surface are available through the management CLI
  rather than depending on raw SQL or temporary operator scripts.
- High-risk action paths have regression tests that prove policy and approval
  failures block execution.
- Repository CI automatically runs the required approval and safety suites before
  changes to governed action-taking paths can merge.

### Phase 6: Recovery, operational hardening, and v1 readiness

#### Phase 6 goal

Make the runtime durable enough to run continuously by completing recovery,
fault-handling, migration discipline, and release-grade verification.

#### Phase 6 primary outcomes

- Implement recovery checkpoints, recovery triggers, lease and heartbeat logic,
  retry policies, and fail-closed handling for ambiguous side effects.
- Generalize recovery policy beyond the first backlog-aware foreground slice so
  crash, restart, stall, approval-transition, and degraded-state recovery all
  use one coherent recovery checkpoint and continuation model.
- Implement health surfaces, operator diagnostics, and operational metrics that
  feed both humans and internal-state modeling.
- Extend the management CLI where needed so recovery state, health status,
  diagnostics, and other durable operator workflows are exposed through the
  established operator surface before considering heavier auxiliary surfaces.
- Complete migration operational conventions, upgrade-path validation, and
  compatibility handling for persisted cross-process artifacts.
- Expand the automated test suite to cover the architecture-critical paths
  defined in the implementation design.
- Validate end-to-end behavior for crash recovery, stalled workers, approval
  expiry, policy re-check failure, migration safety, and wake-signal routing.
- Add targeted fault-injection, upgrade-path, smoke, and test-effectiveness
  checks for the critical safety modules.
- Evolve the CI/CD baseline into staged pre-merge, mainline, and release gates,
  with controlled packaging or release automation where justified by v1
  readiness.

#### Phase 6 exit criteria

- The runtime can recover safely from crash, restart, timeout, and interrupted
  execution scenarios without violating canonical write or side-effect safety
  rules.
- The minimum required recovery, health, and diagnostics workflows are
  accessible through the management CLI rather than depending on raw SQL,
  manual database inspection, or one-off operator procedures.
- Required automated gates are green at the unit, component, integration, and
  release-critical layers.
- The project has a coherent first runnable implementation that matches the
  agreed v1 architecture posture.
- Release readiness is justified by explicit automated evidence rather than by
  manual confidence alone.
- CI/CD is capable of enforcing the required staged gates and supporting the
  first controlled release workflow without bypassing safety checks.

### Phase 7: Post-Phase-6 drift closure and user-facing documentation

#### Phase 7 goal

Close any remaining implementation drift against the canonical requirements
without weakening those requirements, and publish stable user-facing
documentation once the required surface is actually implemented.

#### Phase 7 primary outcomes

- Implement the required scheduled foreground task trigger end to end so the
  runtime matches the canonical conscious-trigger model.
- Add the harness-owned planning, policy, audit, recovery, and management
  surfaces needed for scheduled foreground work to be safe and diagnosable.
- Run a full post-Phase-6 self-check against `docs/REQUIREMENTS.md`,
  `docs/LOOP_ARCHITECTURE.md`, and `docs/IMPLEMENTATION_DESIGN.md`, and treat
  any remaining mismatch as implementation work rather than as a documentation
  rewrite.
- Rewrite `README.md` as a true user-facing quick start only after the shipped
  runtime behavior and command surface match the canonical design.
- Add a dedicated user manual for normal setup, run, approval, recovery,
  upgrade, and troubleshooting workflows after the user-facing README is
  grounded in stable runtime behavior.

#### Phase 7 exit criteria

- Scheduled foreground task triggering exists in the shipped runtime and is
  covered by automated tests at the appropriate unit, component, and
  integration layers.
- The canonical requirements, loop architecture, and implementation design no
  longer require caveats or narrowing language to fit the implementation.
- `README.md` and any dedicated user manual describe the actual supported user
  workflows rather than planned or inferred behavior.
- The roadmap and detailed phase plan clearly capture the closure of the last
  known post-Phase-6 drift items.

## Cross-cutting work that starts on day one

The following work should not be postponed to the end of the project. It should
begin in the first phase and deepen throughout the plan:

- Migration discipline and schema review.
- Production-runnability-oriented automated testing.
- Real PostgreSQL automated testing for persistence-critical behavior.
- Per-phase test planning and definition of the lowest effective test layer for
  each new capability.
- Repository-hosted CI/CD gate expansion aligned to the risk and cost of each
  newly implemented capability.
- Structured audit and trace instrumentation.
- Clear contracts for cross-process types.
- Minimal fail-closed recovery posture for interrupted execution.
- Deployment simplicity aligned with the agreed single-node v1 topology.
- Ongoing assessment of what each new capability should expose through the
  management CLI for durable operator inspection or safe explicit control.
- Clear mapping between local verification commands and GitHub Actions workflows.
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

With Phases 1, 1.1, and 2 complete, the next planning step is the detailed
implementation plan for Phase 3. That document should break the next phase
into:

- concrete subsystems
- major schema work
- contract definitions
- milestone-sized deliverables
- test requirements
- dependency ordering
- proposal-validation and merge boundaries
- canonical self-model and retrieval-read paths
- backlog-aware foreground recovery behavior

That Phase 3 document should stay subordinate to this one. It should add
execution detail, not re-open the high-level architectural sequence without an
explicit decision.

With Phase 3 complete, detailed planning for Phase 4 now lives in
`docs/PHASE_4_DETAILED_IMPLEMENTATION_PLAN.md`, focused on the unconscious
loop, bounded background maintenance, and wake-signal production and
evaluation.

With Phase 4 complete, the bridging Phase 4.5 implementation added the narrow
management CLI surface needed before broader runtime complexity expands again.

Detailed planning for Phase 5 now lives in
`docs/PHASE_5_DETAILED_IMPLEMENTATION_PLAN.md`, where the next execution work
should focus on workspace state, governed execution, approval handling,
capability scoping, and the required management CLI extensions.

Detailed planning and execution evidence for Phase 6 now lives in
`docs/PHASE_6_DETAILED_IMPLEMENTATION_PLAN.md`.

Detailed planning for Phase 7 now lives in
`docs/PHASE_7_DETAILED_IMPLEMENTATION_PLAN.md`, focused on closing remaining
post-Phase-6 drift against the canonical requirements and on publishing
user-facing documentation only after that drift is resolved.
