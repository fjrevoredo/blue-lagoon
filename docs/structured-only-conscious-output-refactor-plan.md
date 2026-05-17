# Structured-Only Conscious Output Refactor Plan

## Metadata

- Plan Status: COMPLETED
- Created: 2026-05-17
- Last Updated: 2026-05-17
- Owner: Coding agent
- Approval: APPROVED

## Status Legend

- Plan Status values: DRAFT, QUESTIONS PENDING, READY FOR APPROVAL, APPROVED, IN PROGRESS, COMPLETED, BLOCKED
- Task/Milestone Status values: TO BE DONE, IN PROGRESS, COMPLETED, BLOCKED, SKIPPED

## Goal

Refactor foreground conscious-model output to a strict structured-only contract
with no text-parser fallback, where user-facing reply text is a first-class
field in the same output object as governed-action and identity control data.

## Scope

- Replace conscious foreground `ModelOutputMode::PlainText` with
  `ModelOutputMode::JsonObject`.
- Define and enforce one strict conscious-output schema.
- Treat `json_object` as the transport baseline for this refactor and enforce
  strict schema/semantic validation in worker/harness code.
- Remove fenced-block parsing and bare-token/untagged-payload heuristics for
  governed actions.
- Carry user-facing reply text as a structured field.
- Define model/provider selection rules for structured output compatibility and
  enforce fail-closed behavior when a selected route cannot satisfy the
  structured-output contract.
- Keep same-turn harness re-steer behavior, but drive it from structured
  validation failures and explicit metadata (not parser tolerance).
- Update tests and internal documentation to reflect the new protocol.

## Non-Goals

- Changing governed-action policy semantics, risk tiers, approval thresholds, or
  capability scope policy.
- Changing unconscious worker output contract.
- Introducing compatibility fallback paths that parse legacy fenced
  `blue-lagoon-governed-actions` output.
- Broad provider migration work beyond what is required for this foreground
  structured-output contract.
- Introducing `response_format: {type: "json_schema", ...}` transport support
  in the model gateway as part of this refactor.

## Assumptions

- The user requirement is strict: no compatibility fallback to legacy
  free-text governed-action parsing.
- Identity kickstart control should be included in the same structured conscious
  output object (no separate fenced control block).
- If a provider/model route does not support structured output for conscious
  calls, the foreground path must fail closed rather than silently degrade to
  plain text.
- Existing harness same-turn re-steer remains desirable, but should be
  triggered by strict schema/semantic validation failures rather than permissive
  parsing logic.
- `json_object` mode plus strict worker/harness validation is sufficient to
  satisfy the structured-only contract for this milestone.

## Open Questions

None.

## Exploration Findings

1. Current foreground conscious output is plain text and embeds control data in
   fenced tagged blocks (`crates/workers/src/main.rs`, `build_model_call_request`,
   `build_governed_action_proposals`, `strip_worker_control_blocks`).
2. Governed-action validity currently depends on block extraction plus
   post-hoc heuristic rejection for alternate shapes.
3. Harness retry steering already exists and is metadata-driven when worker
   emits `WorkerFailureMetadata` with
   `failure_kind=malformed_action_proposal` and
   `side_effect_status=none_executed`.
4. Failure classification still contains message-substring fallbacks
   (`crates/harness/src/foreground_orchestration.rs`,
   `classify_conscious_worker_failure`), which weakens first-principles
   guarantees.
5. Internal docs currently codify tagged-block behavior and scenario-gated full
   schema reminders, so docs must be updated in the same change set.
6. Structured-output support is provider/model dependent: the harness always
   sends `response_format` for JSON mode, but route selection still needs an
   explicit compatibility policy and preflight validation for conscious models.

## Milestones

### Milestone 1: Structured Contract Definition

- Status: COMPLETED
- Purpose: Define one authoritative conscious-output schema and failure model.
- Exit Criteria: The output contract, strictness rules, and retry/failure
  semantics are explicit, testable, and implementation-ready.

#### Task 1.1: Define Conscious Structured Output Type

- Status: COMPLETED
- Objective: Introduce a typed output envelope for conscious foreground model
  responses.
- Steps:
  1. Define a Rust type for conscious model output (assistant text + optional
     governed-action proposals + optional identity kickstart directive).
  2. Place the type in the shared contracts layer if needed by multiple crates;
     otherwise keep it local with explicit serialization boundaries.
  3. Enforce `deny_unknown_fields` at top level and control sub-objects.
- Validation: `cargo test -p contracts --lib -- --nocapture` passes with new
  round-trip and strictness tests.
- Notes: `assistant_text` must be the canonical user-facing message field.

#### Task 1.2: Define Conscious JSON Schema Artifact

