# Runtime Workflow Reliability Remediation Plan

## Metadata

- Plan Status: COMPLETED
- Created: 2026-05-07
- Last Updated: 2026-05-07
- Owner: Coding agent
- Approval: APPROVED

## Status Legend

- Plan Status values: DRAFT, QUESTIONS PENDING, READY FOR APPROVAL, APPROVED, IN PROGRESS, COMPLETED, BLOCKED
- Task/Milestone Status values: TO BE DONE, IN PROGRESS, COMPLETED, BLOCKED, SKIPPED

## Goal

Make foreground workflows reliable enough to iterate with Richard without silent stalls, misleading responses, or repeated infrastructure failures. The end state is that common workflows either complete, ask for approval, or fail with precise user-facing and agent-visible diagnostics that identify the failed subsystem, failed boundary, retry safety, and next operator action.

## Scope

- Fix the worker subprocess protocol boundary so broken pipes and early worker exits preserve stderr, exit status, protocol phase, and model-call context.
- Fix scheduled foreground task semantics so one-shot reminders and recurring scheduled foreground tasks are represented and validated correctly.
- Fix trace classification so known workflow failures are not reported as `unknown_failure`.
- Fix governed-action proposal validation feedback so malformed proposals identify the missing or invalid fields without requiring raw trace spelunking.
- Verify that agent-visible diagnostic actions expose the same read-only information an operator needs for first-line troubleshooting.
- Verify that context assembly gives Richard actual relevant continuity, identity, runtime, and action-observation information rather than opaque IDs or fabricated-looking summaries.
- Add focused regression tests for the latest failures shown in `logs.txt`.
- Update internal documentation when behavior, defaults, or trace semantics change.

## Non-Goals

- Do not add unrestricted shell or database access to Richard.
- Do not bypass approval policy for tiered governed actions.
- Do not replace the worker subprocess architecture in this plan; harden the existing boundary first.
- Do not manually edit operator database rows as the primary fix.
- Do not redesign long-term memory, identity, or retrieval policy beyond correcting clearly broken context surfaces.
- Do not suppress user-facing failure notices; failures must become more actionable, not quieter.

## Current Findings

- Latest scheduled-message test in `logs.txt` created and approved `upsert_scheduled_foreground_task`, then failed at the due time with trace `019e02d5-7e01-7f32-b431-e3cdfcda001f`.
- `admin trace explain` for `019e02d5-7e01-7f32-b431-e3cdfcda001f` reports `worker_protocol_failure` with likely cause `failed to write worker protocol line: Broken pipe (os error 32)`.
- The trace graph shows scheduled foreground staging, context assembly, failed foreground execution, persisted assistant failure notice, and scheduled task failure audit. That means the user got feedback, but the trace still does not expose the worker child stderr or exit reason.
- `crates/harness/src/worker.rs` returns immediately when writing the model-call response to worker stdin fails. In that path it does not wait for the child, read stderr, or attach a protocol phase. This makes the actual worker failure opaque.
- Recent failed traces `019dff98-e14c-7b00-88be-530bc0653f42` and `019dff9a-1590-7f23-abff-bcfbf8fdea12` both reached a successful model call but failed with `scheduled foreground cadence_seconds must be greater than zero`. That indicates one-shot scheduling is being proposed or normalized into an invalid recurring-task shape.
- A newer scheduled foreground trace, `019e030c-d97f-78f3-a116-a9c7842600b2`, failed with the same broken-pipe worker protocol cause as `019e02d5-7e01-7f32-b431-e3cdfcda001f`.
- `admin foreground schedules list` shows the user-requested one-shot task `oneoff_1625_vienna_20260507` is still `active`, has `cadence=86400s`, and was rescheduled after failure to `2026-05-07 16:27:19 UTC`. This means one-shot tasks can become recurring retry sources after failure instead of becoming completed, disabled, or terminally failed.
- Long-running 5-minute failure traces such as `019dff20-02f6-7c01-a67c-0a4470a8dd4e` and `019dff77-7856-7673-8b0c-fb083587a859` failed with `malformed_action_proposal` because a read-only list payload was missing `limit`. The current code has bounded defaults for read-only limits, but implementation must prove the full parser/prompt/harness path uses those defaults and does not regress.
- Trace `019dff7c-1d00-7382-9eaa-9afda0a63f61` failed because `run_diagnostic` existed in Rust but not in persisted DB action-kind constraints before migration 0014. The migration exists now, but the plan must preserve this as a classification and migration-drift regression case.
- Trace classification still reports several known errors as `unknown_failure`, including scheduled foreground validation failures and worker protocol failures.
- `admin workspace artifacts list` shows task-list artifacts exist, so earlier task-list failures are not simply missing data; the issue is likely action proposal/observation surfacing or worker response handling.
- Richard's diagnostic reply now improves over earlier behavior, but it still says he lacks harness logs and operator state even though read-only diagnostics should make some of that available through governed actions.
- The current operator DB reports no pending foreground conversations, so the previous stale-processing loop is not currently active. The remaining problem is broader workflow reliability.

