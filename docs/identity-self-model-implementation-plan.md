# Identity and Self-Model Implementation Plan

## Metadata

- Plan Status: COMPLETED
- Created: 2026-04-30
- Last Updated: 2026-05-01
- Owner: Coding agent
- Approval: APPROVED

## Status Legend

- Plan Status values: DRAFT, QUESTIONS PENDING, READY FOR APPROVAL, APPROVED, IN PROGRESS, COMPLETED, BLOCKED
- Task/Milestone Status values: TO BE DONE, IN PROGRESS, COMPLETED, BLOCKED, SKIPPED

## Goal

Implement the missing identity and self-model capabilities identified in
`docs/IDENTITY_SELF_MODEL_IMPLEMENTATION_GAP_REPORT.md`: a rich typed identity
model, one-time first identity kickstart, structured identity evolution,
drift-resistant merge rules, action-relevant identity and internal state, user
and operator identity workflows, and verification proving the behavior is not
prompt-only.

## Current Status

The repository currently has a working narrow self-model baseline: bootstrap seed
loading, canonical `self_model_artifacts`, compact prompt injection, foreground
episode persistence, memory proposals, background reflection stubs, retrieval
artifacts, merge decisions, governed actions, wake signals, and management CLI
surfaces. The identity model is still flat, prompt-oriented, and too small to
support the richer evolving identity described in the original requirements.

This plan is implemented and final verification is complete.

## Scope

- Extend the self-model from a flat prompt snapshot into a typed canonical
  identity model with stable and evolving identity items.
- Add lifecycle state for bootstrap-only, kickstart-in-progress, complete
  identity active, and reset behavior.
- Add a conscious, interactive, one-time identity kickstart flow with three
  predefined identities and a custom interview path.
- Keep identity machinery hidden from the conscious loop; expose only
  user-meaningful identity formation and identity use.
- Replace coarse self-model observations with structured identity deltas.
- Add identity merge validation, evidence requirements, duplicate handling,
  contradiction handling, protected-core rules, and drift diagnostics.
- Upgrade background reflection to produce structured identity deltas and compact
  self-description updates.
- Make identity and internal state available to policy, scheduling, wake-signal,
  and explanation decisions where appropriate.
- Add management CLI inspection, reset, and controlled identity edit workflows.
- Update canonical and internal documentation affected by the implementation.
- Add automated tests at unit, component, integration, and use-case layers.

## Non-Goals

- No multi-user or multi-tenant identity model.
- No additional production conversation channel beyond the existing Telegram
  posture.
- No browser-based admin UI.
- No unconstrained persona editing or direct canonical identity writes by the
  model.
- No requirement to choose final product copy for the three predefined
  identities in this plan; implementation may use repository-approved placeholder
  templates if product copy is not separately supplied.
- No raw SQL operator workflow as the primary identity management path.

## Assumptions

- The richer identity profile is intended to become part of the current product
  direction rather than staying only in archived source material.
- Existing harness sovereignty remains unchanged: workers propose, the harness
  validates and commits.
- Existing migrations are reviewed SQL files; new schema work should use the next
  available monotonic migration number after the current latest migration. At
  plan creation time, the latest observed migration is
  `0012__causal_links.sql`.
- The existing `SelfModelSnapshot` may remain the compact worker-facing view, but
  it should be derived from richer canonical identity state.
- The identity kickstart is a conscious foreground workflow because it is a
  user-facing relationship event.
- The conscious loop may know it does not yet have a complete identity, but must
  not see schema, table, merge, or lifecycle implementation details.

## Open Questions

None. Product copy for the three predefined identities can be filled with
reviewable placeholders during implementation and revised later without changing
the architecture.

## Milestones

### Milestone 1: Canonical Identity Design And Documentation

- Status: COMPLETED
- Purpose: Make the richer identity requirements explicit enough to implement
  without relying on archived source material or this conversation.
- Exit Criteria: Canonical docs, gap report references, and implementation plan
  agree on the identity model, lifecycle states, proposal flow, hidden machinery
  boundary, and validation posture.

#### Task 1.1: Promote Identity Requirements Into Canonical Docs

- Status: COMPLETED
- Objective: Update canonical requirements and design docs to state the richer
  identity model and first-identity kickstart behavior.
