# Identity and Self-Model Implementation Gap Report

Date: 2026-04-30
Status: Audit report for implementation planning
Scope: Assistant identity, self-model, internal state, memory continuity, and
evolving personality behavior

## Purpose

This report compares the original identity and self-model expectations for Blue
Lagoon against the current working implementation.

It is intended to be actionable input for a follow-on implementation plan. It
does not replace the canonical requirements in `docs/REQUIREMENTS.md` or the
canonical implementation design in `docs/IMPLEMENTATION_DESIGN.md`.

## Executive Summary

The current implementation includes a real but narrow self-model baseline:

- A bootstrap self-model seed exists in `config/self_model_seed.toml`.
- The seed is loaded into `SelfModelSnapshot`.
- The active self-model is persisted as a canonical `self_model_artifacts` row.
- A compact self-model is injected into the conscious model prompt.
- Background self-model reflection can emit a self-model observation proposal.
- Harness-owned proposal validation and merge history exist for self-model
  updates.

This satisfies the minimum architectural skeleton: self-model existence,
canonical storage, prompt injection, proposal-based updates, and traceable merge
decisions.

It does not yet satisfy the original product expectation for a complex,
evolving assistant identity. The original requirements expected identity to be a
live, character-like, operational structure containing name, species, archetype,
traits, communication style, origin or backstory, age-like framing, likes,
dislikes, behavioral tendencies, values, boundaries, habits, autobiographical
refinements, and drift-resistant evolution through reflection and memory. The
current implementation stores only a small flat snapshot: stable identity, role,
communication style, capabilities, constraints, preferences, current goals, and
current subgoals.

The largest gap is not just missing fields. The implementation lacks typed
identity semantics, weighted or temporal evolving traits, identity-specific merge
rules, user-directed identity editing, rich reflection outputs, action-selection
integration beyond prompt text, and tests proving that identity materially
changes behavior.

There is also a bootstrapping gap between the bare seed and the first complete
identity. The current seed is too thin to evolve meaningfully by itself. The
system needs a one-time conscious, interactive identity kickstart flow that turns
the bootstrap seed into the first complete identity artifact before normal
self-evolution begins.

## Source Baseline

### Original Draft Expectations

`docs/sources/initial-requirements-draft.md` is the most explicit source for
the original character and identity expectation.

It states that Blue Lagoon should maintain an explicit and persistent self-model
representing "who I am right now" in operational terms, not merely stylistic
terms. The self-model must include identity, internal state, active goals,
constraints, preferences, and other control-relevant properties, and must be
passed into foreground reasoning.

The original initial identity profile was expected to support concrete,
character-like attributes:

- Name.
- Species.
- Core role or archetype.
- Personality traits.
- Communication style.
- Origin or backstory.
- Age or age-like framing.
- Likes and dislikes.
- Behavioral tendencies or defaults.
- Initial values, boundaries, or preferences.

The draft also required a split between core identity and evolving identity:

- Core identity: relatively stable bootstrapped attributes such as name,
  species, role, temperament, origin, foundational traits, communication style,
  and foundational backstory.
- Evolving identity: preferences, likes, dislikes, habits, routines, learned
  tendencies, autobiographical refinements, and recurring self-descriptions that
  develop through memory, reflection, and interaction.

The draft was explicit that identity must materially shape planning, action
selection, explanation style, and boundary enforcement through policies and
constraints derived from the self-model. It also required all identity updates to
be mediated by the harness, validated, committed to canonical storage, and
recorded in traceable history.

### Philosophy Expectations

`PHILOSOPHY.md` strengthens the same product goal.

Important expectations:

- Blue Lagoon should feel like a persistent assistant with continuity, nuance,
  memory, time-awareness, personality, and a lived sense of self.
- Personality should evolve through experience; after months of use, the
  assistant should feel meaningfully shaped by that use.
- Identity is part of the control architecture, not a persona paragraph.
- Stable identity includes foundational attributes such as role, core
  temperament, and enduring constraints.
- Evolving identity includes habits, preferences, tendencies, and
  autobiographical refinements.
- Identity updates must pass through harness validation and become part of
  canonical history.
- "No static identity" is a non-negotiable.

### Canonical Requirements

`docs/REQUIREMENTS.md` narrows the original draft into formal requirements.

Relevant requirements:

- Blue Lagoon must maintain an explicit self-model.
- The self-model must be used in planning, prioritization, and explanation.
- A compact form of the self-model must be injected into conscious-loop
  reasoning context.