## Failure Matrix

| Boundary | Representative trace | Last successful step | First failing step | Side effects | Root signal | Remediation target |
| --- | --- | --- | --- | --- | --- | --- |
| Worker protocol | `019e02d5-7e01-7f32-b431-e3cdfcda001f`, `019e030c-d97f-78f3-a116-a9c7842600b2` | `foreground_context_assembled` | `foreground_execution_failed` | Failure notice and scheduled-task failure audit | `failed to write worker protocol line: Broken pipe (os error 32)` | Worker protocol phase tracking, child stderr/exit capture, trace classification, actionable failure notice |
| Scheduled foreground semantics | `019dff98-e14c-7b00-88be-530bc0653f42`, `019dff9a-1590-7f23-abff-bcfbf8fdea12` | `model_call:succeeded` | `foreground_execution_failed` | No governed action executed | `scheduled foreground cadence_seconds must be greater than zero` | One-shot schedule representation and pre-approval payload validation |
| Scheduled one-shot retry loop | Live row `oneoff_1625_vienna_20260507` | Scheduled task created and approved | Due execution failed | Task remained `active` and was rescheduled | One-shot stored as recurring `cadence=86400s` and retried after failure | Terminal one-shot completion/failure semantics and legacy row reconciliation |
| Governed-action parser | `019dff20-02f6-7c01-a67c-0a4470a8dd4e`, `019dff77-7856-7673-8b0c-fb083587a859` | `model_call:succeeded` | `foreground_execution_failed` | No governed action executed | `invalid governed-action proposal block: missing field limit` | Bounded defaults at actual parser boundary and prompt/schema tests |
| DB action-kind constraint drift | `019dff7c-1d00-7382-9eaa-9afda0a63f61` | `model_call:succeeded` | `foreground_execution_failed` | Failure notice only | `governed_action_executions_action_kind_check` rejected `run_diagnostic` | Forward migration regression and trace classification for schema drift |
| Model gateway transport | `019dff80-b31a-7b70-94a8-448561965bfb` | `foreground_context_assembled` | `model_call:failed` | Failure notice only | Provider request timeout/transport error | Trace classification and concise retry guidance |
| Action observation re-entry | User-visible raw `json {"governed-action": ...}` from earlier logs | Model generated action-shaped text | User received raw action JSON | No useful answer | Action block/wrapper mismatch and missing observation loop proof | Prompt/parser alignment and final-answer-after-observation regression |
| Context assembly | Earlier user reports of useless memory IDs, broken identity context, fabricated-looking runtime state, missing timestamps/authors | Context assembled | User-visible reasoning used weak context | None | Context projection quality uncertain | Fixture-backed assertions for real memory, identity, runtime, timestamps, authors, and observations |

## Assumptions

- The live runtime uses the same code and migrations as this repository after deployment or restart.
- The operator database is allowed to receive reviewed forward migrations, but implementation must not rely on manual SQL patching.
- Richard should use only harness-mediated read-only diagnostic capabilities for troubleshooting.
- User-facing Telegram responses should include enough detail to continue constructively without exposing secrets or raw internal payloads.
- Tests that use PostgreSQL must use the repository disposable database fixture pattern.

## Open Questions

- None.

## Milestones

### Milestone 1: Failure Taxonomy And Evidence

- Status: COMPLETED
- Purpose: Establish a concrete failure map before changing behavior so the implementation targets root causes rather than symptoms.
- Exit Criteria: Each latest failure class has a corresponding trace evidence note, affected module list, and expected remediation target.

#### Task 1.1: Build A Failure Matrix

