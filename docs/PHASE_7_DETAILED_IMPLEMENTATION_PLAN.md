# Blue Lagoon

## Phase 7 Detailed Implementation Plan

Date: 2026-04-24
Status: In progress; Milestone A and Milestone B complete, Milestone C in progress, and Milestone D not started
Scope: High-level plan Phase 7 only
Audience: LLM-assisted implementation work and human review

## Purpose

This document defines the detailed implementation plan for Phase 7 of Blue
Lagoon.

It translates the approved Phase 7 scope from
`docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md` into concrete, trackable, and
LLM-executable work items.

Phase 7 exists because the post-Phase-6 self-check found one remaining
canonical implementation drift that must be fixed in code rather than explained
away in the requirements: scheduled foreground tasks are still required by the
canonical conscious-trigger model, but the shipped runtime does not yet
implement them end to end. Phase 7 also carries the deferred user-facing
documentation work that should be published only after that drift is resolved.

## Canonical inputs

This plan is subordinate to the following canonical documents:

- `PHILOSOPHY.md`
- `docs/REQUIREMENTS.md`
- `docs/LOOP_ARCHITECTURE.md`
- `docs/IMPLEMENTATION_DESIGN.md`
- `docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md`
- `docs/PHASE_6_DETAILED_IMPLEMENTATION_PLAN.md`

If this document conflicts with the canonical documents, the canonical
documents win.

## Documentation readiness review

The Phase 7 planning baseline is ready.

The current canonical documents agree on the core Phase 7 intent:

- scheduled foreground tasks remain part of the conscious-trigger model and
  must be implemented rather than silently dropped
- the harness must remain the sole owner of proactive scheduling, policy
  evaluation, canonical writes, recovery, and auditability
- proactive foreground behavior must remain policy-gated, bounded, and
  diagnosable through durable management surfaces
- post-phase drift must be closed by implementation work or explicit roadmap
  deferral, not by weakening requirements after the fact
- user-facing documentation must describe shipped workflows, not inferred or
  partially implemented behavior
- Phase 7 is a drift-closure and productization phase, not a broad new feature
  expansion phase

No blocking contradiction was found between
`docs/REQUIREMENTS.md`,
`docs/LOOP_ARCHITECTURE.md`,
`docs/IMPLEMENTATION_DESIGN.md`, and
`docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md`.

Several Phase 7 details are already settled by the current canonical documents
and the completed implementation state:

- the conscious-trigger model still includes:
  - user input
  - scheduled foreground task
  - approved wake signal
  - supervisor recovery event
  - approval resolution event
- `runtime harness` and `runtime telegram` already run as continuous services
  by default, so scheduled foreground work must integrate into the existing
  long-lived runtime posture rather than reintroducing one-shot-only behavior
- foreground approval-resolution handling and supervisor-recovery handling
  already exist and must remain compatible with any new scheduled trigger path
- the management CLI remains the durable operator surface and should gain the
  minimum scheduled-foreground inspection and bounded explicit-control commands
  needed for routine operation, diagnosis, and local verification
- user-facing `README.md` and any dedicated user manual must be written only
  after the runtime behavior and command surface are stable enough to document
  directly

## CI assessment for Phase 7

At Phase 7 planning start, the repository-hosted CI posture is already strong
for workspace verification, foreground runtime, canonical persistence,
background maintenance, management CLI coverage, governed actions, recovery
hardening, and release readiness.

The current stable jobs remain useful:

- `workspace-verification`
- `foreground-runtime`
- `canonical-persistence`
- `background-maintenance`
- `management-cli`
- `governed-actions`
- `recovery-hardening`
- `release-readiness`

Phase 7 should preserve those stable job identities and extend the minimum
existing gates needed to cover scheduled foreground work and user-facing
documentation correctness. The default posture should be to extend current jobs
rather than add a new permanent CI job unless the cost or scope boundary is
clearly justified.

The Phase 7 CI posture is locked as follows:

- Keep `workspace-verification` as the fast repository-wide baseline gate.
- Keep `foreground-runtime` focused on trigger intake, backlog recovery,
  approval-resolution behavior, and the new scheduled foreground execution path.
