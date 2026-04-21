# Blue Lagoon

## Phase 5 Detailed Implementation Plan

Date: 2026-04-21
Status: Draft; ready for implementation review
Scope: High-level plan Phase 5 only
Audience: LLM-assisted implementation work and human review

## Purpose

This document defines the detailed implementation plan for Phase 5 of Blue
Lagoon.

It translates the approved Phase 5 scope from
`docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md` into concrete, trackable, and
LLM-executable work items.

Phase 5 introduces governed action-taking behind the completed Phase 4.5
management CLI surface. Its purpose is to add workspace state, risk-tiered tool
execution, approval handling, and capability-scoped subprocess execution without
weakening harness sovereignty, canonical-write ownership, or the existing
dual-loop isolation model.

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

If this document conflicts with the canonical documents, the canonical
documents win.

## Documentation readiness review

The Phase 5 planning baseline is ready.

The current canonical documents agree on the core Phase 5 intent:

- governed action-taking must remain harness-mediated rather than becoming a
  worker-owned authority path
- the workspace subsystem must remain distinct from autobiographical memory,
  retrieval artifacts, and self-model artifacts
- tool governance must be risk-tiered and capability-scoped rather than a flat
  allowlist or unrestricted shell model
- approval objects must be canonical harness objects first and Telegram
  renderings second
- policy must be re-checked immediately before execution, especially for
  sensitive or side-effecting actions
- the management CLI must expand where Phase 5 introduces durable operator
  inspection or bounded explicit-control needs
- Phase 5 should remain intentionally narrow: it should introduce the first
  governed action surface, not a broad automation platform, browser control
  plane, or Phase 6 recovery supervisor

No blocking contradiction was found between
`docs/REQUIREMENTS.md`,
`docs/IMPLEMENTATION_DESIGN.md`,
`docs/LOOP_ARCHITECTURE.md`, and
`docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md`.

Several Phase 5 details are already constrained by the canonical documents and
therefore should be treated as planning inputs rather than reopened design
questions:

- PostgreSQL remains the canonical system of record for workspace artifacts,
  approvals, scripts, script versions, script run history, and governed-action
  execution state
- tool execution remains outside the model gateway and inside harness-owned
  execution-policy layers
- Telegram remains the primary user-facing approval surface in v1
- the management CLI remains the durable operator surface and should gain the
  minimum additional approval, workspace, and governed-action commands needed
  for inspection and bounded local verification

## CI assessment for Phase 5

At Phase 5 planning start, the repository-hosted CI posture is already strong
for foundation, foreground runtime, canonical continuity, background
maintenance, and the Phase 4.5 management CLI surface.

The current stable jobs remain useful:

- `workspace-verification`
- `foreground-runtime`
- `canonical-persistence`
- `background-maintenance`
- `management-cli`

Phase 5 should preserve those stable job identities and add one new
capability-based gate rather than folding governed-action regressions into an
unrelated existing job.

The Phase 5 CI posture is locked as follows:

- Keep `workspace-verification` as the fast repository-wide baseline gate.
- Keep `foreground-runtime` focused on Telegram-first foreground execution and
  transport-specific approval rendering or callback normalization behavior.
- Keep `canonical-persistence` focused on migration-sensitive and
  canonical-write-sensitive continuity regressions.
- Keep `background-maintenance` focused on scheduler, unconscious-worker, and
  wake-signal regressions.
- Keep `management-cli` focused on operator-surface parsing, formatting, and
  persistence-backed management workflows.
- Add `governed-actions` for workspace, approval, policy re-check, capability
  scoping, subprocess execution, and blocked-action regression coverage.

The intended Phase 5 gate-to-suite mapping is:

- `workspace-verification`
  Run formatting, compile checks, clippy, and fast unit-focused verification
  that does not require PostgreSQL.
- `foreground-runtime`
  Continue running `foreground_component` and `foreground_integration`,
  extended where needed for Telegram approval rendering and callback-trigger
  normalization.
- `canonical-persistence`
  Continue running `continuity_component` and `continuity_integration`.
- `background-maintenance`
  Continue running `unconscious_component` and `unconscious_integration`.
