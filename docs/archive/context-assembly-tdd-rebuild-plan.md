# Context Assembly TDD Rebuild Plan

## Metadata

- Plan Status: COMPLETED
- Created: 2026-05-09
- Last Updated: 2026-05-09
- Owner: Coding agent
- Approval: APPROVED 2026-05-09

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

- Status: COMPLETED
- Purpose: Turn “good context” from an intuition into an explicit,
  repository-owned contract.
- Exit Criteria: The repository has a written scenario matrix, measurable
  context invariants, and a stable list of user situations that the assembly
  pipeline must support.

#### Task 1.1: Write The Foreground Scenario Matrix

- Status: COMPLETED
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

Execution Result:

| Scenario | Trigger Shape | Required Antecedents | Required Context | Forbidden Context | Governed-Action Disclosure |
|---|---|---|---|---|---|
| Routine greeting | Greeting or light check-in such as `hello`, `hi`, or `hello again richard`; no action verbs or diagnostic terms | None | system prompt, current trigger, compact recent user/assistant history after sanitation | retrieved episode context, stale approval prompts, malformed-action residue, operational summaries | short reminder only |
| Plain factual question | User asks an answerable non-action question without workspace, schedule, web, diagnostic, or execution intent | None | system prompt, current trigger, compact recent history after sanitation | unrelated retrieved context, full action schema, troubleshooting guidance | short reminder only |
| Continuity preference follow-up | User asks about, reinforces, or updates remembered preferences, naming, directness, concision, or interaction style | Prior preference/memory may exist in retrieval | system prompt, current trigger, relevant retrieved memory/episode context when supplied | full action schema unless a governed capability is also requested | short reminder only |
| Explicit action request | User asks to inspect, list, read, create, update, run, fetch, search, open, schedule, remind, or otherwise do work through a governed capability | None | system prompt, current trigger, recent history, relevant retrieved context when supplied | unrelated stale approval prompts and malformed payload residue | full schema |
| Reminder scheduling | User asks for future foreground work with `remind`, `schedule`, `later`, or equivalent scheduling intent | None | system prompt, current trigger, recent history, scheduling schema examples | unrelated retrieval unless explicitly relevant to the schedule request | full schema |
| Troubleshooting | User asks about errors, traces, logs, diagnostics, failures, why something got stuck, or a trace id | Optional trace id or failure notice | system prompt, current trigger, recent history, troubleshooting guidance, relevant failure notice | normal shell-oriented troubleshooting guidance, unrelated retrieval | full schema plus troubleshooting guidance |
| Pending approval follow-up | Current turn is generated from an approval callback or approval state rather than ordinary chat | approval payload or pending approval context in recent history | system prompt, current trigger/approval event, immediately relevant approval prompt or observation | unrelated retrieved context and stale unrelated approvals | full schema if another action may be needed |
| Terse confirmation | Short confirmation such as `yes`, `ok`, `sure`, `go ahead`, `please do`, or `do it` | immediately preceding assistant asked whether to continue/proceed/approve | system prompt, current trigger, preceding assistant turn, confirmation bridge | retrieved context, unrelated history pollution | full schema |
| Natural-language confirmation variant | Short confirmation with filler or light natural phrasing such as `well yes` | same as terse confirmation | system prompt, current trigger, preceding assistant turn, confirmation bridge | retrieved context, unrelated history pollution | full schema |
| Retry after malformed action | Short retry request such as `try again` or `try it again properly` | immediately preceding assistant or failure notice reports malformed governed-action proposal | system prompt, current trigger, preceding failure/assistant retry context, retry bridge | retrieved context, short reminder-only action guidance | full schema |
| Post-execution follow-up | Harness supplied governed-action observations in the current continuation turn | at least one governed-action observation | system prompt, current trigger/history, observation message, foreground action-loop state | separate schema/reminder messages, unrelated retrieval unless scenario policy allows it | observation continuation guidance only |
| Backlog recovery | recovery mode is `backlog_recovery` with ordered delayed ingress | ordered ingress batch | system prompt, recent history, current trigger, backlog recovery notice | unrelated retrieved context unless explicit action or troubleshooting intent | scenario-dependent full schema or short reminder |

Validation: This matrix covers the 2026-05-09 weather confirmation and reminder
retry failures, plus the current prompt-assembly happy paths for routine chat,
explicit action requests, retrieved context, troubleshooting, observation
follow-up, and backlog recovery without overlapping scenario ownership.

