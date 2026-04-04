# Blue Lagoon Clean Handover 02

Date: 2026-04-04
Status: Final consolidated implementation-design handover before the first implementation plan
Audience: Next clean session continuing implementation design

## Purpose

This document consolidates the current implementation-design baseline for Blue Lagoon into one clean handover. It preserves settled decisions, incorporates newer clarified material into a single coherent baseline, and updates the remaining open items that should be resolved before drafting the first implementation plan.

This handover is intended to stand on its own as the current working baseline.

## Current product definition

Blue Lagoon is an always-on personal AI assistant runtime, not a stateless chatbot and not a task-triggered agent that exits after one job.

It is intended to behave as a persistent assistant with continuity across time, policy-gated proactive behavior, durable autobiographical memory, and an explicit self-model that materially influences reasoning and action selection.

The v1 product posture remains single-user personal assistant first, with architecture that can later extend to stronger isolation, richer policy domains, additional channels, and future multi-tenant or enterprise concerns without breaking the core execution model.

## Guiding philosophy

Blue Lagoon is being built as a persistent assistant that should feel coherent, time-aware, and shaped by experience rather than behaving like a wrapped stateless chat session.

The project goal is not just usefulness per interaction, but continuity, evolving identity, durable memory, and a grounded internal point of view.

The core philosophical posture is harness-heavy design. Deterministic continuity, policy, execution control, validation, budgeting, and canonical state management belong in the harness. The model contributes expression, judgment, nuance, and bounded reasoning inside a harness-controlled frame.

Identity is operational, not cosmetic. Memory should feel like a mind rather than a disconnected retrieval layer. Interoception and internal state are first-class, but hidden maintenance machinery must remain invisible to the conscious loop. These are architectural constraints, not style notes.

## Architectural invariants

The following points should be treated as effectively fixed unless the formal requirements themselves are intentionally changed:

- Blue Lagoon is an always-on looping runtime.
- The architecture is dual-loop: conscious foreground execution and unconscious background execution.
- The harness is the sole mediator between loops.
- The harness is the sole canonical writer.
- Conscious and unconscious execution remain isolated at process, writable-data, and context boundaries.
- Canonical memory and self-model changes must follow proposal, validation, and merge flows.
- The system maintains an explicit self-model and autobiographical continuity.
- Background maintenance is bounded and policy-gated.
- Proactive behavior is policy-gated.
- Traceability, auditability, and bounded execution are first-class requirements.
- The conscious loop must not see or control the hidden maintenance machinery that supports memory and identity.
- Identity is operational, not cosmetic; it must shape planning, action selection, explanation style, and constraints.
- Internal state and external state remain explicitly separated.

## Baseline implementation direction

The implementation baseline established so far is:

- Core implementation language: Rust.
- Runtime shape: one harness-centered runtime with isolated conscious and unconscious execution domains.
- Canonical persistence: PostgreSQL-first.
- Retrieval strategy: hybrid lexical and semantic retrieval in PostgreSQL for v1.
- Memory model: episode-first canonical memory with derived semantic and retrieval layers.
- Database stance: relational-first, JSONB-minimal.
- Control model: proposal, validation, merge, audit.
- Product posture: single-user first, enterprise-extensible later.

The canonical persistence direction is settled enough to treat as fixed for v1:

- PostgreSQL is the canonical system of record for episodes, memory artifacts, retrieval artifacts, self-model artifacts, proposals, merge history, wake signals, job state, recovery state, workspace state, and durable audit events.
- Hybrid retrieval remains Postgres-native for v1 rather than introducing a separate vector database.
- Canonical memory is not a flat append-only vector store. It is an episode-first system with background consolidation, contradiction handling, provenance, and temporal or supersession-aware reasoning.

## Loop model

### Conscious loop

The conscious loop remains the foreground executive. It is responsible for perception, user interaction, present-moment reasoning, planning, proposing actions, requesting tool use through the harness, observing results through harness-managed channels, emitting episodic records, and optionally requesting background jobs.