- Status: COMPLETED
- Objective: Produce a checked failure matrix covering the latest user-visible failures.
- Steps:
  1. Use `logs.txt`, `admin trace recent`, `admin trace explain`, `admin actions list`, `admin diagnostics list`, and `admin workspace artifacts list` to enumerate recent failures.
  2. Group failures by boundary: model gateway, worker protocol, scheduled foreground validation, governed-action parsing, action execution, context assembly, delivery, and recovery.
  3. Record representative trace IDs, first failing step, last successful step, user-visible message, and whether side effects occurred.
- Validation: Completed. The `Failure Matrix` section above records representative traces, failing boundaries, root signals, side-effect posture, and remediation targets.
- Notes: Do not include secrets, raw Telegram payloads, or full model prompts in the document.

#### Task 1.2: Identify Trace Classification Gaps

- Status: COMPLETED
- Objective: Map known direct error strings to stable failure classes.
- Steps:
  1. Inspect `crates/harness/src/management.rs` trace explanation classification.
  2. Inspect `crates/harness/src/foreground_orchestration.rs` failure-kind classification.
  3. Compare current classification with direct facts from the representative traces.
  4. List missing classifications for worker protocol broken pipe, scheduled foreground validation, action proposal validation, DB constraint drift, delivery failure, and model gateway timeout.
- Validation: Completed. Worker protocol broken pipes, scheduled foreground validation failures, malformed governed-action proposals, persistence/constraint drift, and provider transport failures have stable classification targets in `crates/harness/src/foreground_orchestration.rs` and `crates/harness/src/management.rs`.
- Notes: Classification was verified manually with representative traces and through unit/component/admin tests.

### Milestone 2: Worker Protocol Hardening

- Status: COMPLETED
- Purpose: Ensure worker failures are never opaque at the harness boundary.
- Exit Criteria: When a worker exits early, panics, emits malformed protocol, or closes stdin before the harness writes the model response, the trace and user-facing failure include protocol phase, child exit status, stderr excerpt, retry safety, and side-effect status.

#### Task 2.1: Add Worker Protocol Phase Tracking

- Status: COMPLETED
- Objective: Every worker protocol error identifies where the failure happened.
- Steps:
  1. Introduce a small internal enum for protocol phases such as `spawn`, `write_initial_request`, `read_model_request`, `execute_model_call`, `write_model_response`, `read_final_response`, `wait_child`, and `read_stderr`.
  2. Attach the phase to errors returned by `launch_conscious_worker_with_timeout` and `launch_unconscious_worker_with_timeout`.
  3. Preserve existing public contracts unless a contract change is needed for trace diagnostics.
- Validation: Completed. `conscious_worker_protocol_failure_includes_phase_exit_and_stderr` asserts the synthetic failure includes `worker_protocol_phase=write_model_response`.
- Notes: Affected file: `crates/harness/src/worker.rs`.

#### Task 2.2: Always Collect Worker Exit Context On Protocol Failure

- Status: COMPLETED
- Objective: Broken pipes and early exits include child stderr and exit status when available.
- Steps:
  1. Refactor conscious and unconscious worker launchers so protocol errors do not immediately return before child cleanup.
  2. On any protocol error, close stdin, wait for the child with a bounded timeout, read stderr, and attach an excerpt to the error.
  3. Avoid deadlocks by preserving the existing timeout envelope and killing the worker if cleanup exceeds the bound.
  4. Keep stdout parsing strict; do not accept malformed protocol as success.
- Validation: Completed. The worker-protocol regression asserts phase, child exit status, and stderr excerpt are retained on early worker exit.
- Notes: This is the highest priority task because it turns the current broken pipe into a diagnosable failure.

#### Task 2.3: Add Worker Crash Regression Entrypoints

- Status: COMPLETED
- Objective: Tests can deterministically simulate the protocol failures seen in production.
- Steps:
  1. Add hidden worker test subcommands for early exit after first protocol message and malformed final response if existing test workers are insufficient.
  2. Keep hidden subcommands unavailable from normal operator workflows.
  3. Cover both conscious and unconscious worker paths where practical.
- Validation: Completed. `crates/workers/src/main.rs` now has a hidden `exit-after-model-request-worker` entrypoint exercised by the foreground component worker-protocol regression.
- Notes: Affected files may include `crates/workers/src/main.rs` and harness worker tests.

### Milestone 3: Scheduled Foreground Semantics

