# Context Assembly TDD Rebuild Plan

## Metadata

- Plan Status: READY FOR APPROVAL
- Created: 2026-05-09
- Last Updated: 2026-05-09
- Owner: Coding agent
- Approval: PENDING

## Status Legend

- Plan Status values: DRAFT, QUESTIONS PENDING, READY FOR APPROVAL, APPROVED, IN PROGRESS, COMPLETED, BLOCKED
- Task/Milestone Status values: TO BE DONE, IN PROGRESS, COMPLETED, BLOCKED, SKIPPED

## Goal

Rebuild foreground context assembly around explicit scenario contracts and
golden-context tests so the conscious worker receives the right context for each
conversation situation by construction. The end state is a deterministic,
inspectable assembly policy that is specified first in tests, then implemented
to pass those tests, and finally refactored into maintainable context builders
instead of accumulating one-off heuristics. Where a scenario boundary cannot be
cleanly determined with deterministic rules, the design must prefer bounded
semantic techniques such as a small classifier or an explicit
background/unconscious evaluation path over growing ad hoc phrase lists.

## Current Status

The current implementation has improved from the earlier broken state, but it
is still reactive and fragile. The live failures on 2026-05-09 show that:

- scenario selection is encoded in scattered heuristics inside
  `crates/workers/src/main.rs`
- retrieval, recent history, developer instructions, and action guidance still
  interact implicitly instead of through a declared context policy
- sparse follow-ups, approvals, retries, reminder scheduling, and post-action
  turns do not yet have authoritative “ideal context” definitions
- the repository has unit tests for individual prompt fragments, but not a
  comprehensive matrix of end-to-end context shapes that own the intended
  behavior

This plan addresses that gap with a TDD-first rebuild strategy.

## Scope

- Define a canonical scenario matrix for foreground context assembly.
- Add golden-context tests that specify the ideal `ModelInput` for each
  scenario before implementation changes.
- Introduce a clearer assembly architecture that separates scenario detection,
  context selection, message shaping, and final trimming.
- Replace brittle prompt-time heuristics with deterministic scenario-owned
  policies where feasible.
- Evaluate and, if justified by the scenario matrix, design a bounded semantic
  classification or unconscious-evaluation fallback for cases that cannot be
  classified cleanly with deterministic logic.
- Add observability that explains not only what was assembled, but why each
  section was included or excluded.
- Add regression coverage for approvals, terse confirmations, retries,
  reminders, troubleshooting, routine chat, and post-action follow-up turns.
- Update internal docs so the new policy is the documented source of truth.

## Non-Goals

- No redesign of Telegram ingress, approvals, or governed-action execution.
- No provider or model-routing redesign.
- No broad rewrite of the self-model or identity system beyond context
  consumption boundaries.
- No migration of historical traces or old persisted episode text.
- No change to the canonical dual-loop architecture.
- No implementation work in this plan itself beyond the planning artifact.
- No unbounded expansion of hand-maintained phrase catalogs as the primary
  solution to scenario detection.

## Assumptions

- The foreground worker remains the final owner of model-facing prompt
  assembly.
- The harness will continue to assemble `ConsciousContext` and the worker will
  continue to turn it into `ModelInput`.
- Existing trace retention plus `prompt_metrics` are sufficient to validate
  most improvements without adding a second tracing system.
- The repository prefers deterministic automated tests over manual prompt
  inspection.
- Any semantic fallback must be explicitly bounded by cost, latency, and scope,
  and must not become the default path for routine foreground turns.
- Windows Git output remains the authoritative worktree view for final cleanup
  decisions.

## Design Constraints

- Prefer clean deterministic classification based on structured state,
  antecedent message class, governed-action state, approval state, and recent
  turn topology before considering lexical phrase matching.
- Do not solve scenario detection primarily by adding ever-growing hardcoded
  lists like `yes`, `well yes`, `please schedule`, or similar surface forms.
- If deterministic classification is insufficient for a scenario class, require
  an explicit decision record comparing:
  - stronger programmatic features
  - a small local classifier or bounded semantic scorer
  - unconscious/background evaluation with wake-up follow-up