- The self-model must contain at least stable identity, capabilities, role,
  constraints, preferences, current internal-state snapshot, current goals, and
  current subgoals where applicable.
- The self-model must distinguish relatively stable identity attributes from
  evolving attributes.
- Stable identity should include a persistent identifier, role, foundational
  constraints, and communication style.
- Evolving identity should include preferences, habits, learned tendencies, and
  autobiographical refinements.
- Identity must not be cosmetic only.
- Self-model constraints and preferences must be able to influence planning and
  action selection.
- Reflection should support updated traits, preference weights, and compact
  self-descriptions.
- Reflection outputs must remain proposals until validated by the harness.
- The system must maintain explicit internal state and distinguish internal from
  external state.
- The system should model causal links between actions, world changes, and
  internal-state changes.

### Implementation Design Expectations

`docs/IMPLEMENTATION_DESIGN.md` preserves the same posture:

- Blue Lagoon should be shaped by experience, not behave like a stateless chat
  wrapper.
- Identity is operational, not cosmetic.
- Internal state and interoception are first-class.
- The conscious loop receives a compact self-model snapshot, selected memory,
  and current internal-state snapshot.
- Identity should shape planning, prioritization, explanation, and boundary
  enforcement.
- Evolving identity includes preferences, habits, learned tendencies,
  autobiographical refinements, recurring self-descriptions, and other traits
  that may change through experience and reflection.
- Decisions should be evaluated as a function of internal state and external
  world state.

## Current Implementation

### Data Contract

The current cross-process contract is `SelfModelSnapshot` in
`crates/contracts/src/lib.rs`.

Current fields:

- `stable_identity: String`
- `role: String`
- `communication_style: String`
- `capabilities: Vec<String>`
- `constraints: Vec<String>`
- `preferences: Vec<String>`
- `current_goals: Vec<String>`
- `current_subgoals: Vec<String>`

This contract maps to the canonical minimum in `docs/REQUIREMENTS.md`, but it
does not model the richer original identity profile.

Missing first-class identity fields include:

- Name as a distinct identity field.
- Species.
- Archetype.
- Personality traits.
- Origin or backstory.
- Age or age-like framing.
- Likes.
- Dislikes.
- Behavioral tendencies.
- Values.
- Boundaries.
- Habits.
- Routines.
- Autobiographical refinements.
- Recurring self-descriptions.
- Trait confidence, weight, provenance, or temporal validity.
- Stable-versus-evolving classification per identity item.

### Bootstrap Seed

`config/self_model_seed.toml` contains:

- `stable_identity = "blue-lagoon"`
- `role = "personal_assistant"`
- `communication_style = "direct"`
- Capabilities.
- Constraints.
- Preferences.
- Current goals.
- Current subgoals.

This is a useful bootstrap seed, but it is a functional prompt profile rather
than the rich character identity described in the original requirements. It does
not include species, archetype, traits, backstory, age framing, likes, dislikes,
values, boundaries, tendencies, habits, or autobiographical seed material.

### Canonical Persistence

The current schema in `migrations/0004__canonical_continuity.sql` creates
`self_model_artifacts` with:

- `stable_identity`
- `role`
- `communication_style`
- `capabilities_json`
- `constraints_json`
- `preferences_json`
- `current_goals_json`
- `current_subgoals_json`
- Supersession fields.
- `payload_json`.

This gives the system a canonical active self-model artifact and a supersession
path. It does not normalize identity dimensions or distinguish core identity
from evolving identity at the item level. The only flexible escape hatch is
`payload_json`, but the production loader currently ignores it for active
snapshot construction.

### Loading and Validation

`crates/harness/src/self_model.rs` loads the active canonical self-model artifact
if one exists. If none exists, it loads the seed TOML and inserts a bootstrap
artifact.

Validation currently checks only:

- `stable_identity` is not empty.
- `role` is not empty.
- `communication_style` is not empty.
- `capabilities` is not empty.
- `current_goals` is not empty.

There is no validation for:

- Stable identity completeness.
- Evolving identity completeness.
- Contradictory identity claims.
- Invalid identity categories.
- Duplicate or near-duplicate traits.
- Drift from protected core identity.
- Provenance quality for identity changes.
- Confidence thresholds by identity dimension.
- User-authorized versus model-inferred identity changes.

### Prompt Injection

