# Conscious-Loop Capability Gap Report

## Purpose and Evidence Rules

This report traces user-visible and conscious-loop capabilities from the
canonical requirements and design documents to the current implementation. It is
an analysis artifact, not a canonical behavior specification.

Evidence precedence:

1. Canonical behavior docs: `docs/REQUIREMENTS.md`,
   `docs/LOOP_ARCHITECTURE.md`, and `docs/IMPLEMENTATION_DESIGN.md`.
2. Implementation detail: `docs/internal/conscious_loop/*.md`, Rust code,
   migrations, tests, and operator docs.
3. Historical/source context: `docs/sources/initial-requirements-draft.md`,
   used only to explain origin when useful.

Status meanings:

- `Working`: the model can use the capability through the conscious loop, the
  harness mediates it, and results are available through the expected runtime
  path.
- `Partial`: durable backend, admin/operator support, or some runtime path
  exists, but the conscious model cannot use the capability end to end or a key
  backend is blocked.
- `Missing`: the capability is required or designed but no meaningful current
  implementation path exists for the model.

## Executive Summary

The core governed-action bridge now exists: the conscious worker is told about
governed actions, can emit a `blue-lagoon-governed-actions` JSON block, and the
harness can plan, validate, execute, approval-gate, audit, and feed back
observations for several action kinds. The best-supported model-usable actions
are `run_subprocess`, `run_workspace_script`, and `web_fetch`.

The main gaps are around capabilities that have storage or admin support but no
model-facing operation:

- Workspace artifacts can be persisted, listed, and updated by service code, but
  the model cannot create, edit, or inspect notes, scratchpads, task lists, or
  runbooks through a governed action.
- Workspace scripts can be created, versioned, listed, executed, and have run
  history stored at the service layer, but the model can only run an existing
  script by UUID. It cannot create, edit/version, discover, or inspect scripts
  unless a human/operator or prior context gives it the script ID and details.
- `inspect_workspace_artifact` exists in contracts and database constraints, but
  it is deliberately not exposed to the model and is blocked at execution time.
- Scheduled foreground tasks are implemented for operator/admin creation and
  runtime firing, but the model cannot schedule reminders or recurring tasks
  through a governed action.
- Background jobs, wake signals, memory proposals, and self-model proposals have
  significant infrastructure, but conscious-loop delegation and proactive
  creation from model intent are not wired as user-facing model tools.

## Capability Matrix

