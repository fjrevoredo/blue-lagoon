# Real-World Assistant Gap-Closure Implementation Plan

## Metadata

- Plan Status: IN PROGRESS
- Created: 2026-05-15
- Last Updated: 2026-05-15
- Owner: Coding agent
- Approval: APPROVED

## Status Legend

- Plan Status values: DRAFT, QUESTIONS PENDING, READY FOR APPROVAL, APPROVED, IN PROGRESS, COMPLETED, BLOCKED
- Task/Milestone Status values: TO BE DONE, IN PROGRESS, COMPLETED, BLOCKED, SKIPPED

## Goal

Implement the post-v1 gap-closure roadmap defined in
`docs/REAL_WORLD_ASSISTANT_GAP_CLOSURE_DESIGN.md` so Blue Lagoon can support
practical daily workflows (attachments, external workflow integrations,
autonomous maintenance origination, route specialization, stronger approval UX,
and bounded collaboration) while preserving harness sovereignty and fail-closed
behavior.

## Current Status

The repository currently has a stable harness-governed runtime with:

- governed actions, approvals, and durable audit trails
- scheduled foreground tasks and bounded unconscious jobs
- Telegram-first foreground channel
- management CLI for operational inspection and control

Known implementation gaps relative to the new design baseline:

- no governed attachment processing pipeline
- no typed first-class external workflow integrations
- no autonomous background origination planner stage
- no separate unconscious model route
- approval flow exists, but in-channel status UX needs hardening
- archived immutability behavior exists in code but is under-specified in
  canonical docs
- principal model remains single-user-first

## Scope

- Deliver implementation phases R1-R7 from
  `docs/REAL_WORLD_ASSISTANT_GAP_CLOSURE_DESIGN.md`.
- Update canonical and internal docs when behavior changes.
- Add schema, contracts, harness logic, CLI surfaces, and automated tests for
  each new capability.
- Preserve existing harness-first architecture constraints.

## Non-Goals

- Full multi-tenant architecture.
- Enterprise-grade RBAC or policy domain expansion.
- Kubernetes/distributed worker redesign.
- Browser admin console.
- Unbounded direct model access to external integrations.

## Assumptions

- New schema work will use the next available reviewed migration numbers.
- External integration adapters will be implemented behind harness-owned
  abstractions and tested with deterministic fakes first.
- Telegram remains the primary user-facing surface during this plan.
- Existing runtime behavior that already satisfies design intent should be
  hardened or documented, not replaced.

## Open Questions

None.

Resolved on 2026-05-15:

- Milestone 3 should start with the easiest integration slice first; this plan
  treats calendar as Wave 1 and expands to email/task sync after that baseline.
- Milestone 5 should fail closed by default on unconscious-route
  misconfiguration.
- Milestone 7 approval resolution should be configurable and allowed for
  delegates by default.
- Milestone 2 first slice should prioritize document processing; photo/OCR
  support is optional in the same milestone only if straightforward.

## Milestones

### Milestone 1: Canonical Contract Alignment (R1)

- Status: COMPLETED
- Purpose: Align canonical docs with implemented behavior and explicit
  gap-closure scope before code expansion.
- Exit Criteria: Canonical docs clearly define archived immutability and post-v1
  expansion boundaries, with no known contradictions to current implementation.

#### Task 1.1: Promote Archived Immutability To Canonical Requirements

- Status: COMPLETED
- Objective: Add explicit normative requirements that archived workspace
  artifacts/scripts are immutable unless restored by an explicit controlled path.
- Steps:
  1. Update `docs/REQUIREMENTS.md` with explicit MUST/MUST NOT language for
     archived-state mutation rules.
  2. Add expected restore/unarchive governance requirements.
  3. Cross-check language against existing governed-action behavior.
- Validation: Manual review confirms requirements include immutable archived
  behavior and contain no conflicting rules.
- Notes: Keep requirement wording technology-agnostic.

#### Task 1.2: Update Implementation Design And User Manual For Approval/Archive Reality

- Status: COMPLETED
- Objective: Ensure design and user docs reflect current approval-callback
  behavior and archived mutability posture accurately.