- Any semantic fallback must declare:
  - triggering conditions
  - latency budget
  - cost budget
  - failure posture
  - why it is better than deterministic policy for that scenario
- Background or unconscious evaluation is acceptable only for cases where the
  user experience can tolerate deferred resolution. It must not be inserted
  into the hot path for routine foreground turns without a separate approval
  decision in the plan execution.

## Open Questions

None.

## Milestones

### Milestone 1: Define The Contract Surface

- Status: TO BE DONE
- Purpose: Turn “good context” from an intuition into an explicit,
  repository-owned contract.
- Exit Criteria: The repository has a written scenario matrix, measurable
  context invariants, and a stable list of user situations that the assembly
  pipeline must support.

#### Task 1.1: Write The Foreground Scenario Matrix

- Status: TO BE DONE
- Objective: Enumerate the distinct foreground situations that need different
  context shapes.
- Steps:
  1. Derive scenario classes from live failures, current code paths, and
     existing docs.
  2. Include at minimum: routine greeting, plain factual question, explicit
     action request, approval follow-up, terse confirmation, natural-language
     confirmation variant, retry after malformed action, post-execution
     follow-up, reminder scheduling, troubleshooting, and backlog recovery.
  3. Record each scenario’s trigger shape, required antecedents, forbidden
     context elements, and expected governed-action disclosure posture.
- Validation: The resulting scenario matrix can classify all known 2026-05-09
  failures and the currently tested happy paths without overlap or ambiguity.
- Notes: Place the scenario matrix in this plan and later mirror it into an
  internal doc if implementation starts.

#### Task 1.2: Define Ideal Context Invariants Per Scenario

- Status: TO BE DONE
- Objective: Make the expected `ModelInput` shape explicit enough to test.
- Steps:
  1. For each scenario, define which message kinds must be present,
     conditionally present, or forbidden.
  2. Define constraints on retrieved context, assistant history, developer
     instruction density, and schema disclosure level.
  3. Define allowed user-visible output classes where relevant, such as whether
     a governed-action block should be possible, required, or impossible.
- Validation: Each scenario has observable invariants that can be asserted in
  automated tests without depending on provider behavior.
- Notes: Prefer invariants like “no retrieved context for terse confirmation”
  over vague statements like “keep it focused.”

#### Task 1.3: Define Acceptance Metrics For Context Quality

- Status: TO BE DONE
- Objective: Create measurable pass/fail bounds for the rebuilt assembly
  policy.
- Steps:
  1. Set acceptable bounds for developer-message share, retrieved-context
     count, assistant-history count, and trim behavior per scenario class.
  2. Define mandatory exclusions such as instruction bleed, operational summary
     noise, stale approval prompts, and unrelated same-conversation retrieval.
  3. Define when a failure notice or approval observation is allowed to remain
     in history versus when it must be suppressed.
- Validation: The metrics can be expressed directly against
  `PromptCompositionMetrics` and message-kind assertions in tests.
- Notes: These metrics should inform both unit tests and later trace audits.

#### Task 1.4: Decide Which Scenario Boundaries Must Stay Deterministic

- Status: TO BE DONE
- Objective: Prevent the rebuild from drifting into brittle phrase lists or
  unjustified semantic fallback.
- Steps:
  1. Review the scenario matrix and classify each scenario boundary as
     deterministic, semantically ambiguous but latency-sensitive, or
     semantically ambiguous and deferrable.
  2. For each non-deterministic candidate, document why structured programmatic
     features are insufficient.
  3. Record whether the preferred solution is stronger deterministic features,
     a bounded classifier, or unconscious/background evaluation.
- Validation: Every scenario in the matrix has an explicit decision on whether
  deterministic policy is required or whether bounded semantic fallback is
  permitted.
- Notes: This task is the guardrail against reintroducing hardcoded phrase
  sprawl.

### Milestone 2: Build The Golden Test Harness

- Status: TO BE DONE
- Purpose: Create the test infrastructure needed to specify the ideal context
  before refactoring implementation.