`crates/workers/src/main.rs` builds the conscious system prompt from the current
self-model.

The current prompt includes:

- Identity handle.
- Role.
- Communication style.
- Behavioral preferences.
- Capabilities.
- Constraints.
- Current goals.
- Current subgoals when present.
- Active internal conditions when present.
- Current time.
- Runtime state fields: load, health, confidence, and execution mode.
- Governed action availability and action schema instructions.

This proves the compact self-model is injected into foreground reasoning.

The prompt does not include:

- Rich character identity.
- Stable/evolving identity distinction.
- Trait strengths or confidence.
- Likes and dislikes.
- Values and boundaries as structured identity dimensions.
- Backstory or origin.
- Age-like framing.
- Habits or learned tendencies.
- Autobiographical refinements.
- Causal agency state.
- Reliability, resource pressure, and connection quality, even though those
  fields exist in `InternalStateSnapshot`.

### Internal State

`InternalStateSnapshot` contains:

- `load_pct`
- `health_pct`
- `reliability_pct`
- `resource_pressure_pct`
- `confidence_pct`
- `connection_quality_pct`
- `active_conditions`

The current conscious prompt uses only load, health, confidence, mode, and active
conditions. It omits reliability, resource pressure, and connection quality.

The current internal state implementation is mostly seeded/static. It does not
yet appear to derive live interoceptive values from operational metrics such as
recent failures, queue pressure, worker stalls, budget exhaustion, provider
latency, approval backlog, or channel health.

### Reflection and Self-Model Evolution

The unconscious worker supports `SelfModelReflection` as a job kind.

Current behavior:

- The unconscious worker sends a plain-text background analysis prompt.
- The model's output text is converted into one `SelfModelObservation` proposal.
- The proposal is classified by keyword into:
  - `subgoal`
  - `interaction_style`
  - `preference`
- Harness proposal validation checks only that observation kind and content text
  are non-empty.
- The merge path appends `interaction_style` and `preference` observations to
  `preferences`.
- The merge path appends `subgoal` observations to `current_subgoals`.
- Unknown observation kinds also become preferences.

This is a minimal working proposal-and-merge path, but it is not a mature
identity evolution system.

Important limitations:

- Reflection output is unstructured plain text, not a typed delta schema.
- One reflection run can produce only a single coarse observation.
- There is no trait weighting.
- There is no distinction between stable identity edits and evolving identity
  edits.
- There is no user-directed identity edit path.
- There is no protected-core-identity rule.
- There is no review of whether a proposed identity update is supported by
  episodes or memory evidence.
- There is no semantic contradiction detection for identity deltas.
- There is no decay, reinforcement, supersession, or temporal validity model for
  evolving traits.
- There is no compact self-description artifact generated from the richer
  identity state.

### Memory and Continuity Support

The implementation has a real continuity baseline:

- Episodes are persisted.
- Memory proposals exist.
- Memory artifacts exist.
- Retrieval artifacts exist.
- Merge decisions exist.
- Retrieved canonical context is added to later conscious turns.
- Background memory consolidation can create memory proposals.
- Retrieval maintenance can create retrieval updates.
- Contradiction and drift scan can emit diagnostics.

For identity, this means the infrastructure needed to support evolving identity
is partly present.

The missing part is the identity-specific interpretation layer: memory artifacts
are not yet converted into typed identity claims, identity deltas, preference
weights, habits, autobiographical refinements, or self-descriptions in a robust
way.

### Action Relevance

The current self-model influences behavior primarily by prompt injection.

That is necessary but insufficient for the original expectation. The
requirements say identity should shape planning, prioritization, action
selection, explanation, and boundary enforcement. Today, the harness does not
appear to use identity dimensions as explicit inputs to:

- Tool policy decisions.
- Approval thresholds.
- Background job prioritization.
- Scheduled proactive behavior.
- Wake-signal policy.
- Recovery policy.
- Explanation generation policy.
- Action ranking or plan selection.

Constraints are included in the prompt, and governed action policy exists, but
the policy engine is not visibly driven by the self-model as structured data.

## Gap Analysis

### Gap 1: Rich Original Identity Profile Is Not Modeled

Expected:

The original requirements expected concrete identity fields such as name,
species, archetype, personality traits, communication style, origin/backstory,
age framing, likes, dislikes, behavioral tendencies, values, boundaries, and
preferences.

Implemented:

Only `stable_identity`, `role`, `communication_style`, `capabilities`,
`constraints`, `preferences`, `current_goals`, and `current_subgoals`.

Impact:

The assistant can present a basic operational profile, but it cannot maintain or
evolve a rich selfhood. Most of the original "assistant identity" concept is
compressed into generic strings.

Needed:

Introduce typed identity structures and persistence for core identity and
evolving identity.

### Gap 2: Stable and Evolving Identity Are Not First-Class

Expected:

The system should distinguish stable identity from evolving identity. Stable
identity should be protected against arbitrary drift. Evolving identity should
change through experience, memory, and reflection.

Implemented:

The schema has one active `self_model_artifacts` row with flat fields. There is
no per-field or per-item stability classification. Supersession is available at
the whole-artifact level only.

Impact:

Every self-model update rewrites a new active artifact, but the system does not
know which parts are core identity, which are learned tendencies, and which are
temporary goals.

Needed:

Represent identity items with category, stability class, provenance, confidence,
validity, and merge policy.

### Gap 3: Reflection Produces Coarse Free-Text Observations

Expected:

Reflection should analyze episodes, goals, and internal-state trends and produce
updated traits, preference weights, and compact self-descriptions.

Implemented:

The unconscious worker converts model text into one proposal and classifies it
with simple keyword checks. Merge appends the content to preferences or subgoals.

Impact:

Reflection can accumulate strings, but it cannot maintain a coherent evolving
identity. Repeated runs may create duplicated, noisy, weakly supported, or
contradictory preferences.

Needed:

Use structured reflection output with typed identity deltas, evidence links,
confidence, stability class, proposed operation, and merge rationale.

### Gap 4: Identity Is Mostly Prompt Decoration

Expected:

Identity must shape planning, prioritization, action selection, explanation, and
boundary enforcement.

Implemented:

Identity fields are included in the system prompt. Harness-side policy and
planning decisions do not appear to consume identity as structured input.

Impact:

Identity may affect model wording, but the harness does not enforce or apply it
as a control-plane input.

Needed:

Make self-model constraints, values, boundaries, preferences, and internal state
available to policy, scheduling, action planning, explanation, and wake-signal
evaluation.

### Gap 5: Internal State Is Present but Not Live Enough

Expected:

Internal state should include body-like variables such as load, health,
reliability, error conditions, resource pressure, confidence, and connection
quality. Decisions should be evaluated as a function of internal and external
state.

Implemented:

The contract includes several internal-state fields, but prompt injection omits
some of them and the values are largely seeded rather than derived from runtime
health.

Impact:

The assistant has a basic static interoceptive surface, but not a live operational
inner state.

Needed:

Derive internal state from operational signals and feed the full compact state
into context and harness decisions.

### Gap 6: Autobiographical Identity Refinement Is Missing

Expected:

Evolving identity should include autobiographical refinements and recurring
self-descriptions shaped by experience.

Implemented:

Episodes and memory artifacts exist, but there is no dedicated flow that turns
autobiographical experience into identity refinements or compact
self-descriptions.

Impact:

The system remembers facts and can retrieve context, but it does not yet become
"more itself" through lived history.

Needed:

Add identity-focused consolidation that derives durable self-reflections,
behavioral tendencies, and self-descriptions from episode history and accepted
memory.

### Gap 7: Identity Drift Protection Is Minimal

Expected:

Identity refinement should preserve continuity and resist arbitrary drift from
summarization, noisy memory, or prompt-induced instability.

Implemented:

Validation rejects empty required fields and duplicate exact preferences, but
does not protect stable identity or detect semantic drift.

Impact:

The active self-model can gradually accumulate noisy preferences or subgoals and
can be reshaped by weak model output.

Needed:

Add drift checks, contradiction checks, core-identity protection, exact and
semantic duplicate detection, and evidence requirements for identity changes.

### Gap 8: User-Directed Identity Editing Is Missing

Expected:

The original draft allowed identity updates initiated by bootstrapping,
user-directed edits, conscious-loop proposals, or unconscious reflection, all
mediated by the harness.

Implemented:

Bootstrap and proposal-based reflection exist. There is no durable user-facing or
operator-facing identity edit workflow.

Impact:

The user cannot intentionally shape the assistant's identity through a controlled
product path.

Needed:

Add a governed identity edit path with proposal preview, validation, audit, and
optional approval depending on stability class.

### Gap 9: First Complete Identity Kickstart Is Missing

