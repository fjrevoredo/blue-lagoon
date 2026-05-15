# Context Assembly Remediation Plan

## Metadata

- Plan Status: COMPLETED
- Created: 2026-05-08
- Last Updated: 2026-05-08
- Owner: Coding agent
- Approval: APPROVED

## Status Legend

- Plan Status values: DRAFT, QUESTIONS PENDING, READY FOR APPROVAL, APPROVED, IN PROGRESS, COMPLETED, BLOCKED
- Task/Milestone Status values: TO BE DONE, IN PROGRESS, COMPLETED, BLOCKED, SKIPPED

## Goal

Repair the foreground context-assembly pipeline so the conscious worker receives
compact, relevant, and behaviorally safe context. The plan targets the defects
confirmed in code and retained traces: corrupted canonical identity values,
instruction-heavy prompt composition, recursive contamination of stored
assistant messages, over-broad retrieval of failure and tooling text, and weak
observability around prompt composition quality.

## Current Status

Static analysis, retained model-call traces, and live management CLI inspection
show that the current context frequently over-represents developer instructions
and retrieved summaries while under-representing the active user request.
Canonical identity state is already polluted by unvalidated custom interview
answers. Retrieval artifacts amplify low-quality assistant outputs and failure
notices back into future prompts. Some recent runtime failures are unrelated to
context assembly, but the context itself is degraded enough to plausibly
increase malformed outputs, repeated tool-schema parroting, and poor action
selection.

## Scope

- Add validation and conservative normalization for identity kickstart answers
  before canonical identity commits.
- Add repair tooling or controlled reset flow for already-corrupted identity
  state.
- Reduce default foreground prompt size and instruction dominance.
- Normalize assistant text before persistence to prevent recursive
  `[timestamp] Assistant:` contamination.
- Improve retrieval candidate hygiene and retrieval-summary compactness.
- Add explicit prompt-composition observability so operators can measure context
  quality instead of inferring it from model behavior.
- Update implementation docs and tests so the new posture remains stable.

## Non-Goals

- No redesign of the dual-loop architecture.
- No provider migration or model-routing redesign beyond what is required to
  measure or cap prompt inputs.
- No multi-user identity model.
- No broad rewrite of the governed-action system.
- No attempt to retroactively fix every historical trace payload.
- No automatic mutation of corrupted identity records without an auditable,
  harness-owned path.

## Assumptions

- The primary delivery target remains the existing Telegram-first foreground
  runtime.
- The harness remains the sole authority for canonical writes, prompt assembly,
  policy, and observability.
- Existing retained traces and CLI surfaces are sufficient to validate most of
  the remediation work.
- Identity corruption came from real custom interview flows and should be
  treated as a product defect, not as acceptable operator-authored content.
- A conservative reduction in prompt breadth is preferable to a broad context
  window filled with noisy history and schema text.

## Open Questions

None.

## Milestones

### Milestone 1: Freeze The Problem Surface

- Status: COMPLETED
- Purpose: Capture the current failure modes and establish a measurable baseline
  before changing behavior.
- Exit Criteria: The repository has a stable defect ledger, representative trace
  samples, and explicit metrics for prompt composition quality that later
  milestones can compare against.

#### Task 1.1: Capture Baseline Context Metrics

- Status: COMPLETED
- Objective: Record the current prompt-composition footprint for representative
  successful and failed traces.
- Steps:
  1. Select a bounded sample of retained foreground traces that includes
     ordinary chat, governed-action turns, scheduled foreground turns, and
     troubleshooting turns.
  2. Record system prompt length, message count, developer-message share,
     retrieved-context share, history share, and provider-recorded input tokens.
  3. Record the baseline findings in this plan under a short implementation note
     section or in a dedicated internal doc under `docs/internal/` so later
     milestones can compare before and after values from a stable repository
     location.
- Validation: Manual inspection confirms the baseline includes at least one
  successful noisy trace and one successful clean trace with exact measured
  values.
- Notes: Use the existing `admin trace show --json` model-call payloads as the
  source of truth.

#### Task 1.2: Classify Context-Related Failure Patterns

- Status: COMPLETED
- Objective: Separate context-caused defects from unrelated infrastructure or
  state-transition failures.
- Steps:
  1. Review recent traces and group them into context-related failures,
     approval-state failures, worker launch/protocol failures, and provider
     response-shape failures.
  2. Document which failure classes this plan is intended to reduce and which
     are explicitly out of scope.
  3. Add representative trace ids to tests or docs where useful.
