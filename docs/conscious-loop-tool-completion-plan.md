# Conscious-Loop Tool Completion Plan

## Metadata

- Plan Status: COMPLETED
- Created: 2026-04-29
- Last Updated: 2026-04-29
- Owner: Coding agent
- Approval: APPROVED

## Status Legend

- Plan Status values: DRAFT, QUESTIONS PENDING, READY FOR APPROVAL, APPROVED, IN PROGRESS, COMPLETED, BLOCKED
- Task/Milestone Status values: TO BE DONE, IN PROGRESS, COMPLETED, BLOCKED, SKIPPED

## Goal

Implement every currently missing or partially model-usable conscious-loop tool
identified by `docs/wip/CONSCIOUS_LOOP_CAPABILITY_GAP_REPORT.md`, while
preserving the repository's harness-sovereign architecture. The end state is
that the assistant can discover, inspect, create, update, schedule, delegate,
and observe the required workspace, script, schedule, background, and wake-signal
capabilities only through harness-mediated governed actions or harness-approved
context surfaces.

## Scope

- Add model-facing governed action kinds for read-only workspace inspection and
  discovery.
- Add model-facing governed action kinds for workspace artifact creation and
  update.
- Add model-facing governed action kinds for workspace script creation,
  versioning, discovery, inspection, and run-history inspection.
- Add model-facing governed action support for scheduled foreground task
  creation and update.
- Add conscious-loop foreground delegation to background jobs through a
  harness-validated request contract.
- Complete or verify the wake-signal-to-approved-foreground-trigger path needed
  for policy-gated proactive behavior.
- Surface relevant tool, workspace, schedule, self-model, and memory-selection
  affordances in conscious context where context injection is safer than making
  the model query raw implementation state.
- Update internal documentation with exact implementation details and an E2E
  guide for adding an architecture-compliant tool.
- Add migrations, tests, config updates, user/operator documentation, and
  release verification for the completed tool surface.

## Non-Goals

- Do not replace the proposal-only governed-action mechanism with native model
  provider tool calls.
- Do not let the conscious worker directly mutate canonical memory,
  self-model artifacts, workspace tables, schedules, background jobs, or wake
  signals.
- Do not expose hidden unconscious-loop maintenance internals to the conscious
  model.
- Do not add a general admin shell, raw SQL operator workflow, or arbitrary
  unconstrained filesystem workspace.
- Do not broaden production channels beyond the existing Telegram-first v1
  posture.
- Do not implement enterprise RBAC, multi-tenant policy domains, or a new
  sandboxing backend unless a specific tool requires a narrow compatibility
  hook.

## Assumptions

- `docs/REQUIREMENTS.md`, `docs/LOOP_ARCHITECTURE.md`,
  `docs/IMPLEMENTATION_DESIGN.md`, and `PHILOSOPHY.md` remain canonical when
  they conflict with planning or internal documentation.
- `docs/wip/CONSCIOUS_LOOP_CAPABILITY_GAP_REPORT.md` is an analysis input, not
  a canonical behavior specification.
- New model-usable operations should be represented as governed actions unless
  the safer architecture is to inject bounded summaries into conscious context.
- Mutating operations must remain auditable, risk-classified, and approval-gated
  where policy requires it.
- Persistence-critical tests use disposable real PostgreSQL through existing
  test fixtures.
- New action kinds that are persisted in constrained `TEXT` columns require a
  reviewed SQL migration updating the relevant check constraints.
- Some planned payloads intentionally extend the current service contracts. When
  a planned field is not already represented in the service layer, implementation
  must either add the missing harness-owned validation/storage support or narrow
  the payload in this plan before execution.

## Open Questions

- None.

## Proposed Model-Visible Tool Surface

The implementation should converge on this model-facing surface unless a task
discovers a narrower existing contract that better fits the codebase:

| Capability | Proposed action or context surface | Default posture |
|---|---|---|
| Inspect one workspace artifact | `inspect_workspace_artifact` | Tier 0 read-only |
| List/search workspace artifacts | `list_workspace_artifacts` | Tier 0 read-only |
| Create note, runbook, scratchpad, task list | `create_workspace_artifact` | Tier 1 or Tier 2 by content/policy |
| Update note, runbook, scratchpad, task list | `update_workspace_artifact` | Tier 1 or Tier 2 by content/policy |
| List/search scripts | `list_workspace_scripts` or bounded context injection | Tier 0 read-only |
| Inspect script metadata/content | `inspect_workspace_script` | Tier 0 read-only |
| Create script | `create_workspace_script` | Tier 2 approval-gated by default |
| Append script version | `append_workspace_script_version` | Tier 2 approval-gated by default |
| Inspect script run history | `list_workspace_script_runs` | Tier 0 read-only |
| Create/update foreground schedule | `upsert_scheduled_foreground_task` | Tier 2 approval-gated by default |
| Request background work | `request_background_job` | Harness-validated, non-user-facing |
| Wake signal handling | No direct model tool; policy-approved trigger path | Harness-owned |

## Milestones

### Milestone 1: Tool Contract And Policy Foundation

- Status: COMPLETED
- Purpose: Define the stable cross-process and persisted shape for the expanded
  tool surface before adding individual tool backends.
- Exit Criteria: Contracts, migrations, policy classification, validation, and
  worker schema handling support the full planned action set without execution
  backends bypassing the harness.

#### Task 1.1: Finalize Governed Action Taxonomy

- Status: COMPLETED
- Objective: Produce the authoritative action-kind list and payload contracts
  for all missing or partial tools.
- Steps:
  1. Reconcile the proposed model-visible surface with
     `docs/wip/CONSCIOUS_LOOP_CAPABILITY_GAP_REPORT.md`.
  2. Update `crates/contracts/src/lib.rs` with new
     `GovernedActionKind` and `GovernedActionPayload` variants.
  3. Add serialization/deserialization tests for every new payload shape.
  4. Keep wake-signal conversion out of the model-visible action enum unless a
     concrete requirement proves direct model control is needed.
- Validation: `cargo test -p contracts --lib -- --nocapture`
- Notes: Preserve schema compatibility and avoid exposing unconscious-loop
  implementation details in the conscious contract.

#### Task 1.2: Add Persistence Migration For New Action Kinds

- Status: COMPLETED
- Objective: Persist every new governed action kind in audited execution and
  approval tables.
- Steps:
  1. Add the next reviewed migration under `migrations/`.
  2. Drop and recreate the constrained `action_kind` checks on
     `governed_action_executions` and `approval_requests`.
  3. Include all existing values plus the new action-kind strings.
  4. Add migration comments covering compatibility and corrective path.
- Validation: `cargo test -p harness --test migration_component -- --nocapture`
- Notes: Follow the pattern from `migrations/0009__web_fetch_action_kind.sql`.

#### Task 1.3: Centralize Payload Parsing And Canonicalization

- Status: COMPLETED
- Objective: Ensure each new action kind has a canonical internal payload before
  execution, approval, recovery, and management code sees it.
- Steps:
  1. Extend `crates/harness/src/governed_actions.rs` parsing for new action
     strings.
  2. Add canonical payload variants and conversion from contract payloads.
  3. Update every `GovernedActionKind` match in `approval.rs`,
     `management.rs`, `recovery.rs`, `foreground_orchestration.rs`, and
     `workers/src/main.rs`.
  4. Add exhaustive unit tests for parse, canonical conversion, and unknown
     action rejection.
- Validation: `cargo test --workspace --lib -- --nocapture`
- Notes: Compilation failures from exhaustive matching are useful; resolve them
  intentionally rather than adding wildcard arms.

#### Task 1.4: Extend Scope Validation And Risk Classification

- Status: COMPLETED
- Objective: Classify each new action by explicit capability scope, side-effect
  class, and approval posture.
- Steps:
  1. Extend `validate_capability_scope()` and proposal-shape validation for
     read-only, write, schedule, script-authoring, and background-delegation
     payloads.
  2. Extend `policy::classify_governed_action_risk()`.
  3. Keep read-only inspections Tier 0 and side-effecting or future-triggering
     operations approval-gated unless policy explicitly allows lower risk.
  4. Add tests for invalid filesystem, network, environment, budget, schedule,
     content-size, and script-language scopes.
- Validation: `cargo test -p harness --test governed_actions_component -- --nocapture`
- Notes: Side effects include delayed future user contact and background job
  creation, not only filesystem or network mutation.

#### Task 1.5: Expand Worker Schema Exposure Safely

- Status: COMPLETED
- Objective: Teach the conscious worker exactly which governed actions it may
  propose and the complete JSON shape for each.