- `management-cli`
  Continue running `crates/runtime/tests/admin_cli.rs`, extended for the Phase 5
  admin commands and persistence-backed operator workflows.
- `governed-actions`
  Run the new PostgreSQL-backed workspace, approval, and governed-execution
  suites, beginning with dedicated harness component and integration tests for
  risk classification, approval validation, policy re-check, capability scoping,
  and blocked execution paths.

Phase 5 CI expansion should avoid duplicating the same expensive PostgreSQL
suite across multiple jobs unless a stricter later-stage gate intentionally
reuses it.

## Implementation starting point

The current repository already contains several Phase 5 starting points that
should be extended rather than replaced.

The default implementation starting points are:

- `crates/contracts/src/lib.rs`
  for shared loop, model-gateway, ingress, and worker-facing contracts that
  already include `ToolPolicy` and `ApprovalPayload`
- `crates/harness/src/foreground.rs`
  for persisted normalized ingress, foreground trigger construction, and the
  current storage of approval callback payloads
- `crates/harness/src/ingress.rs`
  for Telegram update normalization that already preserves approval callback
  data in the canonical ingress contract
- `crates/harness/src/policy.rs`
  for current foreground, wake-signal, and budget policy logic; it currently
  rejects approval callbacks as foreground triggers and therefore forms a clear
  seam for canonical approval-resolution handling
- `crates/harness/src/model_gateway.rs`
  for the provider-agnostic gateway contract and current `ToolPolicy`
  validation, which still stops at `NoTools` and `ProposalOnly`
- `crates/workers/src/main.rs`
  for the current conscious and unconscious worker protocol, which will need to
  grow a governed-action proposal posture on the conscious path without giving
  workers execution authority
- `crates/harness/src/audit.rs`
  for trace-linked audit persistence that Phase 5 mutating operations must
  continue to use
- `crates/harness/src/execution.rs`
  for durable execution-state handling that governed actions should reuse rather
  than duplicating
- `crates/harness/src/management.rs`
  for the existing harness-side operator surface that Phase 5 should extend
  instead of bypassing
- `crates/runtime/src/admin.rs`
  for the existing management CLI namespace that should gain Phase 5
  subcommands without introducing a separate control-plane binary
- `crates/runtime/src/main.rs`
  for the thin Clap entrypoint that should stay thin as the command tree grows
- `crates/harness/tests/support/mod.rs`
  for disposable PostgreSQL test support and migration-backed test setup
- `migrations/0005__unconscious_loop.sql`
  as the last reviewed migration before workspace, approval, and governed-action
  state is added

At Phase 5 planning start, the current repository state also makes several
important constraints explicit:

- the schema currently ends at `0005__unconscious_loop.sql`, so no canonical
  workspace, approval, script, or governed-execution tables exist yet
- the foreground path already normalizes and persists approval callback payloads,
  but approval callbacks are still policy-rejected as executable foreground
  triggers
- the model gateway already carries tool-policy intent in contracts, but actual
  governed tool execution does not exist yet
- the current conscious worker result shape has no explicit governed-action
  proposal payload, so Phase 5 must extend the cross-process contract rather
  than overloading the existing canonical memory or self-model proposal model
- the management CLI now exists, but it has no approval, workspace, or
  governed-action command tree
- the repository currently has no dedicated harness modules for workspace state,
  approval lifecycle, or governed execution orchestration

## Phase 5 target

Phase 5 is complete only when Blue Lagoon proves the following:

- the conscious loop can propose governed tool or script use without owning
  execution authority
- the harness can classify proposed actions by risk tier, derive or validate the
  required capability scope, and either block, approve, or execute the action
  through one coherent governed-execution path
- approval requests exist as canonical harness objects with TTL, action
  fingerprinting, resolution events, and policy re-check before execution
- Telegram can render approval requests and feed approval resolutions back into
  the harness through the canonical approval-resolution path
- workspace artifacts, scripts, script versions, and script run history are
  stored canonically and remain clearly separated from autobiographical memory