- Steps:
  1. Update `docs/REQUIREMENTS.md` with explicit stable identity, evolving
     identity, lifecycle, kickstart, reset, and action-relevance requirements.
  2. Update `docs/IMPLEMENTATION_DESIGN.md` with the implementation posture:
     typed identity items, compact snapshot projection, conscious kickstart,
     structured deltas, and harness-owned merge.
  3. Update `docs/LOOP_ARCHITECTURE.md` if conscious trigger/tool availability
     or hidden machinery boundaries need clarification.
- Validation: Manual diff review confirms canonical docs do not contradict
  `docs/IDENTITY_SELF_MODEL_IMPLEMENTATION_GAP_REPORT.md`.
- Notes: Preserve normative terms in `docs/REQUIREMENTS.md`.

#### Task 1.2: Define Identity Domain Model

- Status: COMPLETED
- Objective: Specify the typed identity data model before changing schema or
  contracts.
- Steps:
  1. Add or update a design section defining stable identity fields: name,
     species or identity form, role, archetype, origin/backstory, age framing,
     foundational traits, foundational values, enduring boundaries, and default
     communication style.
  2. Define evolving identity fields: preferences, likes, dislikes, habits,
     routines, learned tendencies, autobiographical refinements, recurring
     self-descriptions, interaction-style adaptations, goals, and subgoals.
  3. Define item metadata: stability class, category, confidence, weight,
     provenance, evidence references, valid-from, valid-to, supersession,
     source, and merge policy.
  4. Define the compact worker-facing projection and what must never be exposed
     to the conscious loop.
- Validation: Design text lists every identity field and metadata dimension
  needed by later milestones.
- Notes: Keep the compact projection bounded; do not make the worker context a
  dump of all identity rows.

#### Task 1.3: Define Identity Lifecycle And Kickstart Contract

- Status: COMPLETED
- Objective: Specify the state machine and user flow for first identity
  formation.
- Steps:
  1. Define lifecycle states: `bootstrap_seed_only`,
     `identity_kickstart_in_progress`, `complete_identity_active`, and any reset
     transitional state needed.
  2. Define when the identity kickstart tool is visible to the conscious loop.
  3. Define when the tool must disappear.
  4. Define the predefined-template path and custom interview path.
  5. Define completion, interruption, resume, cancellation, and admin reset
     semantics.
- Validation: Lifecycle spec includes allowed transitions, blocked transitions,
  and audit requirements.
- Notes: From the conscious loop's perspective, bootstrap-only means the agent
  does not yet know who it is and can form an identity with the user.

### Milestone 2: Schema, Contracts, And Seed Baseline

- Status: COMPLETED
- Purpose: Add durable storage and cross-process types that can represent rich
  identity without breaking existing self-model behavior.
- Exit Criteria: New migrations, contracts, seed parsing, repository APIs, and
  migration tests support rich identity while preserving compatibility with
  existing self-model artifacts.

#### Task 2.1: Add Identity Persistence Migration

- Status: COMPLETED
- Objective: Create reviewed schema for identity lifecycle, identity items,
  identity templates, interview state, and identity diagnostics.
- Steps:
  1. Add the next numbered migration under `migrations/`.
  2. Add tables or columns for identity lifecycle state and kickstart completion.
  3. Add normalized identity item storage with category, stability class, value,
     weight, confidence, provenance, evidence refs, validity, status, and
     supersession fields.
  4. Add durable identity kickstart interview state.
  5. Add identity drift or contradiction diagnostic storage if existing
     diagnostic tables are insufficient.
  6. Add indexes for active identity lookup, lifecycle lookup, category lookup,
     and supersession lookup.
- Validation: `cargo test -p harness --test migration_component -- --nocapture`.
- Notes: Avoid storing the canonical model only in `payload_json`; use JSONB only
  for awkward or extension fields.

#### Task 2.2: Extend Contracts For Identity Snapshots And Deltas

- Status: COMPLETED
- Objective: Add shared types for identity snapshots, lifecycle state, kickstart
  actions, templates, interview answers, and identity deltas.
- Steps:
  1. Update `crates/contracts/src/lib.rs` with identity item categories,
     stability classes, lifecycle states, delta operations, and compact snapshot
     types.
  2. Add typed payloads for identity delta proposals.
  3. Add any conscious-context field needed to expose identity kickstart
     availability without exposing implementation internals.
  4. Preserve serialization compatibility for existing worker requests where
     possible.