- Steps:
  1. Update `governed_action_schema_message()` in `crates/workers/src/main.rs`.
  2. Include complete `capability_scope` examples for every alternate payload.
  3. Preserve the proposal-only instruction and at-most-one-action-per-turn
     constraint unless tests prove the orchestration can safely handle more.
  4. Update parsing/stripping tests for new action blocks.
- Validation: `cargo test -p workers -- --nocapture`
- Notes: The schema text is a model contract; incomplete examples commonly
  produce invalid JSON proposals.

### Milestone 2: Workspace Artifact Tools

- Status: COMPLETED
- Purpose: Make non-script workspace artifacts usable by the conscious assistant
  without confusing workspace state with autobiographical memory.
- Exit Criteria: The model can list, inspect, create, and update notes,
  runbooks, scratchpads, and task lists through governed actions with bounded
  observations and durable audit history.

#### Task 2.1: Implement Read-Only Artifact Inspection

- Status: COMPLETED
- Objective: Replace the current blocked `inspect_workspace_artifact` stub with
  a working read-only governed action.
- Steps:
  1. Use existing `workspace.rs` lookup services for artifact metadata and
     content.
  2. Reject archived, missing, wrong-kind, or unauthorized artifacts with
     explicit blocked observations.
  3. Return a bounded model-facing preview with truncation metadata.
  4. Store the full relevant execution metadata in governed-action execution
     payloads where appropriate.
- Validation: `cargo test -p harness --test governed_actions_component -- --nocapture`
- Notes: Remove the `NOT IMPLEMENTED` callout from internal docs only after this
  task is implemented and tested.

#### Task 2.2: Implement Artifact Listing And Search

- Status: COMPLETED
- Objective: Let the assistant discover available workspace artifacts without
  requiring pasted UUIDs.
- Steps:
  1. Add `list_workspace_artifacts` payload fields for kind filter, status
     filter, optional query, and limit.
  2. Reuse management/workspace listing code where it preserves capability
     boundaries.
  3. Return bounded summaries with IDs, kind, title, status, timestamps, and
     concise content snippets.
  4. Add tests for limits, filters, empty results, archived exclusion, and
     excessive output protection.
- Validation: `cargo test -p harness --test governed_actions_component -- --nocapture`
- Notes: Prefer deterministic query semantics over model-dependent filtering.

#### Task 2.3: Implement Artifact Creation

- Status: COMPLETED
- Objective: Let the assistant create notes, runbooks, scratchpads, and task
  lists as governed workspace artifacts.
- Steps:
  1. Add `create_workspace_artifact` execution using workspace service
     validation.
  2. Require artifact kind, title, content, provenance, and conversation or
     actor context where available.
  3. Enforce configured content-size and title limits.
  4. Record audit details and return the new artifact ID plus summary.
  5. Add approval tests for any policy-classified higher-risk creation.
- Validation: `cargo test -p harness --test governed_actions_component -- --nocapture`
- Notes: Workspace artifact creation is not canonical memory creation; do not
  route it through memory proposal tables.

#### Task 2.4: Implement Artifact Update

- Status: COMPLETED
- Objective: Let the assistant update non-script artifacts with conflict-aware
  provenance.
- Steps:
  1. Add `update_workspace_artifact` with artifact ID, expected version or
     content hash, replacement content or patch-style update, and change summary.
  2. Reject stale expected versions or archived artifacts.
  3. If the existing workspace service still updates artifacts in place, use
     `updated_at`, content hash, or another explicit optimistic-concurrency token
     rather than implying artifact revision history exists.
  4. Add an explicit artifact revision/audit model only if the implementation
     chooses to make non-script artifact history first-class.
  5. Return updated concurrency token and bounded content summary.
  6. Add tests for success, conflict, missing artifact, archived artifact, and
     size-limit rejection.
- Validation: `cargo test -p harness --test governed_actions_component -- --nocapture`
- Notes: Existing script versions are append-only; non-script artifact history
  must not be assumed unless a reviewed migration adds it.

### Milestone 3: Workspace Script Authoring And Discovery

- Status: COMPLETED
- Purpose: Complete the script lifecycle so the assistant can discover, inspect,
  author, version, execute, and review scripts through governed paths.
- Exit Criteria: Script execution no longer depends on the user pasting UUIDs,
  and script creation/versioning remains distinct from script execution policy.