- Exit Criteria: The repository can express scenario-owned golden tests against
  assembled `ModelInput` and explain failures at the message-kind level.

#### Task 2.1: Introduce A Reusable Foreground Context Fixture Builder

- Status: TO BE DONE
- Objective: Make it cheap to create scenario-specific `ConsciousContext`
  inputs without copy-pasting huge inline fixtures.
- Steps:
  1. Extract or add a builder helper for `ConsciousContext`, recent history,
     retrieved items, governed-action observations, and identity lifecycle
     state.
  2. Add concise fixture helpers for approval, reminder, retry, and
     troubleshooting antecedents.
  3. Keep the helper inside the worker test surface or a shared test module
     with minimal production impact.
- Validation: Existing worker prompt-assembly tests can be migrated to the new
  helper without losing coverage or readability.
- Notes: Avoid introducing production-only abstractions purely to support tests.

#### Task 2.2: Add Message-Kind Level Assertions

- Status: TO BE DONE
- Objective: Let tests validate context structure directly instead of only
  matching raw substrings.
- Steps:
  1. Expose or derive a test-only representation of assembled message kinds and
     normalized content.
  2. Add helpers to assert presence, absence, order, and count of message
     kinds.
  3. Add helpers to assert prompt metrics and trim events against scenario
     expectations.
- Validation: At least one existing prompt-assembly test is rewritten to use
  kind-level assertions instead of ad hoc string matching.
- Notes: The test surface should stay stable even if prompt wording changes.

#### Task 2.3: Add Golden Snapshot Coverage For Full `ModelInput`

- Status: TO BE DONE
- Objective: Capture the ideal full prompt shape for key scenarios.
- Steps:
  1. Choose a bounded golden representation for `system_prompt`,
     `messages`, and `prompt_metrics`.
  2. Add normalized golden snapshots for the highest-risk scenarios.
  3. Ensure snapshots are deterministic by normalizing timestamps, UUIDs, and
     other incidental values where needed.
- Validation: Golden tests fail with readable diffs when assembly behavior
  changes.
- Notes: Keep snapshots small and focused; scenario matrix assertions remain the
  primary guardrail.

### Milestone 3: Specify The Ideal Scenarios In Tests First

- Status: TO BE DONE
- Purpose: Encode the intended behavior before touching the assembly logic.
- Exit Criteria: The test suite contains failing or newly added scenario tests
  that fully describe the target context behavior for high-risk turns.

#### Task 3.1: Add Routine Chat Golden Tests

- Status: TO BE DONE
- Objective: Specify the ideal context for greetings and plain non-action chat.
- Steps:
  1. Add tests for simple greetings like `hello` and `hello again richard`.
  2. Assert that these turns use the short governed-action reminder, compact
     recent history, and no unrelated retrieved context.
  3. Assert that approval or failure residue is excluded unless it is directly
     relevant to the current user message.
- Validation: New routine-chat tests fail under intentionally polluted fixtures
  and pass only when unrelated context is suppressed.
- Notes: This closes the failure where old weather context bled into a new
  greeting.

#### Task 3.2: Add Action-Request Golden Tests

- Status: TO BE DONE
- Objective: Specify ideal context for direct action requests.
- Steps:
  1. Add tests for explicit action requests such as weather fetch, workspace
     inspection, and reminder scheduling.
  2. Assert that full governed-action schema appears when action intent is
     explicit.
  3. Assert that retrieved context is allowed only when it is directly relevant
     to fulfilling the current request.
- Validation: Each explicit-action scenario has a golden prompt and a kind-level
  assertion set.
- Notes: Cover both approval-gated and non-approval action classes.

#### Task 3.3: Add Confirmation And Retry Golden Tests

- Status: TO BE DONE
- Objective: Specify ideal context for terse confirmations and short retries.
- Steps:
  1. Add tests for `yes`, `well yes`, `go ahead`, `try again`, and
     `try it again properly`.
  2. Assert that these turns anchor on the immediately preceding assistant
     message, suppress retrieved context, and use full governed-action schema
     when continuing an action.
  3. Assert that malformed-action retry turns never receive only the short
     reminder.