- Validation: `cargo test -p contracts --lib`.
- Notes: Keep enum names domain-oriented, not phase-oriented.

#### Task 2.3: Upgrade Self-Model Seed Format

- Status: COMPLETED
- Objective: Support a richer seed document while preserving current bootstrap
  behavior during migration.
- Steps:
  1. Extend `config/self_model_seed.toml` or add a versioned seed format with
     stable identity, evolving identity, constraints, capabilities, internal
     defaults, and goals.
  2. Update `crates/harness/src/self_model.rs` seed parsing and validation.
  3. Add compatibility parsing for the existing flat seed shape if required.
  4. Add strict validation errors for incomplete rich identity templates and
     invalid lifecycle configuration.
- Validation: `cargo test -p harness --lib self_model -- --nocapture`.
- Notes: The seed remains a minimal bootstrap source, not the complete identity
  once kickstart has completed.

#### Task 2.4: Add Identity Repository APIs

- Status: COMPLETED
- Objective: Add harness-owned database access functions for identity lifecycle,
  items, templates, interview state, and diagnostics.
- Steps:
  1. Extend `crates/harness/src/continuity.rs` or add a focused harness module
     such as `identity.rs` if that better matches local patterns.
  2. Add functions to load active identity state and reconstruct the compact
     snapshot.
  3. Add functions to insert identity items, supersede items, record lifecycle
     transitions, persist interview state, and list diagnostics.
  4. Add unit or component tests against disposable PostgreSQL.
- Validation: `cargo test -p harness --test continuity_component -- --nocapture`.
- Notes: Keep transaction ownership in harness services, not low-level helpers.

### Milestone 3: Identity Lifecycle And Kickstart Flow

- Status: COMPLETED
- Purpose: Implement the first complete identity formation path before normal
  identity evolution runs.
- Exit Criteria: A seed-only system exposes a one-time identity kickstart to the
  conscious loop; the user can complete predefined or custom identity formation;
  completion hides the tool; admin reset reopens it.

#### Task 3.1: Implement Lifecycle Detection In Context Assembly

- Status: COMPLETED
- Objective: Make foreground context know whether identity kickstart is
  available without revealing storage internals.
- Steps:
  1. Load identity lifecycle state during `assemble_foreground_context()`.
  2. Add compact context fields indicating identity state and allowed identity
     formation capability.
  3. Ensure complete-identity state injects the compact identity snapshot.
  4. Ensure bootstrap-only state frames the assistant as not yet having a
     complete identity.
- Validation: `cargo test -p harness --test foreground_component -- --nocapture`.
- Notes: Update `docs/internal/conscious_loop/CONTEXT_ASSEMBLY.md` in a later
  documentation task after code settles.

#### Task 3.2: Add Kickstart Tool Schema To Conscious Prompt Conditionally

- Status: COMPLETED
- Objective: Expose identity formation as a governed conscious capability only
  while lifecycle state allows it.
- Steps:
  1. Extend the governed action schema or add a harness-native proposal kind for
     identity kickstart actions.
  2. Include identity kickstart instructions only in bootstrap-only or
     kickstart-in-progress state.
  3. Ensure the prompt avoids implementation terms such as table, schema, merge
     internals, and lifecycle internals.
  4. Add tests proving the tool appears before completion and disappears after
     completion.
- Validation: `cargo test -p workers --bin workers -- --nocapture`.
- Notes: The tool should be framed as identity formation with the user.

#### Task 3.3: Implement Predefined Identity Selection

- Status: COMPLETED
- Objective: Let the user choose one of three predefined complete identities and
  commit it through harness validation.
- Steps:
  1. Add three predefined identity templates in a reviewable config or code
     location.
  2. Add validation ensuring each template satisfies the rich identity model.
  3. Add harness handling for selecting a template and converting it to identity
     items plus compact self-description.
  4. Persist proposal, merge decision, identity items, lifecycle transition, and
     audit events.
- Validation: `cargo test -p harness --test foreground_integration -- --nocapture`.
- Notes: Template content may be placeholder product copy, but all structural
  fields must be complete.

#### Task 3.4: Implement Custom Interview State Machine

