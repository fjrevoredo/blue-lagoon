# OpenRouter Reasoning Support Plan

Date: 2026-05-07
Plan Status: COMPLETED
Owner: Codex
Scope: Bring Blue Lagoon's model-provider and model-call design up to date for both reasoning and non-reasoning OpenRouter usage without regressing the current foreground reply contract.

## Goal

Add first-class reasoning-policy support to Blue Lagoon so OpenRouter models can be used in both reasoning and non-reasoning modes intentionally, with requirements, design, implementation, observability, and tests aligned.

## Scope

- Define the product and architectural posture for reasoning-capable models.
- Add a harness-owned reasoning policy layer instead of relying on ad hoc provider defaults.
- Make OpenRouter request shaping conditional on resolved reasoning policy and route capability.
- Preserve the current safe foreground behavior while adding an explicit path for reasoning-enabled operation.
- Improve traces and internal docs so malformed or partial reasoning responses are diagnosable.

## Non-Goals

- Implement multi-turn reasoning continuation or autonomous retry loops in this slice.
- Add streaming model-call support in this slice.
- Add provider-specific reasoning support for every provider beyond the minimum architectural extension points.
- Change the committed default provider away from Z.AI.

## Current Status

- OpenRouter provider support exists.
- Debug-log SQLx noise suppression exists.
- Blue Lagoon now exposes a provider-agnostic foreground reasoning policy with
  explicit OpenRouter compatibility mapping.
- Canonical requirements and design docs now define harness-owned
  reasoning-policy resolution.

## Assumptions

- The harness remains the sole owner of model-call request shaping and response validation.
- The current worker protocol still expects a single completed `ModelCallResponse` per model call.
- Provider-specific reasoning payloads are an implementation detail of the harness, not a worker concern.
- For this slice, correctness and explicit policy are more important than maximizing reasoning-token usage.

## Open Questions

None.

## Self-Check Before Approval

- Plan location follows repository planning guidance: yes, under `docs/`.
- Historical pre-approval gate was satisfied before execution: yes.
- Scope, non-goals, assumptions, and validation are explicit: yes.
- More than 10 tasks are grouped into milestones: yes.
- Cleanup and final verification are included: yes.
- No unresolved correctness or scope questions remain: yes.
- Backward compatibility for the currently introduced OpenRouter reasoning knobs is explicitly planned: yes.
- The validation surface includes at least one live provider verification step in addition to deterministic tests: yes.

## Milestone 1: Canonical Policy Definition

Status: COMPLETED
Purpose: Add the missing product-level and architectural definition for reasoning-capable model behavior.
Exit Criteria: Canonical docs explicitly define how Blue Lagoon treats reasoning modes, who owns the decision, and what runtime guarantees must hold when reasoning is enabled or disabled.

### Task 1: Document the product requirement for reasoning modes

Status: COMPLETED
Objective: Update canonical requirements so both reasoning and non-reasoning model operation are explicitly supported concepts.
Steps:
1. Update `docs/REQUIREMENTS.md` with a requirement that Blue Lagoon must support intentional reasoning-policy selection where supported by the configured route.
2. Define the minimum operator-visible outcomes: disabled reasoning, enabled reasoning, and safe fallback behavior.
3. State the failure-closed expectation when a provider route cannot satisfy the active reasoning policy safely.
Validation:
- Inspect `docs/REQUIREMENTS.md` and confirm the requirement is explicit and non-contradictory with current harness sovereignty.
- Run `git diff -- docs/REQUIREMENTS.md`.
Notes:
- Keep this at requirement level, not provider-implementation detail.

### Task 2: Update architecture/design treatment of reasoning policy

Status: COMPLETED
Objective: Make reasoning-policy resolution a harness-owned design concept in canonical architecture/design docs.
Steps:
1. Update `docs/IMPLEMENTATION_DESIGN.md` to state that reasoning policy is resolved by the harness before provider request shaping.
2. Update `docs/LOOP_ARCHITECTURE.md` if needed so foreground model-call execution explicitly allows policy-governed reasoning configuration.
3. State the boundary between provider-agnostic reasoning policy and provider-specific request encoding.
Validation:
- Inspect modified canonical docs for consistency with `PHILOSOPHY.md`.
- Run `git diff -- docs/IMPLEMENTATION_DESIGN.md docs/LOOP_ARCHITECTURE.md`.
Notes:
- Do not encode OpenRouter-only details in canonical docs.