- Status: COMPLETED
- Purpose: Make scheduled messages work as users expect and prevent invalid scheduled-task proposals from entering the execution path.
- Exit Criteria: A user can schedule a one-time foreground message, approve it, receive it at due time, and the task is completed, disabled, or terminally failed without recurring invalid cadence or cooldown retry failures.

#### Task 3.1: Define One-Shot Versus Recurring Scheduled Tasks

- Status: COMPLETED
- Objective: Scheduled foreground task data model and action payload distinguish one-shot reminders from recurring tasks.
- Steps:
  1. Inspect `UpsertScheduledForegroundTaskAction`, `scheduled_foreground_tasks`, and `scheduled_foreground.rs` validation.
  2. Choose the smallest compatible representation for one-shot tasks: nullable cadence, explicit schedule kind, or disable-after-run semantics.
  3. If a schema change is needed, add a forward migration rather than editing applied migrations.
  4. Define outcome transitions for one-shot success, one-shot failure, recurring success, recurring failure, manual pause, and disabled tasks.
  5. Preserve recurring scheduled task behavior.
- Validation: Completed. One-shot tasks use `oneoff_` or `one_shot_` task-key prefixes with `cadence_seconds: 0`; recurring tasks still require positive cadence. No migration was needed because the harness stores a bounded placeholder cadence internally.
- Notes: The observed failure `cadence_seconds must be greater than zero` is eliminated for valid one-shot proposals while remaining enforced for invalid recurring proposals.

#### Task 3.2: Validate Scheduled Action Payloads Before Approval

- Status: COMPLETED
- Objective: Invalid scheduled foreground proposals fail before user approval.
- Steps:
  1. Move or duplicate scheduled foreground validation into governed-action proposal validation before approval request creation.
  2. Ensure model-proposed one-shot schedules are normalized into the supported representation.
  3. Return a precise malformed proposal error if required schedule fields are absent.
- Validation: Completed. Governed-action validation rejects `cadence_seconds: 0` unless the task key uses an approved one-shot prefix, before execution. The full governed-action component and integration suites pass.
- Notes: Affected files likely include `crates/harness/src/governed_actions.rs`, `crates/harness/src/approval.rs`, and `crates/harness/src/scheduled_foreground.rs`.

#### Task 3.3: Add Scheduled Foreground End-To-End Regression

- Status: COMPLETED
- Objective: The latest scheduled-message workflow is covered by a deterministic test.
- Steps:
  1. Create or extend a PostgreSQL-backed integration test that proposes, approves, executes, and runs a due one-shot scheduled foreground task.
  2. Use a fake model transport and fake Telegram delivery.
  3. Assert task outcome, ingress status, episode messages, audit events, and trace graph links.
- Validation: Completed. `scheduled_foreground_runtime_executes_one_shot_task_and_disables_after_success` covers due one-shot foreground execution through worker and fake Telegram delivery; `cargo test -p harness --test foreground_integration -- --nocapture` and `cargo test -p harness --test governed_actions_integration -- --nocapture` also pass.
- Notes: The test must not use the live operator database or Telegram network.

#### Task 3.4: Stop Misclassified One-Shot Retry Loops

- Status: COMPLETED
- Objective: Already-created and newly-created one-shot tasks cannot repeatedly fire after failure.
- Steps:
  1. Add scheduler logic that disables or terminally marks one-shot tasks after their due execution reaches a terminal failure notice path.
  2. Ensure failure cooldown does not convert one-shot tasks into repeated hourly or daily retries unless the user explicitly requested retrying.
  3. Add a migration, cleanup command, or harness-owned reconciliation path if existing rows need to be normalized from legacy recurring shape into one-shot shape.
  4. Add audit events for automatic one-shot disablement or terminal failure so traceability remains complete.
- Validation: Completed. `scheduled_foreground_one_shot_failure_disables_task_and_stops_retry` asserts a failed `oneoff_*` task becomes `Disabled` and is not selected again by the due-task scanner.
- Notes: This directly covers the live `oneoff_1625_vienna_20260507` row that remained active after failure.

### Milestone 4: Governed Action And Context Reliability

- Status: COMPLETED
- Purpose: Ensure Richard can propose actions, receive observations, and inspect actual context without guessing or fabricating.
- Exit Criteria: Task-list lookup, diagnostics, and follow-up action loops are covered by tests showing proposal, execution, observation injection, and final user response.

#### Task 4.1: Audit Governed Action Schema Against Worker Prompt