- Status: COMPLETED
- Objective: Let the user build a custom first identity through a durable
  directed conversation.
- Steps:
  1. Define interview steps for name, species or identity form, archetype/role,
     temperament, communication style, backstory, age framing, likes, dislikes,
     values, boundaries, tendencies, goals, and relationship to the user.
  2. Persist each answer as interview state.
  3. Resume interrupted interviews from the next missing step.
  4. Generate a complete identity proposal when the interview is finished.
  5. Validate and commit through the same path as predefined templates.
- Validation: `cargo test -p harness --test foreground_integration -- --nocapture`.
- Notes: The model can conduct the interview, but the harness owns required
  fields, state, completion, and commit.

#### Task 3.5: Add Admin Identity Reset

- Status: COMPLETED
- Objective: Add a management CLI path to reset identity back to bootstrap-only
  state so kickstart can run again.
- Steps:
  1. Extend `crates/harness/src/management.rs` with an identity reset service.
  2. Extend `crates/runtime/src/admin.rs` with `admin identity reset` or an
     equivalent capability-oriented command.
  3. Require explicit confirmation or a force flag.
  4. Supersede or archive active identity state without deleting audit history.
  5. Record audit events and lifecycle transition.
- Validation: `cargo test -p runtime --test admin_cli -- --nocapture` and
  `cargo test -p harness --test management_component -- --nocapture`.
- Notes: Reset is operator-facing and should not be available as a normal
  conscious-loop action.

#### Task 3.6: Add Kickstart Use-Case Tests

- Status: COMPLETED
- Objective: Prove the full first identity lifecycle works end to end.
- Steps:
  1. Add or extend tests in `crates/harness/tests/use_case_scenarios.rs`.
  2. Cover seed-only tool exposure, predefined selection, custom interview
     completion, completed-state tool hiding, interrupted interview resume, and
     admin reset reopening the flow.
  3. Assert canonical identity artifacts/items and lifecycle state after each
     path.
- Validation: `cargo test -p harness --test use_case_scenarios -- --nocapture`.
- Notes: Prefer deterministic fake model outputs.

### Milestone 4: Structured Identity Proposals And Merge Rules

- Status: COMPLETED
- Purpose: Replace free-text self-model observations with validated, auditable
  identity deltas.
- Exit Criteria: Identity deltas support add, reinforce, weaken, revise,
  supersede, and expire operations with evidence, conflict handling, and
  protected-core rules.

#### Task 4.1: Implement Identity Delta Proposal Payloads

- Status: COMPLETED
- Objective: Add typed proposal payloads for identity item changes and compact
  self-description changes.
- Steps:
  1. Extend `CanonicalProposalPayload` with identity delta variants.
  2. Add serialization and deserialization tests.
  3. Update proposal recording to persist identity delta payloads and content
     summaries.
  4. Keep existing `SelfModelObservation` as a compatibility shim if needed.
- Validation: `cargo test -p contracts --lib` and `cargo test -p harness --lib proposal -- --nocapture`.
- Notes: Avoid breaking existing memory and self-model proposal tests.

#### Task 4.2: Implement Identity Proposal Validation

- Status: COMPLETED
- Objective: Validate identity deltas before any canonical identity mutation.
- Steps:
  1. Add validation for required category, operation, value, confidence,
     stability class, and evidence references.
  2. Require stronger validation for stable identity changes.
  3. Reject unsupported categories and operations.
  4. Reject inferred stable identity changes unless an explicit policy allows
     them.
- Validation: `cargo test -p harness --lib proposal -- --nocapture`.
- Notes: The validator should produce clear rejection reasons for audit and CLI.

#### Task 4.3: Implement Identity Merge Engine

- Status: COMPLETED
- Objective: Apply accepted identity deltas to canonical identity items with
  supersession and audit history.
- Steps:
  1. Add merge functions in `self_model.rs` or a dedicated identity module.
  2. Implement add, reinforce, weaken, revise, supersede, and expire.
  3. Update merge decisions with accepted identity targets.
  4. Ensure active compact snapshot reconstruction reflects merged state.
- Validation: `cargo test -p harness --test continuity_component -- --nocapture`.
- Notes: All writes must remain harness-owned and transactional.

#### Task 4.4: Add Duplicate, Contradiction, And Drift Controls