### Task 3: Define reasoning-policy vocabulary

Status: COMPLETED
Objective: Establish the exact reasoning-policy states Blue Lagoon supports in this implementation slice.
Steps:
1. Decide and document a minimal stable vocabulary, for example `off`, `minimal`, `low`, `medium`, `high`, `xhigh`, and `provider_default`, or a narrower approved subset.
2. Define which states are mandatory for v1 and which are optional or provider-dependent.
3. Record how unsupported policies must resolve or fail.
Validation:
- Inspect canonical docs and internal plan consistency.
- Confirm the chosen vocabulary can be represented without leaking provider-specific enums into requirements text.
Notes:
- This choice directly affects config shape and test surface in later milestones.

## Milestone 2: Harness Policy and Config Design

Status: COMPLETED
Purpose: Introduce a clean runtime-owned reasoning-policy model instead of a hardcoded OpenRouter safety override.
Exit Criteria: Runtime config and internal docs describe a stable reasoning-policy resolution path independent of any one provider.

### Task 4: Design the runtime reasoning-policy config surface

Status: COMPLETED
Objective: Choose where reasoning policy lives in config and how it is overridden.
Steps:
1. Add a provider-agnostic config shape for foreground reasoning policy under the model gateway or another appropriate harness-owned section.
2. Define precedence between committed defaults, local config, and environment overrides.
3. Preserve current local OpenRouter testing workflows where possible.
Validation:
- Review the proposed config layout against `config/default.toml`, `config/local.example.toml`, and `.env.example`.
- Confirm the config shape can support future providers without redesign.
Notes:
- Avoid making OpenRouter-specific knobs the primary public contract.

### Task 5: Define compatibility and migration behavior for existing OpenRouter reasoning config

Status: COMPLETED
Objective: Ensure the new provider-agnostic reasoning-policy contract does not break the already-added local OpenRouter config workflow.
Steps:
1. Decide whether the current `model_gateway.openrouter.reasoning_effort` and `exclude_reasoning` keys remain supported as compatibility aliases, are mapped into the new generic policy layer, or are removed.
2. Define precedence when both generic and OpenRouter-specific reasoning settings are present.
3. Update examples and docs so operators can migrate cleanly without ambiguous behavior.
Validation:
- Add explicit config tests covering generic-only, provider-specific-only, and mixed configuration cases.
- Confirm `.env.example`, `config/default.toml`, and `config/local.example.toml` describe the final supported contract accurately.
Notes:
- This is required because the repository already contains OpenRouter-specific reasoning keys from the current stopgap implementation.

### Task 6: Define capability-aware reasoning resolution rules

Status: COMPLETED
Objective: Specify how the harness resolves requested reasoning policy against route/provider/model capability.
Steps:
1. Add an internal resolution rule set that maps the configured policy to the active route.
2. Define behavior for unsupported routes: omit reasoning payload, downgrade safely, or fail closed.
3. Define behavior for `openrouter/auto`-style or otherwise opaque routes if Blue Lagoon later supports them.
Validation:
- Add the rules to the plan’s implementation notes and later to internal docs.
- Confirm the rules do not require worker awareness.
Notes:
- This is the Blue Lagoon equivalent of OpenClaw’s conditional reasoning injection behavior.

### Task 7: Update internal model-provider documentation design

Status: COMPLETED
Objective: Make `docs/internal/harness/MODEL_PROVIDERS.md` reflect the new architecture before implementation is considered complete.
Steps:
1. Add sections describing reasoning-policy resolution, provider-specific request encoding, and malformed reasoning-response diagnostics.
2. Record every new config key and extension point.
3. Ensure source references are updated after code lands.
Validation:
- Re-read `docs/internal/harness/MODEL_PROVIDERS.md` rendered as Markdown.
- Verify every referenced `file.rs:line` still resolves after implementation.
Notes:
- This task must be completed in the same commit as the code changes that affect behavior.

## Milestone 3: Provider and Runtime Implementation

Status: COMPLETED
Purpose: Replace the current stopgap with an intentional reasoning-policy implementation in the harness.
Exit Criteria: The runtime resolves reasoning policy explicitly, encodes it safely for OpenRouter, and retains safe behavior for current non-streaming foreground calls.