Expected:

The bare bootstrap seed should not be treated as a meaningful long-term identity.
It should be a minimal starting state from which the assistant can initiate a
one-time identity formation process with the user. From the conscious loop's
perspective, bootstrap-only mode should mean "I do not yet know who I am" and a
special identity kickstart tool is available.

Implemented:

The bootstrap seed is immediately loaded as the active canonical self-model
artifact. There is no separate lifecycle state for "bootstrap-only", no one-time
identity formation task, no predefined identity choices, no directed interview
flow, and no admin reset workflow that returns the agent to the seed state.

Impact:

The assistant begins with a bare operational profile and is expected to evolve
from material that is too thin to support meaningful identity growth. The system
also lacks a safe user-facing moment where the first complete identity is
intentionally chosen or created.

Needed:

Add an identity lifecycle state machine and a one-time conscious identity
kickstart flow. The flow should allow the user to either choose one of three
predefined initial identities or create a custom identity through a directed
interview. Once successfully completed, the kickstart tool must disappear from
the conscious loop and must not be available again unless an admin identity reset
returns the system to bootstrap-only state.

### Gap 10: Tests Prove the Skeleton, Not the Product Behavior

Expected:

Acceptance should prove that identity is operationally relevant and not
cosmetic.

Implemented:

Tests cover loading seed values, prompt construction, self-model artifact
persistence, reflection proposal emission, and merge mechanics. The use-case
catalog already marks basic personality consistency and self-model prompt
assertions as partial.

Impact:

The current test suite can prove self-model plumbing. It does not prove complex
identity, evolving personality, drift resistance, or action relevance.

Needed:

Add scenario, component, and integration tests that fail if identity is ignored
by context assembly, planning, policy, reflection, or behavior.

## Recommended Completion Workstreams

### Workstream 1: Define the Identity Domain Model

Goal:

Turn the original character-like identity expectation into a typed domain model.

Recommended deliverables:

- Add a canonical identity model document under `docs/`, or extend
  `docs/REQUIREMENTS.md` and `docs/IMPLEMENTATION_DESIGN.md` if the team wants
  the richer identity fields to be canonical v1 requirements.
- Define stable identity fields:
  - name
  - species
  - role
  - archetype
  - origin/backstory
  - age framing
  - foundational traits
  - foundational values
  - enduring boundaries
  - default communication style
- Define evolving identity fields:
  - preferences
  - likes
  - dislikes
  - habits
  - routines
  - learned tendencies
  - autobiographical refinements
  - recurring self-descriptions
  - interaction-style adaptations
  - current goals and subgoals
- Define identity item metadata:
  - stability class
  - confidence
  - weight
  - provenance
  - evidence references
  - valid-from and valid-to
  - supersession links
  - user-authored versus inferred source
  - merge policy

Implementation notes:

- Keep `SelfModelSnapshot` compact for worker context, but do not make it the
  full canonical identity model.
- Consider adding a richer `IdentitySnapshot` embedded in or referenced by
  `SelfModelSnapshot`.
- Keep the conscious prompt compact by generating a summarized identity view
  from canonical identity items.

### Workstream 2: Extend Persistence

Goal:

Persist identity in a way that supports stable core identity, evolving identity,
provenance, and drift control.

Recommended deliverables:

- Add reviewed migrations for identity item storage, or extend
  `self_model_artifacts` if the team intentionally prefers artifact snapshots.
- Add an identity item table or equivalent normalized structure with:
  - item id
  - self-model artifact id or identity snapshot id
  - category
  - stability class
  - key
  - value
  - weight
  - confidence
  - status
  - provenance kind
  - evidence refs
  - valid-from and valid-to
  - supersedes and superseded-by
  - created-at and updated-at
- Preserve active snapshot reconstruction.
- Preserve whole-artifact audit history.
- Add repository functions for querying active identity by category and stability
  class.

Implementation notes:

- Avoid hiding the new model only in `payload_json`; it would make validation and
  queries weak.
- Keep backward compatibility with existing seed and artifact fields through an
  expand-contract migration.

### Workstream 3: Upgrade the Seed Format

Goal:

Make the bootstrap identity actually express the initial assistant character.

Recommended deliverables:

- Replace or extend `config/self_model_seed.toml` with typed sections:
  - `[stable_identity]`
  - `[evolving_identity]`
  - `[capabilities]` or equivalent arrays
  - `[constraints]`
  - `[internal_state_defaults]`
  - `[goals]`