- Status: COMPLETED
- Objective: Prevent noisy identity growth and protect stable identity.
- Steps:
  1. Add exact duplicate checks.
  2. Add conservative semantic near-duplicate checks using existing lexical or
     semantic token helpers where appropriate.
  3. Add category-specific contradiction checks.
  4. Add protected-core drift checks.
  5. Route uncertain cases to diagnostics or user confirmation instead of
     accepting silently.
- Validation: `cargo test -p harness --test continuity_component -- --nocapture`.
- Notes: Start conservative; false negatives are safer than unsafe automatic
  rewrites of stable identity.

### Milestone 5: Reflection, Memory, And Autobiographical Evolution

- Status: COMPLETED
- Purpose: Make identity evolve from episodes, memory, and reflection through
  structured proposals instead of appended free text.
- Exit Criteria: Background reflection produces typed identity deltas,
  self-description updates, no-change outcomes, diagnostics, and optional wake
  signals when user guidance is needed.

#### Task 5.1: Add Structured Reflection Output Schema

- Status: COMPLETED
- Objective: Define and use structured model output for self-model reflection.
- Steps:
  1. Add schema contract for reflection outputs: deltas, no-change rationale,
     diagnostics, and wake-signal requests.
  2. Update unconscious model request construction to request structured
     identity output.
  3. Parse and validate model JSON or structured output.
  4. Fall back safely on invalid output with diagnostics.
- Validation: `cargo test -p workers --bin workers -- --nocapture`.
- Notes: Unconscious work must still return structured outputs only.

#### Task 5.2: Scope Reflection With Identity Evidence

- Status: COMPLETED
- Objective: Give reflection jobs the evidence needed to propose useful identity
  changes without exposing too much state.
- Steps:
  1. Update background scoping to include current compact identity, relevant
     identity items, recent episodes, memory artifacts, and internal-state
     trends.
  2. Keep hidden implementation machinery out of worker-facing context.
  3. Add scope metadata for diagnostics and traceability.
- Validation: `cargo test -p harness --test unconscious_component -- --nocapture`.
- Notes: Scope must remain bounded and job-specific.

#### Task 5.3: Apply Reflection Identity Deltas

- Status: COMPLETED
- Objective: Route reflection deltas through the identity proposal and merge
  path.
- Steps:
  1. Convert parsed reflection outputs into `CanonicalProposal` identity deltas.
  2. Apply proposal validation and merge.
  3. Persist diagnostics for rejected or deferred reflection outputs.
  4. Emit wake signals when reflection needs user guidance.
- Validation: `cargo test -p harness --test unconscious_integration -- --nocapture`.
- Notes: Reflection must support "no change" without creating noisy proposals.

#### Task 5.4: Add Autobiographical Self-Description Generation

- Status: COMPLETED
- Objective: Maintain a compact self-description grounded in accepted identity
  and autobiographical evidence.
- Steps:
  1. Add self-description item or artifact representation.
  2. Generate or update it from accepted identity items and reflection outputs.
  3. Include it in the compact conscious prompt when complete identity is active.
  4. Add supersession and provenance for updates.
- Validation: `cargo test -p harness --test continuity_integration -- --nocapture`.
- Notes: The self-description is a product-facing identity projection, not a raw
  dump of identity internals.

### Milestone 6: Action-Relevant Identity And Live Internal State

- Status: COMPLETED
- Purpose: Make identity and interoception affect behavior through harness
  decisions, not only prompt wording.
- Exit Criteria: Tests prove boundaries, preferences, and internal state can
  alter policy, scheduling, wake-signal, or explanation behavior.

#### Task 6.1: Derive Live Internal State

- Status: COMPLETED
- Objective: Replace mostly static internal-state values with bounded values
  derived from runtime signals.
- Steps:
  1. Identify available operational inputs: diagnostics, worker failures,
     recovery state, pending approvals, wake-signal backlog, channel binding
     health, and budget exhaustion.
  2. Add internal-state derivation logic in the harness.
  3. Preserve defaults when signals are unavailable.
  4. Include reliability, resource pressure, and connection quality in compact
     prompt projection.
- Validation: `cargo test -p harness --lib self_model -- --nocapture` and
  `cargo test -p harness --test foreground_component -- --nocapture`.