- Steps:
  1. Update `docs/IMPLEMENTATION_DESIGN.md` where archive lifecycle behavior is
     described or implied.
  2. Update `docs/USER_MANUAL.md` approval workflow section to include callback
     resolution posture and fallback command behavior.
  3. Validate no regressions in stated single-user-first scope.
- Validation: `git diff -- docs/IMPLEMENTATION_DESIGN.md docs/USER_MANUAL.md docs/REQUIREMENTS.md`
  shows coherent, non-contradictory changes.
- Notes: Preserve stable operator workflow guidance.

#### Task 1.3: Reconcile Planning/Use-Case Documentation Drift

- Status: COMPLETED
- Objective: Correct stale status statements in planning-oriented docs that no
  longer match implemented tests.
- Steps:
  1. Review `docs/USE_CASE_CATALOG.md` and related planning docs for stale
     `Partial` markers already implemented.
  2. Update statuses and gap text where evidence exists in tests.
  3. Keep unresolved gaps explicitly listed.
- Validation: `cargo test -p harness --test use_case_scenarios -- --nocapture`.
- Notes: Do not invent completion claims without test evidence.

### Milestone 2: Attachment Processing Pipeline (R2)

- Status: COMPLETED
- Purpose: Make user attachments first-class governed inputs with bounded
  parsing and context projection.
- Exit Criteria: Attachments can be ingested, processed through harness-owned
  flow, and surfaced in bounded conscious context with tests.

#### Task 2.1: Add Attachment Persistence Schema

- Status: COMPLETED
- Objective: Introduce reviewed schema for attachment processing state and
  extracted content references.
- Steps:
  1. Add migration(s) for attachment records, processing attempts, and extracted
     artifact links.
  2. Add indexes for lookup by ingress event, status, and updated time.
  3. Keep raw payload references bounded and auditable.
- Validation: `cargo test -p harness --test migration_component -- --nocapture`.
- Notes: Reuse existing ingress attachment metadata where possible.

#### Task 2.2: Extend Contracts For Attachment Actions And Summaries

- Status: COMPLETED
- Objective: Add cross-process types for attachment inspection and processing
  requests/results.
- Steps:
  1. Extend `crates/contracts/src/lib.rs` with attachment action payloads and
     summary types.
  2. Add serialization tests for new payload families.
  3. Preserve compatibility for existing governed-action payload parsing.
- Validation: `cargo test -p contracts --lib`.
- Notes: Keep action names domain-oriented.

#### Task 2.3: Implement Harness Attachment Services

- Status: COMPLETED
- Objective: Add harness-side storage/services for attachment processing
  lifecycle and extracted-content storage.
- Steps:
  1. Add or extend harness modules for attachment CRUD and processing status.
  2. Implement bounded MIME-aware extraction entry points (text/document first).
  3. Persist extracted content with provenance to source attachment.
- Validation: `cargo test -p harness --test foundation_component -- --nocapture`.
- Notes: Keep processing deterministic and fail closed on unsupported types.

#### Task 2.4: Add Governed Attachment Actions

- Status: COMPLETED
- Objective: Expose attachment inspection and processing through governed
  actions with proper risk/policy checks.
- Steps:
  1. Add governed action kinds and payload validation.
  2. Add execution handlers and observation payload caps.
  3. Route higher-risk operations through approval flow where required.
- Validation: `cargo test -p harness --test governed_actions_component -- --nocapture`.
- Notes: Respect existing capability-scope enforcement rules.

#### Task 2.5: Add Attachment Context Projection

- Status: COMPLETED
- Objective: Inject bounded extracted attachment summaries/excerpts into
  conscious context assembly.
- Steps:
  1. Extend context assembly inputs with selected attachment-derived summaries.
  2. Add token/size caps and truncation telemetry.
  3. Ensure raw unbounded attachment payloads are never injected.
- Validation: `cargo test -p harness --test foreground_component -- --nocapture`.
- Notes: Preserve existing context budget behavior.

#### Task 2.6: Add End-To-End Attachment Tests

- Status: COMPLETED
- Objective: Prove attachment flow from Telegram ingress through governed
  processing to context-visible assistant behavior.
- Steps:
  1. Add fixture-driven integration tests for document ingestion as the required
     first slice.
  2. Assert attachment metadata persistence and processing status transitions.
  3. Assert bounded attachment content appears in model request context.
  4. Add photo-ingestion coverage only if the extraction path is straightforward
     without destabilizing the document-first delivery slice.