- the first bounded subprocess execution path enforces filesystem reach,
  environment exposure, network posture, and execution budgets through explicit
  capability scoping
- blocked or expired actions fail closed with traceable audit history rather
  than silently degrading into execution
- the management CLI exposes the minimum Phase 5 operator workflows for approval
  inspection or bounded resolution, workspace inspection, governed-action state,
  and blocked-action diagnostics without raw SQL
- required Phase 5 unit, component, and integration tests pass
- repository CI runs the required governed-action regression suites under stable
  capability-based gate identities

## Settled implementation clarifications

The following Phase 5 decisions are treated as settled for execution unless
later canonical documents intentionally change them:

- Phase 5 is a governed action-taking phase, not a general automation-platform
  phase. It should introduce the first bounded action path, not a broad catalog
  of uncontrolled tools.
- Workspace is a canonical artifact domain, not an alias for autobiographical
  memory and not an unrestricted mirror of the local filesystem.
- The initial execution backend should be a bounded subprocess path with
  explicit command specification, capability scopes, and audit history rather
  than a generic unrestricted shell surface.
- Tool or script proposal remains distinct from execution. The model may propose
  an action, but the harness must classify risk, validate scope, enforce policy,
  require approval when needed, and own execution.
- Risk-tier language should follow the implementation design:
  - Tier 0: read-only retrieval and inspection
  - Tier 1: safe bounded local transformations without external side effects
  - Tier 2: controlled local or external side effects within bounded policy
  - Tier 3: sensitive or dangerous actions requiring stronger controls
- Permission boundaries must remain explicitly split:
  - permission to create or edit a script
  - permission to execute a script
  - permission to execute a script with broader capabilities such as network
    access or higher-risk filesystem mutation
- Approval requests are canonical objects first and Telegram renderings second.
  Telegram is the primary user-facing approval surface, but the management CLI
  may expose a narrow operator resolution path for local verification or bounded
  recovery as long as it reuses the same approval-validation and audit path.
- Policy must be re-checked immediately before execution, even if an action
  received approval earlier.
- The model gateway should continue to express tool-use intent through structured
  contracts, but actual tool execution must remain outside the gateway.
- Phase 5 may extend the conscious worker result contract and foreground
  model-output posture to carry structured governed-action proposals and
  harness-returned action observations, but executable actions must remain
  distinct from canonical memory and self-model proposals.
- The management CLI should gain a narrow Phase 5 command set rather than a
  broad storage browser. The initial additions should be capability-oriented:
  - `runtime admin approvals list`
  - `runtime admin approvals resolve`
  - `runtime admin actions list`
  - `runtime admin workspace artifacts list`
  - `runtime admin workspace scripts list`
  - `runtime admin workspace runs list`
- `runtime admin status` should grow high-level pending-approval and
  governed-action summary fields rather than creating a second overlapping Phase
  5 status command.
- Phase 5 should not add arbitrary SQL execution, arbitrary process execution,
  arbitrary environment exposure, or an unrestricted filesystem browser.
- Phase 5 should not yet introduce browser automation, broad external-service
  tool catalogs, enterprise RBAC, or the richer generalized recovery controls
  deferred to Phase 6.
- The default new artifact names for Phase 5 should be:
  - reviewed migration file:
    `migrations/0006__workspace_and_governed_actions.sql`
  - workspace service module: `crates/harness/src/workspace.rs`
  - approval service module: `crates/harness/src/approval.rs`
  - governed-execution service module:
    `crates/harness/src/governed_actions.rs`
  - bounded subprocess execution module:
    `crates/harness/src/tool_execution.rs`
  - PostgreSQL-backed harness component suite:
    `crates/harness/tests/governed_actions_component.rs`
  - architecture-critical harness integration suite:
    `crates/harness/tests/governed_actions_integration.rs`
  - repository CI gate: `governed-actions`

## Phase 5 scope boundary

### In scope for Phase 5

- canonical workspace tables and harness services for notes, task artifacts,
  scripts, script versions, and script run history
- shared contracts for governed action proposals, capability scopes, approval
  objects, approval resolutions, and execution observations