A conscious episode should follow this high-level flow:

1. Harness receives a valid trigger.
2. Harness assembles a compact self-model snapshot, selected memory, and current internal-state snapshot.
3. Harness initializes or restores budgets.
4. The conscious loop perceives, plans, proposes actions, and receives approved observations.
5. The conscious loop emits user-facing outputs, episodic records, candidate memories, and optional background job requests.
6. The loop halts when the goal is satisfied or its budgets are exhausted.

Allowed conscious triggers remain:

- User input.
- Scheduled foreground task.
- Approved wake signal.
- Supervisor recovery event.
- Approval resolution event.

### Unconscious loop

The unconscious loop remains a background maintenance system executed only through harness-managed jobs and ephemeral specialist workers.

Its responsibilities are:

- Consolidating episodic material.
- Maintaining retrieval structures.
- Detecting contradiction and drift.
- Generating self-model delta proposals.
- Producing diagnostics and alerts.
- Optionally emitting wake signals with typed reasons and reason codes.

An unconscious job should follow this high-level flow:

1. Harness receives a valid background trigger.
2. Harness scopes the job and assigns bounded budgets.
3. Harness spawns an isolated worker with only the required scoped inputs.
4. The worker performs bounded analysis or transformation.
5. The worker returns structured outputs only.
6. The harness validates and merges accepted proposals.
7. The worker terminates with no retained unstored identity or context.

Allowed unconscious triggers remain:

- Time-based schedule.
- Volume or backlog threshold.
- Drift or anomaly signal.
- Foreground delegation.
- External passive event.
- Maintenance trigger.

The unconscious loop may produce only:

- Memory delta proposals.
- Retrieval or index update proposals.
- Self-model delta proposals.
- Diagnostics and alerts.
- Optional wake signals with typed reasons and reason codes.

The unconscious loop must not:

- Produce direct user-facing output.
- Directly mutate canonical state.
- Directly wake the conscious loop.
- Execute side-effecting actions outside explicitly approved harness mechanisms.

## Harness sovereignty

The harness remains the central control plane and the heaviest architectural layer.

It owns:

- Context assembly.
- Policy enforcement.
- Permissions.
- Job scheduling.
- Budget enforcement.
- Canonical write validation.
- Merge decisions.
- Action mediation.
- Logging and trace correlation.
- Recovery coordination.
- Recovery-trigger issuance.

This remains a hard design posture:

- Deterministic plumbing belongs in the harness, not in prompts.
- Neither loop owns canonical writes.
- Tool execution is harness-controlled.
- Merge decisions are validated and auditable.
- Foreground and background work are explicitly bounded and forcibly terminable.
- Recovery coordination is harness-owned.
- No worker self-recovery exists as an authority path.

Transaction boundaries remain harness-owned. The harness may expose reusable service-layer write paths, but low-level repositories or workers must not become de facto owners of business-critical transaction logic.

## Self-model, identity, and interoception

Blue Lagoon maintains an explicit self-model representing who it is right now in operational terms.

At minimum the self-model contains:

- Stable identity.
- Capabilities.
- Role.
- Constraints.
- Preferences.
- Current internal-state snapshot.
- Current goals.
- Current subgoals where applicable.

The identity model remains split into two categories:

### Stable identity

Persistent identifier, role, foundational temperament, communication style, enduring constraints, and similar continuity-preserving attributes.

### Evolving identity

Preferences, habits, learned tendencies, autobiographical refinements, recurring self-descriptions, and other traits that may change through experience and reflection.

Identity remains action-relevant. It is not a decorative persona block. Planning, prioritization, explanation, and boundary enforcement should all be able to depend on the self-model.

Internal state also remains first-class. Blue Lagoon should explicitly track interoceptive or body-like internal variables such as:

- Load.
- Health.
- Reliability.
- Error conditions.
- Resource pressure.
- Confidence.
- Connection quality.

Decisions should be evaluated as a function of both internal state and external world state.