- Notes: Keep signals abstract; do not leak schema names or hidden machinery to
  the conscious loop.

#### Task 6.2: Integrate Identity Into Governed Action Policy

- Status: COMPLETED
- Objective: Let identity boundaries and values affect action policy
  deterministically.
- Steps:
  1. Pass relevant compact identity boundaries into policy evaluation.
  2. Add rules that can block, require approval, or add diagnostics based on
     identity boundaries.
  3. Add tests where a boundary changes the outcome of a governed action.
- Validation: `cargo test -p harness --test governed_actions_component -- --nocapture`.
- Notes: Harness policy remains authoritative; model-stated preferences are not
  blindly trusted.

#### Task 6.3: Integrate Identity And Internal State Into Proactive Decisions

- Status: COMPLETED
- Objective: Let preferences, internal state, and boundaries influence
  scheduled foreground and wake-signal behavior.
- Steps:
  1. Update wake-signal evaluation to consider internal state and relevant
     identity preferences or boundaries.
  2. Update scheduled foreground planning or rendering where identity should
     affect tone, urgency, or deferment.
  3. Add tests for non-urgent proactive deferral under degraded internal state.
- Validation: `cargo test -p harness --test unconscious_integration -- --nocapture` and
  `cargo test -p harness --test foreground_integration -- --nocapture`.
- Notes: Keep behavior conservative and auditable.

### Milestone 7: Management CLI And Operator Workflows

- Status: COMPLETED
- Purpose: Provide durable operator inspection and control without raw SQL.
- Exit Criteria: Operators can inspect identity, history, diagnostics, pending
  identity proposals, and reset identity through management CLI commands.

#### Task 7.1: Add Identity Inspection Commands

- Status: COMPLETED
- Objective: Expose active identity and lifecycle state through the management
  CLI.
- Steps:
  1. Add management service methods to summarize active identity, lifecycle
     state, and compact self-description.
  2. Add runtime admin commands for identity status and identity show.
  3. Support concise human-readable output and structured output if local CLI
     patterns already support it.
- Validation: `cargo test -p runtime --test admin_cli -- --nocapture` and
  `cargo test -p harness --test management_component -- --nocapture`.
- Notes: Do not expose raw table rows as the primary UX.

#### Task 7.2: Add Identity History And Diagnostics Commands

- Status: COMPLETED
- Objective: Let operators inspect identity evolution and problems.
- Steps:
  1. Add commands to list identity item history and merge decisions.
  2. Add commands to list identity drift, contradiction, and deferred-decision
     diagnostics.
  3. Include proposal ids and audit refs needed for follow-up.
- Validation: `cargo test -p harness --test management_component -- --nocapture`.
- Notes: Keep outputs bounded by default.

#### Task 7.3: Add Controlled Identity Edit Proposal Commands

- Status: COMPLETED
- Objective: Let operators or approved user flows propose identity edits without
  direct writes.
- Steps:
  1. Add management service for creating identity edit proposals.
  2. Add commands to approve, reject, or inspect pending identity edits if not
     covered by existing approvals.
  3. Require stronger confirmation for stable identity changes.
- Validation: `cargo test -p harness --test management_integration -- --nocapture`.
- Notes: This is separate from first identity kickstart; edits happen after a
  complete identity exists.

### Milestone 8: Documentation, Cleanup, And Final Verification

- Status: COMPLETED
- Purpose: Ensure code, docs, and tests agree and the repository contains only
  intentional final artifacts.
- Exit Criteria: Internal docs are updated with source references and verified
  dates, temporary artifacts are removed, and the relevant verification suite
  passes or blockers are recorded.

#### Task 8.1: Update Internal Documentation

- Status: COMPLETED
- Objective: Keep implementation reference docs accurate after behavior changes.
- Steps:
  1. Update `docs/internal/conscious_loop/CONTEXT_ASSEMBLY.md` with new identity
     snapshot fields, lifecycle-aware prompt assembly, and kickstart tool
     exposure rules.
  2. Update `docs/internal/conscious_loop/GOVERNED_ACTIONS.md` if identity
     kickstart or identity edit actions are implemented through governed action
     schema.
  3. Update relevant harness internal docs if identity merge, trace, or
     management surfaces gain new source references.
  4. Re-stamp verified dates.
