# Blue Lagoon
## Formal Requirements Specification
Version: 1.0
Status: Baseline approved for implementation design
Scope: Personal AI assistant, single-user first, enterprise-extensible by design

## 1. Purpose

Blue Lagoon is a persistent personal AI assistant implemented as an always-on looping runtime. It is intended to preserve continuity across time, combine reactive and policy-gated proactive behavior, maintain a coherent self-model and autobiographical memory, and separate foreground cognition from background maintenance through a harness-mediated dual-loop architecture.

This document defines the authoritative baseline requirements for implementation design. It specifies what must be true of the system, while intentionally leaving technology-stack and component-selection choices open for design work.

## 2. Normative language

The key words in this document are to be interpreted as follows:

- MUST / MUST NOT: absolute requirement
- SHOULD / SHOULD NOT: recommended requirement unless a documented reason justifies deviation
- MAY: optional capability

## 3. Definitions

- **Conscious loop**: the foreground execution domain responsible for perception, reasoning, user interaction, planning, and user-facing action.
- **Unconscious loop**: the background execution domain responsible for bounded maintenance work such as consolidation, reflection, contradiction analysis, retrieval maintenance, and self-model update proposals.
- **Harness**: the central control plane responsible for mediation, policy enforcement, context assembly, job scheduling, validation, execution control, and canonical writes.
- **Canonical store**: any durable system-of-record store managed by the harness, including episodic records, long-term memory, retrieval artifacts, and self-model artifacts.
- **Self-model**: the structured representation of the assistantâ€™s identity, capabilities, constraints, preferences, current internal state, and current goals.
- **Episodic record**: a structured record of what happened during a meaningful interaction or execution episode.
- **Wake signal**: a structured background-to-foreground request for attention, carrying a typed reason and reason code.
- **Proactive behavior**: foreground behavior initiated without direct user prompting, but only through policy-approved scheduled triggers or approved wake signals.
- **Proposal**: a structured candidate change emitted by a loop and subject to harness validation before canonical commit.

## 4. Product class and scope

### 4.1 Product class
- Blue Lagoon MUST be a true looping agent runtime.
- Blue Lagoon MUST operate as an always-on persistent assistant.
- Blue Lagoon MUST support reactive behavior and policy-gated proactive behavior.
- Blue Lagoon MUST NOT be defined as a stateless chatbot.
- Blue Lagoon MUST NOT be defined as a task-triggered runtime that spins up for a single task and then exits.

### 4.2 Initial scope
- The initial target deployment MUST be a single-user personal assistant.
- The initial system MUST optimize for continuity, coherence, and safety in a personal-assistant context.
- The architecture SHOULD remain extensible to future enterprise or multi-tenant deployment without fundamental redesign of loop boundaries or canonical write ownership.

### 4.3 Non-goals of this specification
This specification does not select:
- a programming language
- a model provider
- a vector database
- a graph database
- a memory framework vendor
- a sandboxing vendor
- a messaging-channel set beyond what the implementation design will determine

## 5. Core architectural requirements

### 5.1 Dual-loop architecture
- Blue Lagoon MUST implement two distinct execution domains:
  - conscious loop
  - unconscious loop
- The conscious loop MUST handle foreground cognition and user-facing behavior.
- The unconscious loop MUST handle bounded background maintenance and transformation work.

### 5.2 Harness
- A central harness MUST exist.
- The harness MUST be the sole mediator between the two loops.
- The harness MUST be the sole controller of canonical writes.
- The harness MUST assemble conscious-loop execution context.
- The harness MUST scope and schedule unconscious-loop jobs.
- The harness MUST validate proposed actions before execution.
- The harness MUST validate proposals before committing them to canonical stores.
- The harness MUST enforce policy, permissions, and execution budgets.

### 5.3 Isolation
- The conscious loop and unconscious loop MUST be isolated at the process boundary.
- The conscious loop and unconscious loop MUST be isolated at the writable-data boundary.
- The conscious loop and unconscious loop MUST be isolated at the context boundary.
- No shared writable memory MAY exist between the loops.
- No direct calls between loops MAY bypass the harness.
- Unconscious workers MUST receive only the minimum scoped inputs required for the assigned job.
- Conscious execution MUST receive only the context relevant to the active trigger and current task.

