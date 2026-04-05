# Blue Lagoon
## Implementation Design

Date: 2026-04-04
Status: Final validated implementation-design baseline
Audience: Implementation planning and implementation work

## Purpose

This document defines the validated implementation design for Blue Lagoon.

It consolidates the settled architectural decisions, implementation-direction choices, operational constraints, and validation guidance gathered during the design phase into one canonical baseline for implementation planning and implementation work.

This document is intended to stand on its own as the current implementation-design source of truth.

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

- PostgreSQL is the canonical system of record for episodes, memory artifacts, retrieval artifacts, self-model artifacts, proposals, merge history, approval state, wake signals, job state, recovery state, workspace state, and durable audit events.
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

The tool safety model is risk-tiered rather than based on a flat allowlist.

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

The model gateway design is settled at the architectural level.

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

Recovery is now settled in implementation-design detail.

The core recovery rule is that Blue Lagoon recovers by rehydrating persisted episode or job state and spawning a fresh worker with a recovery trigger. It does not restore prior conscious or unconscious process memory in place.

The harness is the sole recovery authority. Workers are always disposable and no worker self-recovery path should exist.

### Recovery posture

The v1 recovery posture is:

- Harness-led.
- Checkpoint-light.
- Fresh-worker based.
- Proof-based for continuation.
- Fail-closed in ambiguous side-effect cases.

Recovery should reconstruct only the minimum safe durable context needed for continuation, retry, clarification, re-approval, deferment, or graceful abandonment.

### Checkpoints

A checkpoint in v1 should be harness-owned structured state, not an opaque serialized model session.

V1 should use one typed `recovery_checkpoints` table rather than separate foreground and background checkpoint tables.

The checkpoint model should remain structured and narrow rather than attempting full session serialization.

Typed common fields should include at least:

- `checkpoint_id`.
- `execution_kind`.
- `execution_id`.
- `trigger_type`.
- `active_goal` where applicable.
- `job_type` where applicable.
- `approved_plan_ref` where applicable.
- `remaining_budget_state`.
- `pending_approval_ref` where applicable.
- `retry_count`.
- `recovery_reason_code`.
- `created_at`.
- `updated_at`.

The table may include a tightly scoped JSONB field for class-specific progress markers, scoped input references, or similar execution-specific cursors that would be awkward to normalize early. This JSONB field must not become an opaque restored model session.

For conscious work, the checkpoint should preserve at least:

- Trigger type.
- Active goal.
- Approved plan state by reference.
- Remaining budgets.
- Relevant tool and action outcomes so far.
- Pending approvals.
- References to already-written episodic or proposal artifacts.

For unconscious jobs, the checkpoint should preserve at least:

- Job type.
- Scoped inputs.
- Progress markers.
- Already-emitted proposals.
- Retry count.
- Exact recovery reason.

The v1 interpretation of checkpointing should stay narrow. It should preserve durable execution state needed for safe continuation, not attempt to preserve hidden chain-of-thought or full transient model session state.

### Recovery reason taxonomy

The recovery reason taxonomy should remain compact in v1:

- `crash`
- `timeout_or_stall`
- `supervisor_restart`
- `approval_transition`
- `integrity_or_policy_block`

Finer operational detail may exist in diagnostic payloads, but these codes should be sufficient for the main recovery decision paths.

### Continuation and abandonment

Foreground continuation is allowed only when the harness can prove what already happened from durable records.

Automatic continuation is allowed for:

- Read-only work.
- Deterministic local work whose outcomes are already durably recorded.
- External actions the harness can prove are idempotent and whose execution state is durably known.
- Re-entry after approval resolution.

Automatic continuation is not allowed for:

- Ambiguous external side effects.
- Non-repeatable external actions.
- Corrupted or incomplete checkpoint state.
- Exhausted recovery budget.

If the system cannot prove whether a side-effecting action completed, the episode must not blindly continue. It must enter a clarification, explicit re-approval, deferment, or graceful-abandonment path.

Graceful abandonment is not a design failure. In ambiguous cases it is the correct safe behavior.

### Idempotence classification

The v1 recovery model should use a compact action classification:

- `safe_replay`
- `provably_idempotent_external`
- `ambiguous_or_nonrepeatable`

Every side-effecting tool or external action should have an explicit recovery classification. The harness should persist enough action metadata to justify continuation or retry, including fields equivalent to:

- Action fingerprint.
- Idempotency key where applicable.
- Dispatch time.
- Completion state.
- Durable external reference or result where available.

### Worker crash and stall handling

Worker crash handling should be asymmetric:

- If a conscious worker dies, the harness records an interrupted foreground episode and may emit a supervisor recovery event.
- If an unconscious worker dies, the harness decides whether to retry, defer, or mark the job failed based on job class, action classification, retry count, and budget.

Stalled work should be treated as a first-class maintenance case.

The v1 operational rule is heartbeat plus lease expiry:

1. The harness assigns each active worker a lease with a deadline.
2. Progress events refresh the lease.
3. The harness may send soft termination at a warning threshold.
4. The harness hard-kills on expiry.
5. The harness then moves the episode or job into recovery evaluation.

Heartbeat cadence and lease defaults should be tiered by worker class rather than globally fixed:

- Foreground work.
- Normal background work.
- Heavy background work such as rebuild or backfill jobs.

Exact timeout numbers should be treated as initial defaults, not architectural invariants.

### Retry policy and recovery budgets

Recovery policy should be class-based rather than global. Different work types may resume, retry, defer, abandon, or seek clarification according to side-effect risk, determinism, and policy.

Retry ceilings should be defined by work class rather than by one global retry limit. These ceilings should be treated as initial operational defaults, not architectural truths.

Recovery should consume bounded supplemental budgets rather than implicitly inheriting unlimited continuation rights from the original execution. Repeated recovery attempts should consume a separate recovery budget ceiling. Exhaustion of that ceiling should force fail-closed behavior, deferment, or human clarification.

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

- `runtime`: thin entrypoints and runtime wiring.
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

This area is settled at the implementation-design level.

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

This area is settled at the implementation-design level.

The v1 surface decision is best expressed as conversation-first, Telegram-first for v1.

The interaction model is chat-like, but the first real production channel and long-lived main surface is Telegram. Any simple local chat UI may exist for development, testing, or fallback, but it is not the primary product surface.

### Surface posture

- Blue Lagoon should begin with one primary foreground conversation surface.
- Telegram is the primary v1 ingress and egress channel.
- The core system remains channel-agnostic even though Telegram is the first dominant adapter.
- Message normalization, trigger creation, approvals, policy checks, identity handling, and rate control belong in the core harness-mediated system, not in Telegram-specific business logic.
- Broader multi-channel rollout should be deferred until it clearly improves the single-user core assistant experience.

### Telegram foreground control rules

- Accepted Telegram foreground-trigger intake must be atomic: execution start, conversation-binding reconciliation, ingress persistence, and acceptance audit either commit together or do not commit.
- Conversation rebinding is allowed, but the canonical internal conversation binding identity must be preserved across rebinds.
- If duplicate binding rows must be merged, historical ingress rows must be rewired to the canonical binding before any superseded binding row is removed.
- Live Telegram fetch failures must fail closed and emit durable audit events even when no foreground execution record is created.
- Provider-specific API-surface differences belong in provider-scoped model-gateway configuration rather than Telegram-specific runtime logic.

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

## Testing strategy

The architecture is now concrete enough that Blue Lagoon requires an explicit testing strategy as part of the implementation design.

This is especially important because a substantial portion of implementation work is expected to be carried out by an LLM agent. The testing posture therefore must not be advisory only. It must define what is required to treat the runtime as safe enough to run.

### Primary objective

The primary testing goal is production-runnability, not abstract coverage maximization.

The intended meaning of a green required test suite is:

- The runtime is expected to be production-runnable.
- Core architectural guarantees hold.
- Known catastrophic failure classes are strongly guarded against.
- Smaller behavioral deviations may still exist, but the system should not violate its hard safety envelope.

This strategy does not claim that automated testing can prove perfection or eliminate every bug. It does claim that when the required gates are green, the build should not catastrophically violate the project's core architectural and safety assumptions.

### Testing pyramid

Blue Lagoon should follow a strict testing pyramid.

The dominant base should be fast deterministic unit tests. Above that should sit component tests for subsystem boundaries. Above that should sit a smaller set of integration tests for architecture-critical flows. At the top should sit a very small smoke or system suite.

The v1 posture is:

- Unit tests as the largest layer by far.
- Component tests for real subsystem behavior with controlled dependencies.
- Integration tests only for architecture-critical end-to-end flows.
- A minimal smoke layer for release confidence.

This is the required balance for keeping the suite fast enough for LLM-driven implementation while still providing meaningful release confidence.