- Validation: Manual check that referenced source paths and line references
  resolve after implementation.
- Notes: A stale internal source reference is a defect under repository rules.

#### Task 8.2: Update User-Facing Documentation

- Status: COMPLETED
- Objective: Document supported operator and user workflows after implementation.
- Steps:
  1. Update `README.md` or `docs/USER_MANUAL.md` with identity kickstart,
     identity reset, and identity inspection workflows.
  2. Keep the explanation user-oriented and avoid exposing hidden maintenance
     internals as the assistant's experience.
  3. Update `docs/USE_CASE_CATALOG.md` with new identity use cases and status.
- Validation: Manual rendered Markdown review for hierarchy, clarity, and
  consistency with canonical docs.
- Notes: Do not write as a temporary handoff note.

#### Task 8.3: Cleanup Intermediate Artifacts

- Status: COMPLETED
- Objective: Remove artifacts created only to support implementation.
- Steps:
  1. Inspect the worktree for temporary scripts, scratch fixtures, debug logs,
     obsolete plan fragments, and generated data.
  2. Remove only artifacts that are not part of the intended final repository
     state.
  3. Keep maintainable tests, fixtures, docs, migrations, and configs that are
     part of the repository contract.
- Validation: `cmd.exe /c git status --short` and `cmd.exe /c git diff --name-only`
  show only intended final changes.
- Notes: Do not remove user-provided files or unrelated worktree changes.

#### Task 8.4: Final Verification

- Status: COMPLETED
- Objective: Validate the integrated identity implementation after cleanup.
- Steps:
  1. Run formatting and compilation checks.
  2. Run targeted identity, foreground, background, management, migration, and
     governed-action suites.
  3. Run broader workspace validation if targeted checks pass.
  4. Record any blocked checks with the exact failure reason.
- Validation:
  - `cargo fmt --all --check`
  - `cargo check --workspace`
  - `cargo test -p contracts --lib`
  - `cargo test -p harness --test migration_component -- --nocapture`
  - `cargo test -p harness --test continuity_component -- --nocapture`
  - `cargo test -p harness --test continuity_integration -- --nocapture`
  - `cargo test -p harness --test foreground_component -- --nocapture`
  - `cargo test -p harness --test foreground_integration -- --nocapture`
  - `cargo test -p harness --test unconscious_component -- --nocapture`
  - `cargo test -p harness --test unconscious_integration -- --nocapture`
  - `cargo test -p harness --test governed_actions_component -- --nocapture`
  - `cargo test -p harness --test management_component -- --nocapture`
  - `cargo test -p harness --test management_integration -- --nocapture`
  - `cargo test -p harness --test use_case_scenarios -- --nocapture`
  - `cargo test -p runtime --test admin_cli -- --nocapture`
  - `cargo test --workspace --lib -- --nocapture`
- Notes: If runtime is too long for one pass, run the targeted suites first and
  record any skipped broader check explicitly.

## Approval Gate

Implementation must not start until the user approves this plan. After approval,
set Plan Status to `APPROVED`, then to `IN PROGRESS` when implementation begins.
Update each task status before and after work according to the execution ledger
rules.

## Plan Self-Check

- Result: PASS on 2026-04-30 after reviewing the plan against the manual
  planning rules and the current repository structure.
- [x] Plan location follows the default location rule.
- [x] Current status is explicit.
- [x] Scope, non-goals, assumptions, and open questions are explicit.
- [x] Open questions are explicit and there are no unresolved questions to
  surface.
- [x] Tasks are grouped into milestones because the plan has more than 10 tasks.
- [x] Every task has concrete steps and validation.
- [x] Every milestone has exit criteria.
- [x] Cleanup and final verification are included.
- [x] The plan avoids vague actions without concrete targets.
- [x] The plan can be executed by a coding agent without reading the original conversation.
- [x] Validation commands reference existing workspace crates and test surfaces.
- [x] Migration sequencing notes match the latest observed migration file.

## Execution Notes

- Update milestone and task status before starting and after validation.
- Update each task to COMPLETED immediately after its validation passes.
- Mark tasks or milestones BLOCKED with a short reason when progress cannot
  continue.
- Preserve unrelated worktree changes. Windows Git output is authoritative for
  status and diff decisions in this repository.
