# Malformed Action Re-Steer And Guardrails Plan

## Metadata

- Plan Status: COMPLETED
- Created: 2026-05-16
- Last Updated: 2026-05-17
- Owner: Coding agent
- Approval: APPROVED

## Status Legend

- Plan Status values: DRAFT, QUESTIONS PENDING, READY FOR APPROVAL, APPROVED, IN PROGRESS, COMPLETED, BLOCKED
- Task/Milestone Status values: TO BE DONE, IN PROGRESS, COMPLETED, BLOCKED, SKIPPED

## Goal

Make malformed governed-action proposals recoverable without silent coercion by
adding bounded harness-led re-steering in the same foreground turn, while
preserving strict proposal validation and removing masking behavior that turns
invalid model output into accepted behavior.

## Current Status

Foreground turns currently fail fast when conscious worker output is classified
as `invalid_model_output` for malformed governed-action proposals. This emits a
failure notice to the user and ends the turn instead of attempting harness-led
same-turn repair. There is also existing compatibility logic in worker parsing
and contracts that can mask malformed payload shape issues.

## Scope

- Keep governed-action schema validation strict for conscious model outputs.
- Add bounded same-turn harness re-steer for recoverable malformed outputs.
- Remove or constrain masking behavior in governed-action parsing and payload
  acceptance paths.
- Perform a repository-wide audit for similar masking patterns in model-output
  parsing and apply guardrails.
- Add automated coverage and diagnostics for both strict rejection and repair.
- Update internal docs for live behavior and source references.

## Non-Goals

- General-purpose fuzzy parsing for action proposals.
- Expanding action schema breadth or adding new action kinds.
- Redesigning risk-tier policy or approval semantics.
- Reworking unconscious job semantics beyond masking-audit fixes.

## Assumptions

- A malformed governed-action proposal is recoverable only when no side effects
  have been executed in the failed attempt.
- Bounded re-steer attempts (for example, 1-2 retries) are acceptable when
  fully auditable and capped by foreground budgets.
- Existing strict behavior for malformed mutating payload fields should remain
  strict after this work.
- The current in-progress optional-field tolerance edits in
  `crates/contracts/src/lib.rs`, `crates/harness/src/governed_actions.rs`, and
  `crates/harness/tests/governed_actions_component.rs` are transitional and
  should not ship as the final strategy.

## Open Questions

None.

## Milestones

### Milestone 1: Baseline And Strictness Alignment

- Status: COMPLETED
- Purpose: Re-establish strict baseline behavior and define explicit
  anti-masking guardrails before implementation.
- Exit Criteria: Current failure baseline is captured, transitional masking
  edits are removed, and strictness guardrails are documented for execution.

#### Task 1.1: Capture Failure Baseline And Reproduction Fixtures

- Status: COMPLETED
- Objective: Preserve reproducible evidence for malformed governed-action
  failures and expected strict classification.
- Steps:
  1. Capture representative trace explain outputs for malformed proposal cases
     and map them to parser failure messages and failure-kind classification.
  2. Add or update deterministic unit tests that reproduce the observed malformed
     shapes (for example, missing `actions`, missing required payload fields).
  3. Record baseline expectations for pre-repair behavior in test assertions.
- Validation: `cargo test -p workers governed_action -- --nocapture` with
  target tests for
  malformed governed-action parsing and classification.
- Notes: Use existing malformed examples from recent operator traces.

#### Task 1.2: Revert Transitional Parser/Contract Masking Edits

- Status: COMPLETED
- Objective: Remove in-progress tolerance changes that relax strict parsing.
- Steps:
  1. Restore `InspectWorkspaceArtifactAction.artifact_kind` to required status.
  2. Restore harness validation/execution checks expecting explicit artifact
     kind for inspect payloads.
  3. Update impacted tests back to strict required-field expectations.
- Validation: `cargo test -p harness --test governed_actions_component -- --nocapture`.
- Notes: This rollback happens before introducing harness re-steer logic.

#### Task 1.3: Define Anti-Masking Contract For Model-Output Parsing

- Status: COMPLETED
- Objective: Establish concrete acceptance/rejection rules that separate
  guardrails from masking.
- Steps:
  1. Add an internal rule set that defines forbidden masking patterns (for
     example, implicit payload shape coercion, legacy schema reinterpretation).
  2. Define allowed normalization boundaries (for example, transport trimming)
     with explicit criteria and rationale.
  3. Link the rule set to worker/harness test expectations.