- Validation: Manual review shows each sampled trace is assigned to a failure
  class with a short rationale.
- Notes: This task prevents later regressions from being blamed on context when
  they are infrastructure defects.

#### Task 1.3: Add Prompt-Quality Acceptance Criteria

- Status: COMPLETED
- Objective: Define concrete targets for what counts as an improved foreground
  context.
- Steps:
  1. Define acceptable bounds for developer-message share, retrieved-context
     size, recent-history size, and overall token budget conformance.
  2. Define qualitative criteria for clean assistant history, clean identity
     projection, and clean retrieval summaries.
  3. Add these criteria to this plan's final verification section and any new
     test helpers.
- Validation: The acceptance criteria are explicit enough that a later agent can
  determine pass or fail without re-reading the original investigation.
- Notes: Prefer conservative, measurable bounds over vague statements like
  "smaller prompt" or "better context."

### Milestone 2: Repair Identity Hygiene At The Source

- Status: COMPLETED
- Purpose: Stop invalid identity data from entering canonical state and define a
  safe remediation path for already-corrupted active identity.
- Exit Criteria: Custom identity interview answers are validated before commit,
  corrupted answers are rejected with diagnostics, and operators have an
  auditable way to recover from existing bad identity state.

#### Task 2.1: Add Field-Level Validation For Custom Identity Answers

- Status: COMPLETED
- Objective: Prevent arbitrary conversational text from being committed as
  stable or evolving identity fields.
- Steps:
  1. Define field-specific validation rules for each custom interview step such
     as name, identity form, role, values, boundaries, goals, and relationship
     to the user.
  2. Reject empty, obviously off-step, or meta-conversational answers such as
     acknowledgements, questions about the process, or unrelated operator
     instructions.
  3. Surface rejection reasons through identity diagnostics and resume the
     interview at the same step instead of silently accepting bad values.
- Validation: Add unit or component tests proving invalid answers are rejected
  and valid answers still pass for every required interview step category.
- Notes: Stable identity fields should have stricter validation than evolving
  preference-like fields.

#### Task 2.2: Add Safe Inference Rules For Missing Or Ambiguous Interview Blocks

- Status: COMPLETED
- Objective: Keep the tolerant interview UX without allowing arbitrary trigger
  text to become canonical identity.
- Steps:
  1. Review the current inference path that derives interview answers from raw
     trigger text when the model omits the identity control block.
  2. Restrict inference to clearly bounded cases where the answer is a direct,
     step-appropriate response.
  3. Require explicit repetition of the question or a retry prompt when the
     trigger is ambiguous.
- Validation: Tests prove that ambiguous answers such as `ok`, unrelated
  comments, and process questions do not become identity deltas.
- Notes: This should preserve resilient UX while failing closed on bad data.

#### Task 2.3: Add Identity Diagnostics For Invalid Or Suspicious Answers

- Status: COMPLETED
- Objective: Make bad identity inputs observable instead of silently accepted.
- Steps:
  1. Add diagnostic kinds for invalid interview answer, suspicious stable-field
     answer, and rejected identity projection input.
  2. Persist diagnostics through the existing management surfaces.
  3. Extend CLI inspection so operators can see why a proposed or inferred
     answer was rejected.
- Validation: `admin identity diagnostics list` returns meaningful records after
  deterministic invalid-answer tests.
- Notes: A rejected identity answer should be auditable in the same way as
  rejected governed or memory proposals.

#### Task 2.4: Add Controlled Recovery For Existing Corrupted Identity

- Status: COMPLETED
- Objective: Provide an auditable path to restore a clean active identity when
  the current canonical identity is already polluted.
- Steps:
  1. Implement documented operator reset plus re-kickstart as the primary
     recovery path for already-corrupted active identity.
  2. Implement any missing management CLI helpers needed to inspect suspicious
     active identity items and recover safely.
  3. Add a controlled edit-proposal follow-up only if inspection during
     implementation proves reset plus re-kickstart is insufficient for bounded
     operator recovery.
  4. Add tests proving the recovery path preserves history and supersession
     state instead of mutating rows in place without audit.
- Validation: Management component or integration tests prove a corrupted active
  identity can be replaced through a documented harness-owned path.
- Notes: Do not silently rewrite existing active identity rows during normal
  foreground execution.

### Milestone 3: Reduce Foreground Context Noise

- Status: COMPLETED
- Purpose: Make the default foreground model input compact and focused on the
  current user request.
- Exit Criteria: Ordinary turns no longer carry the full governed-action schema
  by default, developer-message share is materially reduced, and the final
  assembled prompt respects explicit size priorities.