- Keep `canonical-persistence` focused on schedule persistence, deduplication,
  canonical write safety, and migration-sensitive scheduled-task state.
- Keep `management-cli` focused on parser coverage, formatter coverage, and
  persistence-backed scheduled-foreground operator workflows.
- Keep `recovery-hardening` focused on crash, restart, timeout, and interrupted
  scheduled-foreground recovery semantics once the new capability exists.
- Keep `release-readiness` as the final gate for the full post-Phase-7 drift
  audit and the user-facing documentation consistency pass.

The intended Phase 7 gate-to-suite mapping is:

- `workspace-verification`
  Run formatting, compile checks, clippy, and fast unit-focused verification.
- `foreground-runtime`
  Continue running `foreground_component` and `foreground_integration`,
  extended for scheduled foreground planning, execution, and delivery.
- `canonical-persistence`
  Continue running persistence-sensitive suites, extended for schedule state,
  deduplication, migration safety, and canonical trigger records.
- `management-cli`
  Continue running `crates/runtime/tests/admin_cli.rs`, extended for scheduled
  foreground admin commands and text or JSON output checks.
- `recovery-hardening`
  Extend recovery-focused suites for scheduled task interruption, replay
  safety, lease expiry, and supervisor restart recovery.
- `release-readiness`
  Add the final consistency pass for canonical docs, user docs, and shipped
  runtime behavior.

Phase 7 CI expansion should avoid inventing a docs-only gate unless the
repository’s Markdown and link-check baseline is strong enough to keep that
gate stable.

## Implementation starting point

The current repository already contains several Phase 7 starting points that
should be extended rather than replaced.

The default implementation starting points are:

- `crates/contracts/src/lib.rs`
  for the foreground trigger taxonomy, which already includes
  `ScheduledTask` but does not yet define the full scheduled-task persistence
  or payload model
- `crates/harness/src/runtime.rs`
  for the current continuous service loops where scheduled foreground planning
  and due-task execution should be integrated
- `crates/harness/src/foreground.rs`
  for canonical foreground trigger construction, ingress-linked planning, and
  existing recovery seams that scheduled work must reuse where possible
- `crates/harness/src/foreground_orchestration.rs`
  for the current conscious orchestration path that already recognizes the
  `ScheduledTask` trigger kind but does not receive it from a real producer
- `crates/harness/src/recovery.rs`
  for worker leases, checkpoints, recovery decisions, and supervisor-restart
  handling that scheduled foreground executions must plug into rather than
  bypass
- `crates/harness/src/management.rs`
  for the existing operator-facing service layer that should gain scheduled
  foreground inspection and safe explicit control workflows
- `crates/runtime/src/admin.rs`
  for the existing management CLI namespace that should gain any Phase 7
  command additions without introducing a separate operator binary
- `crates/harness/src/policy.rs`
  for proactive gating, budget assignment, and any new scheduling-window or
  rate-limit policy rules
- `crates/harness/src/audit.rs`
  for durable trace-linked event recording that scheduled planning, execution,
  suppression, and recovery must continue to use
- `crates/harness/src/execution.rs`
  for durable execution-state handling that scheduled foreground executions
  should reuse instead of duplicating
- `crates/harness/tests/support/mod.rs`
  for disposable PostgreSQL test support and migration-backed test setup
- `.github/workflows/ci.yml`
  for the existing stable repository-hosted gate structure
- `README.md`
  which remains repository-oriented until the user-facing rewrite is justified
- `migrations/0007__recovery_hardening.sql`
  as the last reviewed migration before scheduled foreground state is added

At Phase 7 planning start, the current repository state also makes several
important constraints explicit:

- the trigger taxonomy now exposes `ScheduledTask`, `SupervisorRecoveryEvent`,
  and `ApprovalResolutionEvent`, so Phase 7 must finish the missing producer
  and persistence path for scheduled work rather than revisiting the trigger
  naming
- the runtime already defaults to continuous service mode for both harness and
  Telegram, so scheduled foreground work must fit into that posture cleanly