- Validation: `cargo test -p harness --test foreground_integration -- --nocapture`.
- Notes: Include failure-path coverage for unsupported attachments.

### Milestone 3: Typed Workflow Integrations (R3)

- Status: TO BE DONE
- Purpose: Add first-class harness-governed workflow integrations, beginning
  with calendar as the easiest vertical slice, then extending to email and task
  sync.
- Exit Criteria: Calendar integration is live first with policy/approval/audit
  coverage, then email/task sync extensions follow the same governed pattern.

#### Task 3.1: Define Integration Adapter Interfaces

- Status: TO BE DONE
- Objective: Introduce harness-owned adapter traits and configuration surfaces
  for external workflow systems.
- Steps:
  1. Add adapter interfaces in harness with calendar adapter as the required
     Wave 1 implementation target.
  2. Add config structures and fail-closed validation when enabled but
     misconfigured.
  3. Add deterministic fake adapters for automated tests and extension points
     for later email/task sync adapters.
- Validation: `cargo check --workspace`.
- Notes: Keep adapters provider-agnostic at interface boundary.

#### Task 3.2: Add Integration Governed Action Contracts

- Status: TO BE DONE
- Objective: Extend governed action kinds/payloads for calendar operations first
  and define compatible extension pattern for email/task sync.
- Steps:
  1. Add calendar contract enum/payload variants and risk-tier mapping inputs.
  2. Add parser and schema prompts for new action families.
  3. Add compatibility parsing tests for malformed payload rejection.
  4. Add follow-on contract variants for email/task sync after calendar payload
     validation is stable.
- Validation: `cargo test -p contracts --lib`.
- Notes: Separate read-only and side-effecting payload shapes.

#### Task 3.3: Implement Integration Action Execution In Harness

- Status: TO BE DONE
- Objective: Execute calendar integration actions first, then extend to
  email/task sync through the same adapter and governance layer.
- Steps:
  1. Add governed-action execution handlers for calendar integration kinds.
  2. Enforce capability scopes, risk classification, and approval requirements.
  3. Persist execution outcomes and structured error reasons.
  4. Extend handlers to email/task sync after calendar baseline is validated.
- Validation: `cargo test -p harness --test governed_actions_integration -- --nocapture`.
- Notes: Preserve existing recovery classification rules.

#### Task 3.4: Add Integration Management Inspection Commands

- Status: TO BE DONE
- Objective: Extend management/admin surfaces for integration run visibility and
  troubleshooting.
- Steps:
  1. Add management service queries for calendar integration execution
     summaries.
  2. Add runtime admin subcommands and text/json renderers.
  3. Add parser/render tests for new admin commands.
  4. Extend management inspection to email/task sync actions after calendar
     command surface is stable.
- Validation: `cargo test -p runtime --test admin_cli -- --nocapture`.
- Notes: Keep output automation-friendly with `--json`.

#### Task 3.5: Add Integration Component/Integration Tests

- Status: TO BE DONE
- Objective: Prove read/write integration flows across approval and failure
  paths.
- Steps:
  1. Add component tests for calendar policy and adapter error handling.
  2. Add integration tests for approval-required calendar actions.
  3. Add tests for audit trail and diagnostics emission.
- Validation: `cargo test -p harness --test governed_actions_component -- --nocapture`.
- Notes: Include idempotency assertions for retry/recovery scenarios.

#### Task 3.6: Expand Integration Coverage To Email And Task Sync

- Status: TO BE DONE
- Objective: Extend the proven calendar integration pattern to email and task
  sync workflows.
- Steps:
  1. Implement email integration action handlers and adapter-backed execution
     paths.
  2. Implement task-sync integration action handlers and adapter-backed
     execution paths.
  3. Add approval/policy/audit coverage and management inspection for both
     action families.
- Validation: `cargo test -p harness --test governed_actions_integration -- --nocapture`.
- Notes: Start only after calendar integration tests are stable.

### Milestone 4: Autonomous Background Origination (R4)

- Status: TO BE DONE
- Purpose: Add deterministic harness-side origination of background work from
  configured thresholds/schedules.
- Exit Criteria: Scheduler can originate due jobs without manual enqueue and
  still enforce deduplication/budgets/audit rules.