- Status: COMPLETED
- Objective: Provide a concrete JSON schema for `ModelOutputMode::JsonObject`
  foreground calls.
- Steps:
  1. Add a schema builder function for conscious output.
  2. Ensure schema includes required `assistant_text` and bounded optional
     control fields.
  3. Wire schema name + schema JSON into conscious model-call request.
- Validation: Worker unit test asserts `output_mode=json_object`,
  `schema_name` set, and `schema_json` present for conscious requests.
- Notes: This is strict structured output; no legacy block channel.

#### Task 1.3: Define Strict Failure Taxonomy For Structured Output

- Status: COMPLETED
- Objective: Make structured-output validation failures explicit and auditable.
- Steps:
  1. Define error categories for schema/shape/semantic validation failures.
  2. Map them to `WorkerFailureMetadata` so harness can classify and re-steer
     deterministically.
  3. Ensure failure details include actionable parse/validation reason and path.
- Validation: Unit tests cover metadata emission for representative malformed
  structured outputs.
- Notes: Avoid string-matching dependency for failure semantics.

### Milestone 2: Worker Refactor To Structured-Only Foreground Output

- Status: COMPLETED
- Purpose: Remove mixed-channel parsing and enforce one structured response path.
- Exit Criteria: Conscious worker accepts only structured foreground model
  output and produces `ConsciousWorkerResult` without fenced-block parsing.

#### Task 2.1: Switch Foreground Model Request To JSON Mode

- Status: COMPLETED
- Objective: Request structured JSON output for conscious loop model calls.
- Steps:
  1. Update conscious `build_model_call_request()` to use
     `ModelOutputMode::JsonObject`.
  2. Set foreground schema name and schema JSON.
  3. Keep unconscious behavior unchanged.
- Validation: Worker tests asserting conscious output mode are updated and pass.
- Notes: No plain-text foreground mode remains for conscious worker.

#### Task 2.2: Replace Governed-Action Tagged-Block Parsing

- Status: COMPLETED
- Objective: Remove extraction of governed actions from assistant free text.
- Steps:
  1. Replace `build_governed_action_proposals()` text-block extraction logic with
     parsing from structured output fields.
  2. Remove or decommission block/tag helpers from foreground flow.
  3. Preserve at-most-one-action-per-response enforcement in structured path.
- Validation: Existing governed-action extraction tests are replaced by
  structured-output parsing tests and pass.
- Notes: No fallback to fenced block parsing.

#### Task 2.3: Replace Identity Kickstart Tagged-Block Parsing

- Status: COMPLETED
- Objective: Move identity kickstart control to structured output fields.
- Steps:
  1. Parse identity directive from the structured envelope.
  2. Convert valid directives into canonical identity proposals as before.
  3. Remove dependence on `blue-lagoon-identity-kickstart` fenced blocks in
     foreground output.
- Validation: Identity kickstart worker tests pass using structured output
  fixtures.
- Notes: Invalid identity directives should fail with explicit metadata, not be
  silently ignored.

#### Task 2.4: Build Conscious Worker Response From Structured Fields

- Status: COMPLETED
- Objective: Populate `assistant_output.text`, governed-action proposals, and
  candidate proposals from structured model JSON only.
- Steps:
  1. Parse structured model JSON from `model_response.output.json`.
  2. Validate required fields and semantic constraints.
  3. Populate `ConsciousWorkerResult` from validated structured object.
- Validation: `cargo test -p workers --bin workers -- --nocapture` passes with
  updated response-shape tests.
- Notes: `assistant_output.text` must come directly from structured field.

#### Task 2.5: Remove Free-Text Heuristic Guardrails For Governed Action

- Status: COMPLETED
- Objective: Delete legacy heuristics that inferred malformed control payloads
  from arbitrary text.
- Steps:
  1. Remove bare-token and untagged-payload heuristic checks from foreground
     response shape validation path.
  2. Replace with strict structured-output validation errors.
  3. Keep strict side-effect posture (`none_executed`) for malformed output.
- Validation: Worker malformed-output tests cover new structured errors.
- Notes: This is removal of masking-style behavior, not tolerance expansion.

#### Task 2.6: Update Foreground Prompting Instructions

- Status: COMPLETED
- Objective: Align developer instructions with structured-only output channel.
- Steps:
  1. Replace fenced-block instructions with structured JSON-object instructions.
  2. Keep scenario-policy context logic, but remove block-specific wording.
  3. Ensure observation follow-up guidance references structured output fields.
- Validation: Golden-context tests updated and passing in workers crate.
- Notes: Do not reintroduce mixed prose/control instructions.

### Milestone 3: Harness Classification, Retry, And Traceability Alignment