- Validation: Manual inspection of updated internal doc section confirms each
  rule maps to concrete code locations and test expectations.
- Notes: Update `docs/internal/conscious_loop/GOVERNED_ACTIONS.md`.

### Milestone 2: Harness-Led Same-Turn Re-Steer

- Status: COMPLETED
- Purpose: Recover from strict parse failures through bounded harness retries
  instead of terminating the turn immediately.
- Exit Criteria: Foreground orchestration performs bounded same-turn repair for
  recoverable malformed outputs with deterministic stop conditions.

#### Task 2.1: Add Structured Recoverable Failure Signals

- Status: COMPLETED
- Objective: Emit machine-actionable malformed-output details for harness retry
  decisions.
- Steps:
  1. Extend worker error payload handling to preserve parse-failure reason class
     and key detail text.
  2. Add harness-side extraction utilities for recoverable invalid-output
     signals.
  3. Keep existing strict failure codes unchanged for external compatibility.
- Validation: Unit tests in harness and worker layers verify classification of
  malformed proposal errors into recoverable vs non-recoverable.
- Notes: Keep signal format auditable and deterministic.

#### Task 2.2: Implement Bounded Re-Steer Controller In Foreground Loop

- Status: COMPLETED
- Objective: Retry conscious worker execution in the same turn when strict
  malformed-output conditions are recoverable.
- Steps:
 1. Add a bounded retry loop in foreground orchestration around conscious
     worker calls for recoverable malformed outputs.
  2. Gate retries on code-observable no-side-effect criteria (malformed output
     returned before any governed action execution is recorded for that pass)
     plus retry budget checks.
  3. Ensure retry loop termination is explicit (success, non-recoverable
     failure, or retry budget exhausted).
- Validation: `cargo test -p harness --test foreground_component -- --nocapture`.
- Notes: Do not suppress non-recoverable worker failures.

#### Task 2.3: Inject Repair Guidance For Re-Steer Attempts

- Status: COMPLETED
- Objective: Provide targeted harness guidance so retry attempts correct the
  exact schema errors.
- Steps:
  1. Add a repair-context message path in worker input composition for
     harness-initiated malformed-output retries.
  2. Include exact validation errors and strict instruction to return corrected
     governed-action JSON only when action is needed.
  3. Keep governed-action schema disclosure consistent with strict contract.
- Validation: Worker unit tests confirm repair attempts include explicit error
  guidance and strict block requirements.
- Notes: Avoid introducing permissive parser logic in this step.

#### Task 2.4: Add Re-Steer Configuration And Validation

- Status: COMPLETED
- Objective: Make retry behavior bounded and operator-configurable.
- Steps:
  1. Add governed-action re-steer config knobs (retry count and optional retry
     timeout cap) under repository config defaults.
  2. Add config validation to reject unsafe values.
  3. Wire runtime config resolution and tests for defaults and overrides.
- Validation: `cargo test -p harness --lib config -- --nocapture`.
- Notes: Defaults should be conservative and fail closed on invalid config.

#### Task 2.5: Add Audit And Diagnostic Coverage For Re-Steer Lifecycle

- Status: COMPLETED
- Objective: Make re-steer attempts and exhaustion visible to operators.
- Steps:
  1. Emit audit events for each retry attempt with attempt index and failure
     reason class.
  2. Emit operational diagnostics when retry budget is exhausted.
  3. Ensure final failure classification remains explicit and queryable.
- Validation: `cargo test -p harness --test management_component -- --nocapture`.
- Notes: Include trace/execution identifiers in every retry diagnostic event.

### Milestone 3: Remove Governed-Action Masking Paths

- Status: COMPLETED
- Purpose: Eliminate known parser/contract coercions that accept malformed
  governed-action output instead of failing and re-steering.
- Exit Criteria: Known masking paths are removed or formally constrained with
  explicit tests proving strict rejection.

#### Task 3.1: Remove Legacy Governed-Action Shape Translation

- Status: COMPLETED
- Objective: Stop converting legacy governed-action payload shapes into current
  canonical proposals.
- Steps:
  1. Remove `build_legacy_governed_action_proposals` conversion flow from worker
     governed-action parsing path.
  2. Preserve strict error messages for unsupported or malformed proposal
     envelopes.
  3. Update worker tests to assert rejection instead of conversion.