- Telegram offset tracking now works correctly in long-lived service mode, so
  Phase 7 should not regress the live-ingress path while adding proactive work
- the completed Phase 6 management surfaces already cover health, recovery,
  diagnostics, and schema workflows, but there is no scheduled-foreground
  inspection or explicit-control surface yet
- the repository still lacks any true user-facing quick-start or user manual,
  and Phase 7 must not publish them prematurely

## Phase 7 target

Phase 7 is complete only when Blue Lagoon proves the following:

- the harness can create, persist, deduplicate, schedule, execute, audit, and
  recover scheduled foreground work through one coherent canonical path
- scheduled foreground tasks can enter the system through one explicit and
  productized v1 authoring path rather than raw SQL or test-only fixtures
- scheduled foreground tasks are policy-gated and bounded rather than acting as
  an unrestricted proactive execution loophole
- interrupted scheduled foreground work is supervised and recovered using the
  same harness-owned recovery posture as other foreground work
- scheduled foreground state is visible through the management CLI without raw
  SQL or ad hoc operator-only scripts
- the canonical requirements, loop architecture, implementation design, and
  shipped implementation all agree without narrowing caveats
- `README.md` can be rewritten as a true user-facing quick start because the
  runtime surface it describes actually exists
- a dedicated user manual can be added for normal setup, operations, approvals,
  recovery, upgrades, and troubleshooting
- required unit, component, integration, and release-readiness checks pass

## Settled implementation clarifications

The following Phase 7 decisions are treated as settled for execution unless
later canonical documents intentionally change them:

- Phase 7 is a drift-closure phase, not a broad feature-exploration phase.
- Scheduled foreground tasks must be harness-owned from planning through
  execution, recovery, and auditability.
- Scheduled foreground tasks are not a replacement for wake signals; they are a
  distinct conscious-trigger path with their own canonical semantics.
- Scheduled foreground tasks must not require live Telegram ingress to exist in
  order to run. They need their own canonical trigger source and persistence
  model.
- Phase 7 must settle one v1 schedule-authoring path. The default path should
  be harness-mediated management CLI creation and update workflows rather than
  raw SQL, while richer end-user scheduling UX remains deferred.
- Proactive scheduling must remain bounded by policy. The implementation should
  prefer explicit schedules, cooldowns, or user-safe windows over free-running
  proactive autonomy.
- Any new operator workflows introduced by scheduled foreground execution should
  default into the existing management CLI rather than raw SQL or one-off
  scripts.
- User-facing documentation is a deliverable, but it is downstream of drift
  closure. It should not lead implementation or redefine behavior.

## Phase 7 scope boundary

### In scope for Phase 7

- canonical scheduled foreground task persistence and execution
- bounded explicit scheduled-task creation and update through the management CLI
- proactive scheduling policy, windows, cooldowns, and budget assignment
- scheduled foreground recovery, lease supervision, and restart handling
- scheduled-foreground management CLI inspection and bounded explicit control
- final post-Phase-7 drift audit against the canonical docs
- user-facing `README.md`
- dedicated user manual

### Explicitly out of scope for Phase 7

- multi-user or tenant-aware scheduling
- a general-purpose reminder or workflow-engine product surface
- browser-based operator consoles
- new primary user channels beyond Telegram
- broad product redesign of the conscious or unconscious loop architecture
- weakening canonical requirements to fit already-shipped code

### Deferred by later phases

- rich recurring-user automation surfaces
- complex calendar semantics or timezone-aware scheduling UI
- broad proactive behavior personalization beyond the minimum safe v1 policy
- advanced reminder composition, templating, or workflow chaining

### Execution posture confirmed for Phase 7

- fix the remaining canonical drift in code first
- prove the new capability through tests and management surfaces
- rerun the canonical drift audit only after the capability exists
- publish user-facing docs only after the implementation and operator surface
  are stable enough to document directly

## LLM execution rules

Phase 7 should be executed with the same repository posture used in earlier
phase plans:

- do not change the canonical requirements to fit the code
- prefer extending existing harness, runtime, recovery, and management modules
  over adding parallel subsystems without a strong reason
