# Governed-Action Reliability Foundation Plan

## Metadata

- Plan Status: READY FOR APPROVAL
- Created: 2026-05-18
- Last Updated: 2026-05-18
- Owner: Coding agent
- Approval: PENDING

## Status Legend

- Plan Status values: DRAFT, QUESTIONS PENDING, READY FOR APPROVAL, APPROVED, IN PROGRESS, COMPLETED, BLOCKED
- Task/Milestone Status values: TO BE DONE, IN PROGRESS, COMPLETED, BLOCKED, SKIPPED

## Goal

Build a durable governed-action reliability foundation that reduces malformed
assistant outputs by design, shifts brittle payload assembly from the model to
deterministic harness code, and preserves strict structured-only behavior with
no plain-text fallback path.

## Current Status

Recent production traces still show systemic foreground failures despite the
structured-only cutover:

- repeated `malformed_action_proposal` failures
- `worker_protocol_failure` caused by invalid/truncated structured payloads
- operational friction from low-clarity failure debugging in some runs

The current posture is still too fragile for iterative product delivery because
small output mistakes become user-visible hard failures.

## First-Principles Design Constraints

1. The model should express intent, not assemble high-coupling execution
   payloads.
2. The harness should own deterministic compilation, validation, and execution
   planning.
3. All control content must remain structured; user text is just one field in
   the same structure.
4. Retries are orchestration behavior only; they must not rely on permissive
   parsing or silent coercion.
5. Failures must be observable and actionable from operator tooling in one
   command path.

## Scope

- Introduce a compact structured intent contract for conscious turns.
- Add a deterministic harness intent compiler that resolves workspace entities
  and builds governed-action payloads.
- Add a strict worker/harness state machine for parse/compile/execute/reply.
- Add transport hardening for truncation and invalid structured responses.
- Improve trace and admin diagnostics for exact failing model payload visibility.
- Add rollout and quality gates that prevent regressions.

## Non-Goals

- Reintroducing any plain-text governed-action parser path.
- Weakening approval, capability scope, or risk-tier policy semantics.
- Broad unconscious-loop redesign.
- Provider-specific hacks that bypass the canonical structured protocol.

## Assumptions

- Structured-only output remains mandatory.
- Harness-owned retries within a bounded same-turn budget remain acceptable.
- A smaller intent contract is sufficient to represent current governed-action
  use cases.
- Operators can run explicit preflight and trace diagnostics before promoting
  model-route changes.

## Open Questions

None.

## Milestones

### Milestone 1: Contract Simplification and Error Model

- Status: TO BE DONE
- Purpose: Reduce model burden and standardize failure semantics.
- Exit Criteria: A compact intent contract and error taxonomy are implemented
  and covered by unit tests.

#### Task 1.1: Define Conscious Intent Envelope v1

- Status: TO BE DONE
- Objective: Replace direct low-level action payload generation with a compact
  intent envelope.
- Steps:
  1. Define `ConsciousIntentEnvelope` with required `assistant_text` and
     optional `intents`.
  2. Keep intent fields minimal and semantic (target handle + desired
     operation), avoiding executor-specific fields.
  3. Enforce strict `deny_unknown_fields` and bounded list sizes.
- Validation: `cargo test -p contracts --lib -- --nocapture`.

#### Task 1.2: Define Compiler Error Taxonomy

- Status: TO BE DONE
- Objective: Standardize parse/compile/semantic/policy failures for deterministic
  harness behavior.
- Steps:
  1. Add explicit error categories for parse failure, missing entity,
     ambiguous entity, invalid intent arguments, and policy rejection.
  2. Map each category to `WorkerFailureMetadata` and harness diagnostics.
  3. Remove message-substring reliance where metadata is available.
- Validation: `cargo test -p harness --lib foreground_orchestration -- --nocapture`.

#### Task 1.3: Define Intent-to-Action Coverage Matrix

- Status: TO BE DONE
- Objective: Guarantee every supported intent has one deterministic compilation
  path.
- Steps:
  1. Create a matrix doc mapping intent kinds to governed-action kinds and
     required resolver inputs.
  2. Mark unsupported intents as explicit compile errors.
  3. Link matrix to automated regression tests.
- Validation: Manual review of matrix plus targeted harness tests.

### Milestone 2: Deterministic Harness Intent Compiler

- Status: TO BE DONE
- Purpose: Move payload complexity into deterministic harness code.
- Exit Criteria: Intent compiler produces canonical governed-action proposals
  from valid intents and emits structured compiler errors otherwise.