- Validation: The new confirmation and retry tests fail if the bridge note is
  absent or if retrieved context leaks back in.
- Notes: This directly targets the observed weather and reminder failures.

#### Task 3.4: Add Post-Execution And Approval Golden Tests

- Status: TO BE DONE
- Objective: Specify ideal context after approvals and after successful or
  failed action execution.
- Steps:
  1. Add tests for pending approval, approval granted, action executed, and
     action failed states.
  2. Assert which observations must be visible in the immediate same-turn
     continuation and which should not persist into later unrelated turns.
  3. Assert that stale approval prompts do not keep dominating later routine
     chat.
- Validation: Approval and post-execution tests distinguish immediate
  continuation context from later independent turns.
- Notes: This is likely where future prompt pollution will re-enter if not
  owned by tests.

### Milestone 4: Refactor The Assembly Architecture

- Status: TO BE DONE
- Purpose: Replace scattered heuristics with a clearer policy pipeline that can
- be reasoned about from the scenario tests.
- Exit Criteria: Assembly logic is structured around scenario classification and
  message-selection policy, with less cross-cutting conditional logic inside
  `build_model_input()`.

#### Task 4.1: Extract Scenario Classification From Prompt Assembly

- Status: TO BE DONE
- Objective: Separate “what kind of turn is this?” from “how do we render the
  prompt?”
- Steps:
  1. Introduce a focused scenario classification layer that derives a bounded
     foreground context mode from `ConsciousContext`.
  2. Move scattered special-case checks for confirmation, retry, troubleshooting,
     and action intent behind that classifier.
  3. Keep the classifier deterministic and fully unit-tested.
- Validation: Scenario tests and classifier-specific unit tests both pass, and
  `build_model_input()` no longer owns ad hoc scenario detection logic.
- Notes: The classifier should not call retrieval or mutate context.

#### Task 4.1a: Design The Escalation Boundary For Semantic Classification

- Status: TO BE DONE
- Objective: Define a clean architectural seam for cases that cannot be
  determined reliably with deterministic features.
- Steps:
  1. Specify the interface between deterministic scenario classification and an
     optional semantic fallback.
  2. Ensure the default path remains deterministic and cheap.
  3. Define how a fallback result is surfaced back into context assembly
     without letting arbitrary model output directly shape the prompt.
- Validation: The architecture doc or execution notes include a concrete
  boundary that an implementing agent can follow without inventing a second
  prompt assembly path.
- Notes: This is design work first; implementation is optional and should be
  justified by failing scenario tests.

#### Task 4.2: Extract Message Selection Policy

- Status: TO BE DONE
- Objective: Centralize inclusion and exclusion rules for each message kind.
- Steps:
  1. Create a message-selection policy layer that decides whether recent
     history, retrieved context, observations, schema guidance, troubleshooting
     guidance, and identity guidance are included.
  2. Drive the policy from the scenario classification result rather than from
     independent inline `if` conditions.
  3. Make the selection policy return explicit inclusion reasons for tracing and
     test diagnostics.
- Validation: Golden tests pass, and unit tests can inspect policy decisions
  without rendering the final prompt.
- Notes: This is the core architectural cleanup that prevents further heuristic
  sprawl.

#### Task 4.3: Extract Rendering And Normalization

- Status: TO BE DONE
- Objective: Isolate string rendering from scenario and selection logic.
- Steps:
  1. Move history normalization, retrieved-summary compaction, and developer
     message formatting behind dedicated render helpers.
  2. Ensure render helpers are pure and deterministic.
  3. Add focused tests for formatting and normalization edge cases, including
     instruction bleed, standalone JSON payloads, and approval boilerplate.
- Validation: Rendering helpers have direct unit coverage and
  `build_model_input()` becomes an orchestration layer instead of a string-mix
  function.
- Notes: This makes later prompt wording changes safer and more local.

### Milestone 5: Strengthen Retrieval And History Ownership

- Status: TO BE DONE
- Purpose: Make retrieval and durable history explicitly subordinate to scenario
  policy instead of independently surfacing noisy material.