## 6. Conscious loop requirements

### 6.1 Responsibilities
- The conscious loop MUST interpret incoming messages, goals, and events.
- The conscious loop MUST reason over harness-assembled present-moment context.
- The conscious loop MUST use a compact self-model in reasoning context.
- The conscious loop MUST be able to generate plans, subgoals, and action proposals.
- The conscious loop MUST request tool or external actions through the harness.
- The conscious loop MUST receive observations and tool results through harness-managed channels.
- The conscious loop MUST emit episodic records for meaningful work.
- The conscious loop MUST emit candidate memory items when relevant facts, preferences, or relationships are observed.
- The conscious loop MAY request background work through the harness.

### 6.2 Prohibited behaviors
- The conscious loop MUST NOT directly mutate canonical long-term memory.
- The conscious loop MUST NOT directly rewrite canonical self-model artifacts.
- The conscious loop MUST NOT directly instantiate or control unconscious workers.
- The conscious loop MUST NOT bypass harness policy or permission checks.

## 7. Unconscious loop requirements

### 7.1 Responsibilities
- The unconscious loop MUST run only through harness-managed jobs or ephemeral specialist workers.
- The unconscious loop MUST support memory consolidation.
- The unconscious loop MUST support retrieval and index maintenance.
- The unconscious loop MUST support contradiction detection.
- The unconscious loop MUST support drift analysis.
- The unconscious loop MUST support self-model delta proposal generation.
- The unconscious loop MAY emit wake signals when foreground attention is warranted.

### 7.2 Output constraints
- Unconscious workers MUST return structured outputs only.
- Structured outputs MUST be limited to:
  - memory delta proposals
  - retrieval or index update proposals
  - self-model delta proposals
  - diagnostics or alerts
  - optional wake signals

### 7.3 Prohibited behaviors
- The unconscious loop MUST NOT generate direct user-facing outputs.
- The unconscious loop MUST NOT directly mutate canonical memory.
- The unconscious loop MUST NOT directly mutate canonical self-model artifacts.
- The unconscious loop MUST NOT directly trigger foreground execution.
- The unconscious loop MUST NOT execute side-effecting external actions except through explicitly approved harness mechanisms.

### 7.4 Lifecycle
- Every unconscious worker MUST be bounded.
- Every unconscious worker MUST terminate after completion.
- No unconscious worker MAY retain unstored persistent identity or context after termination.

## 8. Trigger model

### 8.1 Conscious-loop triggers
The conscious loop MUST start or resume only from harness-issued triggers.
Allowed conscious-loop triggers MUST include:
1. User input
2. Scheduled foreground task
3. Approved wake signal
4. Supervisor recovery event
5. Approval resolution event

### 8.2 Unconscious-loop triggers
The unconscious loop MUST start jobs only from harness-issued triggers.
Allowed unconscious-loop triggers MUST include:
1. Time-based schedule
2. Volume or backlog threshold
3. Drift or anomaly signal
4. Foreground delegation
5. External passive event
6. Maintenance trigger

## 9. Results model

### 9.1 Conscious-loop results
The conscious loop MUST be allowed to produce:
- user-facing outputs
- plans and delegations
- tool-action proposals
- episodic records
- candidate memory events
- background job requests

### 9.2 Unconscious-loop results
The unconscious loop MUST be allowed to produce:
- memory delta proposals
- retrieval or index update proposals
- self-model delta proposals
- diagnostics and alerts
- wake signals with typed reasons and reason codes

## 10. Budgeting and bounded execution

### 10.1 Conscious execution
- Every conscious episode MUST have explicit budgets.
- Conscious budgets MUST include:
  - iteration budget
  - wall-clock budget
  - compute and/or token budget
- The harness MUST initialize or restore those budgets at episode start or resume.
- The harness MUST halt or terminate the episode when budgets are exhausted.

### 10.2 Background execution
- Every unconscious job MUST have explicit scope and execution budgets.
- Background budgets MUST include:
  - iteration budget
  - wall-clock budget
  - compute and/or token budget
- The harness MUST terminate background jobs when budgets are exhausted.