- Validation: `cargo test -p workers governed_action -- --nocapture`.
- Notes: This includes removing legacy `schedule_task` compatibility conversion.

#### Task 3.2: Enforce Tagged-Block-Only Governed-Action Intake

- Status: COMPLETED
- Objective: Require the official tagged control block and reject standalone
  untagged JSON payload intake.
- Steps:
  1. Remove standalone governed-action payload extraction from parse acceptance
     path.
  2. Keep strict malformed-block marker checks and explicit rejection errors.
  3. Update control-block stripping logic and tests to match tagged-only policy.
- Validation: `cargo test -p workers governed_action -- --nocapture`.
- Notes: Preserve deterministic behavior for non-action plain-text replies.

#### Task 3.3: Tighten Implicit Defaults For Model-Originated Proposals

- Status: COMPLETED
- Objective: Ensure required model-supplied governed-action fields fail when
  omitted instead of being silently defaulted.
- Steps:
  1. Audit `serde(default)` usage on governed-action payload structs and classify
     each default as allowed normalization or masking.
  2. Remove defaults that mask proposal completeness for model-originated
     payloads; keep only explicitly approved non-semantic defaults.
  3. Update worker schema prompt examples to match strict required-field rules.
- Validation: `cargo test -p contracts --lib` and
  `cargo test -p workers governed_action -- --nocapture`.
- Notes: Any retained default must be justified in docs and tests.

#### Task 3.4: Add Strict-Rejection Regression Suite

- Status: COMPLETED
- Objective: Prevent reintroduction of masking behavior in governed-action
  parsing.
- Steps:
  1. Add regression tests for missing envelope, missing required fields,
     untagged payloads, and alias action names.
  2. Assert failures classify as malformed action proposals and are eligible for
     harness re-steer when side effects are absent.
  3. Add at least one integration test showing successful same-turn recovery via
     harness retry.
- Validation: `cargo test -p harness --test foreground_integration -- --nocapture`.
- Notes: Keep test fixtures minimal and deterministic.

### Milestone 4: Repository-Wide Anti-Masking Audit

- Status: COMPLETED
- Purpose: Ensure the no-masking posture is not limited to one parser path.
- Exit Criteria: Model-output parsing paths are inventoried, risky masking
  patterns are remediated, and retained normalizations are documented with
  rationale.

#### Task 4.1: Build Model-Output Parsing Inventory

- Status: COMPLETED
- Objective: Enumerate where model outputs are parsed and potentially coerced.
- Steps:
  1. Inventory parsing/normalization entry points in workers and harness
     (conscious, unconscious, identity, and proposal paths).
  2. Classify each path against anti-masking policy categories.
  3. Record inventory as an internal engineering checklist artifact in docs.
- Validation: Manual inspection confirms every parser entry point has a
  classification and disposition.
- Notes: Include exact file+symbol references for each parser path.

#### Task 4.2: Remediate High-Risk Masking Findings

- Status: COMPLETED
- Objective: Remove or hard-fail high-risk masking behavior discovered by the
  audit.
- Steps:
  1. Implement code changes for each high-risk finding with explicit rejection
     or bounded harness guardrail replacement.
  2. Add diagnostics for guardrail-triggered rejections where appropriate.
  3. Avoid broad refactors outside identified masking paths.
- Validation: Targeted tests for each remediated parser path pass.
- Notes: Track each finding-to-fix mapping in execution notes.

#### Task 4.3: Lock In Guardrails With Cross-Layer Tests

- Status: COMPLETED
- Objective: Ensure anti-masking guarantees remain stable over time.
- Steps:
  1. Add cross-layer tests asserting invalid model outputs do not get accepted
     through coercion.
  2. Verify harness retry only applies to explicitly recoverable cases.
  3. Verify non-recoverable invalid outputs remain explicit failures.
- Validation: `cargo test -p harness --test governed_actions_component -- --nocapture` and
  `cargo test -p harness --test governed_actions_integration -- --nocapture`.
- Notes: Keep assertions specific to acceptance/rejection contracts.

### Milestone 5: Documentation And Operational Surfacing

- Status: COMPLETED
- Purpose: Keep internal documentation and operator diagnostics aligned with the
  implemented strict+re-steer behavior.
- Exit Criteria: Internal docs describe live behavior precisely with verified
  source references and updated dates.