#### Task 3.1: Implement Script Discovery

- Status: COMPLETED
- Objective: Let the assistant find existing scripts by bounded summaries.
- Steps:
  1. Add `list_workspace_scripts` or inject selected script summaries into
     conscious context when a trigger is likely to need them.
  2. Include script ID, latest version ID, title, language, status, updated time,
     and concise description.
  3. Add filters for status, language, optional query, and limit.
  4. Add tests for no scripts, limit truncation, archived scripts, and query
     filtering.
- Validation: `cargo test -p harness --test governed_actions_component -- --nocapture`
- Notes: If using context injection, also update `CONTEXT_ASSEMBLY.md` and add
  context assembly tests.

#### Task 3.2: Implement Script Inspection

- Status: COMPLETED
- Objective: Let the assistant inspect script metadata and bounded content before
  proposing execution or edits.
- Steps:
  1. Add `inspect_workspace_script` with script ID and optional version ID.
  2. Return metadata, latest version metadata, language, capability hints, and a
     bounded content preview.
  3. Reject missing, archived, or invalid version references.
  4. Add tests for latest-version resolution, explicit version inspection,
     truncation, and rejection cases.
- Validation: `cargo test -p harness --test governed_actions_component -- --nocapture`
- Notes: This is read-only and should not imply execution permission.

#### Task 3.3: Implement Script Creation

- Status: COMPLETED
- Objective: Let the assistant create first-class governed workspace scripts.
- Steps:
  1. Add `create_workspace_script` with title, language, content, description or
     rationale, and requested capability hints.
  2. Enforce language allowlist, content-size limits, and provenance metadata.
  3. Classify script creation separately from script execution.
  4. Route through approval by default.
  5. Return script ID, initial version ID, and concise summary after creation.
- Validation: `cargo test -p harness --test governed_actions_component -- --nocapture`
- Notes: Do not automatically execute a newly created script in the same
  immediate follow-up unless the existing governed-action loop is explicitly
  extended and tested for chained actions.

#### Task 3.4: Implement Script Version Append

- Status: COMPLETED
- Objective: Let the assistant edit scripts by appending auditable versions with
  conflict detection.
- Steps:
  1. Add `append_workspace_script_version` with script ID, expected latest
     version ID or content hash, language, content, and change summary.
  2. Reject stale versions, language mismatches, archived scripts, and size
     violations.
  3. Preserve existing script version history and provenance.
  4. Return new version ID and summary.
  5. Add tests for version conflict, successful append, policy approval, and
     post-append run using `run_workspace_script`.
- Validation: `cargo test -p harness --test governed_actions_component -- --nocapture`
- Notes: Script versioning must be append-only, not destructive overwrite.

#### Task 3.5: Implement Script Run-History Inspection

- Status: COMPLETED
- Objective: Let the assistant review prior script runs through bounded
  observations.
- Steps:
  1. Add `list_workspace_script_runs` with script ID, optional status filter,
     and limit.
  2. Include run ID, version ID, status, timestamps, exit code, and bounded
     stdout/stderr summaries.
  3. Protect against leaking excessive output into conscious context.
  4. Add tests for empty history, limit enforcement, failure runs, and output
     truncation.
- Validation: `cargo test -p harness --test governed_actions_component -- --nocapture`
- Notes: The immediate observation from a current run remains separate from
  historical inspection.

### Milestone 4: Scheduling And Background Delegation

- Status: COMPLETED
- Purpose: Complete model-usable future work: scheduled foreground tasks,
  conscious-to-background delegation, and policy-approved proactive wakeups.
- Exit Criteria: The assistant can propose future foreground work and
  background work through harness-owned contracts, and accepted wake signals can
  become approved foreground triggers under policy.

#### Task 4.1: Implement Scheduled Foreground Task Upsert

- Status: COMPLETED
- Objective: Let the assistant create or update reminders, check-ins, and
  recurring foreground tasks through a governed action.
- Steps:
  1. Add `upsert_scheduled_foreground_task` payload fields for task key, title,
     user-facing prompt, next due time in UTC, cadence or one-shot mode,
     optional cooldown, conversation binding, and active status.
  2. Reuse `scheduled_foreground::upsert_task()` and management validation.
  3. If user-local timezone input is accepted, convert it to UTC before calling
     the service and add explicit validation for invalid timezone names.
  4. If one-shot reminders are not already supported, implement explicit
     disabled-after-first-run semantics or narrow this plan before execution.
  5. Classify schedule creation/update as approval-gated by default.
  6. Return confirmation with task key, next due time, cadence or one-shot mode,
     and status.
  7. Add tests for one-shot, recurring, invalid cadence, optional invalid
     timezone, approval, update, and runtime firing.