#### Task 1.2: Define Ideal Context Invariants Per Scenario

- Status: COMPLETED
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

Execution Result:

Common invariants:

- The system prompt is always present and includes identity, runtime estimates,
  current time, and the governed-action availability baseline.
- The current trigger user message is present when `text_body` exists.
- Assistant history is replayed only after prompt-time sanitation removes
  instruction bleed, operational summaries, standalone JSON/control payloads,
  and stale failure/approval residue that is not directly relevant.
- Retrieved context appears only when the scenario policy marks retrieval
  eligible and the harness supplied items.
- Governed-action observations replace normal schema/reminder disclosure for
  same-turn continuation calls.

Scenario-specific invariants:

- Routine greeting and plain factual question: no retrieved context, no full
  governed-action schema, no troubleshooting guidance, no approval/failure
  residue.
- Continuity preference follow-up: retrieved memory/episode context is eligible
  so remembered preferences can shape the answer, but the short governed-action
  reminder remains sufficient unless the user also asks for an explicit action.
- Explicit action request and reminder scheduling: full schema required;
  retrieval eligible only as scenario-owned supporting context.
- Troubleshooting: full schema and troubleshooting guidance required; diagnostic
  guidance must prefer `run_diagnostic`.
- Terse confirmation, natural-language confirmation, and malformed-action retry:
  confirmation bridge required, retrieved context forbidden, full schema
  required, immediate antecedent retained.
- Post-execution follow-up: governed-action observation message required;
  normal schema/reminder and identity/troubleshooting add-ons suppressed for
  that continuation call.
- Backlog recovery: backlog notice required when ordered ingress exists.

#### Task 1.3: Define Acceptance Metrics For Context Quality

- Status: COMPLETED
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

Execution Result:

- Routine greeting and plain factual question: developer messages must contain
  only the short governed-action reminder unless identity formation is active;
  retrieved-context count must be zero; full-schema count must be zero.
- Explicit action, reminder scheduling, retry, confirmation, and
  troubleshooting: exactly one full governed-action schema message unless a
  same-turn observation replaces schema disclosure.
- Post-execution continuation: at least one governed-action observation
  developer message, zero schema/reminder developer messages.
- Confirmation and retry turns: retrieved-context count must be zero and one
  confirmation bridge must be present.
- Prompt metrics must report message count, character counts, trim events, the
  classified scenario, schema disclosure mode, retrieval eligibility, and
  inclusion/exclusion decisions.
- Trim behavior remains deterministic: retrieved context, recovery notice,
  troubleshooting guidance, assistant history, then user history.
- Mandatory exclusions for replayed assistant history: instruction bleed,
  operational summaries, stale approval prompts in independent turns,
  malformed-action JSON residue, and harness observation tails in unrelated
  turns.

#### Task 1.4: Decide Which Scenario Boundaries Must Stay Deterministic

- Status: COMPLETED
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

Execution Result:

| Scenario Boundary | Decision | Rationale |
|---|---|---|
| Routine greeting | Deterministic | Low-risk and recognizable from compact greeting intent plus absence of action/troubleshooting features. |
| Plain factual question | Deterministic | Owned by absence of structured action/troubleshooting/recovery/observation state. |
| Continuity preference follow-up | Deterministic with bounded lexical compatibility guard | Preserves mind-like memory for preference and identity continuity without broadening routine/factual retrieval. |
| Explicit action request | Deterministic with bounded lexical compatibility guard | Current contracts expose action intent only through text; phrase guards remain narrow and map to governed capability names. |
| Reminder scheduling | Deterministic with bounded lexical compatibility guard | Scheduling has explicit model-facing action kind and predictable time-intent terms. |
| Troubleshooting | Deterministic | Diagnostic intent is latency-sensitive and maps to a fixed capability surface. |
| Approval follow-up | Deterministic | Approval payloads and approval-state observations are structured state. |
| Terse/natural confirmation | Deterministic with compact compatibility guard | Must be hot-path and anchored by immediate assistant antecedent; no semantic fallback in the foreground path. |
| Retry after malformed action | Deterministic | Owned by previous malformed-action/failure notice plus short retry trigger. |
| Post-execution follow-up | Deterministic | Governed-action observations are structured state. |
| Backlog recovery | Deterministic | Recovery mode and ordered ingress are structured state. |