#### Task 4.1: Add Scheduler Planning Stage

- Status: TO BE DONE
- Objective: Introduce planner stage in runtime scheduler before due-job
  execution leasing.
- Steps:
  1. Extend `run_background_scheduler_iteration()` flow with planning pass.
  2. Invoke a new planner function with deterministic inputs.
  3. Preserve current execution leasing behavior for already-planned jobs.
- Validation: `cargo test -p harness --test unconscious_component -- --nocapture`.
- Notes: Keep planner and executor responsibilities separate.

#### Task 4.2: Implement Threshold/Schedule Trigger Producers

- Status: TO BE DONE
- Objective: Generate background planning requests from thresholds and periodic
  schedule criteria.
- Steps:
  1. Implement producers for volume-threshold and time-schedule triggers.
  2. Reuse `plan_background_job()` for final planning path.
  3. Add clear rationale payloads for generated triggers.
- Validation: `cargo test -p harness --test unconscious_component -- --nocapture`.
- Notes: Start with bounded, explicit trigger classes first.

#### Task 4.3: Add Deduplication, Telemetry, And Audit Coverage

- Status: TO BE DONE
- Objective: Ensure autonomous planning remains bounded and diagnosable.
- Steps:
  1. Add planner-level dedup checks and counters.
  2. Emit audit events for planned/skipped/blocked originations.
  3. Expose planning diagnostics in management queries where appropriate.
- Validation: `cargo test -p harness --test unconscious_integration -- --nocapture`.
- Notes: Prevent planner spam under high-frequency polling.

#### Task 4.4: Add End-To-End Autonomous Origination Tests

- Status: TO BE DONE
- Objective: Prove originate -> execute -> merge path from runtime scheduler.
- Steps:
  1. Add integration tests where no manual enqueue occurs.
  2. Assert trigger kind persistence and execution completion.
  3. Assert dedup behavior across repeated iterations.
- Validation: `cargo test -p harness --test unconscious_integration -- --nocapture`.
- Notes: Include failure-path assertions for budget and policy gating.

### Milestone 5: Foreground/Unconscious Model Route Split (R5)

- Status: COMPLETED
- Purpose: Separate user-facing and background model routing, policy, and cost
  posture.
- Exit Criteria: `model_gateway.unconscious.*` exists, is validated, and is
  used by background model calls with automated coverage, with fail-closed
  default behavior on misconfiguration.

#### Task 5.1: Extend Model Gateway Config For Unconscious Route

- Status: COMPLETED
- Objective: Add unconscious route config, defaults, and validation.
- Steps:
  1. Extend config structs and loader logic with
     `model_gateway.unconscious.*`.
  2. Add environment override handling where appropriate.
  3. Add explicit fail-closed validation rules for misconfiguration.
- Validation: `cargo test -p harness --lib config -- --nocapture`.
- Notes: Avoid silent fallback unless explicitly configured.

#### Task 5.2: Route Background Calls Through Unconscious Route

- Status: COMPLETED
- Objective: Ensure `execute_background_model_call()` uses unconscious route
  resolution, not foreground route reuse.
- Steps:
  1. Split route resolution logic in `model_gateway.rs`.
  2. Keep request-shape validation for unconscious loop calls.
  3. Preserve provider-agnostic reasoning policy behavior.
- Validation: `cargo test -p harness --lib model_gateway -- --nocapture`.
- Notes: Keep compatibility behavior explicitly documented and tested.

#### Task 5.3: Update Internal Provider/Background Docs

- Status: COMPLETED
- Objective: Update internal docs to remove stale "not implemented" callouts
  once route split is live.
- Steps:
  1. Update `docs/internal/harness/MODEL_PROVIDERS.md`.
  2. Update `docs/internal/unconscious_loop/BACKGROUND_JOBS.md`.
  3. Re-verify source line references and restamp verified dates.
- Validation: Manual inspection confirms no stale route-split callouts remain.
- Notes: Keep internal docs consistent with canonical docs.

### Milestone 6: Approval UX Hardening (R6)

- Status: TO BE DONE
- Purpose: Improve approval status clarity and follow-up messaging while keeping
  canonical approval semantics unchanged.