- Status: COMPLETED
- Objective: Worker prompt examples exactly match contract types and harness parser expectations.
- Steps:
  1. Compare governed-action schema in `crates/workers/src/main.rs` with `contracts::GovernedActionPayload`.
  2. Compare parser behavior in harness governed-action code with the examples shown to Richard.
  3. Add tests for all prompt examples, including `list_workspace_artifacts`, `run_diagnostic`, `upsert_scheduled_foreground_task`, and follow-up action proposals.
  4. Verify read-only list and diagnostic payloads get bounded default limits when `limit` is omitted.
  5. Remove or correct examples that can parse but fail later validation.
- Validation: Completed. Worker tests cover bare-action rejection, tool-call wrapper rejection, governed-action block extraction, and defaulting omitted read-only list limits; contracts tests cover bounded defaults for list and diagnostic payloads.
- Notes: This addresses the recurring malformed-action and task-list workflow failures, including `missing field limit`.

#### Task 4.2: Verify Action Observation Re-Entry

- Status: COMPLETED
- Objective: After a governed action executes, Richard receives a clear observation and can produce the final answer in the same user-visible workflow.
- Steps:
  1. Inspect the foreground governed-action continuation loop and max-actions-per-turn handling.
  2. Add a regression where Richard proposes `list_workspace_artifacts`, receives the artifact list observation, then answers with the task-list summary.
  3. Assert the final Telegram response is not raw JSON and does not ask the user to retry in the next turn.
- Validation: Completed. `foreground_orchestration_executes_immediate_governed_action_and_runs_follow_up_turn` and related governed-action integration tests verify observation re-entry and final user-facing text instead of raw action JSON.
- Notes: This is the direct test for the earlier `json {"governed-action": ...}` user-visible failure.

#### Task 4.3: Audit Context Assembly For Real Content

- Status: COMPLETED
- Objective: Richard receives actual useful identity, memory, runtime, and conversation context with timestamps and authors.
- Steps:
  1. Inspect context assembly for recent history, retrieved memories, self-model snapshot, runtime/internal state, and governed-action observations.
  2. Add fixture data with known memory artifact content, identity fields, timestamps, and author roles.
  3. Assert the worker model input contains the content and metadata, not only opaque IDs.
  4. Remove any fabricated runtime-state text or clearly label derived health summaries as harness-derived.
- Validation: Completed. Worker and foreground component tests verify author/time labels, retrieved context content-first summaries, derived runtime metrics marked as estimates, identity filtering, complete identity projection, and governed-action observation follow-up guidance.
- Notes: Affected files likely include `crates/harness/src/foreground_orchestration.rs`, continuity retrieval modules, `crates/workers/src/main.rs`, and internal docs.

### Milestone 5: Trace And Diagnostic Usability

- Status: COMPLETED
- Purpose: Make both operator CLI diagnostics and Richard's read-only diagnostic action useful for first-line troubleshooting.
- Exit Criteria: A failed trace explanation gives an actionable class, likely cause, next command, retry safety, and relevant child/model/action details without raw SQL.

#### Task 5.1: Expand Trace Explain Classification

- Status: COMPLETED
- Objective: Known failures are classified by subsystem instead of `unknown_failure`.
- Steps:
  1. Add classification rules for worker protocol phases, scheduled foreground validation, action proposal validation, database constraint drift, delivery failure, model gateway transport timeout, and scheduled one-shot retry-loop prevention.
  2. Ensure `first_failing_step`, `last_successful_step`, `side_effects`, `user_reply`, and `retry_safety` remain accurate.
  3. Add regression tests for representative trace records.
- Validation: Completed. `cargo test -p runtime --test admin_cli -- --nocapture`, `cargo test -p harness --test management_component -- --nocapture`, and live `admin trace explain --trace-id 019e030c-d97f-78f3-a116-a9c7842600b2` pass and classify the representative trace as `worker_protocol_failure`.
- Notes: Affected files likely include `crates/harness/src/management.rs` and runtime CLI formatters.

#### Task 5.2: Align Run-Diagnostic Output With Admin Trace Explain

- Status: COMPLETED
- Objective: Richard's `run_diagnostic` action exposes the same safe summary an operator would see.
- Steps:
  1. Inspect `run_diagnostic` execution in governed-action handling.
  2. Ensure diagnostic payloads include trace verdict, failure class, likely cause, retry safety, side effects, and suggested next steps.
  3. Ensure diagnostic output excludes secrets and raw SQL payloads.
  4. Add tests for a worker protocol trace and a scheduled validation trace.