The system should also preserve a sense of agency by tracking the relationship between actions, resulting world changes, and changes in internal state. That gives the assistant a functional distinction between changes it caused and changes that merely happened around it.

The conscious loop must experience these internal signals as part of its own present condition without being exposed to hidden implementation machinery, schema details, or unconscious maintenance internals.

## Memory model

The memory baseline is settled in broad form.

Canonical layers are:

- Episodic records.
- Long-term memory artifacts.
- Retrieval artifacts.
- Self-model artifacts.
- Relation or graph-like support where useful for cross-episode reasoning.

The decisive memory posture is:

- Episodes are first-class and preserved for meaningful interactions.
- Background jobs consolidate episodes into longer-lived semantic structures.
- Memory proposals carry provenance.
- Contradictions, stale facts, and duplicate proliferation are treated as defects.
- The system should support supersession or temporal invalidation of outdated facts.
- Immutable or recoverable episodic traces should exist so higher-level artifacts can be re-derived if needed.

This means Blue Lagoon should feel as though it remembers like a mind rather than storing disconnected embeddings.

The assistant sees selected memory surfaced into conscious context, while deeper consolidation, retrieval maintenance, contradiction detection, and self-model refinement remain hidden in unconscious jobs and harness logic.

## Workspace subsystem

The workspace subsystem is now part of the agreed design.

It exists to hold governed operational artifacts that are adjacent to work but do not belong in canonical autobiographical or semantic memory tables.

The workspace should cover artifacts such as:

- Notes.
- Runbooks.
- Scratchpads.
- Task lists.
- Scripts.
- Script versions.
- Script run history.
- Other working materials generated or used during operations.

Workspace remains explicitly distinct from canonical autobiographical memory and should retain that separation in both schema design and code ownership.

## Tool safety and script governance

The current working decision is a risk-tiered tool safety model, and that model should now be treated as agreed even though some implementation choices remain open.

Tool and script actions are classified by risk tier rather than by a flat allowlist model:

- Tier 0: read-only retrieval and inspection.
- Tier 1: safe bounded local transformations without external side effects.
- Tier 2: controlled local or external side effects within bounded policy.
- Tier 3: sensitive or dangerous actions requiring stronger controls.

Scripts are first-class governed artifacts inside the workspace subsystem. This means script creation, editing, retrieval, versioning, and run history belong to workspace. Actual execution remains governed by the harness tool and policy layers.

Permission boundaries are explicitly split:

- Permission to create or edit a script.
- Permission to execute a script.
- Permission to execute a script with broader capabilities such as network access or higher-risk filesystem mutation.

The v1 direction remains:

- Bounded subprocess isolation by default for lower-risk tools.
- Stronger sandboxing for sensitive tools and scripts.
- Explicit capability scoping for filesystem reach, environment exposure, network access, and execution budgets.
- Policy-driven approval that considers action type, target sensitivity, side-effect class, script provenance, loop of origin, and repeat or retry context.

The locked principle is that models or workers may propose tool use, but real execution remains harness-controlled.

## Model gateway

The model gateway design is now substantially settled at the architectural level.

The gateway must be provider-agnostic. Conscious and unconscious execution must be able to route to different providers or model classes, and the architecture must remain compatible with:

- OpenAI-compatible providers.
- Anthropic-native integrations.
- Local backends such as Ollama.

Starting operations with a specific provider is acceptable, but the architecture must not collapse into a provider-specific design.

The gateway remains harness-owned. Workers and loop implementations should emit structured model-call requests, while the harness decides routing, budget enforcement, schema handling, retry policy, tracing, and provider adapter selection.

A canonical internal gateway contract should exist and remain intent-based rather than vendor-native. At minimum it should capture fields equivalent to:

- Purpose.
- Loop kind.
- Task class.
- Budget envelope.
- Input context.
- Expected output mode.
- Schema reference or inline schema.
- Tool policy.
- Provider hints.
- Trace context.

Logical model tiers are also part of the current direction:

- Tier A: high-quality foreground reasoning.
- Tier B: mid-cost structured background analytic work.
- Tier C: low-cost utility extraction and bounded transforms.