#### Task 2.1: Add Workspace Entity Resolver Layer

- Status: TO BE DONE
- Objective: Resolve model-facing handles into canonical IDs and kinds.
- Steps:
  1. Build resolver APIs for artifact/script/task identifiers from current
     workspace snapshots.
  2. Support explicit IDs and alias-style handles in a deterministic way.
  3. Emit ambiguous/missing resolution errors with candidate context.
- Validation: `cargo test -p harness --test governed_actions_component -- --nocapture`.

#### Task 2.2: Implement Intent Compiler Core

- Status: TO BE DONE
- Objective: Translate intent envelope into canonical governed-action proposals.
- Steps:
  1. Add a dedicated compiler module in harness with pure input/output
     boundaries.
  2. Generate full action payload fields from resolver outputs and policy
     defaults.
  3. Preserve explicit traceability from source intent to compiled action.
- Validation: `cargo test -p harness --lib governed_actions -- --nocapture`.

#### Task 2.3: Add Compiler Property and Regression Tests

- Status: TO BE DONE
- Objective: Lock deterministic behavior and prevent future protocol drift.
- Steps:
  1. Add regression fixtures for known malformed categories.
  2. Add deterministic tests for ambiguous resolution and invalid arguments.
  3. Verify identical intent input always yields identical compiled payloads.
- Validation: `cargo test -p harness --test governed_actions_integration -- --nocapture`.

### Milestone 3: Foreground State-Machine Hardening

- Status: TO BE DONE
- Purpose: Make foreground behavior predictable and recoverable without masking.
- Exit Criteria: Foreground turn flow runs as explicit parse -> compile ->
  policy -> execute -> reply stages with bounded same-turn re-steer.

#### Task 3.1: Refactor Foreground Turn Pipeline Stages

- Status: TO BE DONE
- Objective: Separate parsing, compilation, and execution into explicit stages.
- Steps:
  1. Add stage transitions in foreground orchestration with typed stage
     results.
  2. Ensure no side effects occur before compile/policy validation success.
  3. Persist stage outcomes in trace metadata.
- Validation: `cargo test -p harness --test foreground_component -- --nocapture`.

#### Task 3.2: Re-Steer From Compiler Failure Metadata

- Status: TO BE DONE
- Objective: Retry only when failures are transient/recoverable and side-effect
  free.
- Steps:
  1. Drive re-steer from compiler/parsing metadata categories.
  2. Generate focused repair guidance from failure metadata, not heuristics.
  3. Preserve existing bounded retry budget and exhaustion diagnostics.
- Validation: `cargo test -p harness --test foreground_integration -- --nocapture`.

#### Task 3.3: Enforce Deterministic Terminal Failure Paths

- Status: TO BE DONE
- Objective: Prevent undefined failure modes and inconsistent user outcomes.
- Steps:
  1. Define terminal failure mapping per stage and failure category.
  2. Ensure worker protocol failures become actionable diagnostics with trace
     focus hints.
  3. Add regression tests for terminal mapping behavior.
- Validation: `cargo test -p runtime --test admin_cli -- --nocapture`.

### Milestone 4: Transport and Provider Reliability Guardrails

- Status: TO BE DONE
- Purpose: Reduce truncation and invalid structured-response risks at the route
  boundary.
- Exit Criteria: Conscious route calls have explicit structured-output guardrails
  and deterministic preflight checks.

#### Task 4.1: Add Structured Response Size Budgeting

- Status: TO BE DONE
- Objective: Prevent predictable truncation from undersized output budgets.
- Steps:
  1. Add conscious-route output-token budgeting rules aligned with envelope
     size limits.
  2. Fail fast on impossible budget configs.
  3. Emit diagnostics when truncation risk is detected pre-call.
- Validation: `cargo test -p harness --lib model_gateway -- --nocapture`.

#### Task 4.2: Add Truncation-Aware Failure Classification

- Status: TO BE DONE
- Objective: Distinguish truncation transport failures from semantic protocol
  failures.
- Steps:
  1. Classify `finish_reason=length` with invalid JSON as transport truncation.
  2. Route it through bounded retry policy with explicit reason tagging.
  3. Keep non-recoverable semantic failures fail-closed.
- Validation: `cargo test -p harness --test foreground_component -- --nocapture`.

#### Task 4.3: Harden Structured-Output Preflight Workflow

- Status: TO BE DONE
- Objective: Make model/provider compatibility verifiable before runtime use.
- Steps:
  1. Extend preflight checks to validate representative intent envelope output.
  2. Record provider/model request and parsed response diagnostics.
  3. Document required operator preflight workflow before route changes.