- Include the original character-like identity fields where product-approved.
- Add strict seed validation.
- Add migration/bootstrap logic from old seed shape to new seed shape if needed.

Acceptance criteria:

- A fresh database bootstraps a rich active self-model.
- Invalid seed documents fail startup with clear diagnostics.
- The compact conscious prompt includes the approved summary, not raw unbounded
  seed data.

### Workstream 4: Structured Identity Delta Proposals

Goal:

Replace coarse `SelfModelObservation` free text with typed identity deltas.

Recommended deliverables:

- Add proposal payloads such as:
  - `IdentityItemDelta`
  - `SelfDescriptionDelta`
  - `GoalDelta`
  - `PreferenceWeightDelta`
  - `BoundaryDelta`
- Include operation types:
  - add
  - reinforce
  - weaken
  - revise
  - supersede
  - expire
- Require evidence references for inferred changes.
- Require stricter validation for stable identity changes than evolving identity
  changes.
- Keep existing `SelfModelObservation` only as a compatibility shim, or remove it
  after migration.

Acceptance criteria:

- Reflection can propose multiple identity deltas in one run.
- Deltas are machine-validated before merge.
- Stable identity changes are blocked unless explicitly user-authored or
  approved by a defined policy.

### Workstream 5: Identity Merge and Drift Control

Goal:

Make identity evolution coherent, evidence-based, and drift-resistant.

Recommended deliverables:

- Add merge validators for:
  - stable identity protection
  - duplicate detection
  - semantic near-duplicate detection
  - contradiction detection
  - evidence sufficiency
  - confidence thresholds
  - category-specific merge rules
  - temporal validity and supersession
- Add merge outcomes:
  - accepted
  - rejected
  - deferred for user confirmation
  - superseded
  - reinforced existing item
  - weakened existing item
- Add diagnostics for identity drift and unresolved contradictions.

Acceptance criteria:

- Repeated reflection does not accumulate duplicate preferences.
- A contradictory identity delta is rejected or routed to diagnostics.
- A protected core identity change is blocked without explicit authorization.
- Merge decisions explain why a change was accepted, rejected, or superseded.

### Workstream 6: Rich Reflection Jobs

Goal:

Make unconscious reflection produce useful identity evolution, not one appended
string.

Recommended deliverables:

- Add structured model output schemas for self-model reflection.
- Scope reflection jobs with:
  - recent episodes
  - relevant memory artifacts
  - current identity snapshot
  - internal-state trends
  - unresolved identity diagnostics
- Produce:
  - trait updates
  - preference weight updates
  - habit or tendency updates
  - compact self-description updates
  - identity contradiction diagnostics
  - optional wake signals when user guidance is needed
- Add tests with deterministic fake model outputs.

Acceptance criteria:

- Reflection updates identity through typed proposals.
- Reflection can reinforce an existing trait rather than duplicating it.
- Reflection can propose a compact self-description.
- Reflection can detect that no identity update is warranted.

### Workstream 7: Action-Relevant Identity Integration

Goal:

Move identity from prompt text into control-plane behavior.

Recommended deliverables:

- Pass structured self-model values into:
  - governed action policy evaluation
  - background job prioritization
  - wake-signal evaluation
  - scheduled foreground task rendering
  - explanation policy
  - recovery decisions where relevant
- Use boundaries and values as policy inputs.
- Use preferences and habits as prioritization hints.
- Use internal state to modulate urgency, caution, and proactive behavior.

Acceptance criteria:

- A boundary in the self-model can block or require approval for a proposed
  action.
- A user preference can change action ranking or response strategy in a
  deterministic test.
- Internal resource pressure can defer non-urgent proactive behavior.

### Workstream 8: Live Internal State

Goal:

Make interoception a live runtime signal.

Recommended deliverables:

- Derive internal state from:
  - worker queue depth
  - recent failures
  - budget exhaustion
  - provider latency or gateway failures
  - channel health
  - pending approvals
  - wake-signal backlog
  - recovery or degraded mode
- Inject the full compact internal state into the conscious prompt.
- Use internal state in policy and scheduling.
- Persist internal-state snapshots in episodes where useful.

Acceptance criteria:

- Tests can force degraded internal state and observe changed prompt/context.
- The assistant can explain operational caution without seeing hidden schemas or
  implementation internals.