### 10.3 Runaway prevention
- The system MUST include forced termination for runaway foreground or background work.
- The system SHOULD include warning thresholds before hard termination.
- The system SHOULD surface repeated budget exhaustion as an operational signal.

## 11. Self-model and identity

### 11.1 Existence and use
- Blue Lagoon MUST maintain an explicit self-model.
- The self-model MUST be used in planning, prioritization, and explanation.
- A compact form of the self-model MUST be injected into conscious-loop reasoning context.

### 11.2 Minimum required contents
The self-model MUST contain at least:
- stable identity
- capabilities
- role
- constraints
- preferences
- current internal-state snapshot
- current goals
- current subgoals where applicable

### 11.3 Identity structure
- The self-model MUST distinguish relatively stable identity attributes from evolving attributes.
- Stable identity SHOULD include a persistent identifier, role, foundational constraints, and communication style.
- Evolving identity SHOULD include preferences, habits, learned tendencies, and autobiographical refinements.

### 11.4 Action relevance
- Identity MUST NOT be cosmetic only.
- Self-model constraints and preferences MUST be able to influence planning and action selection.
- The system SHOULD use identity and internal-state information to preserve coherent behavior over time.

### 11.5 Reflection
- The system SHOULD support scheduled reflection over recent episodes, goals, and internal-state trends.
- Reflection SHOULD be able to produce updated traits, preference weights, and compact self-descriptions.
- Reflection outputs MUST remain proposals until validated by the harness.

## 12. Internal state and agency

### 12.1 Internal-state model
- The system MUST maintain an explicit internal-state model.
- Internal state SHOULD include operational variables analogous to interoception, such as load, health, error rate, resource pressure, or connection quality.
- Internal state MUST be distinguishable from external world state.

### 12.2 Internal vs external state
- The reasoning model MUST separate internal state from external state.
- External state MUST include users, tools, files, channels, environment inputs, and external systems.

### 12.3 Action ownership
- The system SHOULD model causal links between actions, world changes, and internal-state changes.
- The system SHOULD support distinguishing agent-caused changes from externally caused changes.
- These ownership signals SHOULD be available for explanation and future planning.

## 13. Memory requirements

### 13.1 Autobiographical continuity
- Blue Lagoon MUST maintain autobiographical continuity across sessions.
- The system MUST preserve a persistent timeline of what it did, in what context, and with what outcome.

### 13.2 Canonical memory layers
The system MUST maintain at least:
- episodic records
- long-term memory artifacts
- retrieval artifacts
- self-model artifacts

### 13.3 Episodic records
Each meaningful episodic record SHOULD capture:
- timestamp
- trigger source
- relevant self-state before and/or after
- action taken
- context summary
- outcome
- optional evaluation markers

### 13.4 Memory architecture
- The memory layer MUST NOT be a naÃ¯ve append-only system with unconstrained autonomous writes.
- The memory layer MUST support bounded, validated writes.
- The memory layer SHOULD support hybrid retrieval rather than plain vector-only retrieval.
- The memory layer SHOULD support temporal awareness or fact validity handling.
- The memory layer SHOULD support structured relations, graph-like reasoning support, summaries, or equivalent mechanisms for cross-episode reasoning.

### 13.5 Drift mitigation
- The system MUST include drift detection or drift monitoring.
- The system MUST include contradiction detection or equivalent inconsistency monitoring.
- The system MUST support memory consolidation in background jobs.
- The system SHOULD preserve immutable or recoverable episodic traces to enable rollback or re-derivation.

### 13.6 Memory quality
- The system SHOULD reduce duplicate fact proliferation.
- The system SHOULD reduce stale-fact contamination of current reasoning.
- The system SHOULD support correction, supersession, or temporal invalidation of outdated facts.
- Memory proposals SHOULD carry provenance.

## 14. Canonical write and merge model

### 14.1 Ownership
- Only the harness MAY commit changes to canonical stores.
- Canonical stores MUST include memory artifacts, retrieval artifacts, and self-model artifacts.

### 14.2 Proposal flow
- Both loops MAY emit proposals.
- Proposals MUST be validated before commit.
- Validation MUST consider policy, provenance, confidence, and conflict state where applicable.
- Merge outcomes MUST be logged.