### Task 8: Introduce provider-agnostic reasoning-policy types in the harness

Status: COMPLETED
Objective: Add stable internal types for resolved reasoning policy and policy outcome.
Steps:
1. Add new config/runtime types in `crates/harness/src/config.rs` or a focused module to represent configured and resolved reasoning policy.
2. Avoid exposing provider-specific payload shapes outside the harness request builder path.
3. Refactor the current `provider_reasoning` handling so it is derived from resolved policy instead of acting as the source of truth.
Validation:
- `cargo check --workspace`
- Focused unit tests for policy parsing and resolution.
Notes:
- Keep the worker protocol provider-agnostic unless a protocol change becomes necessary.

### Task 9: Implement OpenRouter reasoning payload mapping from resolved policy

Status: COMPLETED
Objective: Encode OpenRouter `reasoning` request payloads only from resolved policy, not direct ad hoc defaults.
Steps:
1. Update `crates/harness/src/model_gateway.rs` request construction to derive OpenRouter reasoning payloads from the resolved policy.
2. Preserve documented attribution headers and current OpenAI-compatible request body behavior.
3. Ensure disabled reasoning is represented correctly and unsupported states are handled explicitly.
Validation:
- `cargo test -p harness --lib model_gateway::tests -- --nocapture`
- Direct inspection of request-body assertions in gateway tests.
Notes:
- Use OpenRouter docs as the primary source for request semantics.

### Task 10: Preserve current safe foreground behavior while making it explicit

Status: COMPLETED
Objective: Keep current reply reliability during the transition from stopgap to policy-driven behavior.
Steps:
1. Ensure the resolved default policy for current OpenRouter foreground routes remains safe for non-streaming foreground replies.
2. Replace hardcoded “none because otherwise it breaks” logic with documented policy resolution.
3. Add comments only where the safety reasoning is not obvious from code.
Validation:
- `cargo check --workspace`
- Targeted tests confirming the resolved default policy for current OpenRouter config.
Notes:
- The implementation must not silently regress the user’s currently working OpenRouter setup.

### Task 11: Improve invalid-response classification for reasoning-heavy failures

Status: COMPLETED
Objective: Make failures involving reasoning-only or null-content responses diagnosable and policy-aware.
Steps:
1. Extend invalid-response diagnostics so traces distinguish between malformed payloads and reasoning-budget exhaustion scenarios.
2. Preserve the raw provider response body on failure.
3. Ensure the error summary is concise enough for operator CLI output while still precise.
Validation:
- Add or update unit tests in `crates/harness/src/model_gateway.rs`.
- Run `cargo test -p harness --lib model_gateway::tests -- --nocapture`.
Notes:
- This builds on the raw failure payload persistence already added.

## Milestone 4: Validation and Regression Coverage

Status: COMPLETED
Purpose: Prove the reasoning-policy implementation does not regress current behavior and covers the new capability surface.
Exit Criteria: The relevant config, gateway, trace, and component tests pass with coverage for both reasoning-off and reasoning-enabled route behavior.

### Task 12: Add config parsing and precedence tests

Status: COMPLETED
Objective: Prove config defaults, local overrides, and environment overrides resolve reasoning policy correctly.
Steps:
1. Add unit tests for the new config keys and precedence rules.
2. Cover the current OpenRouter default path and at least one explicit reasoning-enabled override.
3. Cover invalid values and fail-closed behavior.
Validation:
- `cargo test -p harness --lib config::tests -- --nocapture`
Notes:
- Tests must stay deterministic and not depend on live providers.

### Task 13: Add gateway tests for both reasoning-disabled and reasoning-enabled paths

Status: COMPLETED
Objective: Cover the request-shaping and response-handling differences introduced by reasoning policy.
Steps:
1. Assert the OpenRouter request body omits or includes the appropriate `reasoning` payload based on resolved policy.
2. Cover a successful non-reasoning response path.
3. Cover a reasoning-heavy null-content path and the resulting diagnostics.
Validation:
- `cargo test -p harness --lib model_gateway::tests -- --nocapture`
Notes:
- Include provider-body assertions, not just high-level success/failure.

### Task 14: Add harness-level regression coverage for model-call recording and traces