Structured outputs should be the default whenever practical, especially for unconscious work. Unconscious jobs should stay constrained to structured results such as proposals, diagnostics, alerts, retrieval updates, self-model deltas, and optional wake signals.

Tool execution stays outside the gateway. The gateway may return structured tool-call candidates or equivalent proposals, but actual tool execution, validation, approval, and routing remain inside the harness execution and policy layers.

## Orchestration and worker model

The current orchestration direction is now clear:

- Tokio-based async orchestration inside the harness.
- Persistent job state stored in PostgreSQL.
- Ephemeral out-of-process workers for conscious and unconscious execution.
- No heavyweight workflow engine in v1.

The harness remains responsible for trigger intake, job creation, scoping, budget initialization, worker dispatch, timeout handling, retries where appropriate, stalled-worker cleanup, and recovery coordination.

This preserves a harness-heavy design while avoiding premature operational complexity in v1.

## Recovery model

Recovery is settled in principle.

The core recovery rule is that Blue Lagoon recovers by rehydrating persisted episode or job state and spawning a fresh worker with a recovery trigger. It does not restore prior conscious or unconscious process memory in place.

The harness is the sole recovery authority. Workers are always disposable and no worker self-recovery path should exist.

### Checkpoints

A checkpoint in v1 should be harness-owned structured state, not an opaque serialized model session.

For conscious work, the checkpoint should include at least:

- Trigger type.
- Active goal.
- Approved plan state.
- Remaining budgets.
- Relevant tool and action outcomes so far.
- Pending approvals.
- References to already-written episodic or proposal artifacts.

For unconscious jobs, the checkpoint should include at least:

- Job type.
- Scoped inputs.
- Progress markers.
- Already-emitted proposals.
- Retry count.
- Exact recovery reason.

The v1 interpretation of checkpointing should stay narrow. It should preserve durable execution state needed for safe continuation, not attempt to preserve hidden chain-of-thought or full transient model session state.

### Continuation and abandonment

Foreground continuation should be allowed only when the interrupted episode has no ambiguous external side effect and the harness can prove what already happened from durable records.

If the system cannot prove whether a side-effecting action completed, the episode must not blindly continue. It should enter a graceful-abandonment, clarification, or approval-mediated path.

This yields the practical rule set:

- Resume read-only reasoning when durable context is sufficient.
- Resume after deterministic tool results already captured.
- Resume after approval resolution.
- Retry idempotent bounded work with safeguards where policy allows.
- Abandon, pause, or seek clarification after uncertain side effects, corrupted context, or exhausted recovery budget.

Graceful abandonment is not a design failure. In ambiguous cases it is the correct safe behavior.

### Worker crash and stall handling

Worker crash handling should be asymmetric:

- If a conscious worker dies, the harness records an interrupted foreground episode and may emit a supervisor recovery event.
- If an unconscious worker dies, the harness decides whether to retry, defer, or mark the job failed based on job class, idempotence, retry count, and budget.

Stalled work should be treated as a first-class maintenance case.

The v1 operational rule is heartbeat plus lease expiry:

1. The harness assigns each active worker a lease with a deadline.
2. Progress events refresh the lease.
3. The harness may send soft termination at a warning threshold.
4. The harness hard-kills on expiry.
5. The harness then moves the episode or job into recovery evaluation.

Recovery policy should be class-based rather than global. Different work types may resume, retry, defer, abandon, or seek clarification according to side-effect risk, determinism, and policy.

## Observability

Observability is resolved enough to treat as a settled design area for v1.

The system should emit structured events rather than relying on free-text logs as the primary source of truth. Durable audit events and high-volume operational tracing are distinct concerns and should not be treated as the same sink.

### Durable audit events

Canonical audit history should be stored in PostgreSQL in a dedicated `audit_events` table, separate from business tables.

These rows are durable, harness-owned, queryable, and part of the system's forensic and audit surface.

The recommended structured envelope includes fields equivalent to:

- `event_id` as UUIDv7.
- `occurred_at`.
- `loop_kind`.
- `subsystem`.
- `event_kind`.
- `severity`.
- `trace_id`.
- `span_id`.
- `parent_span_id` where applicable.
- `job_id` where applicable.
- `worker_pid` where applicable.
- `model_tier` where applicable.
- `payload` as structured JSON.

Monthly time-based partitioning is the default posture for controlling growth.

### Operational traces

Operational trace or span events are best-effort and high-volume. They should be buffered asynchronously and may be dropped under pressure rather than competing with canonical writes.

The logging and trace stack should use Rust `tracing` plus `tracing-subscriber` with structured JSON output.

PostgreSQL audit writes must not happen inline from the event loop. Instead, events should be enqueued into a bounded in-memory channel and flushed in batches by a dedicated background task.

### Correlation model

The harness assigns `trace_id` at trigger intake. Workers inherit trace context through the job specification rather than through ambient process state.

Model gateway calls carry trace context as part of the internal gateway contract, but the harness copy remains canonical and any echoed model value is only validated, never trusted as authoritative.

Tool execution also inherits trace context from harness dispatch rather than from workers or model output. If an unconscious wake signal leads to a conscious trigger, the new foreground trace should carry a causal reference to the originating unconscious trace.

### Metrics and health

Operational metrics should not depend on the same PostgreSQL write path as canonical data.

The better v1 posture is:

- An in-process metrics registry.
- A Prometheus-compatible exporter or equivalent local scrape surface.
- Optional periodic snapshots later if trend storage becomes necessary.

The harness should expose a structured health query surface, either as a CLI command, local endpoint, or both.

That health view serves two roles:

- Human operator diagnostics.
- A structured interoception feed for the system's own internal-state model.

Core metrics to track include:

- Job dispatch, completion, failure, and timeout counts.
- Model gateway calls by tier and provider, including latency.
- Tool executions by risk tier and outcome.
- Proposal merge and rejection rates.
- Memory write throughput.
- Stalled worker counts.
- Error spikes and repeated recovery conditions.

### Alerting and dashboards

Alerting in v1 should remain inside the existing policy-gated proactivity model rather than introducing an external paging stack.

Operational anomalies should produce structured internal alert events and, where policy allows, internal wake signals. System-health wake signals should use a lower deferral threshold than ordinary content-driven wake signals so serious runtime issues are not hidden by overly conservative policy.

V1 dashboards remain lightweight:

- A health query.
- A `--tail-events` mode.
- An optional small read-only status page served by the harness.

The schema should remain compatible with future OTLP export, but OTLP collectors, Jaeger, and full Grafana-style stacks are not required in v1.

## Deployment topology

Deployment topology is settled enough to treat as the default v1 path.

Blue Lagoon v1 should run as a single-node harness-centered multi-process runtime with PostgreSQL as a separate durable service.

### Runtime shape

The harness is the long-lived control service. Conscious and unconscious work run in isolated ephemeral subprocesses under harness control.

This satisfies the required process isolation while preserving the agreed orchestration model and avoiding premature worker fleets, brokers, or service choreography.

### Packaging and distribution

The default packaging posture is one packaged application or image with multiple entry modes and a multi-process runtime. That is the preferred v1 deployment shape, but it is not an eternal architectural truth.

Packaging remains intentionally flexible as long as the following remain intact:

- Harness sovereignty.
- Process isolation.
- Canonical write ownership.
- Worker ephemerality.
- Bounded execution.

A future harness-owned sidecar remains an allowed extension for narrowly justified concerns such as stronger sandboxing or provider isolation, so long as it does not become a second control plane and does not gain canonical write authority.

### Services, Compose, and supervision

A pure single-process binary is rejected because it violates the process-isolation requirement.

Separate long-lived worker daemons are also rejected as the default v1 posture because they add restart, routing, and lease complexity without improving the single-user product.

Docker Compose should exist for local development and single-node production-like setups, but only with the core durable services:

- `postgres`
- the runtime service that hosts the harness

Workers should not be separate Compose services by default because the harness is expected to create and supervise ephemeral worker processes directly.

The supervision boundary should be split cleanly:

- An external supervisor such as systemd or a container restart policy watches the harness process.
- The harness supervises worker lifecycle, timeouts, retries, stalled-job cleanup, and recovery coordination.

### Configuration and secrets

Configuration should remain simple in v1:

- One versioned non-secret config file.
- Environment-variable or mounted-secret injection for sensitive values.
- Provider credentials terminate at the harness unless a specific tool execution requires a tightly scoped subset.

The v1 deployment posture therefore is:

- Single-node first.
- Harness-centered.
- PostgreSQL separate.
- No distributed worker pool.
- No message broker.
- No Kubernetes requirement.

## Codebase boundaries

The codebase boundary discussion is settled enough for implementation planning.

The most important rule is that architectural authority boundaries matter more than Cargo granularity. The codebase should make it difficult to bypass harness sovereignty by accident.

### Boundary rules

The following rules should be treated as fixed:

- The harness is the only authority that orchestrates canonical write flows end to end.
- Storage and repositories may persist data but must not decide whether canonical mutations are valid.
- Workers may emit proposals, diagnostics, wake signals, and structured outputs, but must not gain direct canonical write authority.
- Workers must not own provider-specific gateway logic.
- Tool execution authority remains outside workers and outside the model gateway.
- Workspace remains separate from canonical autobiographical memory.
- Shared cross-process types should be explicit and stable.

### Recommended v1 crate posture

The most defensible v1 codebase shape is:

- `app`: thin entrypoints and runtime wiring.
- `harness`: first-class crate and primary control plane.
- `contracts`: first-class crate for stable shared types across process boundaries.
- `workers`: first-class crate for conscious and unconscious worker runtimes.

Most other concerns should begin as disciplined internal modules inside `harness` or an adjacent support layer rather than as separate top-level crates on day one. That includes:

- gateway
- memory
- storage
- tools
- policy
- scheduler
- observability
- workspace

This is the current best balance between architectural clarity and implementation pragmatism. If specific subsystems become cognitively or operationally heavy later, they can be extracted into their own crates without changing the control model.

### Why harness is first-class

`harness` should be treated as first-class in a stronger sense than most other subsystems because it is not just another service area. It is the control plane of the runtime.

Giving `harness` its own top-level boundary helps prevent illegal architectures such as:

- workers directly mutating canonical state
- repositories silently becoming business authorities
- provider-specific logic leaking into the wrong layer
- policy and scheduling drifting into independent control planes

The system's center of gravity is explicitly harness-centered, so the codebase should reflect that.

## Migration and schema-evolution posture

This area is now substantially resolved and should no longer be treated as broadly open.

The core v1 migration decision is controlled canonical evolution under harness-governed safety rules. Because PostgreSQL is the canonical system of record, the design is relational-first, and canonical writes are harness-owned, schema evolution is part of the control architecture rather than a casual persistence concern.

### Core rules

- Use reviewed numbered SQL migrations as the source of truth for schema history.
- Do not rely on ad hoc manual database changes in normal development or production-like environments.
- Prefer additive evolution by default.
- Use expand-contract as the standard destructive-change pattern:
  1. Introduce new structures.
  2. Backfill and validate.
  3. Cut readers and writers over.
  4. Remove obsolete structures only after clean validation.
- Destructive changes such as dropping columns or rewriting meaning in place should be rare and only happen after staged migration and validation.
- Keep the database posture relational-first, with JSONB used minimally and intentionally.

### Ownership and runtime safety

- The harness owns migration policy and schema-version safety gates, even if the low-level SQL files and helpers live elsewhere.
- Runtime startup must verify schema-version compatibility before accepting work.
- The runtime must fail closed on unsupported schema versions rather than operating against ambiguous canonical state.
- Migrations must complete before normal harness scheduling begins so conscious and unconscious work never run against partially upgraded canonical tables.