## 15. Wake signals and proactive behavior

### 15.1 Wake signals
- Unconscious workers MAY emit wake signals.
- Wake signals MUST include a typed reason and reason code.
- Wake signals SHOULD include an optional payload reference when structured context is needed.

### 15.2 Policy gating
- The harness MUST evaluate wake signals against policy before converting them into conscious-loop triggers.
- The harness MUST be able to throttle, defer, or drop low-priority wake signals.
- Proactive behavior MUST be policy-gated.
- Proactive behavior SHOULD respect user settings, urgency, timing windows, and rate limits.

## 16. Tools and external actions

### 16.1 Mediation
- All side-effecting external actions MUST be executed through the harness.
- Proposed actions MUST be checked against policy and permissions before execution.
- Tool and action results MUST be returned to the conscious loop through harness-managed channels.

### 16.2 Safety posture
- The implementation MUST assume that model output can be incorrect, manipulated, or adversarial.
- The implementation design SHOULD prefer least privilege, bounded execution, approval paths for sensitive actions, and isolated execution for risky tools.

## 17. Logging, traceability, and observability

### 17.1 Event logging
- The system MUST maintain a traceable event history.
- Logged events MUST include, where applicable:
  - trigger source
  - context assembly metadata
  - proposed actions
  - executed actions
  - tool results
  - episodic outputs
  - memory proposals
  - self-model proposals
  - merge decisions
  - wake-signal evaluations

### 17.2 Auditability
- The system MUST support explaining why a canonical mutation was accepted, rejected, or superseded.
- The system SHOULD support replay or forensic reconstruction from logs and episodic records.

## 18. Reliability and recovery

### 18.1 Halt and idle behavior
- The conscious loop MUST halt when goals are satisfied or budgets are exhausted.
- The system MUST return to idle safely when no active trigger exists.

### 18.2 Recovery
- The system MUST support supervisor recovery after crashes, timeouts, or interrupted tasks.
- Recovery SHOULD reconstruct the minimum safe context needed for continuation or graceful abandonment.

### 18.3 Maintenance
- The harness SHOULD support maintenance flows such as checkpoint compaction, index rebuild, failed-merge retry, and stalled-worker cleanup.

## 19. Extensibility constraints

### 19.1 Baseline boundaries
- The first version MUST optimize for personal-assistant coherence and safety rather than enterprise breadth.
- The first version SHOULD avoid premature multi-tenant complexity where it does not improve the single-user product.

### 19.2 Forward compatibility
The architecture SHOULD remain compatible with future additions including:
- multi-tenant policy domains
- stronger secret isolation
- role-based access control
- richer observability
- fleet management
- additional channel gateways

## 20. Acceptance criteria

Blue Lagoon satisfies this baseline specification only if all of the following are true:

1. The system implements separate conscious and unconscious execution domains.
2. The harness is the sole mediator between loops.
3. The harness is the sole canonical writer.
4. The loops are isolated by process and writable-data boundaries.
5. The conscious loop uses a compact self-model in active reasoning context.
6. The system preserves persistent episodic continuity across sessions.
7. The system supports bounded background consolidation and reflection jobs.
8. Memory and self-model updates follow a proposal-and-merge flow.
9. Wake signals are typed and policy-gated.
10. Foreground and background execution are explicitly budgeted and forcibly terminable.
11. The system maintains traceable logs for actions, proposals, and merge decisions.
12. The assistant operates as an always-on persistent runtime with reactive and policy-gated proactive behavior.
13. Identity is operationally relevant and not cosmetic only.
14. The memory layer includes drift monitoring and contradiction monitoring.

## 21. Design Carryover

The following decisions are fixed by this specification and MUST be treated as design inputs:
- always-on personal assistant runtime
- dual-loop architecture
- harness as sole mediator and sole canonical writer
- explicit self-model
- autobiographical memory
- proposal-based canonical writes
- bounded background maintenance
- policy-gated proactive behavior
- traceable canonical history

The following decisions remain open for implementation design:
- language and runtime stack
- exact storage engines
- exact memory framework or custom composition
- scheduler implementation
- sandboxing technology
- observability stack
- channel integrations