### Required test coverage areas

The testing strategy should explicitly require automated testing of:

- Migration safety and upgrade behavior.
- Harness sovereignty and canonical write ownership.
- Policy and approval enforcement.
- Worker isolation and permission boundaries.
- Recovery correctness and fail-closed behavior.
- Foreground and unconscious orchestration.
- Proposal, validation, and merge flows.
- Channel adapter normalization and approval-resolution paths.
- Health-critical observability and audit emission for core operations.

Catastrophic failure classes that the automated strategy is specifically intended to guard against include:

- Migration corruption or unsafe upgrade behavior.
- Worker bypass of harness-owned canonical writes.
- Unsafe permission or policy bypass.
- Invalid recovery continuation across ambiguous side effects.
- Broken startup or upgrade paths.
- Broken trigger-to-execution paths for core foreground or background flows.

### Unit-test base

Unit tests should be the largest part of the suite and should remain fast, deterministic, and local.

They should cover logic such as:

- Policy evaluation.
- Proposal validation and merge rules.
- Budgeting logic.
- Trigger routing.
- Recovery decisions.
- Action classification and idempotence handling.
- Schema-version compatibility checks.
- Normalization and parsing logic.
- Internal-state calculations.
- Small domain transforms and validators.

### Component and integration tests

Component tests should exercise one subsystem with real boundaries but controlled dependencies.

Examples include:

- Harness services against a real test database.
- Migration runner behavior against disposable PostgreSQL.
- Model gateway adapters with stubbed providers.
- Telegram adapter normalization and approval rendering with fake Telegram payloads.
- Tool policy enforcement with fake tools.
- Recovery checkpoint persistence and rehydration.

Integration tests should be fewer, slower, and focused on architecture-critical flows such as:

- Startup against a fresh database.
- Startup against an upgraded database.
- Conscious trigger to harness to worker to persisted outputs.
- Unconscious job to proposal to merge flow.
- Approval request to approval resolution to resumed execution.
- Worker crash or stall to recovery path.
- Wake signal to policy evaluation to foreground trigger.
- Bounded tool execution under policy.

### Real and fake dependencies

Unit tests should use deterministic test doubles by default.

The testing posture for dependencies is:

- Storage-facing component and integration tests must use disposable real PostgreSQL where persistence semantics matter.
- Storage-facing automated tests must provision disposable per-test databases from reviewed migrations and must not target existing operator databases.
- Model providers should normally be stubbed or faked.
- Telegram transport should normally be stubbed or faked.
- External tools and services should normally be stubbed or faked unless a specific integration path requires stronger validation.
- Real external networks or providers must not be required for the core required suites.

Manual Telegram E2E remains an operator workflow against the regular local app
configuration. Test isolation is a test-support responsibility, not a reason to
split the local runtime into a separate E2E config profile.

Mock-only testing is not acceptable for persistence, migration, recovery, or permission-boundary safety.

### Fault injection

The required strategy should include targeted fault-injection tests for architecture-critical safety paths.

These should cover cases such as:

- Worker crash.
- Worker stall or lease expiry.
- Approval expiry.
- Policy re-check failure at execution time.
- Restart during interrupted work.
- Failed or partial migration scenarios where applicable.

Blue Lagoon does not need broad chaos-style testing in v1, but it does need deliberate fault injection for the failure modes most likely to break its core guarantees.

### Testing the automated tests

Because much of the implementation will be produced by an LLM agent, Blue Lagoon should explicitly validate the effectiveness of its automated tests.

The v1 posture should include:

- Mutation-style checks or sampled mutation testing on critical modules.
- Known-bad fixtures for safety-critical paths.
- Regression-test requirements for architecture-critical bugs.
- Periodic CI jobs or equivalent validation that prove failure cases are actually detected.

This meta-validation should focus especially on:

- Policy logic.
- Merge validation.
- Migration safety checks.
- Recovery decision logic.
- Approval validation.

V1 does not need full mutation testing across the whole codebase, but it does need targeted proof that the critical tests can actually fail for the right reasons.

### Release gates

The testing strategy should define explicit required gates rather than treating all tests as operationally identical.

The required posture is:

- Per-change gates for lint or format, unit tests, and fast component tests.
- Pre-merge or mainline gates for the full component suite, core integration suite, and migration suite.
- Release gates for the smoke or system suite, upgrade-path suite, targeted fault-injection suite, and selected meta-validation checks on critical modules.