#### Task 3.1: Replace Always-On Full Governed-Action Schema With Progressive Disclosure

- Status: COMPLETED
- Objective: Stop injecting the full action schema into every ordinary turn.
- Steps:
  1. Define a short default developer capability reminder for routine turns.
  2. Inject the full governed-action schema only when the current trigger or
     current loop state indicates the model is likely to need it.
  3. Preserve special cases where full schema disclosure is required, such as
     immediate action execution, recent malformed action proposals, or
     troubleshooting flows that explicitly require diagnostic query shapes.
- Validation: Worker or foreground component tests prove routine chat turns use
  the short reminder while action-heavy turns still receive the required schema.
- Notes: Keep the short reminder strong enough to prevent the model from
  claiming it has no tools.

#### Task 3.2: Prioritize Prompt Sections Under A Real Input Budget

- Status: COMPLETED
- Objective: Enforce a final prompt-size budget instead of relying on scattered
  char caps.
- Steps:
  1. Add a final assembly phase that estimates size for system prompt, recent
     history, retrieved context, and developer messages before provider
     invocation.
  2. Define a deterministic trimming order that removes lowest-priority content
     first.
  3. Persist trimming metadata so traces show what was excluded and why.
- Validation: Tests prove the final assembled input stays within the configured
  budget and records trimming decisions when content exceeds the budget.
- Notes: The current user trigger and minimal action affordance should be the
  last content trimmed.

#### Task 3.3: Compact Retrieved-Context Rendering

- Status: COMPLETED
- Objective: Stop rendering large verbose retrieved summaries into developer
  messages.
- Steps:
  1. Redesign retrieved-context summaries so they prefer concise factual
     summaries over full latest-message excerpts.
  2. Truncate or omit assistant excerpts when they are long, repetitive, or
     clearly instruction-heavy.
  3. Preserve relevance reason and enough provenance for the model to use the
     context safely.
- Validation: Tests and retained-trace inspection show retrieved developer
  messages are materially shorter while still preserving the intended signal.
- Notes: The retrieved summary should not become a second conversation history
  channel.

#### Task 3.4: Add Prompt-Composition Metadata To Traces And Audits

- Status: COMPLETED
- Objective: Make prompt quality visible in operational tooling.
- Steps:
  1. Extend context-assembly metadata or model-call persistence to include
     counts and sizes for system prompt, user messages, assistant history,
     retrieved context, and developer instructions.
  2. Surface the new fields through trace reporting and focused inspection.
  3. Add diagnosis hints when developer-message share or retrieved-context share
     crosses the acceptance thresholds.
- Validation: `admin trace show --json` exposes the new prompt-composition
  fields for a foreground trace.
- Notes: This work should help future debugging without requiring manual JSON
  parsing.

### Milestone 4: Clean Persistence And Retrieval Feedback Loops

- Status: COMPLETED
- Purpose: Prevent low-quality assistant and failure text from recycling back
  into future prompts.
- Exit Criteria: Persisted assistant messages are normalized, retrieval artifacts
  exclude low-value content by default, and ordinary retrieval no longer returns
  noisy operational failures as user-facing context.

#### Task 4.1: Normalize Assistant Text Before Episode Persistence

- Status: COMPLETED
- Objective: Prevent recursive chat-prefix duplication and obvious meta-format
  bleed from stored assistant messages.
- Steps:
  1. Add a normalization step before assistant episode messages are persisted.
  2. Strip leading synthetic `[timestamp] Assistant:` or equivalent repeated
     chat labels when they were generated by the model rather than the harness.
  3. Preserve legitimate user-facing content and fenced control blocks only as
     required by existing worker parsing rules.
- Validation: Tests prove repeated assistant-prefix contamination is removed and
  clean replies remain unchanged.
- Notes: Apply the same normalization rules to standard foreground replies and
  approval follow-up persistence paths.

#### Task 4.2: Exclude Failure Boilerplate From Ordinary Retrieval Candidates

- Status: COMPLETED
- Objective: Prevent routine retrieval from surfacing worker failures, approval
  boilerplate, or internal-action warnings as canonical conversation context.
- Steps:
  1. Define retrieval-exclusion rules for foreground failure notices, approval
     placeholders, and instruction-dominant assistant messages.
  2. Apply those rules when generating or refreshing episode retrieval
     artifacts.
  3. Preserve explicit operator troubleshooting access through diagnostics and
     trace tooling instead of foreground retrieval.
- Validation: Component tests prove excluded episode classes do not appear in
  normal foreground retrieved context.