- introduce reviewed SQL migrations for new canonical state instead of hiding
  state in config or ephemeral files
- add tests at the lowest effective layer first, then broaden to component and
  integration coverage where architectural risk justifies it
- keep the management CLI capability-oriented rather than storage-oriented
- treat user documentation as an implementation deliverable that must be
  validated against the real runtime behavior before completion

## Progress tracking protocol

Phase 7 progress should be tracked at the task level.

Use the following task states only:

- `TODO`
- `IN_PROGRESS`
- `DONE`
- `BLOCKED`

Only one task should be marked `IN_PROGRESS` at a time unless an explicit
parallelism decision is recorded.

When a task is completed, update the task ledger in this document so the plan
remains the durable execution record.

## Progress snapshot

- Milestone A: `DONE`
- Milestone B: `DONE`
- Milestone C: `IN PROGRESS`
- Milestone D: `TODO`
- Completed task count: `10 / 19`
- Current critical-path task: `C1`

## Execution refinement notes

The post-Phase-6 self-check already established the minimum Phase 7 refinement
inputs:

- the new continuous service-mode runtime changes are correct and should remain
- Telegram polling offset tracking is fixed and should remain
- broadened unconscious-trigger validation is correct and should remain
- approval-resolution and supervisor-recovery trigger modeling are correct and
  should remain
- the remaining material drift is the absence of end-to-end scheduled
  foreground task implementation

This means Phase 7 should be executed as a targeted closure phase rather than a
fresh broad discovery phase.

## Expected Phase 7 verification commands

The expected local verification surface for Phase 7 should include, at minimum:

```bash
cargo fmt --all --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --lib -- --nocapture
cargo test -p runtime --bin runtime -- --nocapture
cargo test -p runtime --test admin_cli -- --nocapture
cargo test -p harness --test foreground_component -- --nocapture
cargo test -p harness --test foreground_integration -- --nocapture
cargo test -p harness --test management_component -- --nocapture
cargo test -p harness --test management_integration -- --nocapture
powershell -ExecutionPolicy Bypass -File .\scripts\pre-commit.ps1
powershell -ExecutionPolicy Bypass -File .\scripts\recovery-hardening.ps1
powershell -ExecutionPolicy Bypass -File .\scripts\release-readiness.ps1
```

If Phase 7 adds new dedicated suites for scheduled foreground work, those
commands should be added here rather than replacing the existing critical
surfaces.

## Phase 7 milestones

Phase 7 is decomposed into four milestones:

- Milestone A: scope lock and scheduled-foreground model translation
- Milestone B: canonical scheduled-task persistence and policy surfaces
- Milestone C: runtime orchestration, recovery, and operator workflows
- Milestone D: final drift audit and user-facing documentation

## Milestone A quality gate

Milestone A is complete only when:

- the exact remaining drift list is locked
- the scheduled foreground task model is translated into concrete runtime,
  persistence, policy, recovery, and operator constraints
- the v1 schedule-authoring path is explicitly chosen and does not depend on
  raw SQL
- explicit non-goals and deferrals are documented so execution does not sprawl
- the CI and verification posture for Phase 7 is defined

## Milestone B quality gate

Milestone B is complete only when:

- the reviewed SQL migration for scheduled foreground state exists
- canonical contracts and service-layer types for scheduled tasks exist
- schedule creation and update flows are defined for the chosen v1 authoring
  path
- policy, budgeting, deduplication, and persistence behavior are implemented
- PostgreSQL-backed component tests protect the new state layer

## Milestone C quality gate

Milestone C is complete only when:

- the harness service can plan and execute due scheduled foreground tasks
- scheduled foreground execution is supervised and recoverable
- the management CLI exposes the minimum scheduled-foreground workflows needed
  for inspection, diagnosis, and bounded explicit control
- integration tests prove end-to-end scheduled foreground behavior

## Milestone D quality gate

Milestone D is complete only when:

- the post-Phase-7 drift audit finds no remaining known canonical mismatch
- user-facing `README.md` is written against real shipped behavior
- a dedicated user manual exists and is cross-checked against the runtime and
  operator surfaces
- release-readiness verification remains green with the new capability included