| Capability | Requirement Source | Current Implementation | Model Can Use It? | Status | Evidence | Implementation Notes |
|---|---|---|---|---|---|---|
| Governed action proposal path from model output | `docs/REQUIREMENTS.md` sections 6.1, 9.1, 16.1; `docs/LOOP_ARCHITECTURE.md` sections 4.1 and 5.2; `docs/IMPLEMENTATION_DESIGN.md` "Tool safety and script governance" | The worker prompt tells the model governed actions are available. `governed_action_schema_message()` injects the JSON schema. `build_governed_action_proposals()` parses the last fenced block. `foreground_orchestration` plans proposals, executes immediate actions, sends approval prompts, and runs a follow-up turn when observations exist. | Yes | Working | `crates/workers/src/main.rs`; `crates/harness/src/foreground_orchestration.rs`; `docs/internal/conscious_loop/GOVERNED_ACTIONS.md`; `crates/harness/tests/governed_actions_integration.rs` | The bridge uses plain text plus fenced JSON, not native provider tool calls. It is intentionally proposal-only and harness-mediated. |
| `run_subprocess` | `docs/REQUIREMENTS.md` section 16; `docs/IMPLEMENTATION_DESIGN.md` "Tool safety and script governance" | Exposed in the schema, validated by `validate_capability_scope()`, risk-classified, executed by the governed-action backend, audited, and surfaced as an observation. | Yes | Working | `crates/workers/src/main.rs`; `crates/harness/src/governed_actions.rs`; `crates/harness/src/policy.rs`; `migrations/0006__workspace_and_governed_actions.sql`; `crates/harness/tests/governed_actions_component.rs`; `crates/harness/tests/governed_actions_integration.rs` | This is the strongest implemented path. Remaining future work is policy/sandbox hardening rather than basic wiring. |
| `run_workspace_script` | `docs/IMPLEMENTATION_DESIGN.md` "Workspace subsystem" and "Tool safety and script governance" | Exposed in the schema, validated, executed by resolving a workspace script/version, records `workspace_script_runs`, and returns a governed-action observation. | Yes, if the model already knows the script UUID | Partial | `crates/workers/src/main.rs`; `crates/harness/src/governed_actions.rs`; `crates/harness/src/workspace.rs`; `migrations/0006__workspace_and_governed_actions.sql`; `crates/harness/tests/governed_actions_component.rs` | Execution works, but discovery and creation are not model-accessible. Minimum later work: expose read/list/inspect script operations or inject relevant script IDs into conscious context; add governed actions for script creation/versioning if the model should author scripts. |
| `web_fetch` | `docs/REQUIREMENTS.md` section 16; design direction for tool mediation and external actions | Exposed in the schema, added to DB action-kind constraints in migration 0009, always classified as Tier 2 approval-gated, executed through `tool_execution::execute_web_fetch()`, formatted, stored, and summarized back to the model. | Yes, after approval | Working | `crates/workers/src/main.rs`; `crates/harness/src/governed_actions.rs`; `crates/harness/src/tool_execution.rs`; `crates/harness/src/fetched_content.rs`; `migrations/0009__web_fetch_action_kind.sql`; `crates/harness/tests/governed_actions_component.rs`; `crates/harness/tests/governed_actions_integration.rs` | The schema requires `network = "enabled"` and zeroed filesystem/execution scope. The model-facing preview is capped; raw body metadata is retained in execution payload. |
| `inspect_workspace_artifact` | `docs/IMPLEMENTATION_DESIGN.md` "Workspace subsystem"; `docs/internal/conscious_loop/GOVERNED_ACTIONS.md` | Exists in `GovernedActionKind`, payload types, validation, and DB constraints. Execution returns `Blocked` with summary that workspace inspection is not implemented. The internal doc explicitly says not to expose it to the agent. | No | Partial | `crates/contracts/src/lib.rs`; `crates/harness/src/governed_actions.rs`; `migrations/0006__workspace_and_governed_actions.sql`; `docs/internal/conscious_loop/GOVERNED_ACTIONS.md` | Minimum later work: implement read-only artifact lookup and result summarization, expose it in `governed_action_schema_message()`, add tests for success, missing artifact, archived artifact, and scope validation. |
| Workspace artifacts: notes, runbooks, scratchpads, task lists | `docs/IMPLEMENTATION_DESIGN.md` "Workspace subsystem" | Database supports artifact kinds `note`, `runbook`, `scratchpad`, `task_list`, and `script`. `workspace.rs` can create, update, get, and list artifacts. Management surfaces can list artifact summaries. | No | Partial | `migrations/0006__workspace_and_governed_actions.sql`; `crates/harness/src/workspace.rs`; `crates/harness/src/management.rs`; `crates/runtime/src/admin.rs`; `crates/harness/tests/governed_actions_component.rs`; `crates/harness/tests/management_component.rs` | Backend persistence exists, but there is no governed action for create/update/list/inspect artifacts and no admin create/update CLI for non-script artifacts. Minimum later work: add model-facing artifact create/update/list/inspect action kinds with bounded content limits and audit events. |
| Workspace script creation | `docs/IMPLEMENTATION_DESIGN.md` says scripts are first-class governed workspace artifacts | `workspace::create_workspace_script()` creates a script artifact plus initial version. Tests cover service persistence. Admin CLI only lists scripts; it does not create them. | No | Partial | `crates/harness/src/workspace.rs`; `crates/runtime/src/admin.rs`; `crates/harness/tests/governed_actions_component.rs`; `crates/harness/tests/management_component.rs` | Minimum later work: add a governed `create_workspace_script` action or an approval-gated admin/operator path; ensure language allowlist, size limits, provenance, and initial version metadata. |
| Workspace script editing/versioning | Same as above | `workspace::append_workspace_script_version()` exists and tests cover version append. No model schema action and no admin CLI command exposes it. | No | Partial | `crates/harness/src/workspace.rs`; `crates/harness/tests/governed_actions_component.rs`; `crates/runtime/src/admin.rs` | Minimum later work: add governed `append_workspace_script_version` with script ID, expected latest version or content hash, change summary, and conflict handling. |
| Workspace script listing/discovery | `docs/IMPLEMENTATION_DESIGN.md` "Workspace subsystem" | Service and management functions list scripts. Admin CLI supports `runtime admin workspace scripts list`. The conscious model is not given a script catalog by default. | No, except from prior context or user-provided ID | Partial | `crates/harness/src/workspace.rs`; `crates/harness/src/management.rs`; `crates/runtime/src/admin.rs`; `docs/USER_MANUAL.md`; `crates/harness/tests/management_component.rs` | Minimum later work: include selected workspace script summaries in conscious context or add a read-only governed list/search action. |
| Workspace script run history inspection | `docs/IMPLEMENTATION_DESIGN.md` "Workspace subsystem" | Script run records are persisted and listable through management and admin CLI. Model-triggered script execution gets its own immediate observation, but the model cannot query historical runs. | No | Partial | `crates/harness/src/workspace.rs`; `crates/harness/src/management.rs`; `crates/runtime/src/admin.rs`; `crates/harness/tests/management_component.rs` | Minimum later work: expose read-only run history by script ID and recent run summaries either in context assembly or a governed inspection action. |
| Scheduled foreground tasks/reminders | `docs/REQUIREMENTS.md` section 8.1; `docs/LOOP_ARCHITECTURE.md` section 3.1; `docs/IMPLEMENTATION_DESIGN.md` user-facing surface and scheduling posture | `scheduled_foreground_tasks` table exists. Service code can upsert/list/claim/complete/recover tasks. Runtime executes due tasks. Admin CLI supports `foreground schedules list/show/upsert`. | No | Partial | `migrations/0008__scheduled_foreground_tasks.sql`; `crates/harness/src/scheduled_foreground.rs`; `crates/harness/src/runtime.rs`; `crates/harness/src/management.rs`; `crates/runtime/src/admin.rs`; `docs/USER_MANUAL.md`; `crates/harness/tests/foreground_integration.rs`; `crates/harness/tests/management_component.rs` | Runtime firing works, but model-created reminders are missing. Minimum later work: add a governed schedule-create/update action, probably approval-gated, with cadence, due time, conversation binding, and user-visible confirmation. |
| Approval-gated execution and approval resolution follow-up | `docs/REQUIREMENTS.md` sections 8.1, 16, 17; `docs/IMPLEMENTATION_DESIGN.md` approval interaction flow | Tiered policy creates approval requests, Telegram prompt delivery exists, callback and command fallback resolve approvals, linked actions execute after approval, and follow-up model calls receive observations. | Yes | Working | `crates/harness/src/approval.rs`; `crates/harness/src/foreground_orchestration.rs`; `crates/harness/src/telegram.rs`; `crates/runtime/src/admin.rs`; `crates/harness/tests/governed_actions_component.rs`; `crates/harness/tests/governed_actions_integration.rs`; `crates/harness/tests/foreground_integration.rs` | UC catalog previously called out a two-run gap, but current integration tests cover approval resolution executing linked governed actions and callback/command routing. |
| Background job delegation from conscious loop | `docs/REQUIREMENTS.md` sections 6.1, 8.2, 9.1; `docs/LOOP_ARCHITECTURE.md` sections 3.2, 4.1, 5.2 | Background jobs, job runs, management enqueue/run-next, and unconscious worker structured outputs exist. The conscious result type can carry candidate proposals, but the current governed-action schema does not expose a model action for foreground delegation to background work. | Not as a direct model tool | Partial | `crates/harness/src/background.rs`; `crates/harness/src/background_planning.rs`; `crates/harness/src/background_execution.rs`; `crates/harness/src/management.rs`; `crates/runtime/src/admin.rs`; `crates/workers/src/main.rs`; `crates/harness/tests/unconscious_component.rs`; `crates/harness/tests/unconscious_integration.rs` | Minimum later work: define and expose a conscious-loop background-job request contract, validate job kind/scope, persist it through harness scheduling, and return acceptance or rejection to the model. |
| Wake signals and proactive behavior | `docs/REQUIREMENTS.md` section 15; `docs/LOOP_ARCHITECTURE.md` sections 3.1, 4.2, 5.2 | Wake signal tables and management summaries exist; unconscious outputs can include wake signals. Scheduled foreground tasks provide one proactive path. End-to-end conversion of model/background wake signals into policy-approved conscious triggers is not fully model-accessible as a user-facing capability. | No direct model control | Partial | `crates/harness/src/background.rs`; `crates/harness/src/management.rs`; `crates/runtime/src/admin.rs`; `docs/USE_CASE_CATALOG.md`; `crates/harness/tests/management_component.rs` | Minimum later work: complete policy evaluation from pending wake signal to `ApprovedWakeSignal` foreground trigger, add throttling/rate-limit assertions, and document operator controls. |
| Self-model and memory proposal paths relevant to action selection | `docs/REQUIREMENTS.md` sections 11, 13, 14; `docs/IMPLEMENTATION_DESIGN.md` self-model and memory sections | Self-model seed/context injection exists. Workers can emit candidate proposals, and harness proposal evaluation/merge infrastructure exists for canonical artifacts. This supports action-relevant identity and memory, but it is separate from external tool/workspace capability wiring. | Indirectly | Partial | `crates/harness/src/context.rs`; `crates/harness/src/self_model.rs`; `crates/harness/src/proposal.rs`; `crates/workers/src/main.rs`; `docs/internal/conscious_loop/CONTEXT_ASSEMBLY.md`; `crates/harness/tests/continuity_component.rs`; `crates/harness/tests/foreground_integration.rs` | Minimum later work is not a new tool, but better context surfacing: include relevant workspace/script/schedule affordances in the context assembly so action selection can use existing durable state. |