- Validation: `cargo test -p harness --test governed_actions_integration -- --nocapture`
- Notes: Future user contact is a side effect even when no external process runs
  immediately.

#### Task 4.2: Implement Background Job Request Action

- Status: COMPLETED
- Objective: Let the conscious loop request bounded unconscious work without
  directly controlling worker internals.
- Steps:
  1. Define `request_background_job` payload fields for allowed job kind,
     rationale, input scope reference, urgency, and optional wake preference.
  2. Validate job kind and input scope in the harness.
  3. Persist accepted requests through existing background planning/scheduling
     paths.
  4. Return an observation with accepted/rejected status and background job ID
     when accepted.
  5. Add tests for allowed job kinds, invalid scope, duplicate or excessive
     requests, and audit emission.
- Validation: `cargo test -p harness --test unconscious_integration -- --nocapture`
- Notes: The conscious loop requests work; it does not instantiate or supervise
  unconscious workers.

#### Task 4.3: Complete Wake-Signal Foreground Trigger Verification

- Status: COMPLETED
- Objective: Prove policy-approved wake signals reliably become foreground
  triggers and suppressed/deferred signals do not.
- Steps:
  1. Audit `background_execution.rs`, `background.rs`, policy evaluation, and
     foreground orchestration for pending gaps.
  2. Implement missing routing, recovery, or status updates if tests expose a
     gap.
  3. Add tests for approve, throttle, defer, drop, duplicate active signal, and
     operator visibility.
  4. Ensure approved wake-signal triggers preserve the conscious/unconscious
     context boundary.
- Validation: `cargo test -p harness --test unconscious_integration -- --nocapture`
- Notes: This should remain a harness-owned path, not a direct model tool.

#### Task 4.4: Add Context Affordance Summaries

- Status: SKIPPED
- Objective: Give the conscious model enough bounded awareness of available
  scripts, schedules, and workspace affordances to choose useful tools.
- Steps:
  1. Extend context assembly with small, bounded summaries where listing actions
     alone would make routine use awkward.
  2. Include only user-action-relevant affordances, not hidden maintenance
     machinery.
  3. Verify the compact self-model and selected memory still expose
     action-relevant capabilities, constraints, preferences, and relevant
     workspace/script/schedule affordances without leaking storage internals.
  4. Add token-budget limits and truncation metadata.
  5. Update tests for message ordering, budget enforcement, and observation
     positioning.
- Validation: `cargo test -p harness --test continuity_component -- --nocapture`
- Notes: Skipped in this implementation because the completed
  `list_workspace_artifacts`, `list_workspace_scripts`, and
  `list_workspace_script_runs` governed actions provide bounded discovery
  without injecting additional routine context. No context assembly behavior
  changed.

### Milestone 5: Management, Documentation, And Operator Surface

- Status: COMPLETED
- Purpose: Make the expanded tool system understandable, inspectable, and
  maintainable for future agents and operators.
- Exit Criteria: Internal docs, operator docs, and admin surfaces describe the
  live tool system accurately, including exact extension steps for adding a new
  compliant tool E2E.

#### Task 5.1: Update Management Read Surfaces

- Status: COMPLETED
- Objective: Ensure operators can inspect tool executions, approvals, workspace
  artifacts, scripts, schedules, background requests, and wake signals without
  raw SQL.
- Steps:
  1. Extend existing management summaries only where new action data is not
     already visible.
  2. Add or adjust `runtime admin` output for new governed action kinds and
     background-delegation records.
  3. Preserve human-readable and machine-readable output where existing commands
     support both.
  4. Add parser/rendering tests for runtime CLI changes.
- Validation: `cargo test -p runtime --test admin_cli -- --nocapture`
- Notes: Mutating operator workflows are not required unless a tool creates a
  new state type that lacks any safe inspection surface.

#### Task 5.2: Update Governed Actions Internal Documentation

- Status: COMPLETED
- Objective: Make `docs/internal/conscious_loop/GOVERNED_ACTIONS.md` match the
  expanded live implementation.