- Exit Criteria: Retrieval and recent history inclusion are controlled by
  scenario-owned tests and explicit policy, not only by lexical heuristics.

#### Task 5.1: Rework Retrieval Eligibility By Scenario

- Status: TO BE DONE
- Objective: Make retrieval participation depend on scenario policy.
- Steps:
  1. Define which scenario classes can use retrieval and which must suppress it.
  2. Encode that decision before retrieval summaries are rendered into the
     prompt.
  3. Add tests proving that greetings, confirmations, retries, and unrelated
     post-action turns do not receive episode retrieval by default.
- Validation: Scenario tests and retrieval tests agree on when retrieval is
  eligible.
- Notes: Retrieval ranking can remain heuristic, but eligibility should be
  policy-driven.

#### Task 5.1a: Research Alternatives To Lexical Retrieval Gating

- Status: TO BE DONE
- Objective: Reduce reliance on string-matched sparse-trigger rules when
  deciding retrieval eligibility.
- Steps:
  1. Evaluate whether retrieval gating can be driven by structured turn state,
     antecedent action state, message role topology, or scenario class alone.
  2. If not, compare bounded semantic gating alternatives such as a tiny
     classifier or scored feature model.
  3. Record the preferred approach and rejection reasons for weaker
     alternatives.
- Validation: The plan execution notes or follow-up design artifact contain a
  reasoned decision instead of defaulting to more string matching.
- Notes: Phrase lists may still appear as narrow compatibility guards, but not
  as the primary design.

#### Task 5.2: Rework Durable Assistant History Eligibility

- Status: TO BE DONE
- Objective: Decide more clearly what kinds of assistant history should ever be
  replayed into later prompts.
- Steps:
  1. Audit currently persisted assistant message classes: normal reply, failure
     notice, approval prompt, action follow-up, harness observation tail, and
     malformed-payload residue.
  2. Define replay eligibility rules per class.
  3. Add tests proving that operational summaries and malformed-payload residue
     are never replayed as assistant history.
- Validation: Prompt-assembly tests and persistence-path tests agree on replay
  eligibility.
- Notes: This may require small metadata additions if message classes cannot be
  inferred reliably from text alone.

#### Task 5.3: Re-evaluate Legacy Payload Compatibility Scope

- Status: TO BE DONE
- Objective: Ensure compatibility fallbacks remain bounded and do not become a
  second schema surface.
- Steps:
  1. Review every legacy or standalone payload shape currently tolerated by the
     worker.
  2. Keep only narrowly justified compatibility conversions with explicit tests
     and documentation.
  3. Add failure tests proving unsupported legacy shapes still fail closed.
- Validation: Compatibility behavior is documented, tested, and bounded to
  explicitly approved cases.
- Notes: The goal is robustness, not silent acceptance of arbitrary schema
  drift.

### Milestone 6: Add Runtime Observability And Live Validation

- Status: TO BE DONE
- Purpose: Make the new policy inspectable in retained traces and reproducible
  against live failures.
- Exit Criteria: Traces explain scenario classification and message inclusion
  decisions, and live validation covers the observed failure classes.

#### Task 6.1: Record Scenario And Inclusion Decisions In Trace Metadata

- Status: TO BE DONE
- Objective: Make runtime prompt decisions explainable without reading source.
- Steps:
  1. Add trace-visible metadata for classified scenario, retrieval eligibility,
     schema disclosure mode, and major message inclusion/exclusion reasons.
  2. Extend `PromptCompositionMetrics` or adjacent retained metadata without
     bloating the hot path.
  3. Update trace-inspection docs to show how operators read these signals.
- Validation: A retained `ModelCallRequest` or adjacent trace payload shows the
  scenario and at least the key inclusion decisions.
- Notes: Keep operator-facing names stable and domain-specific.

#### Task 6.2: Add Deterministic Live Reproduction Fixtures

- Status: TO BE DONE
- Objective: Preserve the observed failures as reproducible repo-owned cases.
- Steps:
  1. Convert the 2026-05-09 weather confirmation and reminder retry flows into
     deterministic tests or harness fixtures where possible.
  2. If a full fixture is too heavy, create reduced synthetic contexts that
     still reproduce the polluted prompt shape.
  3. Record the exact scenario mapping and expected prompt output in tests.