- Validation: `cargo run -p runtime -- admin model preflight-structured-output --json`.

### Milestone 5: Operator Debuggability and Trace Surfaces

- Status: TO BE DONE
- Purpose: Make failures inspectable in one hop with full actionable context.
- Exit Criteria: Operators can retrieve exact failing model output and
  stage-level failure context from trace/admin tools without manual spelunking.

#### Task 5.1: Improve Failing-Call Payload Surfaces

- Status: TO BE DONE
- Objective: Expose exact failing model output once, without repeated
  blob expansion noise.
- Steps:
  1. Normalize trace serialization so request/response payloads are emitted once
     per call and referenced by ID elsewhere.
  2. Add concise summaries plus expandable full payload sections.
  3. Keep sensitive-content redaction rules intact.
- Validation: `cargo test -p runtime --test admin_cli -- --nocapture`.

#### Task 5.2: Add Failure-Bundle Focus Command Path

- Status: TO BE DONE
- Objective: Reduce time-to-debug for foreground protocol failures.
- Steps:
  1. Add or extend `admin trace explain --focus failing-model-call --json`
     output with stage, failure kind, and raw model response excerpts.
  2. Ensure harness user-facing error text points to this exact command.
  3. Add regression tests for command output schema.
- Validation: `cargo test -p runtime --test admin_cli -- --nocapture`.

### Milestone 6: Rollout Gates, Cleanup, and Final Verification

- Status: TO BE DONE
- Purpose: Ship safely, enforce quality thresholds, and leave no temporary
  scaffolding.
- Exit Criteria: Rollout gate criteria pass, cleanup is complete, and final
  verification suite is green.

#### Task 6.1: Add Reliability Acceptance Gates

- Status: TO BE DONE
- Objective: Block regressions before promotion.
- Steps:
  1. Define target thresholds for malformed-action rate and worker-protocol
     failure rate in synthetic and fixture test runs.
  2. Add CI check that runs the falsification/confirmation suite and validates
     thresholds.
  3. Require gate pass before enabling the new compiler path by default.
- Validation: `cargo test --workspace` plus governed-action falsification script run.

#### Task 6.2: Cleanup Transitional Paths

- Status: TO BE DONE
- Objective: Remove dead compatibility code and temporary diagnostic scaffolding.
- Steps:
  1. Remove obsolete code paths superseded by intent compiler and stage
     machine.
  2. Keep only final diagnostics and tests needed for ongoing operations.
  3. Verify worktree contains only intentional deltas.
- Validation: `cmd.exe /c git status --short`.

#### Task 6.3: Final Verification

- Status: TO BE DONE
- Objective: Validate integrated behavior before completion.
- Steps:
  1. Run the full verification command set below.
  2. Resolve failures and rerun until green, or mark blockers explicitly.
- Validation: All commands in `Final Verification Commands` pass.

## Final Verification Commands

1. `cargo fmt --all --check`
2. `cargo check --workspace`
3. `cargo clippy --workspace --all-targets -- -D warnings`
4. `cargo test -p contracts --lib`
5. `cargo test -p workers --bin workers -- --nocapture`
6. `cargo test -p harness --test foreground_component -- --nocapture`
7. `cargo test -p harness --test foreground_integration -- --nocapture`
8. `cargo test -p harness --test governed_actions_component -- --nocapture`
9. `cargo test -p harness --test governed_actions_integration -- --nocapture`
10. `cargo test -p runtime --test admin_cli -- --nocapture`
11. `cargo test --workspace`
12. `cargo run -p runtime -- admin model preflight-structured-output --json`
13. `cargo run -p runtime -- harness --once --synthetic-trigger smoke`

## Approval Gate

Implementation must not start until the user approves this plan.

## Plan Self-Check

- [x] Plan location follows the default location rule.
- [x] Plan status lifecycle is valid; current status is `READY FOR APPROVAL`.
- [x] Scope, non-goals, assumptions, and open questions are explicit.
- [x] Any unresolved open questions have been surfaced to the user.
- [x] Tasks are grouped into milestones because the plan has more than 10 tasks.
- [x] Every task has concrete steps and validation.
- [x] Every milestone has exit criteria.
- [x] Cleanup and final verification are included.
- [x] The plan avoids vague actions without concrete targets.
- [x] The plan can be executed by a coding agent without reading the original
  conversation.

## Execution Notes

- Update milestone/task statuses as work progresses.
- Record exact validation commands and outcomes as each task completes.
- Mark blockers explicitly with root cause and next decision needed.