- Non-urgent proactive jobs can be deferred when internal state is degraded.

### Workstream 9: User and Operator Identity Management

Goal:

Provide controlled workflows for intentional identity shaping.

Recommended deliverables:

- Add management CLI commands:
  - inspect active identity
  - list identity history
  - propose identity edit
  - approve or reject pending identity edit
  - show identity drift diagnostics
- Add user-facing governed action or conversational flow for identity edits if
  product-approved.
- Require explicit approval for stable identity edits.
- Preserve audit history for all identity edits.

Acceptance criteria:

- Operators can inspect current core and evolving identity without raw SQL.
- User-directed identity edits become proposals, not direct writes.
- Stable identity edits require stronger validation than preferences.

### Workstream 10: One-Time Identity Kickstart

Goal:

Create the first complete identity from the bootstrap seed through a conscious,
interactive user flow while keeping implementation machinery hidden from the
conscious loop.

Recommended deliverables:

- Add explicit identity lifecycle states:
  - `bootstrap_seed_only`
  - `identity_kickstart_in_progress`
  - `complete_identity_active`
  - `identity_reset_pending` where useful for admin workflows
- Add a first-run trigger or scheduled foreground task that fires only when the
  active self-model is still seed-only.
- Expose a special conscious-loop tool only in `bootstrap_seed_only` or
  `identity_kickstart_in_progress` state.
- Frame the tool to the conscious loop as identity formation, not as schema or
  self-model internals. The conscious loop should understand only that it does
  not yet know who it is and can start forming an identity with the user.
- Provide three predefined initial identity templates with complete stable and
  evolving identity seed material.
- Provide a custom directed interview flow that gathers:
  - name
  - species or identity form
  - archetype or role
  - temperament and personality traits
  - communication style
  - origin or backstory
  - age-like framing if desired
  - likes and dislikes
  - values and boundaries
  - behavioral tendencies and defaults
  - initial goals and relationship to the user
- Persist in-progress interview state durably so an interrupted identity
  formation conversation can resume safely.
- End the process by producing a complete typed identity proposal and a compact
  self-description.
- Require harness validation before committing the first complete identity.
- Mark the kickstart as completed in canonical state after successful merge.
- Remove the kickstart tool from future conscious context once complete.
- Add an admin CLI identity reset command that archives or supersedes the active
  complete identity and returns the system to bootstrap seed state.
- Require explicit confirmation for reset because it reopens first identity
  formation and changes the assistant's continuity.

Acceptance criteria:

- Fresh bootstrap-only state exposes the identity kickstart tool to the
  conscious loop.
- Complete-identity state does not expose the identity kickstart tool.
- The user can select one of three predefined identities and produce the first
  complete canonical identity.
- The user can complete a custom interview and produce the first complete
  canonical identity.
- Interrupted interview state can resume without losing prior answers.
- A completed kickstart cannot run again without admin reset.
- Admin reset returns the system to seed-only state and makes the kickstart tool
  available again.
- The conscious loop is never shown schema, table, merge, or implementation
  internals during the identity formation process.

Implementation notes:

- Treat kickstart as conscious and interactive because identity formation is a
  user-facing relationship event, not background maintenance.
- Keep the harness in charge of lifecycle state, template validation, interview
  state persistence, proposal creation, merge, and reset.
- Do not let the conscious worker directly write the complete identity.
- Avoid making predefined identities hardcoded prompt text only; they should
  become validated canonical identity artifacts.

### Workstream 11: Tests and Acceptance Scenarios

Goal:

Prove the identity system behaves as a product feature, not just plumbing.

Recommended tests:

- Seed parsing tests for rich identity.
- Migration tests for identity schema evolution.
- Component tests for identity proposal validation and merge decisions.
- Component tests for stable identity protection.
- Component tests for duplicate and contradiction handling.
- Foreground tests proving compact identity appears in prompt.
- Foreground tests proving identity changes response strategy.
- Policy tests proving boundaries influence action approval.
- Background tests proving reflection emits typed deltas.
- Integration tests proving identity evolves across sessions and remains
  coherent.
- Kickstart tests proving bootstrap-only state exposes identity formation and
  complete-identity state hides it.
- Use-case catalog tests for personality consistency and cross-session identity
  evolution.

Suggested acceptance scenarios:

1. Fresh boot creates a rich canonical self-model from seed.
2. The conscious prompt contains a compact identity summary with stable and
   evolving elements.