- Status: COMPLETED
- Purpose: Preserve robust steering and diagnostics under the new strict protocol.
- Exit Criteria: Harness re-steer and failure classification are metadata-first,
  deterministic, and no longer coupled to old text-parser error strings.

#### Task 3.1: Make Foreground Failure Classification Metadata-First

- Status: COMPLETED
- Objective: Classify malformed structured output using worker metadata as the
  primary source.
- Steps:
  1. Update `classify_conscious_worker_failure()` to prefer
     `worker_error_metadata` and reduce legacy string-based governed-action
     checks.
  2. Keep existing non-structured classes (provider rejected, persistence,
     etc.) intact.
  3. Add regression tests for metadata-driven classification.
- Validation: Foreground orchestration unit tests for failure classification
  pass.
- Notes: Legacy substring checks should not be authoritative after cutover.

#### Task 3.2: Preserve Same-Turn Re-Steer With Structured Failure Detail

- Status: COMPLETED
- Objective: Continue bounded same-turn retry behavior without parser fallback.
- Steps:
  1. Ensure recoverable structured-output failures emit retryable metadata.
  2. Keep attempt budget and exhaustion diagnostics unchanged.
  3. Update repair guidance wording to reference structured fields and schema
     errors.
- Validation: `foreground_orchestration_resteers_malformed_action_and_completes`
  and retry-exhaustion tests pass with structured fixtures.
- Notes: Retry is orchestration policy, not parser tolerance.

#### Task 3.3: Improve Trace Explainability For Structured Failures

- Status: COMPLETED
- Objective: Keep operator debugging first-class after protocol cutover.
- Steps:
  1. Ensure failure detail in diagnostics includes parse/validation reason and
     relevant field path.
  2. Verify `admin trace explain --focus failing-model-call --json` surfaces
     enough detail to reconstruct the malformed structured response.
  3. Update any affected management tests.
- Validation: `cargo test -p runtime --test admin_cli -- --nocapture` passes
  and focused trace assertions are updated where needed.
- Notes: No raw payload masking; preserve direct evidence.

### Milestone 4: Model Selection And Provider Compatibility

- Status: COMPLETED
- Purpose: Ensure structured-only conscious output is matched by an explicit
  model/provider compatibility policy rather than implicit assumptions.
- Exit Criteria: Conscious route selection has a documented compatibility
  contract, fail-closed guardrails, and executable verification steps.

#### Task 4.1: Define Structured-Output Compatibility Policy

- Status: COMPLETED
- Objective: Produce a clear selection policy for conscious models/providers.
- Steps:
  1. Define the compatibility baseline explicitly: conscious route must support
     `response_format: { "type": "json_object" }`; schema adherence is enforced
     by runtime validation, not provider-side schema guarantees.
  2. Define supported conscious-route provider/model posture for structured
     output (including explicit stance on OpenRouter routing modes and model
     allowlisting/selection constraints).
  3. State that OpenRouter `auto` routing is unsupported for conscious
     structured-output routes; only pinned model IDs are eligible.
  4. Document initial known-compatible model posture per provider as an
     operator-managed allowlist that must be validated via preflight.
  5. Document that incompatible routes must fail closed and are not eligible for
     fallback to plain text.
  6. Add operator-facing guidance for selecting compatible model IDs in local
     config and env overrides.
- Validation: Updated docs contain a concrete compatibility matrix/policy and no
  ambiguity about fallback behavior.
- Notes: Policy must be grounded in primary provider docs and repository
  runtime behavior.

#### Task 4.2: Implement Route-Level Guardrails For Conscious Structured Output

- Status: COMPLETED
- Objective: Enforce compatibility constraints at configuration or call-boundary
  validation time.
- Steps:
  1. Add route validation hooks for conscious structured-output requirements.
  2. Reject OpenRouter `auto` and any conscious route not in the explicit
     compatibility policy/allowlist with actionable config errors.
  3. Keep unconscious route behavior unchanged unless explicitly required.
- Validation: Config/model-gateway tests assert fail-closed behavior for
  unsupported structured-output route selections.
- Notes: Guardrails should be deterministic and metadata-driven, not heuristic.

#### Task 4.3: Add Structured-Output Preflight Verification Surface

- Status: COMPLETED
- Objective: Make compatibility testable by operators before production use.
- Steps:
  1. Add a deterministic preflight path (test harness or admin check) that
     verifies conscious route structured-output behavior.
  2. Cover both successful JSON-object responses and non-JSON/incompatible route
     failures.
  3. Document exact operator workflow for running this preflight.
- Validation: Preflight test/check passes on compatible routes and fails with
  clear diagnostics on incompatible routes.
- Notes: This is verification, not a runtime fallback mechanism.

### Milestone 5: Integration Test Migration And Documentation