## Milestone A: Scope lock and scheduled-foreground model translation

### Task A1: Lock the exact post-Phase-6 drift list

Produce the authoritative list of remaining drift items and confirm that Phase
7 is focused on real implementation closure rather than speculative cleanup.

This task should explicitly record:

- scheduled foreground tasks as the remaining canonical trigger gap
- user-facing docs as deferred Phase 7 work rather than missing canonical
  product behavior
- which Phase 6 implementation fixes are already accepted and must not be
  rolled back

### Task A2: Translate the scheduled-foreground model into concrete architecture constraints

Turn the canonical scheduled-task requirement into explicit implementation
constraints for:

- canonical state shape
- trigger construction
- policy and budget assignment
- runtime due-task selection
- Telegram delivery behavior
- deduplication and idempotency
- recovery and restart handling

This task must resolve the minimum v1 semantics without opening a broad product
design loop.

It must also settle the concrete v1 schedule-authoring path so later tasks are
not blocked on an unresolved source-of-truth question.

### Task A3: Define the Phase 7 operator workflows and CLI expansion boundary

Identify the scheduled-foreground workflows that belong in the durable
management CLI.

The default operator workflows to settle are:

- create or update a scheduled foreground task through a bounded operator path
- list scheduled foreground tasks
- inspect due, suppressed, failed, or completed scheduled tasks
- trigger one bounded rescan or run-next workflow if needed
- inspect scheduled-foreground recovery or failure state
- keep storage-oriented or raw SQL workflows out of the durable operator path

### Task A4: Define the Phase 7 test and CI gate strategy

Lock the intended test layers and CI gate placement before implementation
sprawls across modules.

This task should decide:

- which new unit tests belong in contracts, policy, runtime, or formatter code
- which PostgreSQL-backed component tests protect schedule persistence
- which integration flows prove due-task execution and recovery
- which existing CI jobs absorb the new coverage

## Milestone B: Canonical scheduled-task persistence and policy surfaces

### Task B1: Add reviewed SQL migration for scheduled foreground task state

Add the canonical PostgreSQL schema for scheduled foreground state.

The migration should define the minimum durable state needed for:

- task identity
- schedule specification
- target conversation or principal
- creation and last-update provenance
- policy-gated availability
- deduplication
- current status
- due time and last-run timing
- execution linkage
- recovery metadata where required

The migration should stay compact and v1-oriented rather than attempting a
fully generic workflow engine schema.

### Task B2: Implement canonical Phase 7 contracts and harness persistence services

Add or refine the shared contracts and harness services needed to read, write,
and validate scheduled foreground tasks.

This work should cover:

- schedule-spec and task-record contract shapes
- validation rules
- schedule creation and update flows for the chosen v1 authoring path
- persistence insert and update flows
- due-task selection helpers
- deduplication keys
- audit-linked state transitions

### Task B3: Extend config and policy for proactive scheduling controls

Implement the config and policy layer required to keep proactive foreground
work bounded.

Expected policy inputs include:

- schedule enable or disable posture
- cooldowns and rate limits
- due-task batch limits
- quiet-window or safe-window handling if required for v1
- foreground budgets for scheduled work
- suppression rules when policy conditions are not met

### Task B4: Implement management services for scheduled foreground inspection

Extend the harness-side management layer with the read models and summaries
needed by the CLI.

The minimum service surface should cover:

- creating or updating tasks when the chosen v1 authoring path is CLI-driven
- listing tasks by status
- surfacing due or overdue work
- surfacing last-run and last-failure summaries
- surfacing suppression or policy-block reasons
- exposing recovery-relevant identifiers where needed

### Task B5: Add PostgreSQL-backed component tests for scheduled-task state

Add component tests that protect:

- migration-backed persistence
- validation and rejection cases
- schedule creation and update behavior
- due-task selection
- deduplication
- policy suppression persistence
- management read-model correctness

## Milestone C: Runtime orchestration, recovery, and operator workflows

### Task C1: Integrate due scheduled-task planning into the harness service loop

Extend the long-lived harness runtime so it can discover and plan due scheduled
foreground work without disturbing the existing background scheduler and
recovery flows.