No semantic fallback is approved for the rebuild implementation. The future
extension boundary is a deterministic classifier result with an optional bounded
semantic override field, but the default and currently implemented path remains
cheap, deterministic, and traceable.

### Milestone 1 Exit Check

- Scenario matrix is present in this plan.
- Context invariants and metrics are testable against `ModelInput`,
  `PromptCompositionMetrics`, and message-kind assertions.
- Every scenario boundary has an explicit deterministic/semantic decision.

### Milestone 2: Build The Golden Test Harness

- Status: COMPLETED
- Purpose: Create the test infrastructure needed to specify the ideal context
  before refactoring implementation.
- Exit Criteria: The repository can express scenario-owned golden tests against
  assembled `ModelInput` and explain failures at the message-kind level.

#### Task 2.1: Introduce A Reusable Foreground Context Fixture Builder

- Status: COMPLETED
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

Execution Result: Added `ConsciousContextFixture` in the worker test module with
helpers for trigger text, retrieved episodes, approval payloads, governed-action
observations, backlog recovery, confirmation antecedents, retry antecedents, and
deterministic timestamps.

#### Task 2.2: Add Message-Kind Level Assertions

- Status: COMPLETED
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

Execution Result: Added worker test helpers that classify rendered
`ModelInputMessage` values into stable message-kind labels and assert presence,
absence, order, and high-level context shape.

#### Task 2.3: Add Golden Snapshot Coverage For Full `ModelInput`

- Status: COMPLETED
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

Execution Result: Added bounded golden context-shape snapshots that normalize
the full `ModelInput` into scenario, schema disclosure, retrieval eligibility,
and ordered message-kind labels.

### Milestone 3: Specify The Ideal Scenarios In Tests First

- Status: COMPLETED
- Purpose: Encode the intended behavior before touching the assembly logic.
- Exit Criteria: The test suite contains failing or newly added scenario tests
  that fully describe the target context behavior for high-risk turns.

#### Task 3.1: Add Routine Chat Golden Tests

- Status: COMPLETED
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

Execution Result: Added routine greeting and plain factual question golden tests
that suppress polluted retrieval and stale approval residue.

#### Task 3.2: Add Action-Request Golden Tests

- Status: COMPLETED
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

Execution Result: Added action-request coverage for weather fetch, workspace
inspection, and reminder scheduling with full schema and retrieval eligibility.

#### Task 3.3: Add Confirmation And Retry Golden Tests

- Status: COMPLETED
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

Execution Result: Added confirmation and retry scenario tests for `yes`,
`well yes`, `go ahead`, `try again`, and `try it again properly`.

#### Task 3.4: Add Post-Execution And Approval Golden Tests

- Status: COMPLETED
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

Execution Result: Added distinct approval-follow-up and post-execution
observation tests, including stale retrieval suppression.

### Milestone 4: Refactor The Assembly Architecture

- Status: COMPLETED
- Purpose: Replace scattered heuristics with a clearer policy pipeline that can
- be reasoned about from the scenario tests.
- Exit Criteria: Assembly logic is structured around scenario classification and
  message-selection policy, with less cross-cutting conditional logic inside
  `build_model_input()`.

#### Task 4.1: Extract Scenario Classification From Prompt Assembly

- Status: COMPLETED
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

Execution Result: Added deterministic `classify_foreground_context()` and
`foreground_context_policy()` so `build_model_input()` consumes a policy result
instead of owning scenario detection.

#### Task 4.1a: Design The Escalation Boundary For Semantic Classification

- Status: COMPLETED
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

Execution Result: Documented the optional semantic classification boundary in
`docs/internal/conscious_loop/CONTEXT_ASSEMBLY.md`. No semantic fallback is
implemented or approved for the current deterministic rebuild.

#### Task 4.2: Extract Message Selection Policy

- Status: COMPLETED
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

Execution Result: Added scenario-owned retrieval eligibility, schema disclosure,
troubleshooting disclosure, recovery notice selection, assistant-history replay
filtering, and trace-facing inclusion decisions.

#### Task 4.3: Extract Rendering And Normalization

- Status: COMPLETED
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

Execution Result: Kept rendering in deterministic helper functions and added
focused tests for standalone governed-action JSON/control payloads and approval
boilerplate suppression.

### Milestone 5: Strengthen Retrieval And History Ownership

- Status: COMPLETED
- Purpose: Make retrieval and durable history explicitly subordinate to scenario
  policy instead of independently surfacing noisy material.