#### Task 5.1: Update Governed Actions Internal Documentation

- Status: COMPLETED
- Objective: Document strict intake rules, retry guardrails, and removed masking
  compatibility paths.
- Steps:
  1. Update proposal-format and recovery sections in
     `docs/internal/conscious_loop/GOVERNED_ACTIONS.md`.
  2. Remove stale compatibility language that no longer matches implementation.
  3. Re-verify source references and restamp `Last verified`.
- Validation: Manual doc review confirms references resolve and policy text
  matches code/tests.
- Notes: This doc is implementation-authoritative and must stay current.

#### Task 5.2: Update Context Assembly/Internal References As Needed

- Status: COMPLETED
- Objective: Ensure any new repair-context message path is reflected in internal
  context-assembly documentation.
- Steps:
  1. Update `docs/internal/conscious_loop/CONTEXT_ASSEMBLY.md` if message
     ordering or developer-message composition changes for re-steer attempts.
  2. Update any related admin troubleshooting references if failure semantics
     changed.
  3. Re-verify source references and restamp verification date(s).
- Validation: Manual inspection confirms internal docs are consistent and
  non-contradictory.
- Notes: Canonical docs remain the source of normative behavior boundaries.

### Milestone 6: Cleanup And Final Verification

- Status: COMPLETED
- Purpose: Ensure the repository contains only intentional final artifacts and
  the complete change is verified.
- Exit Criteria: Intermediate artifacts are removed, all final verification
  passes, and the plan status is COMPLETED.

#### Task 6.1: Cleanup Intermediate Artifacts

- Status: COMPLETED
- Objective: Remove artifacts created only to support implementation.
- Steps:
  1. Inspect the worktree for temporary docs, scratch tests, debug helpers, and
     audit scratch files.
  2. Remove only artifacts that are not part of the intended final repository
     state.
  3. Keep maintainable tests and docs that are part of the final contract.
- Validation: `cmd.exe /c git status --short` shows only intentional final
  changes.
- Notes: Do not revert unrelated user changes.

#### Task 6.2: Final Verification

- Status: COMPLETED
- Objective: Validate the integrated change after cleanup.
- Steps:
  1. Run the final verification commands listed below.
  2. Fix failures and rerun until verification passes, or record blockers with
     exact failure output.
- Validation: All commands in `Final Verification Commands` pass.
- Notes: If any command is intentionally skipped, record reason in execution
  notes before completion.

## Final Verification Commands

1. `cargo fmt --all --check`
2. `cargo check --workspace`
3. `cargo clippy --workspace --all-targets -- -D warnings`
4. `cargo test -p contracts --lib`
5. `cargo test -p workers governed_action -- --nocapture`
6. `cargo test -p harness --test foreground_component -- --nocapture`
7. `cargo test -p harness --test foreground_integration -- --nocapture`
8. `cargo test -p harness --test governed_actions_component -- --nocapture`
9. `cargo test -p harness --test governed_actions_integration -- --nocapture`
10. `cargo test -p harness --test management_component -- --nocapture`
11. `cargo test -p runtime --test admin_cli -- --nocapture`

## Approval Gate

Implementation must not start until the user approves this plan.

## Plan Self-Check

- [x] Plan location follows the default location rule.
- [x] Plan status lifecycle is valid; current status is `COMPLETED`.
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
- Mark tasks or milestones BLOCKED with a short reason when progress cannot
  continue.
- 2026-05-17: Full plan self-check completed (structure, technical reference
  accuracy, and execution readiness). Result: READY FOR APPROVAL, with retry
  gating wording clarified to code-observable no-side-effect criteria.
- 2026-05-17: User approved execution. Plan moved to IN PROGRESS; Milestone 1
  started with Task 1.2 (reverting transitional masking/tolerance edits).
- 2026-05-17: Task 1.1 completed.
  - Added malformed-proposal regression tests in
    `crates/workers/src/main.rs`:
    `build_governed_action_proposals_rejects_missing_actions_field` and
    `build_governed_action_proposals_rejects_missing_required_payload_field`.
  - Baseline trace mapping confirmed from prior `admin trace explain` output:
    `019e32bf-2ba8-7e62-8949-0dbcf783d488` (`missing field actions`) and
    `019e32c0-9b87-7b63-8918-cc2d630e149d` (`missing field artifact_kind`).
  - Validation run: `cargo test -p workers governed_action -- --nocapture`.