- Validation: Completed. `run_diagnostic` remains harness-native and read-only through `execute_run_diagnostic_action()`, and the governed-actions component/integration plus management/admin suites verify diagnostic execution and trace explanation output.
- Notes: This keeps troubleshooting harness-mediated and read-only.

#### Task 5.3: Improve User-Facing Failure Notices

- Status: COMPLETED
- Objective: Telegram failure messages tell the user what failed and what to do next without implying the agent can fix unavailable infrastructure.
- Steps:
  1. Update failure notice rendering for known classes.
  2. Include trace ID, failure kind, short subsystem, and one safe next step.
  3. Avoid generic `Send another message to continue` when retry is not sufficient.
  4. Preserve concise notices for transient model gateway failures.
- Validation: Completed. Foreground orchestration unit tests assert notice text and classification for malformed action proposals, worker protocol failures, and scheduled foreground validation failures; model gateway failures preserve the existing transient retry notice.
- Notes: Do not leak sensitive provider URLs beyond existing safe summaries unless already documented as acceptable.

### Milestone 6: Documentation, Cleanup, And Final Verification

- Status: COMPLETED
- Purpose: Keep implementation traceable, remove temporary artifacts, and prove the integrated workflow is stable.
- Exit Criteria: Documentation matches code, no temporary files remain, and all relevant test gates pass.

#### Task 6.1: Update Internal Documentation

- Status: COMPLETED
- Objective: Internal docs reflect the final worker, scheduled foreground, context, and diagnostic behavior.
- Steps:
  1. Update `docs/internal/conscious_loop/CONTEXT_ASSEMBLY.md` for context content and failure notice behavior.
  2. Update `docs/internal/conscious_loop/GOVERNED_ACTIONS.md` for action observation and diagnostic capabilities.
  3. Add or update a harness internal doc section for worker protocol phases and trace diagnostics.
  4. Verify line references still resolve and update verified dates.
- Validation: Completed. Updated `docs/internal/conscious_loop/CONTEXT_ASSEMBLY.md`, `docs/internal/conscious_loop/GOVERNED_ACTIONS.md`, `docs/internal/harness/TRACE_EXPLORER.md`, and `docs/internal/harness/TOOL_IMPLEMENTATION.md`; refreshed affected source references and verified the changes do not contradict canonical docs.
- Notes: Internal docs must not contradict canonical docs.

#### Task 6.2: Check Against Philosophy And Requirements

- Status: COMPLETED
- Objective: Confirm the remediation preserves repository principles.
- Steps:
  1. Re-read `PHILOSOPHY.md`, `docs/REQUIREMENTS.md`, and `docs/IMPLEMENTATION_DESIGN.md`.
  2. Verify the implementation remains harness-mediated, auditable, bounded, and agent-safe.
  3. Fix any deviation before final verification.
- Validation: Completed. See the compliance note in `Execution Notes`.
- Notes: Any deviation is a defect, not a follow-up.

#### Task 6.3: Cleanup Intermediate Artifacts

- Status: COMPLETED
- Objective: Remove artifacts created only to support implementation.
- Steps:
  1. Inspect the worktree for scratch scripts, temporary fixtures, generated logs, obsolete plan fragments, and local-only outputs.
  2. Remove only artifacts that are not part of the intended final repository state.
  3. Preserve this plan, maintainable tests, migrations, and internal docs.
- Validation: Completed. `cmd.exe /c git status --short` shows intended code/docs/plan changes plus user-provided `logs.txt`; `cmd.exe /c git diff --check` reports no whitespace errors.
- Notes: Do not edit or delete `logs.txt`; it is user-provided evidence and currently modified outside the implementation scope.

#### Task 6.4: Final Verification

- Status: COMPLETED
- Objective: Validate the complete integrated remediation.
- Steps:
  1. Run formatting, compilation, linting, and relevant component/integration tests.
  2. Run a manual local smoke sequence against disposable or controlled runtime state for hello, task-list lookup, one-shot scheduled message, diagnostic of failed trace, and approval flow.
  3. Inspect resulting traces for correct classification and traceability.
  4. Record any skipped checks with a concrete reason.