Status: COMPLETED
Objective: Ensure failure payload retention and trace surfacing remain correct under the new policy model.
Steps:
1. Add or extend tests that exercise model-call failure recording with `response_payload_json`.
2. Verify management/trace payload output still includes failed provider bodies.
3. Confirm success paths are unchanged.
Validation:
- `cargo test -p harness --lib -- --nocapture`
- If needed, focused management/model-call tests.
Notes:
- Keep the blast radius small; do not broaden into unrelated management CLI changes unless required.

### Task 15: Run focused integration/component checks for foreground safety

Status: COMPLETED
Objective: Verify the foreground runtime path still behaves correctly with the updated policy model.
Steps:
1. Run the smallest relevant component/integration suites touching foreground orchestration and worker model-call execution.
2. Investigate and fix any regressions introduced by the config or gateway changes.
3. Record any existing unrelated flake separately from this work.
Validation:
- `cargo test -p harness --test foreground_component -- --nocapture`
- `cargo test -p harness --test management_component -- --nocapture`
Notes:
- If a known unrelated flake reappears, document it explicitly rather than masking it.
- The known unrelated `foreground_component` flake
  `conscious_worker_protocol_failure_includes_phase_exit_and_stderr` reappeared
  in the full suite and passed immediately when rerun in isolation.

### Task 16: Perform live OpenRouter verification for both reasoning-disabled and reasoning-enabled cases

Status: COMPLETED
Objective: Verify the final implementation against the actual provider behavior that exposed the gap, not just fixtures and unit tests.
Steps:
1. Run at least one direct non-streaming OpenRouter request against a representative reasoning-heavy model with the resolved safe default policy.
2. Run at least one direct request or harness-mediated request with reasoning explicitly enabled and capture the observed response shape.
3. Record the exact observed behavior and any remaining unsupported cases in the implementation summary or internal docs.
Validation:
- Manual observable verification using the configured OpenRouter API key and a concrete model route.
- Confirm that traces preserve enough payload detail to diagnose any remaining live-provider mismatch.
Notes:
- This task is intentionally manual/live because the original failure was caused by real provider behavior not represented in local fixtures.

## Milestone 5: Cleanup and Final Verification

Status: COMPLETED
Purpose: Finish the slice cleanly, remove stopgap framing, and leave a durable execution record.
Exit Criteria: Plan, docs, and code all reflect the final reasoning-policy design; verification has been rerun and the repository contains no temporary debugging leftovers from this slice.

### Task 17: Cleanup temporary stopgap language and artifacts

Status: COMPLETED
Objective: Remove one-off language or temporary scaffolding that should not ship as the long-term design.
Steps:
1. Remove comments or docs that describe the implementation as a temporary workaround once the new policy model is in place.
2. Remove any scratch debugging helpers or local-only diagnostic code added during this effort.
3. Leave `logs.txt` untouched unless the user explicitly asks to change it.
Validation:
- `cmd.exe /c git diff --name-only`
- Manual inspection of touched files for leftover temporary wording.
Notes:
- Do not remove durable observability improvements that belong in the product.

### Task 18: Final verification pass

Status: COMPLETED
Objective: Run the final verification bundle for this slice and record the outcome.
Steps:
1. Run formatting, compile, and the focused test suites used during implementation.
2. Re-read all modified docs for contradictions with canonical docs.
3. Summarize any residual risk, including any provider behaviors still intentionally unsupported.
Validation:
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo test -p harness --lib config::tests -- --nocapture`
- `cargo test -p harness --lib model_gateway::tests -- --nocapture`
- `cargo test -p harness --lib trace::tests -- --nocapture`
- `cargo test -p harness --test foreground_component -- --nocapture`
- `cargo test -p harness --test management_component -- --nocapture`
- Re-run the live OpenRouter verification from Task 16 after the final code and doc state is in place.
Notes:
- Broaden to `cargo test --workspace` only if the implementation surface justifies it or if focused checks expose uncertainty.

## Final Verification Criteria

The work is complete only when all of the following are true:

- Canonical docs explicitly describe reasoning-policy support.
- Runtime config exposes an intentional reasoning-policy model.
- OpenRouter request shaping is driven by resolved policy, not a one-off hard default.
- Failed provider responses remain visible in traces.
- Focused compile and test validation passes without introducing new known regressions.
- Internal docs are updated with correct source references and verified date.

## Approval Gate

This plan was executed after explicit user approval.
