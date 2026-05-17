# Blue Lagoon
## Real-World Assistant Gap-Closure Design

Date: 2026-05-14  
Last Reviewed: 2026-05-15  
Status: Implemented baseline (Phases R1-R7 completed)  
Audience: Product and implementation planning

## Purpose

This document defines the next design slice required to close the highest-value
gaps between the current Blue Lagoon v1 runtime and a broadly useful
real-world assistant for daily workflows.

It is intentionally additive. It does not replace the current canonical v1
scope in `docs/REQUIREMENTS.md` or `docs/IMPLEMENTATION_DESIGN.md`. It defines
the proposed post-v1 expansion path so implementation can proceed in controlled,
testable phases.

## Source Baseline

This design is derived from:

- `PHILOSOPHY.md`
- `docs/REQUIREMENTS.md`
- `docs/LOOP_ARCHITECTURE.md`
- `docs/IMPLEMENTATION_DESIGN.md`
- `docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md`
- `docs/USER_MANUAL.md`
- `docs/internal/conscious_loop/GOVERNED_ACTIONS.md`
- `docs/internal/unconscious_loop/BACKGROUND_JOBS.md`
- `docs/internal/harness/MODEL_PROVIDERS.md`

If this document conflicts with currently approved canonical requirements, the
canonical documents win until those requirements are intentionally revised.

## Scope Posture

The current runtime already provides a strong foundation:

- harness-sovereign execution
- governed actions and approvals
- durable memory and identity flows
- scheduled foreground tasks
- bounded unconscious/background execution
- operational management CLI

The gap-closure scope focuses on what is still missing for practical daily
assistant workflows, not on reworking proven foundation architecture.

## Gap Inventory

| Gap | Current posture | Why it blocks real-world usefulness | Closure target |
|---|---|---|---|
| G1: Attachment workflow | Telegram ingress stores attachment metadata only | The assistant cannot reliably consume user files/images/documents as working context | Add governed attachment fetch, parsing, and bounded context projection |
| G2: Workflow integrations | Action surface is strong for local runtime/workspace operations but lacks first-class external workflow adapters | Core workflows (calendar, email, task sync) still require manual operator glue | Add typed harness-governed integration actions with approval and audit |
| G3: Background origination | Background jobs are enqueued by foreground request or manual admin enqueue | Routine maintenance does not start itself from thresholds/schedules | Add autonomous harness planner stage that originates due jobs deterministically |
| G4: Foreground/background model split | One configured model route is reused for both loops | No cost/latency/quality separation between user-facing and maintenance cognition | Add separate unconscious model route and policy controls |
| G5: Approval usability and visibility | Telegram callback approval exists, but lifecycle visibility and troubleshooting are still operator-centric | Real users need clearer in-channel approval status and follow-up outcomes | Add approval-status UX hardening while preserving canonical audit and CLI controls |
| G6: Archive lifecycle contract | Runtime blocks many archived mutations, but canonical docs are not explicit | Behavior is under-documented and easy to regress | Promote explicit archived-state immutability contract to canonical docs and tests |
| G7: Principal scope | v1 is one user + one primary chat | Real assistants need controlled collaborator/delegate workflows | Add bounded multi-principal support without full multi-tenant expansion |

## Design Constraints (Unchanged)

The following remain hard constraints:

- Harness remains the only canonical write owner.
- Workers remain proposal-only and bounded.
- Side effects remain governed-action mediated.
- High-risk actions remain approval-gated.
- Recovery remains fail-closed on ambiguity.
- Every new capability must have durable audit and management inspection.

## Target Capability Design

### 1. Attachment-Aware Assistant Workflows

The runtime should treat attachments as first-class governed inputs:

- Add attachment ingestion records with source, MIME type, byte size, and
  retrieval status.
- Add bounded fetch + parse pipeline owned by the harness.
- Add content extraction modes by media type (text-first in first slice).
- Store normalized extracted text as immutable artifact snapshots with
  provenance references.
- Inject only bounded summaries or excerpts into conscious context.

The assistant must never receive unbounded raw file payloads directly in model
context.

### 2. Typed Workflow Integration Surface

Blue Lagoon should add typed integration actions for daily workflows:

- calendar events and reminders
- inbound/outbound email drafting and send proposals
- task synchronization with canonical task artifacts

Integration actions must follow the same governed-action lifecycle:

1. Model proposes typed payload.
2. Harness validates capability scope and policy.
3. Approval is requested when risk tier requires it.
4. Harness executes through integration adapters.
5. Results are returned as bounded observations and audited.

No direct model credentials or direct provider API calls from workers.

### 3. Autonomous Background Planner

The background scheduler should gain a planning stage before leasing execution:

- evaluate thresholds and due periodic triggers
- construct deterministic planning requests
- deduplicate against active/planned jobs
- enqueue bounded jobs with explicit trigger kind and rationale

This closes the current "manual or foreground-delegated only" origination gap
while preserving harness control.

### 4. Split Model Routing by Loop

Add independent route configuration for foreground and unconscious loops:

- `model_gateway.foreground.*` remains user-facing route
- `model_gateway.unconscious.*` becomes maintenance route
- independent timeout, model, and reasoning policy posture

If unconscious route is missing, startup should fail closed or use an explicit
compatibility fallback controlled by config (not implicit silent reuse).

### 5. Approval Usability and Visibility

The runtime already supports Telegram approval callbacks and fallback commands.
The next design slice should harden usability and visibility:

- improve in-channel messaging for pending, expired, invalidated, and resolved approvals
- include compact status context for multi-approval situations
- keep canonical approval records identical across Telegram and CLI resolution paths
- preserve CLI as the full-fidelity operator fallback and diagnostics surface

This reduces operational friction while keeping policy enforcement and audit
behavior unchanged.

### 6. Archived-State Immutability Contract

Archived artifacts and scripts should be explicitly immutable:

- Any mutating action against `archived` entities MUST fail closed.
- Archived entities remain readable for audit/history.
- Reactivation MUST require an explicit restore/unarchive action with actor
  attribution and reason.
- Restore operations SHOULD remain approval-gated by policy.

This requirement must be promoted to canonical docs and enforced by regression
tests.

### 7. Bounded Multi-Principal Collaboration

Add a constrained principal model without jumping to enterprise multi-tenant:

- one owner principal plus allowlisted delegate principals
- per-principal conversation bindings and action policy checks
- actor attribution on every approval and side effect

Non-goal: full org RBAC, tenant isolation, or broad enterprise policy engines.

## Phased Implementation Plan

### Phase R1: Canonical Contract Alignment

Goal: Promote gap-closure scope and archived immutability contract into
canonical docs.

Exit criteria:

- Requirements/design docs include archived immutability requirements.
- Post-v1 gap-closure scope is explicit and non-contradictory.
- Documentation drift items are corrected (including stale "partial" status
  labels where implementation is already complete).

### Phase R2: Attachment Pipeline

Goal: Ship attachment fetch + parse + bounded context projection.

Exit criteria:

- Attachment ingestion tables/migrations are reviewed and applied.
- Governed actions exist for attachment inspection and controlled processing.
- Foreground and integration tests prove attachment-to-context behavior.

### Phase R3: Workflow Integrations

Goal: Add first typed real-world integrations (calendar, email, task sync).

Exit criteria:

- Typed contracts and risk tiers are implemented.
- Approval and execution flows are fully audited.
- Management CLI can inspect integration runs and failures.

### Phase R4: Autonomous Background Planning

Goal: Make background maintenance originate itself from policy-approved signals.

Exit criteria:

- Scheduler planning stage enqueues threshold/scheduled jobs deterministically.
- Deduplication and bounded-budget rules are enforced.
- Integration tests cover end-to-end originate -> execute -> merge flow.

### Phase R5: Foreground/Unconscious Route Split

Goal: Separate cognitive cost/quality profiles for user-facing and maintenance
work.

Exit criteria:

- Independent unconscious route config is live.
- Route selection is tested across both loops.
- Failure behavior on misconfiguration is fail-closed and auditable.

### Phase R6: Approval UX Hardening

Goal: Reduce approval latency for daily usage while preserving safety.

Exit criteria:

- Telegram and CLI approval flows remain semantically identical in canonical
  records.
- Duplicate/conflicting approval submissions are idempotently handled.
- In-channel status messaging clearly reports pending, resolved, expired, and
  invalidated approval outcomes.

### Phase R7: Bounded Collaboration

Goal: Support controlled delegated workflows for real-life usage.

Exit criteria:

- Principal model supports owner + delegates with policy boundaries.
- Cross-principal actions are auditable and approval-aware.
- Use-case coverage includes delegate-triggered and owner-approved flows.

## Test And Validation Requirements

Each phase must add automated coverage at the lowest effective layer:

- unit tests for validation and policy rules
- component tests for persistence and orchestration boundaries
- integration tests for architecture-critical end-to-end flows
- management CLI tests for new operator workflows

Required regression focus:

- archived entity immutability
- approval correctness and replay safety
- connector side-effect idempotency
- autonomous planner deduplication
- route-selection correctness per loop

## Migration And Rollout Posture

- Use reviewed additive-first SQL migrations.
- Gate new capabilities with explicit config defaults.
- Keep fallback paths deterministic and auditable.
- Do not remove existing CLI control paths until in-channel paths are proven.
- Roll out connector actions by risk tier from read-only to side-effecting.

## Non-Goals

This design does not include:

- full enterprise multi-tenant architecture
- broad RBAC policy engines
- distributed worker pools or broker-first topology
- browser-first heavy admin UI
- unrestricted plugin execution from the conscious loop

## Definition Of Done

This gap-closure design is complete when:

1. Phases R1-R7 are implemented with tests and docs.
2. Real-world daily workflows (message + attachment + scheduling + approval +
   integration action + follow-up) are executable without raw SQL or ad hoc
   scripts.
3. The assistant remains harness-governed, auditable, and fail-closed under
   interruption and ambiguity.