- Validation: Minimum expected commands:
  - `cargo fmt --all --check`
  - `cargo check --workspace`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace --lib -- --nocapture --test-threads=1`
  - `cargo test -p harness --test foreground_component -- --nocapture`
  - `cargo test -p harness --test foreground_integration -- --nocapture`
  - `cargo test -p harness --test governed_actions_component -- --nocapture`
  - `cargo test -p harness --test governed_actions_integration -- --nocapture`
  - `cargo test -p harness --test management_component -- --nocapture`
  - `cargo test -p runtime --test admin_cli -- --nocapture`
  - `cargo run -p runtime -- admin schema status`
  - `cargo run -p runtime -- admin trace explain --trace-id <representative-fixed-trace>`
- Notes: Broadened to `cargo test --workspace`; all workspace tests passed.

## Approval Gate

Implementation must not start until the user approves this plan.

## Plan Self-Check

- [x] Plan location follows the default location rule.
- [x] Plan progressed from `READY FOR APPROVAL` to `COMPLETED` after user approval and implementation.
- [x] Scope, non-goals, assumptions, and open questions are explicit.
- [x] There are no unresolved open questions to surface.
- [x] Tasks are grouped into milestones because the plan has more than 10 tasks.
- [x] Every task has concrete steps and validation.
- [x] Every milestone has exit criteria.
- [x] Cleanup and final verification are included.
- [x] The plan avoids vague actions without concrete targets.
- [x] The plan can be executed by a coding agent without reading the original conversation.
- [x] Root-cause coverage was rechecked against `logs.txt`, `admin trace recent --limit 40`, representative `admin trace explain` output, `admin foreground schedules list`, `admin status`, and source inspection.
- [x] The plan covers the observed root-cause classes: worker protocol broken pipe, scheduled one-shot semantic drift, scheduled validation failure, malformed action proposal missing read limit, DB action-kind constraint drift, model gateway transport failure, context/action observation uncertainty, and diagnostic/user-facing explainability gaps.

## Execution Notes

- Implementation completed 2026-05-07.
- Worker protocol hardening adds phase context, child exit status, and stderr excerpts on protocol failures; the hidden `exit-after-model-request-worker` entrypoint provides deterministic regression coverage.
- Scheduled foreground one-shot semantics use `oneoff_` or `one_shot_` task-key prefixes with `cadence_seconds: 0`; the harness stores a bounded placeholder cadence and disables one-shot tasks after success, suppression, or failure.
- Trace explain now classifies representative worker broken-pipe traces as `worker_protocol_failure`; persistence/constraint drift and scheduled validation failures are also mapped to stable classes.
- User-facing foreground failure notices now give a trace id, failure kind, and direct `admin trace explain` next step for worker protocol and scheduled validation failures.
- Internal documentation was updated for governed actions, context assembly, worker protocol diagnostics, trace explain behavior, and tool implementation source references.
- Philosophy and requirements compliance check: the implementation remains harness-mediated, bounded, audited, and agent-safe. The conscious worker still proposes actions only; schedule mutation, diagnostics, action execution, failure classification, and trace reporting remain harness-owned. Troubleshooting remains progressively disclosed and read-only through governed diagnostics rather than exposing unrestricted shell or canonical state mutation.
- Cleanup check: no scratch scripts or temporary fixtures were kept. `logs.txt` remains modified as user-provided evidence and was not edited as part of this implementation.
- Verification passed:
  - `cargo fmt --all --check`
  - `cargo check --workspace`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace --lib -- --nocapture --test-threads=1`
  - `cargo test -p harness --test foreground_component -- --nocapture`
  - `cargo test -p harness --test foreground_integration -- --nocapture`
  - `cargo test -p harness --test governed_actions_component -- --nocapture`
  - `cargo test -p harness --test governed_actions_integration -- --nocapture`
  - `cargo test -p harness --test management_component -- --nocapture`
  - `cargo test -p harness --test management_integration -- --nocapture`
  - `cargo test -p harness --test migration_component -- --nocapture`
  - `cargo test -p runtime --test admin_cli -- --nocapture`
  - `cargo test -p runtime --bin runtime -- --nocapture`
  - `cargo test -p workers -- --nocapture`
  - `cargo test --workspace`
  - `cargo run -p runtime -- admin schema status`
  - `cargo run -p runtime -- admin trace explain --trace-id 019e030c-d97f-78f3-a116-a9c7842600b2`
  - `cmd.exe /c git diff --check`