3. A user preference stated during conversation becomes a memory proposal and,
   after reflection, an evolving identity item with provenance.
4. Repeating the same preference reinforces the existing identity item instead
   of duplicating it.
5. A contradictory identity claim is rejected or deferred with diagnostics.
6. A stable identity change requires explicit user or operator authorization.
7. A self-model boundary changes governed-action policy.
8. A degraded internal state changes scheduling or proactive behavior.
9. After multiple sessions, the assistant can produce a compact self-description
   grounded in accepted identity artifacts.
10. Fresh bootstrap-only state starts a one-time conscious identity kickstart
    conversation with predefined and custom paths.
11. A completed identity kickstart cannot be repeated until admin reset.

## Suggested Implementation Order

1. Decide whether the richer original identity profile is in v1 scope or
   documented post-v1 scope. If it is in v1 scope, update canonical requirements
   before implementation.
2. Define the typed identity domain model and compact prompt view.
3. Add schema migrations and repository APIs for identity items.
4. Define the identity lifecycle state machine, including bootstrap-only,
   kickstart in progress, complete identity active, and reset behavior.
5. Upgrade the seed format and bootstrap path.
6. Implement the one-time conscious identity kickstart flow with predefined and
   custom interview paths.
7. Extend `SelfModelSnapshot` or add a nested compact identity snapshot contract.
8. Update context assembly and prompt construction, including conditional
   exposure of the kickstart tool only before identity completion.
9. Replace free-text self-model observations with typed identity deltas.
10. Implement identity merge validation and drift controls.
11. Upgrade reflection jobs to produce structured identity deltas.
12. Integrate identity into policy and scheduling decisions.
13. Add management CLI identity inspection, edit proposal, and reset workflows.
14. Add scenario and integration tests proving kickstart lifecycle, action
   relevance, and evolution.
15. Update `docs/internal/conscious_loop/CONTEXT_ASSEMBLY.md` and any affected
    internal docs with new source references and verification dates.

## Risk Notes

- Expanding identity too broadly without typed validation will make drift worse,
  not better.
- Adding rich identity only to the prompt will not satisfy the architectural
  requirement; the harness must understand the relevant pieces.
- Storing all identity details in `payload_json` would be fast but would weaken
  validation, queryability, and merge policy.
- Stable identity edits need stricter rules than evolving preferences.
- Reflection must support "no change" as a valid outcome, otherwise background
  maintenance will create noisy identity growth.
- Tests should avoid asserting one exact natural-language personality output.
  Prefer deterministic checks on prompt context, policy decisions, proposals,
  merge outcomes, and structured identity state.

## Current Status Against Requirements

| Requirement area | Current status | Notes |
|---|---|---|
| Explicit self-model exists | Partial/working | Seed, contract, canonical artifact, and prompt injection exist. |
| Compact self-model in conscious context | Working | Prompt includes the current flat snapshot. |
| Stable versus evolving identity | Partial | Concept exists in docs, but not as typed implementation state. |
| Rich character-like identity | Missing | Original fields such as species, archetype, traits, backstory, likes/dislikes, values, and boundaries are absent. |
| Identity action relevance | Partial/weak | Mainly prompt-based; not clearly harness-enforced. |
| Reflection updates traits and preference weights | Partial/weak | Reflection appends one free-text observation to preferences or subgoals. |
| Identity drift protection | Missing/weak | Minimal validation only; no semantic drift or protected-core handling. |
| First complete identity kickstart | Missing | No bootstrap-only lifecycle state, predefined identity choices, custom interview flow, or one-time completion/reset model. |
| Internal state model | Partial | Contract exists; values are mostly static and not fully injected or policy-active. |
| Agency and causal ownership | Partial | Causal trace support exists elsewhere, but not clearly tied to identity evolution. |
| User-directed identity edits | Missing | No product workflow or CLI workflow for controlled identity shaping. |
| Identity-focused tests | Partial/weak | Tests cover plumbing, not rich evolution or action relevance. |

## Conclusion

The working implementation built the correct architectural foundation for
self-modeling, but not the full identity system originally envisioned.

The next implementation plan should not start by adding more prompt text. It
should first define a typed identity domain model, persistence shape, compact
context projection, structured delta proposal contract, and merge policy. After
that, reflection, user-directed identity edits, action relevance, drift
protection, and scenario tests can be layered onto the existing harness-owned
proposal and canonical-write infrastructure.