- Exit Criteria: In-channel approval messages clearly represent lifecycle
  outcomes, with idempotent behavior preserved across callback/command paths.

#### Task 6.1: Improve Approval Prompt And Follow-Up Messaging

- Status: TO BE DONE
- Objective: Provide clear user-facing status text for pending, approved,
  rejected, expired, and invalidated approvals.
- Steps:
  1. Update Telegram approval prompt/follow-up text builders.
  2. Include concise lifecycle state and next-step messaging.
  3. Keep inline callback and command fallback paths aligned.
- Validation: `cargo test -p harness --lib telegram -- --nocapture`.
- Notes: Keep copy compact and deterministic for tests.

#### Task 6.2: Add Multi-Approval Context And Idempotency Guards

- Status: TO BE DONE
- Objective: Improve handling and messaging when multiple approvals are pending
  or duplicate resolutions arrive.
- Steps:
  1. Extend foreground orchestration approval summary logic.
  2. Confirm repeated callback/command submissions remain idempotent.
  3. Emit clear diagnostics for stale/unknown tokens.
- Validation: `cargo test -p harness --test foreground_component -- --nocapture`.
- Notes: Preserve existing fail-closed behavior for malformed callback data.

#### Task 6.3: Add Approval UX Integration Coverage

- Status: TO BE DONE
- Objective: Prove approval lifecycle messaging and resolution paths end to end.
- Steps:
  1. Add integration tests for callback-based approve/reject flows.
  2. Add integration tests for fallback `/approve` and `/reject` command flows.
  3. Assert canonical approval records are identical across resolution channels.
- Validation: `cargo test -p harness --test foreground_integration -- --nocapture`.
- Notes: Include expiry/invalidation coverage where feasible.

### Milestone 7: Bounded Multi-Principal Collaboration (R7)

- Status: TO BE DONE
- Purpose: Extend single-user-first runtime to owner+delegate collaboration
  without introducing enterprise multi-tenant complexity.
- Exit Criteria: Allowlisted delegate principals can participate through bounded
  policy and actor attribution rules, with full auditability, and approval
  resolution policy is configurable with delegate-allowed default.

#### Task 7.1: Add Principal/Binding Schema Extensions

- Status: TO BE DONE
- Objective: Add durable structures for owner/delegate principal bindings.
- Steps:
  1. Add migration(s) for principal allowlist and conversation binding metadata.
  2. Add indexes for principal and conversation lookup.
  3. Preserve backward compatibility for single-principal deployments.
- Validation: `cargo test -p harness --test migration_component -- --nocapture`.
- Notes: Keep schema minimal and policy-oriented.

#### Task 7.2: Extend Ingress Normalization For Allowlisted Principals

- Status: TO BE DONE
- Objective: Map ingress events to configured owner/delegate principals safely.
- Steps:
  1. Extend Telegram binding config/resolution rules for bounded principal sets.
  2. Update ingress normalization authorization checks.
  3. Ensure rejected principal attempts are fail-closed and audited.
- Validation: `cargo test -p harness --test foreground_component -- --nocapture`.
- Notes: Preserve strict private-chat posture unless explicitly expanded later.

#### Task 7.3: Extend Policy/Approval Attribution For Delegates

- Status: TO BE DONE
- Objective: Ensure approvals and governed actions retain clear actor identity
  under collaboration flows.
- Steps:
  1. Extend approval validation to support configured delegate actor refs with
     delegate-allowed default behavior.
  2. Enforce owner/delegate boundaries in governed-action policy evaluation.
  3. Add configuration to switch approval resolution policy between
     delegate-allowed and owner-only modes.
  4. Add audit payload fields for principal attribution consistency.
- Validation: `cargo test -p harness --test governed_actions_component -- --nocapture`.
- Notes: Reject ambiguous actor identity transitions.

#### Task 7.4: Add Collaboration Integration Tests

- Status: TO BE DONE
- Objective: Prove delegate-triggered and owner-approved flows under bounded
  policy rules.
- Steps:
  1. Add integration tests for delegate approval resolution under default
     delegate-allowed policy.
  2. Add integration tests for owner-only policy mode and denied delegate
     approval attempts.
  3. Add integration tests for allowed and denied delegate actions.
  4. Verify audit and approval records retain principal attribution.