This must remain compatible with:

- continuous service mode
- existing worker lease supervision
- existing background scheduler iteration
- existing foreground execution semantics

### Task C2: Implement scheduled foreground trigger construction and orchestration

Build the end-to-end path that turns a due scheduled task into a real
`ForegroundTriggerKind::ScheduledTask` execution.

This work should define:

- how the trigger is constructed without Telegram ingress
- how execution state is recorded
- what user-message or context shape the conscious worker receives
- how the resulting Telegram delivery is routed
- how canonical episode and audit records identify the scheduled source

### Task C3: Integrate scheduled foreground recovery and restart handling

Scheduled foreground executions must participate in the same recovery posture
as other foreground work.

This task should cover:

- lease and heartbeat handling where applicable
- checkpoint or restart-trigger integration where applicable
- supervisor restart behavior for scheduled work
- fail-closed behavior when proactive side effects are ambiguous
- recovery-linked audit trails

### Task C4: Extend the management CLI for scheduled foreground workflows

Add the durable CLI surface needed to inspect and safely control scheduled
foreground work.

The expected Phase 7 command family should be capability-oriented and may
include:

- `admin foreground schedules upsert` or equivalent create or update flow
- `admin foreground schedules list`
- `admin foreground schedules show`
- `admin foreground schedules run-next`
- `admin foreground schedules suppress` or equivalent bounded control

Final command names should be settled during implementation, but the surface
must remain coherent with the existing admin hierarchy.

### Task C5: Add end-to-end integration coverage for scheduled foreground behavior

Add integration tests that prove:

- schedule creation or update works through the chosen v1 authoring path
- due scheduled tasks are executed
- policy-suppressed tasks do not execute blindly
- duplicate scheduling is prevented
- Telegram delivery occurs through the canonical path
- interrupted scheduled executions recover safely
- management CLI reads the real persisted state correctly

## Milestone D: Final drift audit and user-facing documentation

### Task D1: Run and document the full post-Phase-7 canonical drift audit

Repeat the implementation-to-doc self-check against:

- `docs/REQUIREMENTS.md`
- `docs/LOOP_ARCHITECTURE.md`
- `docs/IMPLEMENTATION_DESIGN.md`
- `docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md`

The goal is to prove that no known drift remains, not to reinterpret the docs.

### Task D2: Add final parser, formatter, and release-readiness coverage

Add any final unit and CLI tests still missing for:

- scheduled-foreground admin command parsing
- text or JSON output formatting
- release-readiness verification bundles
- CI wiring where Phase 7 changed gate expectations

### Task D3: Rewrite `README.md` as a user-facing quick start

Once the runtime behavior is real and stable, replace the repository-oriented
top-level README with a proper user-facing document that covers:

- prerequisites
- setup
- migrations
- starting the services
- common operator workflows
- where to find deeper documentation

### Task D4: Add a dedicated user manual for normal workflows

Add a durable user manual that covers the normal operational paths:

- first-time setup
- live Telegram use
- scheduled foreground behavior
- approvals
- recovery
- schema and upgrade workflows
- troubleshooting

### Task D5: Final consistency pass across user docs, repo docs, and shipped behavior

Before Phase 7 is closed, verify that:

- the user docs match the actual command surface
- the user docs do not contradict the canonical architecture docs
- the roadmap and detailed plan correctly describe the completed state
- no newly discovered mismatch is left undocumented

## Phase 7 task ledger