- Steps:
  1. Update the source file table with current symbols and line references.
  2. Replace the old three-action schema with the complete action list.
  3. Document every payload shape, validation rule, risk tier, approval path,
     observation shape, and config key.
  4. Remove stale `NOT IMPLEMENTED` callouts after their backends exist.
  5. Re-stamp the last verified branch/session date.
- Validation: Manual inspection confirms every referenced `path:line` points to
  the correct symbol and `cargo test -p harness --test governed_actions_component -- --nocapture` passes.
- Notes: Internal docs must not contradict canonical docs.

#### Task 5.3: Add E2E Tool Implementation Guide

- Status: COMPLETED
- Objective: Document how to implement one architecture-compliant tool from
  requirements to tests and docs.
- Steps:
  1. Create `docs/internal/harness/TOOL_IMPLEMENTATION.md` using the internal
     four-section template.
  2. Cover contract variant, payload shape, migration constraints, validation,
     risk classification, approval routing, execution backend, observation
     formatting, worker schema exposure, management visibility, recovery match
     arms, tests, and documentation updates.
  3. Include a philosophy checklist mapping the implementation to harness-heavy
     design, conscious boundary, traceability, and no direct canonical writes.
  4. Link the guide from `docs/internal/INTERNAL_DOCUMENTATION.md`.
- Validation: Manual inspection confirms the guide follows the required
  internal-doc template and can be followed without reading this plan.
- Notes: This is the secondary goal requested by the user and should be treated
  as a deliverable, not cleanup.

#### Task 5.4: Update Context Assembly And Related Internal Docs

- Status: SKIPPED
- Objective: Keep internal docs accurate for any new conscious-context affordance
  summaries and observation positioning.
- Steps:
  1. Update `docs/internal/conscious_loop/CONTEXT_ASSEMBLY.md` for new script,
     artifact, schedule, or tool-affordance context.
  2. Add new internal docs for background jobs or wake-signal routing if this
     implementation changes those subsystems materially.
  3. Update `docs/internal/INTERNAL_DOCUMENTATION.md` planned additions list as
     docs are created.
  4. Re-stamp verified dates and verify line references.
- Validation: Manual rendered-Markdown inspection plus exact file/line reference
  spot checks.
- Notes: Skipped because this implementation did not add context affordance
  summaries or change observation positioning. `GOVERNED_ACTIONS.md` documents
  the live discovery actions instead.

#### Task 5.5: Update User And Operator Documentation

- Status: COMPLETED
- Objective: Document the expanded tool capabilities from the operator and user
  perspective without exposing raw implementation internals.
- Steps:
  1. Update `docs/USER_MANUAL.md` for model-usable workspace, script, schedule,
     approval, and background-delegation behavior.
  2. Update `README.md` only if the repository-level capability summary is now
     stale.
  3. Avoid temporary execution-status language and planning labels in canonical
     behavior docs.
  4. Verify terminology against `PHILOSOPHY.md`,
     `docs/REQUIREMENTS.md`, `docs/LOOP_ARCHITECTURE.md`, and
     `docs/IMPLEMENTATION_DESIGN.md`.
- Validation: `git diff -- docs/ PHILOSOPHY.md README.md AGENTS.md`
- Notes: Do not promote `docs/wip/CONSCIOUS_LOOP_CAPABILITY_GAP_REPORT.md` into
  canonical behavior.

### Milestone 6: Cleanup And Final Verification

- Status: COMPLETED
- Purpose: Ensure the repository contains only intentional final artifacts and
  the complete tool surface is verified at the correct test layers.
- Exit Criteria: Intermediate artifacts are removed, all required verification
  passes or blockers are recorded, and the plan can be marked COMPLETED after
  implementation.

#### Task 6.1: Cleanup Intermediate Artifacts

- Status: COMPLETED
- Objective: Remove artifacts created only to support implementation.
- Steps:
  1. Inspect the worktree for temporary docs, scratch scripts, one-off fixtures,
     generated data, logs, and obsolete plan fragments.
  2. Remove only artifacts that are not part of the intended final repository
     state.
  3. Keep maintainable tests, fixtures, internal docs, and this plan as the
     execution ledger unless the user asks to archive it.
  4. Leave unrelated user-provided files untouched.
- Validation: `cmd.exe /c git status --short` and
  `cmd.exe /c git diff --name-only`