### Compatibility rules

Because `contracts` is a first-class crate and workers are isolated processes, compatibility must be managed explicitly across persistence and process boundaries.

The v1 rule is:

- Maintain temporary backward compatibility where practical for persisted cross-boundary artifacts such as job records, proposals, triggers, wake signals, and recovery state.
- Do not over-apply this burden to purely internal in-process types.
- If compatibility cannot be maintained, require a coordinated upgrade boundary and block mixed-version execution through startup gating.

### Rollback posture

The v1 rule is not “rollback everywhere” and not “rollback never.” The correct posture is:

- Never assume rollback is free.
- Rollback may be allowed before incompatible data transformation has been committed.
- After cutover of incompatible data shape, the preferred recovery path is usually a new corrective migration rather than an opaque reversal.

### Migration decisions now treated as settled

- Migration mechanism: reviewed SQL migrations.
- Default evolution style: additive-first, expand-contract by default.
- Canonical ownership: harness-governed migration safety and version checks.
- Runtime safety: refuse normal execution on unsupported schema versions.
- Data preservation: stage, copy, validate, cut over before destructive removal.
- JSON posture: relational-first, JSONB-minimal.
- Compatibility scope: persisted cross-process and cross-recovery artifacts where practical.
- Auditability: schema evolution must be traceable and reviewable.

## User-facing surface roadmap

This area is also now substantially resolved and should no longer be treated as broadly open.

The v1 surface decision is best expressed as conversation-first, Telegram-first for v1.

The interaction model is chat-like, but the first real production channel and long-lived main surface is Telegram. Any simple local chat UI may exist for development, testing, or fallback, but it is not the primary product surface.

### Surface posture

- Blue Lagoon should begin with one primary foreground conversation surface.
- Telegram is the primary v1 ingress and egress channel.
- The core system remains channel-agnostic even though Telegram is the first dominant adapter.
- Message normalization, trigger creation, approvals, policy checks, identity handling, and rate control belong in the core harness-mediated system, not in Telegram-specific business logic.
- Broader multi-channel rollout should be deferred until it clearly improves the single-user core assistant experience.

### Proactive behavior and notifications

Proactive behavior remains allowed in v1, but only in a narrow and policy-gated form.

- Notifications are downstream delivery mechanisms for already-approved foreground triggers, not an independent autonomy channel.
- Telegram should support both reactive conversation and approved proactive delivery.
- Proactive delivery should stay narrow in v1, focused on approved cases such as urgent reminders, important follow-ups, or approved wake-signal conversions.
- Proactive behavior must respect user settings, urgency, timing windows, rate limits, and harness policy evaluation.
- Low-priority wake signals may be throttled, deferred, or dropped.

### Diagnostics and auxiliary surfaces

- A simple local chat surface may exist for development and fallback.
- A small status or activity surface may be added later if useful.
- Operator diagnostics should remain clearly distinct from the main end-user conversation surface.

### Surface decisions now treated as settled

- Interaction model: conversation-first.
- Primary production channel: Telegram.
- Local UI posture: secondary development and fallback surface, not the main product identity.
- Product focus: single-user coherence before channel breadth.
- Proactive behavior: allowed, narrow, and policy-gated.
- Notifications: minimal and conservative in v1.
- Expansion posture: keep the architecture ready for future channels without optimizing for them yet.

## Consolidated v1 architecture picture

Putting the settled pieces together, the current v1 runtime picture is:

- A Rust harness-centered runtime.
- Isolated out-of-process conscious and unconscious workers.
- PostgreSQL as canonical system of record, persistent job-state store, and recovery-state store.
- Tokio-based orchestration in the harness.
- Harness-owned transaction and canonical write boundaries.
- A governed workspace subsystem for operational artifacts and scripts.
- A risk-tiered tool execution model with policy checks, approvals, and sandbox gradients.
- A harness-owned, provider-agnostic model gateway with per-loop and per-task routing.
- Tool execution outside the model gateway and inside the harness execution-policy layer.
- A self-model and internal-state model injected into foreground reasoning in compact form.
- An episode-first memory architecture with background consolidation and drift-control mechanisms.
- Harness-led recovery using structured checkpoints and fresh worker instantiation.
- A dual-surface observability model with durable audit events and best-effort operational traces.
- A single-node default deployment topology with multi-process runtime isolation.
- A conversation-first, Telegram-first v1 surface with a channel-agnostic core.
- A codebase with first-class boundaries for app, harness, contracts, and workers, and internal-first packaging for most other subsystems in v1.
- A controlled canonical schema-evolution model using reviewed SQL migrations and expand-contract by default.

## What remains open

The previous broad open items for migration policy and user-facing surface should now be considered largely resolved.

The remaining work before the first implementation plan is mainly narrower implementation-readiness detail.

### 1. Recovery implementation detail tables

Recovery is resolved in principle, but still needs explicit specification for:

- Exact checkpoint schemas.
- Recovery reason taxonomy.
- Retry classes.
- Idempotence classification rules.
- Lease timing defaults.
- Recovery budget defaults.
- Clarification versus abandonment policy triggers.
- Heartbeat cadence and lease-renew semantics.
- Per-job-class retry ceilings.

### 2. Surface implementation details

The high-level surface roadmap is settled, but a few implementation details still need specification:

- Telegram adapter boundary and message-normalization contract.
- Approval interaction flow inside Telegram.
- Exact in-app versus Telegram notification behavior.
- Fallback local chat scope.
- Minimal operator status surface, if any, for v1.

### 3. Migration operational conventions

The migration posture is settled, but some operational specifics still need definition:

- Naming convention for migration files.
- Schema-version metadata table shape.
- Migration review checklist.
- Exact rule for compatibility-window enforcement.
- Which tables are explicitly re-derivable versus strictly canonical.

## Recommended next-session order

The next clean design session should now proceed in this order:

1. Finalize recovery implementation detail tables and defaults.
2. Finalize Telegram adapter and approval-flow specifics.
3. Finalize migration operational conventions and re-derivation boundaries.
4. Draft the first implementation plan after the above is stable.

## Guardrails for the next session

The next session should preserve the following posture unless there is a strong reason to change direction:

- Keep the harness sovereign.
- Keep the loops isolated.
- Keep canonical writes proposal-based and harness-owned.
- Keep PostgreSQL as the canonical store for v1.
- Keep the memory model episode-first.
- Keep workspace separate from canonical autobiographical memory.
- Keep orchestration lightweight and harness-centered in v1.
- Keep tool execution policy-driven and risk-tiered.
- Keep scripts governed and auditable.
- Keep the model gateway provider-agnostic and harness-owned.
- Keep actual tool execution outside the model gateway.
- Keep identity action-relevant and grounded in a live self-model.
- Keep internal-state and interoceptive signals first-class.
- Keep observability harness-centered, structured, and audit-friendly.
- Keep recovery harness-led, checkpoint-light, and fresh-worker based.
- Keep codebase boundaries aligned with authority boundaries rather than premature packaging maximalism.
- Keep deployment topology simple for v1 while avoiding packaging decisions that create unnecessary future migration constraints.
- Keep Telegram as the primary v1 channel without hardwiring channel-specific assumptions into the product core.
- Use expand-contract as the default schema-evolution pattern for canonical data.
- Avoid premature enterprise, fleet, or multi-channel complexity unless it clearly improves the single-user v1 product.

## Readiness status

Blue Lagoon is now past the stage of fundamental architectural uncertainty.

The core runtime model, harness sovereignty, dual-loop isolation, self-model posture, memory direction, canonical persistence stance, migration posture, user-surface posture, orchestration direction, workspace concept, tool safety posture, provider-agnostic model gateway, recovery model, observability posture, codebase boundary posture, and default deployment topology are coherent enough that the remaining work is primarily operational detail.

The project is now very close to implementation-plan readiness, but it should still resolve the open items above before the first concrete implementation plan is drafted.