Green means the required gates for the relevant stage are green, not merely that some subset of tests passed.

### LLM-implementation rules

Because much of implementation is expected to be done by an LLM agent, the design should impose the following rules:

- No core architectural code path is considered complete without tests.
- Every new core module should ship with tests at the lowest effective layer in the pyramid.
- Every production bug in migration, recovery, approvals, permissions, or canonical writes must add a regression test.
- LLM-generated tests should prefer deterministic assertions over weak snapshot-heavy checks.
- Test additions should default to the lowest useful layer rather than prematurely adding slow end-to-end coverage.

### Testing decisions now treated as settled

- Testing objective: production-runnability rather than raw coverage.
- Pyramid posture: unit-heavy with smaller component, integration, and smoke layers.
- Persistence posture: real PostgreSQL for persistence-critical tests.
- Release meaning: green required gates imply the build is safe enough to run.
- Fault posture: targeted architectural fault injection.
- Meta-validation posture: targeted validation of the tests themselves for critical modules.
- LLM implementation posture: no core behavior is complete without tests.

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
- A production-runnability-oriented testing strategy with a unit-heavy pyramid, real-PostgreSQL persistence testing, targeted fault injection, and explicit release gates.
- A codebase with first-class boundaries for app, harness, contracts, and workers, and internal-first packaging for most other subsystems in v1.
- A controlled canonical schema-evolution model using reviewed SQL migrations and expand-contract by default.

## Settled implementation details

The following details are explicit settled parts of the implementation design.

### 1. Surface implementation details

The surface implementation details for v1 are now settled as follows.

#### Telegram production scope

- Private 1:1 Telegram chat is the only production-supported user conversation mode in v1.
- Groups, channels, and broader multi-party behavior are out of v1 scope rather than forbidden in principle.
- Telegram remains the primary production ingress and egress channel.

#### Telegram adapter boundary

The Telegram adapter remains transport-focused. It should only:

- Receive Telegram updates.
- Normalize inbound Telegram events into a canonical internal ingress contract.
- Deliver outbound messages.
- Deliver approval prompts and collect approval responses.
- Map Telegram identifiers to internal principal and conversation references.

The Telegram adapter must not own:

- Policy logic.
- Approval authority.
- Trigger semantics.
- Identity logic.
- Business workflow logic.
- Channel-specific exceptions to core safety or rate policy.

#### Message-normalization contract

The normalized Telegram ingress contract should include fields equivalent to:

- Channel kind.
- External conversation identifier.
- External user identifier.
- Internal principal reference.
- Internal conversation reference.
- External event or message identifiers.
- Event kind.
- Occurred-at timestamp.
- Text body where present.
- Reply-to reference where present.
- Attachment references with basic metadata where present.
- Command hint where applicable.
- Approval token or callback payload where applicable.
- Raw payload reference.

Reply linkage and attachment references should be explicit in the normalized contract from v1 onward. The contract should remain transport-level and should not encode Telegram-specific business semantics.

#### Approval interaction flow

Approval requests remain canonical harness objects first and Telegram renderings second.

The v1 approval flow should be:

1. The harness creates an approval request with TTL, action fingerprint, risk tier, and concise consequence summary.
2. Telegram renders the request primarily with inline approval controls.
3. Typed approval or rejection may exist as a controlled fallback using the same approval token model.
4. The response becomes an approval-resolution event.
5. The harness validates actor identity, TTL, one-shot use, and unchanged action fingerprint.
6. The harness re-checks policy before executing on approval.
7. Rejection or expiry returns control through the normal approval-resolution path for replanning, cancellation, or abandonment.

#### Notifications and local chat scope

- Telegram is the only production user-facing delivery path in v1.
- No duplicate cross-channel fanout should exist by default.
- Local chat remains a development, testing, and fallback surface only.
- Local chat must not become a second production autonomy or notification channel in v1.

#### Attachments

- Attachments are accepted in v1 as normalized references with basic metadata.
- Deeper attachment processing remains harness-mediated and policy-controlled.
- Attachment handling should not force Telegram-specific semantics upward into core business logic.

#### Minimal operator surface

- Operator diagnostics remain distinct from the main end-user conversation surface.
- V1 operator support should include a CLI status view.
- A small read-only local status page may exist if useful.
- A full browser-based admin control plane is out of v1 scope.

### 2. Migration operational conventions