- Notes: Windows Git is the source of truth for worktree decisions in this
  repository.

#### Task 6.2: Run Focused Tool Verification

- Status: COMPLETED
- Objective: Verify the full expanded governed-action surface at component and
  integration layers.
- Steps:
  1. Run the governed-action component and integration suites.
  2. Include regression coverage for existing working actions:
     `run_subprocess`, `run_workspace_script`, and `web_fetch`.
  3. Run workspace, management, foreground, unconscious, and continuity suites
     touched by the implementation.
  4. Fix failures and rerun the failing suites until clean.
- Validation:
  `cargo test -p harness --test governed_actions_component -- --nocapture`;
  `cargo test -p harness --test governed_actions_integration -- --nocapture`;
  `cargo test -p harness --test management_component -- --nocapture`;
  `cargo test -p harness --test foreground_integration -- --nocapture`;
  `cargo test -p harness --test unconscious_integration -- --nocapture`;
  `cargo test -p harness --test continuity_component -- --nocapture`
- Notes: Use the lowest failing layer to diagnose regressions before rerunning
  broader suites.

#### Task 6.3: Run Repository Verification

- Status: COMPLETED
- Objective: Verify formatting, compile, lint, migrations, and broad tests after
  cleanup.
- Steps:
  1. Run formatting, compile, lint, migration, and workspace test gates.
  2. Run the standard pre-commit bundle if the focused gates pass.
  3. Record any environment-only blocker with exact command and failure.
- Validation:
  `cargo fmt --all --check`;
  `cargo check --workspace`;
  `cargo clippy --workspace --all-targets -- -D warnings`;
  `cargo test -p harness --test migration_component -- --nocapture`;
  `cargo test --workspace`;
  `./scripts/pre-commit.ps1`
- Notes: If PowerShell script policy blocks local execution, run the documented
  bash equivalent from WSL or record the blocker.

## Approval Gate

Implementation approved by the user on 2026-04-29.

## Plan Self-Check

- [x] Plan location follows the default location rule.
- [x] Plan status is `READY FOR APPROVAL`.
- [x] Scope, non-goals, assumptions, and open questions are explicit.
- [x] Tasks are grouped into milestones because the plan has more than 10 tasks.
- [x] Every task has concrete steps and validation.
- [x] Every milestone has exit criteria.
- [x] Cleanup and final verification are included.
- [x] The plan avoids vague actions without concrete targets.
- [x] The plan can be executed by a coding agent without reading the original conversation.

## Full Self-Check Findings

- Checked: 2026-04-29
- Result: PASS after corrections in this revision.
- Evidence checked: gap report coverage, canonical architecture/philosophy
  alignment, internal governed-actions extension rules, workspace service shape,
  scheduled foreground service shape, worker crate test targets, ASCII content,
  and Windows Git worktree status.
- Corrections made:
  - Replaced invalid `cargo test -p workers --lib -- --nocapture` validation
    with `cargo test -p workers -- --nocapture`, because `crates/workers` is a
    binary crate with integration tests and no `src/lib.rs`.
  - Reworded artifact update planning so it does not assume non-script artifact
    version history exists in the current workspace service.
  - Reworded schedule payload planning to match the current UTC
    `scheduled_foreground` service fields while still leaving room for validated
    user-local timezone parsing.
  - Added explicit regression coverage for existing working governed actions.
  - Expanded context-affordance work to include action-relevant self-model and
    memory-selection context, not only workspace/script/schedule summaries.
- Residual risks:
  - The proposed action taxonomy is intentionally broader than current contracts;
    Task 1.1 must either confirm each action kind or narrow the surface before
    implementation starts.
  - One-shot reminders may require new service semantics because the current
    scheduled foreground service is cadence-oriented.
  - Non-script artifact conflict detection may require a new explicit
    concurrency token or artifact revision model.

## Execution Notes

- Update this plan status to `APPROVED` only after the user approves execution.
- Update this plan status to `IN PROGRESS` before implementation starts.
- Before starting a task, update that task to `IN PROGRESS`.
- After a task's validation passes, immediately update it to `COMPLETED`.
- Mark tasks or milestones `BLOCKED` with a concrete reason when progress
  cannot continue.
- Keep `docs/internal/` updates in the same commits as behavior changes that
  affect their line references or implementation claims.