- Status: COMPLETED
- Purpose: Migrate fixtures/tests/docs to the new structured-only protocol.
- Exit Criteria: All relevant tests and docs describe structured-only foreground
  output; no canonical docs claim tagged-block parsing remains.

#### Task 5.1: Migrate Harness Integration Fixtures To Structured Output

- Status: COMPLETED
- Objective: Replace text-with-fenced-block provider fixtures in foreground and
  governed-action integration tests.
- Steps:
  1. Update fake provider response builders to emit structured JSON text.
  2. Update integration tests that currently format
     ` ```blue-lagoon-governed-actions ` output.
  3. Keep behavioral assertions (executed actions, approvals, diagnostics)
     unchanged.
- Validation: `cargo test -p harness --test governed_actions_integration -- --nocapture`
  and `cargo test -p harness --test foreground_integration -- --nocapture`
  pass.
- Notes: Preserve test intent; change only protocol shape.

#### Task 5.2: Update Internal Conscious-Loop Docs

- Status: COMPLETED
- Objective: Keep internal docs accurate and line references current.
- Steps:
  1. Update `docs/internal/conscious_loop/GOVERNED_ACTIONS.md` to structured-only
     output contract and strictness posture.
  2. Update `docs/internal/conscious_loop/CONTEXT_ASSEMBLY.md` message-order and
     schema disclosure wording.
  3. Re-verify source line references and restamp verification date(s).
- Validation: Manual doc inspection confirms no stale tagged-block claims or
  broken source-line references.
- Notes: Internal docs must not contradict canonical docs.

#### Task 5.3: Update Canonical Behavior Docs Where Required

- Status: COMPLETED
- Objective: Ensure canonical docs reflect protocol-level behavior change.
- Steps:
  1. Update canonical docs only where behavior statements changed.
  2. Keep terminology aligned with `docs/REQUIREMENTS.md`,
     `docs/LOOP_ARCHITECTURE.md`, and `docs/IMPLEMENTATION_DESIGN.md`.
  3. Verify no planning labels leak into runtime behavior documentation.
- Validation: `git diff -- docs/ PHILOSOPHY.md README.md AGENTS.md` reviewed for
  consistency and scope correctness.
- Notes: Avoid archive edits unless explicitly requested.

### Milestone 6: Cleanup And Final Verification

- Status: COMPLETED
- Purpose: Ensure only intentional artifacts remain and the full refactor is verified.
- Exit Criteria: Temporary artifacts are removed, targeted and broad validation
  pass, and plan status can move to COMPLETED.

#### Task 6.1: Cleanup Intermediate Artifacts

- Status: COMPLETED
- Objective: Remove temporary exploration outputs or transition helpers that are
  not part of the final contract.
- Steps:
  1. Inspect worktree for temporary fixtures, notes, and dead compatibility code.
  2. Remove only artifacts not needed in final repository state.
  3. Keep maintainable tests and documentation that are part of the final
     behavior contract.
- Validation: `cmd.exe /c git status --short` shows only intentional files.
- Notes: Do not remove user-provided logs or unrelated in-progress work.

#### Task 6.2: Final Verification

- Status: COMPLETED
- Objective: Validate the integrated refactor end-to-end.
- Steps:
  1. Run full verification command set.
  2. Fix regressions and rerun until green or mark blockers explicitly.
- Validation: All commands in `Final Verification Commands` pass.
- Notes: Record skipped commands and reasons if any environment limitation occurs.

## Final Verification Commands

1. `cargo fmt --all --check`
2. `cargo check --workspace`
3. `cargo test -p workers --bin workers -- --nocapture`
4. `cargo test -p harness --lib -- --nocapture`
5. `cargo test -p harness --test foreground_component -- --nocapture`
6. `cargo test -p harness --test foreground_integration -- --nocapture`
7. `cargo test -p harness --test governed_actions_integration -- --nocapture`
8. `cargo test -p runtime --test admin_cli -- --nocapture`

## Approval Gate

Implementation must not start until the user approves this plan.

## Plan Self-Check

- [x] Plan location follows the default location rule.
- [x] Scope, non-goals, assumptions, and open questions are explicit.
- [x] Any unresolved open questions have been surfaced to the user.
- [x] Tasks are grouped into milestones because the plan has more than 10 tasks.
- [x] Every task has concrete steps and validation.
- [x] Every milestone has exit criteria.
- [x] Cleanup and final verification are included.
- [x] The plan avoids vague actions without concrete targets.
- [x] The plan can be executed by a coding agent without reading the original conversation.

## Execution Notes

- This plan intentionally forbids legacy fenced-block output parsing fallback.
- Re-steer remains allowed only as bounded orchestration retries after strict
  validation failures with explicit metadata.