- Notes: Failures should remain durable records; they should simply stop being
  default conversational evidence.

#### Task 4.3: Tighten Retrieval Ranking For Same-Conversation Small Talk

- Status: COMPLETED
- Objective: Stop lexical-match fallback from retrieving stale or irrelevant
  history for low-information triggers like `hello again`.
- Steps:
  1. Review the current lexical and semantic scoring logic for sparse triggers.
  2. Add conservative guards so low-information greetings prefer recent local
     history over old lexical matches from distant episodes.
  3. Rebalance recency and lexical scoring where necessary.
- Validation: Retrieval tests prove trivial greetings do not pull distant
  irrelevant episodes ahead of recent local context.
- Notes: This should improve ordinary chat quality without weakening retrieval
  for specific factual recalls.

#### Task 4.4: Add Retrieval-Hygiene Regression Tests

- Status: COMPLETED
- Objective: Lock in the expected retrieval posture after cleanup.
- Steps:
  1. Add tests covering noisy failure episodes, approval boilerplate, old
     irrelevant lexical matches, and clean preference memory retrieval.
  2. Use deterministic fixtures or disposable PostgreSQL-backed component tests
     as appropriate.
  3. Assert both selected items and rendered summaries.
- Validation: Relevant retrieval component tests pass and explicitly assert
  exclusion of known noisy content classes.
- Notes: Cover both candidate selection and worker-facing rendering.

### Milestone 5: Update Docs, Cleanup, And Final Verification

- Status: COMPLETED
- Purpose: Align internal documentation and validation surfaces with the new
  compact-context posture.
- Exit Criteria: Docs reflect the live behavior, temporary investigation
  artifacts are removed or intentionally kept, and the targeted verification
  suite passes or blockers are recorded precisely.

#### Task 5.1: Update Internal Context-Assembly Documentation

- Status: COMPLETED
- Objective: Bring implementation-reference docs in line with the remediated
  context pipeline.
- Steps:
  1. Update `docs/internal/conscious_loop/CONTEXT_ASSEMBLY.md` with the new
     progressive-disclosure behavior, budget enforcement, retrieval hygiene, and
     assistant-text normalization rules.
  2. Update `docs/internal/conscious_loop/GOVERNED_ACTIONS.md` if the governed
     action disclosure posture or schema injection conditions change.
  3. Update any affected line references and re-stamp the verified date.
  4. Correct any wording that still implies the old always-on schema posture.
- Validation: Manual source-reference review confirms all referenced file paths
  and line numbers resolve after implementation.
- Notes: A stale internal source reference is a defect under repository rules.

#### Task 5.2: Update Related Operator Documentation

- Status: COMPLETED
- Objective: Document the new troubleshooting and identity-recovery posture for
  operators.
- Steps:
  1. Update relevant management or trace docs if new prompt metrics or identity
     diagnostics become visible through the CLI.
  2. Document the supported operator workflow for recovering from corrupted
     identity state.
  3. Keep user-facing docs free of hidden-maintenance implementation details.
- Validation: Manual rendered Markdown review confirms terminology matches the
  canonical architecture docs.
- Notes: Keep the docs repository-oriented rather than session-specific.

#### Task 5.3: Cleanup Intermediate Investigation Artifacts

- Status: COMPLETED
- Objective: Remove temporary artifacts that should not ship after remediation.
- Steps:
  1. Review the worktree for temporary analysis notes, ad hoc scripts, scratch
     outputs, or one-off debugging files created during the remediation.
  2. Remove only artifacts that are not part of the intended repository state.
  3. Keep durable tests, docs, and fixtures that add long-term maintainability.
- Validation: `cmd.exe /c git status --short` and `cmd.exe /c git diff --name-only`
  show only intended final repository changes.
- Notes: Preserve unrelated worktree changes.

#### Task 5.4: Final Verification

- Status: COMPLETED
- Objective: Validate the integrated remediation after cleanup.
- Steps:
  1. Run formatting and compilation checks.
  2. Run targeted tests for identity validation, foreground component behavior,
     worker prompt assembly, retrieval selection, and management surfaces.
  3. Run broader workspace validation if the targeted checks pass.
  4. Re-sample retained traces or fresh local traces to confirm prompt-composition
     metrics improved against the baseline.