- typed config and policy for risk tiers, approval TTL, execution budgets,
  environment exposure, filesystem reach, network posture, and blocked-action
  behavior
- the first bounded subprocess execution path under harness control
- canonical approval creation, rendering, resolution, expiry, and policy re-check
- conscious-loop to harness governed-action proposal handling
- audited blocked-execution outcomes for denied, expired, invalidated, or
  policy-recheck-failed actions
- management CLI expansion for approval inspection or bounded resolution,
  governed-action state, workspace inspection, and blocked-action diagnostics
- automated unit, component, and integration coverage for workspace, approval,
  and governed-action behavior
- repository CI expansion for Phase 5 governed-action regression coverage

### Explicitly out of scope for Phase 5

- arbitrary shell access or a general operator command runner
- a broad browser automation or remote-service tool catalog
- enterprise policy domains, RBAC, or multi-user approval workflows
- a browser-based admin plane, dashboard stack, or TUI
- generalized recovery supervision, replay control, or checkpoint tooling from
  Phase 6
- a full local-filesystem workspace mirror as the canonical workspace model
- broad autonomous proactive tooling beyond the governed action path explicitly
  approved and policy-checked in Phase 5

### Deferred by later phases

- richer operator dashboards and observability surfaces
- broader tool catalogs and stronger sandbox backends
- generalized approval-retry and recovery orchestration from Phase 6
- enterprise policy domains, stronger secret isolation, and RBAC
- heavier filesystem synchronization or export workflows for workspace artifacts

### Execution posture confirmed for Phase 5

- implement the first governed action surface behind the existing
  harness-centered control model rather than as a second execution authority
- keep the initial bounded execution backend narrow and auditable
- keep workspace state canonical and separate from autobiographical memory
- extend the management CLI deliberately where Phase 5 introduces durable
  operator workflows
- prefer additive harness modules over new top-level crates
- use disposable real PostgreSQL for persistence-critical workspace, approval,
  and governed-execution verification

## LLM execution rules

The plan should be executed under the following rules:

- Work one task at a time unless a task is explicitly marked as parallel-safe.
- Do not start a task until all of its dependencies are marked `DONE`.
- No core Phase 5 task is complete without the verification listed for it.
- Keep governed execution harness-owned. If workers begin owning side effects,
  approval resolution, or execution retries, stop and split the work first.
- Keep workspace separate from autobiographical memory. If a task begins storing
  governed artifacts in continuity tables or vice versa, stop and narrow scope
  first.
- Keep approval resolution canonical. If Telegram-specific logic starts owning
  business approval semantics, stop and move that logic back into harness.
- Prefer the lowest effective test layer.
- Use disposable real PostgreSQL for persistence-critical verification.
- Update this document immediately after finishing each task.

## Progress tracking protocol

This document becomes the progress ledger for Phase 5 once implementation
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

- Current milestone: `Milestone A`
- Current active task: `none`
- Completed tasks: `0/20`
- Milestone A status: `TODO`
- Milestone B status: `TODO`
- Milestone C status: `TODO`
- Milestone D status: `TODO`

Repository sequencing note:

- Phase 4.5 is complete, and this document is the draft execution ledger for
  the Phase 5 governed-action, workspace, and approval slice.

## Execution refinement notes

The current task count and scope are appropriate for starting implementation,
but several tasks are integration-heavy enough that they should be treated as
likely split points if the code reveals more coupling than expected.

The current execution posture for task sizing is:

- keep the Phase 5 ledger at the current 20-task scale
- split only when a task stops being one coherent implementation unit
- preserve capability-based task names if a split becomes necessary

The most likely split candidates are:

- `P5-04` if workspace schema, approval schema, and governed-execution schema
  stop fitting one coherent migration pass
- `P5-09` if Telegram approval rendering and approval-resolution event handling
  evolve at different speeds
- `P5-10` if capability scoping and subprocess execution isolation require
  separate stabilization passes
- `P5-15` through `P5-17` if the management CLI surface needs to separate
  read-only workspace inspection from explicit approval resolution controls

## Expected Phase 5 verification commands

These are the intended recurring verification commands for this phase. Some
will become available only after earlier tasks are complete.

- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo test -p workers -- --nocapture`
- `cargo test -p runtime --test admin_cli -- --nocapture`
- `cargo test -p harness --test foreground_component -- --nocapture`
- `cargo test -p harness --test foreground_integration -- --nocapture`
- `cargo test -p harness --test governed_actions_component -- --nocapture`
- `cargo test -p harness --test governed_actions_integration -- --nocapture`
- `cargo run -p runtime -- admin status`
- `cargo run -p runtime -- admin approvals list`
- `cargo run -p runtime -- admin approvals resolve ...`
- `cargo run -p runtime -- admin actions list`
- `cargo run -p runtime -- admin workspace artifacts list`
- `cargo run -p runtime -- admin workspace scripts list`
- `cargo run -p runtime -- admin workspace runs list`
- manual review that approval rendering and resolution preserve the canonical
  approval-object model rather than moving business semantics into Telegram code
- manual review that bounded subprocess execution enforces capability scopes and
  fails closed on denied or invalidated actions

## Phase 5 milestones

- Milestone A: scope lock, architecture, schema, contracts, and policy baseline
- Milestone B: workspace and approval foundations
- Milestone C: governed execution and conscious-loop integration
- Milestone D: management CLI, tests, CI, docs, and completion gate

## Milestone A quality gate

Milestone A is green only if:

- the Phase 5 scope boundary and deferred behavior are explicit
- the workspace, approval, and governed-execution boundaries are settled
- the risk-tier and capability-scope model is explicit before execution code
  lands
- the reviewed migration and shared contract posture are defined before heavier
  orchestration work starts
- the cross-process worker protocol posture for governed-action proposals is
  explicit before conscious-loop integration begins
- the CI expansion and management CLI extension posture are defined before new
  suites are added

## Milestone B quality gate

Milestone B is green only if:

- canonical workspace persistence exists and remains separate from
  autobiographical memory
- approval objects, TTL handling, action fingerprinting, and resolution events
  are durably represented
- Telegram approval rendering and callback ingestion are wired into the
  canonical approval lifecycle without Telegram-specific business shortcuts
- the harness has coherent services for workspace and approval state rather than
  ad hoc runtime-side SQL

## Milestone C quality gate

Milestone C is green only if:

- the conscious loop can propose governed actions through one harness-owned path
- the harness can classify risk, enforce capability scopes, request approval
  where required, and execute bounded subprocess actions when allowed
- script editing and script execution permissions remain distinct
- policy re-check and blocked-execution outcomes fail closed with traceable
  audit history

## Milestone D quality gate

Milestone D is green only if:

- required unit coverage exists for risk classification, approval validation,
  capability scoping, and formatter or CLI validation logic
- PostgreSQL-backed component coverage exists for workspace, approvals,
  governed-action persistence, and blocked-action semantics
- architecture-critical integration coverage exists for action proposal to
  approval to execution or block flow
- repository CI runs the required Phase 5 suites under stable capability-based
  gate names
- canonical docs and operator-facing docs reflect the implemented Phase 5
  command surface and verification commands
- this document reflects the final task status and evidence

## Task list

### Task P5-01: Lock the Phase 5 governed-action slice boundary

- Status: `TODO`
- Depends on: none
- Parallel-safe: no
- Deliverables:
  - explicit Phase 5 scope boundary covering what is in and out
  - documented clarification that Phase 5 introduces the first governed action
    surface rather than a broad automation platform
  - documented clarification that workspace remains distinct from
    autobiographical memory
  - documented clarification of the minimum Phase 5 management CLI additions
- Verification:
  - manual review against `docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md`,
    `docs/IMPLEMENTATION_DESIGN.md`, `docs/REQUIREMENTS.md`, and
    `docs/LOOP_ARCHITECTURE.md`
- Evidence:
  - not started

### Task P5-02: Define the Phase 5 runtime and harness architecture

- Status: `TODO`
- Depends on: `P5-01`
- Parallel-safe: no
- Deliverables:
  - settled harness-versus-runtime module structure for workspace, approvals,
    governed actions, and bounded subprocess execution
  - explicit command-tree extension posture for the Phase 5 admin surface
  - explicit boundary between model-gateway tool intent and harness-owned tool
    execution
  - explicit separation between script artifact management and script execution
- Verification:
  - code review of the proposed module and command-tree shape
  - `cargo check --workspace`
- Evidence:
  - not started

### Task P5-03: Extend config and policy for approvals, capabilities, and budgets

- Status: `TODO`
- Depends on: `P5-01`
- Parallel-safe: yes
- Deliverables:
  - typed config for approval TTL, approval rendering defaults, and governed
    action budgets
  - typed config for capability scoping covering filesystem reach, network
    posture, environment exposure, and subprocess timeout posture
  - policy rules for risk-tier classification and approval requirements
  - fail-closed validation for invalid or unsafe Phase 5 settings
- Verification:
  - unit tests for config parsing and validation
  - failed startup on invalid Phase 5 settings
- Evidence:
  - not started

### Task P5-04: Add a reviewed SQL migration for workspace and governed-action state

- Status: `TODO`
- Depends on: `P5-01`
- Parallel-safe: no
- Deliverables:
  - reviewed migration file
    `migrations/0006__workspace_and_governed_actions.sql`
  - canonical tables for workspace artifacts, scripts, script versions, script
    runs, approval requests, approval resolutions or equivalent resolution
    state, and governed-action execution state
  - indexes and constraints for action fingerprinting, approval lookup,
    execution tracing, blocked-action diagnostics, and recent workspace history
  - any required schema refinements needed to link governed actions back to
    execution records and audit events
- Verification:
  - migration discovery and ordering behave correctly
  - migration applies cleanly to disposable PostgreSQL
- Evidence:
  - not started

### Task P5-05: Define canonical Phase 5 contracts

- Status: `TODO`
- Depends on: `P5-01`
- Parallel-safe: yes
- Deliverables:
  - shared contracts for governed action proposals and action fingerprints
  - shared contracts for capability scopes, risk tiers, and execution outcomes
  - shared contracts for approval requests, resolution events, and expiry
    semantics
  - shared cross-process conscious-worker contracts for governed-action proposal
    payloads and execution observations that are distinct from canonical memory
    and self-model proposals
  - shared contracts for workspace artifacts, scripts, script versions, and
    script run summaries where they cross process boundaries
- Verification:
  - contract round-trip tests in `contracts`
  - `cargo test -p workers -- --nocapture`
  - manual review that contract names are capability-based rather than
    phase-labeled
- Evidence:
  - not started

### Task P5-06: Implement harness workspace persistence and service layer

- Status: `TODO`
- Depends on: `P5-02`, `P5-04`, `P5-05`
- Parallel-safe: no
- Deliverables:
  - workspace persistence services for notes, task artifacts, scripts, script
    versions, and script run history
  - typed read and write models aligned with the new Phase 5 tables
  - public harness APIs that higher-level orchestration and management services
    can call without ad hoc SQL
- Verification:
  - `cargo test -p harness --test governed_actions_component -- --nocapture`
- Evidence:
  - not started

### Task P5-07: Implement canonical approval persistence and lifecycle services

- Status: `TODO`
- Depends on: `P5-02`, `P5-04`, `P5-05`
- Parallel-safe: yes
- Deliverables:
  - approval persistence services for request creation, resolution, expiry, and
    lookup by token or action fingerprint
  - canonical validation of actor identity, TTL, one-shot use, and unchanged
    action fingerprint
  - trace-linked audit writing for approval creation, approval resolution,
    expiry, rejection, and invalidation
- Verification:
  - `cargo test -p harness --test governed_actions_component -- --nocapture`
- Evidence:
  - not started

### Task P5-08: Implement governed-action planning, classification, and fingerprinting

- Status: `TODO`
- Depends on: `P5-03`, `P5-05`
- Parallel-safe: yes
- Deliverables:
  - harness-side action classification rules for risk tiers
  - action fingerprinting and deduplication semantics suitable for approval
    validation and policy re-check
  - capability-scope derivation or validation for proposed subprocess actions
  - blocked-action outcomes for denied or unsupported action proposals
- Verification:
  - unit tests for risk classification, fingerprinting, and blocked-action
    planning
  - `cargo test --workspace --lib -- --nocapture`
- Evidence:
  - not started

### Task P5-09: Implement Telegram approval rendering and canonical resolution routing

- Status: `TODO`
- Depends on: `P5-05`, `P5-07`
- Parallel-safe: no
- Deliverables:
  - Telegram approval rendering that uses canonical approval objects with TTL,
    action fingerprint, and concise consequence summaries
  - approval callback or equivalent response handling that becomes a canonical
    approval-resolution event
  - policy and orchestration changes so approval callbacks stop being rejected
    as unsupported foreground triggers and are instead routed through the
    approval-resolution path
- Verification:
  - `cargo test -p harness --test foreground_component -- --nocapture`
  - `cargo test -p harness --test foreground_integration -- --nocapture`
- Evidence:
  - not started

### Task P5-10: Implement bounded subprocess execution with capability scoping

- Status: `TODO`
- Depends on: `P5-03`, `P5-05`, `P5-08`
- Parallel-safe: no
- Deliverables:
  - harness-owned subprocess execution path for the first governed action type
  - explicit capability scoping for filesystem reach, network posture,
    environment exposure, and execution budgets
  - timeout, failure, and malformed-result handling that fails closed
  - trace-linked audit history for execution start, completion, timeout,
    failure, and blocked execution
- Verification:
  - targeted unit tests for capability validation and timeout handling
  - `cargo test -p harness --test governed_actions_component -- --nocapture`
- Evidence:
  - not started

### Task P5-11: Integrate conscious-loop tool proposals with harness-owned execution orchestration

- Status: `TODO`
- Depends on: `P5-08`, `P5-09`, `P5-10`
- Parallel-safe: no
- Deliverables:
  - conscious-worker protocol and worker implementation changes needed to emit
    governed-action proposals through the canonical cross-process contract
  - conscious-loop result handling for governed action proposals
  - harness orchestration that routes actions into block, approval, or execute
    outcomes without bypassing policy
  - observation return path so conscious execution can receive approved tool or
    script outcomes through harness-managed channels
- Verification:
  - `cargo test -p workers -- --nocapture`
  - `cargo test -p harness --test governed_actions_integration -- --nocapture`
- Evidence:
  - not started

### Task P5-12: Implement workspace script artifact creation, versioning, and retrieval

- Status: `TODO`
- Depends on: `P5-06`, `P5-05`
- Parallel-safe: yes
- Deliverables:
  - workspace support for canonical script artifacts and script versions
  - explicit provenance and version-linkage rules for scripts used in governed
    execution
  - read paths that allow governed execution and operator inspection to resolve
    the canonical script version that was approved or run
- Verification:
  - `cargo test -p harness --test governed_actions_component -- --nocapture`
- Evidence:
  - not started

### Task P5-13: Implement governed script execution and run history

- Status: `TODO`
- Depends on: `P5-10`, `P5-12`
- Parallel-safe: no
- Deliverables:
  - execution path for governed workspace scripts using the same bounded
    subprocess and capability model
  - clear distinction between script artifact reads and script execution
  - durable script run history tied to approvals, execution records, and audit
    events as applicable
- Verification:
  - `cargo test -p harness --test governed_actions_component -- --nocapture`
  - `cargo test -p harness --test governed_actions_integration -- --nocapture`
- Evidence:
  - not started

### Task P5-14: Implement policy re-check, invalidation, and blocked-execution outcomes

- Status: `TODO`
- Depends on: `P5-07`, `P5-08`, `P5-10`
- Parallel-safe: no
- Deliverables:
  - policy re-check immediately before execution for approval-gated actions
  - explicit handling for expired approvals, changed action fingerprints,
    invalid scopes, and denied execution contexts
  - user-visible or operator-visible blocked-action outcome records that remain
    auditable and fail closed
- Verification:
  - unit tests for policy re-check failures and invalidation cases
  - `cargo test -p harness --test governed_actions_integration -- --nocapture`
- Evidence:
  - not started

### Task P5-15: Extend harness management services for workspace, approvals, and governed actions

- Status: `TODO`
- Depends on: `P5-06`, `P5-07`, `P5-11`, `P5-13`, `P5-14`
- Parallel-safe: no
- Deliverables:
  - harness-side management read models for pending approvals, recent governed
    actions, blocked-action diagnostics, workspace artifact summaries, script
    summaries, and script-run summaries
  - any narrow mutating management service needed for bounded approval
    resolution during local verification
  - status summary extensions for pending approvals and governed-action counts
- Verification:
  - `cargo test -p harness --test governed_actions_component -- --nocapture`
  - `cargo test -p runtime --test admin_cli -- --nocapture`
- Evidence:
  - not started

### Task P5-16: Implement `runtime admin approvals ...`

- Status: `TODO`
- Depends on: `P5-15`
- Parallel-safe: no
- Deliverables:
  - `runtime admin approvals list` command handler and formatter
  - `runtime admin approvals resolve` command handler and formatter
  - stable text output and `--json` output for pending and recently resolved
    approval state
  - bounded routing of CLI approval resolution through the canonical
    approval-validation path rather than direct storage mutation
- Verification:
  - `cargo test -p runtime --test admin_cli -- --nocapture`
  - `cargo run -p runtime -- admin approvals list`
- Evidence:
  - not started

### Task P5-17: Implement Phase 5 workspace and governed-action admin commands

- Status: `TODO`
- Depends on: `P5-15`
- Parallel-safe: yes
- Deliverables:
  - `runtime admin actions list`
  - `runtime admin workspace artifacts list`
  - `runtime admin workspace scripts list`
  - `runtime admin workspace runs list`
  - stable text output and `--json` output for each command
- Verification:
  - `cargo test -p runtime --test admin_cli -- --nocapture`
  - `cargo run -p runtime -- admin actions list`
  - `cargo run -p runtime -- admin workspace artifacts list`
- Evidence:
  - not started

### Task P5-18: Add unit coverage for risk classification, approvals, capabilities, and formatters

- Status: `TODO`
- Depends on: `P5-10`, `P5-14`, `P5-16`, `P5-17`
- Parallel-safe: no
- Deliverables:
  - unit tests for risk-tier classification and action fingerprinting
  - unit tests for approval validation, expiry, and policy re-check logic
  - unit tests for capability-scope validation and blocked execution paths
  - runtime CLI parsing and formatter tests for the Phase 5 admin commands
- Verification:
  - `cargo test --workspace --lib -- --nocapture`
  - `cargo test -p runtime --test admin_cli -- --nocapture`
- Evidence:
  - not started

### Task P5-19: Add PostgreSQL-backed component and integration coverage for Phase 5 flows

- Status: `TODO`
- Depends on: `P5-11`, `P5-13`, `P5-14`, `P5-18`
- Parallel-safe: no
- Deliverables:
  - `crates/harness/tests/governed_actions_component.rs`
  - `crates/harness/tests/governed_actions_integration.rs`
  - component coverage for workspace persistence, approval lifecycle,
    governed-action state, and blocked-action diagnostics
  - integration coverage for action proposal to approval to execution flow and
    action proposal to blocked-execution flow
- Verification:
  - `cargo test -p harness --test governed_actions_component -- --nocapture`
  - `cargo test -p harness --test governed_actions_integration -- --nocapture`
- Evidence:
  - not started

### Task P5-20: Extend repository CI and docs for the Phase 5 governed-action gate

- Status: `TODO`
- Depends on: `P5-19`
- Parallel-safe: no
- Deliverables:
  - repository-hosted CI updates for the `governed-actions` gate
  - command-surface documentation updates in the appropriate operator-facing
    docs
  - cleanup or replacement of any stale manual guidance that Phase 5
    management-surface commands supersede
  - final Phase 5 plan status updates and evidence completion in this document
- Verification:
  - manual review of `.github/workflows/ci.yml`
  - `cargo test --workspace`
  - manual review that updated operator docs match the implemented command
    surface
- Evidence:
  - not started