## Cross-Cutting Findings

### Model Exposure Is the Main Missing Layer

Several subsystems are implemented below the model boundary:

- Workspace artifact and script tables exist.
- Service functions create, update, list, version, and record runs.
- Admin surfaces list workspace artifacts, scripts, and runs.
- Scheduled foreground task storage and runtime firing exist.

However, the conscious model only sees the governed-action schema in
`governed_action_schema_message()`, which currently exposes:

- `run_subprocess`
- `run_workspace_script`
- `web_fetch`

That schema does not expose artifact creation, artifact editing, artifact
inspection, script creation, script versioning, script discovery, run-history
inspection, or schedule creation. As a result, backend completeness should not
be confused with model usability.

### Existing Script Execution Depends on Prior Knowledge

`run_workspace_script` is functional only when the model already knows a valid
`script_id`, and optionally a `script_version_id`. Current admin and management
surfaces can list scripts for an operator, but that list is not automatically
available in conscious context. A user can still paste a UUID into chat, but
that is not the same as a self-service assistant capability.

### Workspace Inspection Is Explicitly Blocked

`inspect_workspace_artifact` is present in contracts and database constraints,
but `execute_governed_action()` blocks it with a fixed not-implemented summary.
The internal governed-actions doc also says not to expose it to the agent. This
is a partial implementation, not a missing type definition.