- Validation:
  - `cargo fmt --all --check`
  - `cargo check --workspace`
  - `cargo test -p harness --lib -- --nocapture`
  - `cargo test -p harness --test foreground_component -- --nocapture`
  - `cargo test -p harness --test foreground_integration -- --nocapture`
  - `cargo test -p harness --test continuity_component -- --nocapture`
  - `cargo test -p harness --test management_component -- --nocapture`
  - `cargo test -p workers --bin workers -- --nocapture`
  - `cargo test -p runtime --test admin_cli -- --nocapture`
  - `cargo test --workspace --lib -- --nocapture`
  - Manual comparison of pre-change and post-change trace prompt metrics for at
    least one representative ordinary chat turn and one governed-action turn
- Notes: If any broader suite is skipped for time or environment reasons, record
  the exact reason in the final execution update.

## Approval Gate

Do not begin implementation until the user approves this plan. After approval,
set Plan Status to `APPROVED`, then to `IN PROGRESS` when implementation starts.
Update task and milestone status in this file before and after each validated
change so the plan remains the execution ledger.

## Plan Self-Check

- Result: PASS on 2026-05-08 after reviewing the plan against the repository
  manual-planning rules, current repository structure, live CLI surfaces, and
  the retained-trace investigation.
- [x] Plan location follows the default location rule.
- [x] Plan status was `READY FOR APPROVAL` at self-check time.
- [x] Goal, scope, non-goals, assumptions, and open questions are explicit.
- [x] There are no unresolved questions that must be surfaced before approval.
- [x] The plan has more than 10 tasks and is grouped into milestones.
- [x] Every milestone has purpose and exit criteria.
- [x] Every task has concrete steps, validation, and notes.
- [x] Cleanup and final verification are included.
- [x] The plan avoids vague implementation language without concrete targets.
- [x] The plan can be executed by a coding agent without re-reading the original
  conversation.
- [x] Validation commands reference real workspace crates, tests, and admin CLI
  surfaces present in the repository.
- [x] The plan distinguishes context-related defects from unrelated recent
  worker-launch, approval-state, and provider-shape failures.
- [x] The primary remediation path for corrupted active identity is explicit.

## Execution Notes

- Update milestone and task status before starting and after validation.
- Keep trace ids, diagnostic ids, and exact validation failures in the plan or
  implementation notes when useful for later auditability.
- Prefer Windows Git output as authoritative if WSL and Windows Git disagree on
  worktree status.
- 2026-05-08 implementation update:
  Completed `cargo fmt --all`, `cargo check --workspace`, `cargo test -p harness identity::tests --lib -- --nocapture`, `cargo test -p harness retrieval::tests --lib -- --nocapture`, `cargo test -p harness foreground_orchestration --lib -- --nocapture`, and `cargo test -p workers --bin workers -- --nocapture`.
  Implemented conservative custom-identity answer validation with rejection diagnostics, stricter ambiguous-answer inference for worker and harness fallback paths, short-vs-full governed-action disclosure in foreground worker prompt assembly, assistant-history prefix normalization before persistence and replay, retrieval artifact hygiene for failed/noisy episodes, and matching internal documentation updates.
- 2026-05-08 prompt-budget update:
  Completed `cargo test -p contracts --lib -- --nocapture`, `cargo test -p workers --bin workers -- --nocapture`, and `cargo check --workspace` after adding a final foreground input-budget trim pass plus retained `prompt_metrics` on `ModelCallRequest`.
  The worker now estimates post-assembly input size, drops low-priority context in deterministic order when needed, and records post-trim character totals plus trim events directly on the retained request payload.
- 2026-05-08 final remediation update:
  Completed compact retrieved-context rendering, sparse small-talk retrieval ranking guards, explicit identity-diagnostic kind classification for suspicious stable-field interview answers, and operator-doc updates in `docs/USER_MANUAL.md` plus `docs/internal/harness/TRACE_EXPLORER.md`.
  Final verification passed with `cargo fmt --all`, `cargo fmt --all --check`, `cargo check --workspace`, `cargo test -p harness --lib -- --nocapture`, `cargo test -p harness --test foreground_component -- --nocapture`, `cargo test -p harness --test foreground_integration -- --nocapture`, `cargo test -p harness --test continuity_component -- --nocapture`, `cargo test -p harness --test management_component -- --nocapture`, `cargo test -p workers --bin workers -- --nocapture`, `cargo test -p runtime --test admin_cli -- --nocapture`, and `cargo test --workspace --lib -- --nocapture`.
  The final self-check also corrected stale test fixtures for `ModelCallRequest.prompt_metrics` and `ResolvedForegroundModelRouteConfig.reasoning_mode/provider_reasoning`, updated internal source-line references, and confirmed the worktree contains only intended repository changes.