- Validation: `cargo test -p harness --test governed_actions_integration -- --nocapture`.
- Notes: Keep tests deterministic with fixture-based principals.

### Milestone 8: Cleanup And Final Verification

- Status: TO BE DONE
- Purpose: Ensure only intended artifacts remain and the full change set is
  verified before marking completion.
- Exit Criteria: Temporary artifacts are removed, targeted and broad
  verification commands pass, and plan status can move to COMPLETED.

#### Task 8.1: Cleanup Intermediate Artifacts

- Status: TO BE DONE
- Objective: Remove temporary files and scaffolding not part of final
  repository state.
- Steps:
  1. Inspect worktree for temporary notes, scratch fixtures, debug scripts, and
     obsolete fragments.
  2. Remove only artifacts not required by final implementation.
  3. Keep maintainable tests/docs/config needed for long-term support.
- Validation: `cmd.exe /c git status --short` shows only intentional final
  changes.
- Notes: Do not remove unrelated user changes.

#### Task 8.2: Final Verification

- Status: TO BE DONE
- Objective: Run final integrated verification after cleanup.
- Steps:
  1. Run the final verification commands listed below.
  2. Fix failures and rerun until green, or mark blockers explicitly.
- Validation: All commands in `Final Verification Commands` pass.
- Notes: Capture any intentional skips with reason in execution notes.

## Final Verification Commands

Run after all implementation milestones complete:

1. `cargo fmt --all --check`
2. `cargo check --workspace`
3. `cargo clippy --workspace --all-targets -- -D warnings`
4. `cargo test -p contracts --lib`
5. `cargo test -p harness --test migration_component -- --nocapture`
6. `cargo test -p harness --test foreground_component -- --nocapture`
7. `cargo test -p harness --test foreground_integration -- --nocapture`
8. `cargo test -p harness --test unconscious_component -- --nocapture`
9. `cargo test -p harness --test unconscious_integration -- --nocapture`
10. `cargo test -p harness --test governed_actions_component -- --nocapture`
11. `cargo test -p harness --test governed_actions_integration -- --nocapture`
12. `cargo test -p runtime --test admin_cli -- --nocapture`
13. `cargo test --workspace`

## Approval Gate

Implementation must not start until the user approves this plan.

## Plan Self-Check

- [x] Plan location follows the default location rule.
- [x] Plan status lifecycle is valid; current status is `APPROVED`.
- [x] Scope, non-goals, assumptions, and open questions are explicit.
- [x] Any unresolved open questions have been surfaced to the user.
- [x] Tasks are grouped into milestones because the plan has more than 10 tasks.
- [x] Every task has concrete steps and validation.
- [x] Every milestone has exit criteria.
- [x] Cleanup and final verification are included.
- [x] The plan avoids vague actions without concrete targets.
- [x] The plan can be executed by a coding agent without reading the original conversation.

## Execution Notes

- Update milestone and task status before starting and after validation.
- Update each task to COMPLETED immediately after its validation passes.
- Mark tasks or milestones BLOCKED with a short reason when progress cannot continue.
- 2026-05-15: Milestone 1 completed. Validation run: `cargo test -p harness --test use_case_scenarios -- --nocapture`.
- 2026-05-15: Milestone 5 completed.
  - Route split implementation validation: `cargo test -p harness --lib config -- --nocapture`.
  - Route split gateway validation: `cargo test -p harness --lib model_gateway -- --nocapture`.
  - Internal docs updated: `docs/internal/harness/MODEL_PROVIDERS.md`,
    `docs/internal/unconscious_loop/BACKGROUND_JOBS.md` (stale route-split callouts removed; verified dates restamped).
- 2026-05-15: Milestone 2 completed.
  - Added attachment schema migration and governed attachment processing/context projection flow.
  - Added fixture-driven foreground integration coverage for processed and unsupported attachment paths.
  - Validation runs:
    - `cargo test -p harness --test migration_component -- --nocapture`.
    - `cargo test -p contracts --lib`.
    - `cargo test -p harness --test foundation_component -- --nocapture`.
    - `cargo test -p harness --test governed_actions_component -- --nocapture`.
    - `cargo test -p harness --test foreground_component -- --nocapture`.
    - `cargo test -p harness --test foreground_integration -- --nocapture`.