| Task | Title | Status | Notes |
| --- | --- | --- | --- |
| A1 | Lock the exact post-Phase-6 drift list | DONE | Phase 7 remained focused on the scheduled-foreground trigger gap plus deferred user-facing docs |
| A2 | Translate the scheduled-foreground model into architecture constraints | DONE | v1 shape settled around harness-owned persistence, management authoring, and later runtime execution |
| A3 | Define the Phase 7 operator workflows and CLI expansion boundary | DONE | `admin foreground schedules list/show/upsert` established as the bounded v1 authoring and inspection path |
| A4 | Define the Phase 7 test and CI gate strategy | DONE | Focused runtime parser/formatter coverage and PostgreSQL-backed management coverage are now in place |
| B1 | Add reviewed SQL migration for scheduled foreground task state | DONE | Added `migrations/0008__scheduled_foreground_tasks.sql` |
| B2 | Implement canonical Phase 7 contracts and harness persistence services | DONE | Added scheduled-task status/outcome contracts plus harness persistence and lookup helpers |
| B3 | Extend config and policy for proactive scheduling controls | DONE | Added `scheduled_foreground` config with enablement, cadence floor, cooldown default, and due-task iteration cap |
| B4 | Implement management services for scheduled foreground inspection | DONE | Added auditable harness management upsert/list/show with conversation-binding validation |
| B5 | Add PostgreSQL-backed component tests for scheduled-task state | DONE | Added management component coverage for scheduled-task upsert/list/show and audit trace emission |
| C1 | Integrate due scheduled-task planning into the harness service loop | TODO | Continuous runtime integration |
| C2 | Implement scheduled foreground trigger construction and orchestration | TODO | Real `ScheduledTask` producer path |
| C3 | Integrate scheduled foreground recovery and restart handling | TODO | Lease, checkpoint, restart, fail-closed behavior |
| C4 | Extend the management CLI for scheduled foreground workflows | DONE | Added runtime admin parser/help/JSON/text support for scheduled task list/show/upsert |
| C5 | Add end-to-end integration coverage for scheduled foreground behavior | TODO | Execution, suppression, recovery, CLI |
| D1 | Run and document the full post-Phase-7 canonical drift audit | TODO | Requirements-to-implementation recheck |
| D2 | Add final parser, formatter, and release-readiness coverage | TODO | CLI and gate completion |
| D3 | Rewrite `README.md` as a user-facing quick start | TODO | Only after drift closure |
| D4 | Add a dedicated user manual for normal workflows | TODO | Stable user guide |
| D5 | Final consistency pass across user docs, repo docs, and shipped behavior | TODO | Close-out validation |

## Definition of done

Phase 7 is done only when all of the following are true:

- the scheduled foreground task trigger exists as a real harness-owned runtime
  path rather than just an enum variant
- the new path is migration-backed, policy-gated, auditable, and recoverable
- the management CLI exposes the minimum durable workflows needed to operate it
- the full canonical drift audit passes without changing the requirements to fit
  the code
- user-facing docs are published and accurately describe the shipped workflows
- the expected verification surface is green

## Phase 7 exit-criteria compliance matrix

| Exit criterion | Required evidence |
| --- | --- |
| Scheduled foreground tasks exist in the shipped runtime | Runtime orchestration, persistence, and integration tests prove real execution |
| Scheduled foreground tasks can be authored without raw SQL | Management CLI or other explicitly chosen v1 authoring path is implemented and tested |
| Scheduled foreground tasks are bounded and policy-gated | Policy tests, suppression behavior, and management visibility are in place |
| Scheduled foreground work is recoverable | Recovery and restart tests cover interruption and fail-closed cases |
| Canonical docs and implementation agree | Post-Phase-7 drift audit finds no known remaining mismatch |
| User docs describe real workflows | `README.md` and user manual are written against the shipped command surface |
| Phase 7 is release-ready | Pre-commit, recovery-hardening, and release-readiness verification remain green |

## Explicit non-goals for Phase 7

Phase 7 should not attempt to:

- redesign the dual-loop architecture
- add multi-user scheduling semantics
- introduce a full reminder or workflow product surface beyond the minimum
  scheduled foreground trigger implementation
- build a browser-based admin UI
- add new primary transport channels
- revisit already accepted Phase 6 fixes unless a concrete defect is found

## Pre-implementation decisions from plan self-check

The following decisions are locked before implementation starts:

- the requirements remain authoritative and are not to be narrowed to fit the
  current code
- the only material known canonical drift after Phase 6 is the missing
  end-to-end scheduled foreground task capability
- user-facing documentation is Phase 7 work, but it must follow implementation
  closure rather than lead it
- the existing continuous service-mode runtime posture is accepted and should be
  treated as the baseline for scheduled foreground integration