### Scheduled Tasks Work as Operator Infrastructure

The scheduler can run due foreground tasks through the runtime, and admin CLI
can upsert/list/show scheduled tasks. The missing piece is a governed,
model-proposed operation for "remind me later", "schedule this report", or
"create a recurring check-in" from conversation.

### Approval Flow Is Largely Wired

Approval creation, rendering, callback/command resolution, fingerprint checks,
execution after approval, and follow-up observation handling are wired well
enough to classify approval-gated execution as working. Remaining work belongs
to additional action kinds and policy depth rather than basic approval plumbing.

## Minimum Follow-Up Work by Gap

1. Add read-only workspace inspection.
   - Implement `inspect_workspace_artifact` execution.
   - Expose it in the governed-action schema.
   - Return concise artifact summaries/content previews as observations.
   - Test success, missing ID, archived artifacts, and invalid scope.

2. Add model-accessible workspace artifact mutation.
   - Add governed actions for creating and updating non-script artifacts.
   - Preserve artifact kind validation and content size limits from
     `workspace.rs`.
   - Classify writes by risk tier and route sensitive writes through approval.

3. Add model-accessible script authoring.
   - Add governed actions for creating scripts and appending script versions.
   - Require language, content, title, change summary, and expected version/hash.
   - Add tests for script provenance, version conflicts, size limits, and run
     after creation.

4. Add model-accessible script discovery and run-history reads.
   - Either inject relevant script summaries into conscious context or add
     read-only list/search actions.
   - Add run-history inspection by script ID with bounded output.

5. Add model-accessible scheduled task creation.
   - Add a governed action for schedule upsert/update.
   - Treat creation/update as approval-gated unless policy explicitly allows
     low-risk reminders.
   - Reuse `scheduled_foreground::upsert_task()` and management validation.
   - Return confirmation with task key, next due time, cadence, and status.

6. Complete conscious-loop background delegation.
   - Define a model-visible request shape for background jobs.
   - Validate job kind and scoped inputs through the harness.
   - Persist accepted requests into background job scheduling and return a
     harness observation.

7. Close the wake-signal to foreground-trigger loop.
   - Ensure policy-approved wake signals can reliably become
     `ApprovedWakeSignal` foreground triggers.
   - Add tests for throttle/defer/drop decisions and operator visibility.

## Verification Notes

This report intentionally does not modify code, migrations, canonical
requirements, or operator docs. The existing untracked `interaction_log_2.txt`
was left untouched.