- 2026-05-17: Task 1.2 completed.
  - Reverted transitional optional-field tolerance for
    `InspectWorkspaceArtifactAction.artifact_kind` in contracts/harness.
  - Validation run:
    `cargo test -p harness --test governed_actions_component -- --nocapture`.
- 2026-05-17: Task 1.3 completed.
  - Added explicit anti-masking guardrails to
    `docs/internal/conscious_loop/GOVERNED_ACTIONS.md` and restamped the
    verified date.
  - Milestone 1 marked COMPLETED; Milestone 2 started with Task 2.1 IN PROGRESS.
- 2026-05-17: Milestone 2 completed (Tasks 2.1-2.5).
  - Added structured worker failure metadata:
    `WorkerFailureMetadata`, `WorkerFailureKind::MalformedActionProposal`,
    `WorkerFailureSideEffectStatus::NoneExecuted`, and
    `ForegroundGovernedActionRepairGuidance`.
  - Implemented bounded same-turn malformed-action re-steer in
    `execute_conscious_turn_with_governed_action_loop()` with retry guidance
    injection, retry budget/time cap, and explicit stop conditions.
  - Added governed-action re-steer config defaults/validation:
    `malformed_action_resteer_max_attempts` and
    `malformed_action_resteer_timeout_ms`.
  - Added retry lifecycle audit/diagnostic emissions:
    `foreground_malformed_action_resteer_attempt` and
    `foreground_malformed_action_resteer_exhausted`.
  - Validation runs:
    `cargo test -p harness --test foreground_component -- --nocapture`,
    `cargo test -p harness --test management_component -- --nocapture`,
    `cargo test -p harness --lib config -- --nocapture`.
- 2026-05-17: Milestone 3 completed (Tasks 3.1-3.4).
  - Removed legacy governed-action compatibility conversion path and enforced
    tagged-block-only intake in worker parsing.
  - Added strict parser regressions for untagged payload rejection and missing
    envelope handling.
  - Added end-to-end same-turn re-steer integration coverage:
    `telegram_fixture_runtime_resteers_malformed_governed_action_and_completes_same_turn`.
  - Validation runs:
    `cargo test -p workers governed_action -- --nocapture`,
    `cargo test -p harness --test foreground_component -- --nocapture`,
    `cargo test -p harness --test foreground_integration -- --nocapture`.
- 2026-05-17: Milestone 4 completed (Tasks 4.1-4.3).
  - Added a model-output parsing inventory and anti-masking dispositions in
    `docs/internal/conscious_loop/GOVERNED_ACTIONS.md`.
  - Confirmed no remaining high-risk masking path in governed-action proposal
    parsing after strict-intake cleanup; retained non-governed optional
    tolerances are documented as bounded/no-side-effect.
  - Validation runs:
    `cargo test -p harness --test governed_actions_component -- --nocapture`,
    `cargo test -p harness --test governed_actions_integration -- --nocapture`.
- 2026-05-17: Milestone 5 completed (Tasks 5.1-5.2).
  - Updated `docs/internal/conscious_loop/GOVERNED_ACTIONS.md` for strict
    tagged-block-only behavior, same-turn malformed-action re-steer lifecycle,
    config knobs, and regression anchors.
  - Updated `docs/internal/conscious_loop/CONTEXT_ASSEMBLY.md` with the
    malformed-action repair-guidance message path and refreshed source
    references/verification date.
- 2026-05-17: Milestone 6 completed (Tasks 6.1-6.2).
  - Cleanup scan confirmed only intentional implementation/doc/test deltas in
    the worktree.
  - Full final verification command bundle passed:
    1) `cargo fmt --all --check`
    2) `cargo check --workspace`
    3) `cargo clippy --workspace --all-targets -- -D warnings`
    4) `cargo test -p contracts --lib`
    5) `cargo test -p workers governed_action -- --nocapture`
    6) `cargo test -p harness --test foreground_component -- --nocapture`
    7) `cargo test -p harness --test foreground_integration -- --nocapture`
    8) `cargo test -p harness --test governed_actions_component -- --nocapture`
    9) `cargo test -p harness --test governed_actions_integration -- --nocapture`
    10) `cargo test -p harness --test management_component -- --nocapture`
    11) `cargo test -p runtime --test admin_cli -- --nocapture`