- Validation: The repository can reproduce both failure classes without relying
  on the live Telegram channel.
- Notes: Prefer worker-level reproduction first, then harness-level integration
  only where necessary.

#### Task 6.3: Add Cost And Latency Guardrails For Any Semantic Fallback

- Status: TO BE DONE
- Objective: Ensure any AI-based classification path is operationally safe.
- Steps:
  1. Define explicit cost and latency thresholds for any classifier or
     unconscious-evaluation fallback.
  2. Define when fallback is skipped and the system must fail closed or ask the
     user for clarification instead.
  3. Add observability so traces show when semantic fallback was considered,
     invoked, skipped, or timed out.
- Validation: The design includes concrete budgets and failure posture for any
  approved semantic fallback path.
- Notes: This task should remain `SKIPPED` if the final rebuild stays fully
  deterministic.

### Milestone 7: Cleanup And Final Verification

- Status: TO BE DONE
- Purpose: Ensure the repository contains only intentional final artifacts and
  the complete change is verified.
- Exit Criteria: Intermediate artifacts are removed, all final verification
  passes, and the plan status is COMPLETED.

#### Task 7.1: Cleanup Intermediate Artifacts

- Status: TO BE DONE
- Objective: Remove artifacts created only to support implementation.
- Steps:
  1. Inspect the worktree for temporary documentation, one-off scripts, scratch
     tests, generated data, logs, and obsolete plan fragments.
  2. Remove only artifacts that are not part of the intended final repository
     state.
  3. Keep maintainable tests, fixtures, docs, and generated files that are part
     of the repository contract.
- Validation: `cmd.exe /c git status --short` shows only intended final
  repository changes.
- Notes: Do not remove user-provided files or unrelated worktree changes.

#### Task 7.2: Final Verification

- Status: TO BE DONE
- Objective: Validate the integrated change after cleanup.
- Steps:
  1. Run the full targeted verification surface for worker assembly, retrieval,
     foreground component behavior, and integration behavior.
  2. Run trace-facing and documentation checks for updated source references.
  3. Review at least one retained or replayed trace per high-risk scenario to
     confirm runtime context matches the tested contract.
- Validation:
  - `cargo fmt --all --check`
  - `cargo check --workspace`
  - `cargo test -p workers --bin workers -- --nocapture`
  - `cargo test -p harness retrieval::tests --lib -- --nocapture`
  - `cargo test -p harness --test foreground_component -- --nocapture`
  - `cargo test -p harness --test foreground_integration -- --nocapture`
  - `cargo test -p harness --test governed_actions_component -- --nocapture`
  - `cargo test -p harness --test governed_actions_integration -- --nocapture`
  - `cmd.exe /c git diff -- docs/internal/conscious_loop/CONTEXT_ASSEMBLY.md docs/internal/conscious_loop/GOVERNED_ACTIONS.md`
- Notes: If live Telegram validation is used, record exact dates and traces in
  the execution notes.

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

- Analysis summary used for this plan:
  - Current context behavior is governed by a growing set of prompt-time
    heuristics in `crates/workers/src/main.rs` rather than by a declared
    scenario policy.
  - Retrieval suppression, assistant-history suppression, and full-schema
    disclosure are partly correct but still encoded as special cases.
  - Existing worker tests are useful but fragmented; they validate isolated
    fixes rather than a complete scenario matrix.
  - The observed 2026-05-09 failures are best addressed by owning the ideal
    `ModelInput` shape per scenario first, then refactoring the assembly
    architecture to satisfy those tests.
  - A clean final design must avoid expanding hardcoded phrase lists as the
    primary classification mechanism. Deterministic structured features are
    preferred; bounded semantic techniques are acceptable only when justified by
    scenario-specific ambiguity and explicit operational guardrails.
- Update milestone and task status before starting and after validation.
- Update each task to COMPLETED immediately after its validation passes.
- Mark tasks or milestones BLOCKED with a short reason when progress cannot continue.