The migration operational conventions for v1 are now settled as follows.

#### Migration file naming

- Reviewed migration files should use the naming convention `NNNN__short_snake_case.sql`.
- Ordering should be monotonic and explicit.
- Migration names should remain readable and review-friendly rather than tool-opaque.

#### Schema-version metadata table

V1 should maintain a `schema_migrations` table with fields equivalent to:

- `version`
- `name`
- `checksum`
- `applied_at`
- `app_version`
- `applied_by`
- `execution_ms`

This table should provide enough safety and audit support for runtime validation and human review without becoming a governance-heavy control table.

Failed migrations should not be treated as normal applied-history rows. Failure state should instead surface through audit events and failed startup or upgrade state.

#### Compatibility-window enforcement

- Runtime startup should require the database schema version to fall within the runtime's supported bounds.
- If persisted cross-boundary artifacts change shape, the new runtime may temporarily read the immediately previous persisted shape until drain, backfill, or cutover completes.
- This compatibility burden applies mainly to persisted cross-process and cross-recovery artifacts such as jobs, proposals, triggers, wake signals, approvals, and recovery records.
- Mixed-version runtime execution should remain blocked by startup gating when compatibility cannot be safely maintained.
- V1 should not attempt broad multi-version compatibility beyond what is operationally necessary for safe transition.

#### Migration review checklist

Every reviewed migration should explicitly cover at least:

- Canonical versus re-derivable classification.
- Additive versus expand-contract choice.
- Backfill plan where required.
- Validation query or equivalent validation condition.
- Expected lock or runtime impact.
- Compatibility impact on persisted cross-boundary artifacts.
- Corrective-migration path if rollout or validation fails after partial progress.

#### Re-derivation boundaries

The v1 posture should be a strict canonical core plus a re-derivable projection layer.

The following should be treated as re-derivable where practical:

- Embeddings.
- Lexical or vector projection tables.
- Retrieval or materialized search summaries.
- Graph-like retrieval support tables derived from canonical memory.
- Health rollups, caches, and similar operational projections.

The following should be treated as strictly canonical in v1:

- Episodes.
- Accepted long-term memory artifacts.
- Canonical retrieval artifacts and retrieval-layer records that define the current durable retrieval state.
- Self-model artifacts.
- Proposals.
- Merge history.
- Approval requests and resolutions.
- Wake signals and their evaluations.
- Job state.
- Recovery checkpoints and recovery state.
- Workspace artifacts.
- Scripts, script versions, and script run history.
- Audit events.

Retrieval artifacts remain a required canonical layer in v1. The intended split is that the retrieval layer itself is canonical, while concrete derived projections inside that layer should be designed to be rebuildable wherever practical.

Accepted long-term memory artifacts should remain canonical in v1 even if some portions could theoretically be reconstructed from episode history plus merge history.

## Implementation-plan entry point

Implementation planning should proceed directly from this design baseline.

## Implementation guardrails

Implementation work should preserve the following posture unless there is a strong reason to change direction:

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
- Keep recovery harness-led, checkpoint-light, proof-based, and fresh-worker based.
- Allow automatic continuation only after read-safe or provably idempotent durable execution.
- Keep codebase boundaries aligned with authority boundaries rather than premature packaging maximalism.
- Keep deployment topology simple for v1 while avoiding packaging decisions that create unnecessary future migration constraints.
- Keep Telegram as the primary v1 channel without hardwiring channel-specific assumptions into the product core.
- Use expand-contract as the default schema-evolution pattern for canonical data.
- Keep testing production-runnability-oriented rather than coverage-driven.
- Keep the test pyramid unit-heavy and fast by default.
- Require real PostgreSQL for persistence-critical automated tests.
- Require targeted fault injection and test-effectiveness checks for critical safety modules.
- Avoid premature enterprise, fleet, or multi-channel complexity unless it clearly improves the single-user v1 product.

## Design status

Blue Lagoon is now past the stage of fundamental architectural uncertainty.

The core runtime model, harness sovereignty, dual-loop isolation, self-model posture, memory direction, canonical persistence stance, migration posture, user-surface posture, orchestration direction, workspace concept, tool safety posture, provider-agnostic model gateway, recovery model, observability posture, testing posture, codebase boundary posture, and default deployment topology are coherent enough to treat the implementation design as complete.

The project is now at implementation-plan readiness. This document should be treated as the canonical implementation-design baseline unless and until it is intentionally revised.