- Exit Criteria: Retrieval and recent history inclusion are controlled by
  scenario-owned tests and explicit policy, not only by lexical heuristics.

#### Task 5.1: Rework Retrieval Eligibility By Scenario

- Status: COMPLETED
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

Execution Result: Retrieval summaries are rendered only when
`retrieval_eligible_for_scenario()` permits them. Greetings, factual questions,
confirmations, retries, approval follow-ups, and post-execution continuations
suppress retrieved context by policy.

#### Task 5.1a: Research Alternatives To Lexical Retrieval Gating

- Status: COMPLETED
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

Execution Result: Chose scenario-policy gating over additional retrieval phrase
lists. Structured turn state owns observations, approvals, recovery, and
governed-action continuation. Bounded capability-intent terms remain only as
compatibility guards for explicit action and scheduling detection.

#### Task 5.2: Rework Durable Assistant History Eligibility

- Status: COMPLETED
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

Execution Result: Prompt-time replay now suppresses instruction bleed,
operational summaries, standalone control payloads, and stale approval/failure
residue for independent routine and factual turns. No persistence schema change
was required.

#### Task 5.3: Re-evaluate Legacy Payload Compatibility Scope

- Status: COMPLETED
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

Execution Result: Kept the narrow legacy `schedule_task` conversion and added a
failure-closed test for unsupported legacy action shapes.

### Milestone 6: Add Runtime Observability And Live Validation

- Status: COMPLETED
- Purpose: Make the new policy inspectable in retained traces and reproducible
  against live failures.
- Exit Criteria: Traces explain scenario classification and message inclusion
  decisions, and live validation covers the observed failure classes.

#### Task 6.1: Record Scenario And Inclusion Decisions In Trace Metadata

- Status: COMPLETED
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

Execution Result: Extended `PromptCompositionMetrics` with scenario,
schema-disclosure mode, retrieval eligibility, and inclusion decisions. These
fields are retained on `ModelCallRequest.prompt_metrics`.

#### Task 6.2: Add Deterministic Live Reproduction Fixtures

- Status: COMPLETED
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

Execution Result: Converted the weather confirmation and reminder retry failure
classes into deterministic worker-level golden tests.

#### Task 6.3: Add Cost And Latency Guardrails For Any Semantic Fallback

- Status: SKIPPED
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

Execution Result: Skipped because the final rebuild stays fully deterministic.
The future semantic boundary is documented, but no classifier, model call, cost,
or latency path exists in this change.

### Milestone 7: Cleanup And Final Verification

- Status: COMPLETED
- Purpose: Ensure the repository contains only intentional final artifacts and
  the complete change is verified.
- Exit Criteria: Intermediate artifacts are removed, all final verification
  passes, and the plan status is COMPLETED.

#### Task 7.1: Cleanup Intermediate Artifacts

- Status: COMPLETED
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

Execution Result: No temporary scripts, generated data, logs, or scratch files
were created. `cmd.exe /c git status --short` shows only intentional code,
test, plan, and internal documentation changes.

#### Task 7.2: Final Verification

- Status: COMPLETED
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

Execution Result:

- `cargo fmt --all --check` passed.
- `cargo check --workspace` passed.
- `cargo clippy --workspace --all-targets -- -D warnings` passed.
- `cargo test -p workers --bin workers -- --nocapture` passed.
- `cargo test -p harness retrieval::tests --lib -- --nocapture` passed.
- `cargo test -p harness --test foreground_component -- --nocapture` passed.
- `cargo test -p harness --test foreground_integration -- --nocapture` passed.
- `cargo test -p harness --test governed_actions_component -- --nocapture` passed.
- `cargo test -p harness --test governed_actions_integration -- --nocapture` passed.
- `cmd.exe /c git diff -- docs/internal/conscious_loop/CONTEXT_ASSEMBLY.md docs/internal/conscious_loop/GOVERNED_ACTIONS.md` passed as an inspection step and shows the expected source-reference and policy documentation updates.
- No live Telegram validation was run; deterministic worker and harness
  reproduction covered the high-risk scenarios.

### Milestone 7 Exit Check

- Only intended final files are modified.
- All requested final verification commands either passed directly or, for the
  internal-doc diff command, completed as the required review artifact.
- Plan status is now `COMPLETED`.

## Approval Gate

Implementation approved by the user on 2026-05-09.

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
