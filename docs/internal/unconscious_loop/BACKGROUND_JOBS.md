# Background Jobs

## 1. Overview

This document describes the current implementation of the unconscious loop as it
exists in code today. It is an implementation-facing companion to
`docs/LOOP_ARCHITECTURE.md` and records what is live, what is only scaffolded,
and which architectural triggers are not yet backed by automatic runtime
origination logic.

In the current runtime, the unconscious loop is implemented as harness-managed
background jobs stored in PostgreSQL and executed by ephemeral
`unconscious-worker` subprocesses. The harness owns planning, leasing, model
invocation, proposal application, wake-signal policy, audit logging, and job
terminal state. The worker performs only bounded scoped analysis and returns
structured outputs.

---

## 2. Implementation

### Source Files

| File | Relevant symbol |
|---|---|
| `crates/harness/src/runtime.rs` | `run_harness_service()` (`crates/harness/src/runtime.rs:117`), `run_background_scheduler_iteration()` (`crates/harness/src/runtime.rs:520`), `run_background_once_with()` (`crates/harness/src/runtime.rs:191`) |
| `crates/harness/src/background_planning.rs` | `run_scheduler_planning_pass()` (`crates/harness/src/background_planning.rs:62`), `run_scheduler_planning_pass_with_requests()` (`crates/harness/src/background_planning.rs:73`), `plan_background_job()` (`crates/harness/src/background_planning.rs:96`), `validate_background_trigger()` (`crates/harness/src/background_planning.rs:242`), `build_scheduler_planning_requests()` (`crates/harness/src/background_planning.rs:297`), `build_volume_threshold_requests()` (`crates/harness/src/background_planning.rs:342`), `is_periodic_schedule_due()` (`crates/harness/src/background_planning.rs:405`), `record_scheduler_planning_telemetry()` (`crates/harness/src/background_planning.rs:433`), `insert_scheduler_planning_diagnostic_with_cooldown()` (`crates/harness/src/background_planning.rs:510`), `assemble_scope()` (`crates/harness/src/background_planning.rs:557`) |
| `crates/harness/src/background_execution.rs` | `lease_next_due_job()` (`crates/harness/src/background_execution.rs:47`), `execute_next_due_job()` (`crates/harness/src/background_execution.rs:219`), `execute_leased_job()` (`crates/harness/src/background_execution.rs:234`) |
| `crates/harness/src/background.rs` | background job, run, and wake-signal persistence helpers including `insert_job()`, `lease_due_job()`, `insert_job_run()`, and `insert_wake_signal()` |
| `crates/harness/src/worker.rs` | `launch_unconscious_worker()` (`crates/harness/src/worker.rs:329`), `launch_unconscious_worker_with_timeout()` (`crates/harness/src/worker.rs:345`) |
| `crates/workers/src/main.rs` | `run_unconscious_worker()` (`crates/workers/src/main.rs:429`), `build_unconscious_model_call_request()` (`crates/workers/src/main.rs:612`) |
| `crates/harness/src/model_gateway.rs` | `execute_background_model_call()` (`crates/harness/src/model_gateway.rs:204`), `execute_model_call_unchecked()` (`crates/harness/src/model_gateway.rs:219`) |
| `crates/harness/src/policy.rs` | `default_background_budget()` (`crates/harness/src/policy.rs:41`), `evaluate_wake_signal()` (`crates/harness/src/policy.rs:322`) |
| `crates/harness/src/governed_actions.rs` | `execute_request_background_job()` (`crates/harness/src/governed_actions.rs:2282`) |
| `crates/harness/src/management.rs` | `enqueue_background_job()` (`crates/harness/src/management.rs:2008`), `run_next_background_job()` (`crates/harness/src/management.rs:2058`) |
| `config/default.toml` | background scheduler, thresholds, execution budgets, and wake-signal policy defaults |

### Execution Flow

The current end-to-end execution path is:

1. The harness service loop polls every `background.scheduler.poll_interval_seconds` and calls `run_background_scheduler_iteration()` (`crates/harness/src/runtime.rs:133`, `crates/harness/src/runtime.rs:152`, `crates/harness/src/runtime.rs:520`).
2. The scheduler runs a dedicated planning stage through `run_scheduler_planning_pass()` before checking due jobs (`crates/harness/src/runtime.rs:527`, `crates/harness/src/background_planning.rs:61`).
3. The scheduler checks whether any due rows already exist in `background_jobs`. If none exist, it returns (`crates/harness/src/runtime.rs:544`).
4. For each due job up to `background.scheduler.max_due_jobs_per_iteration`, the harness leases one row, constructs a `WorkerRequest::unconscious(...)`, inserts an execution record, and records a `background_job_runs` lease row (`crates/harness/src/background_execution.rs:60` through `crates/harness/src/background_execution.rs:123`).
5. The harness spawns the `unconscious-worker` subprocess and speaks the two-step worker protocol: request from harness, one model-call request from worker, model-call response from harness, final worker response (`crates/harness/src/worker.rs:381` through `crates/harness/src/worker.rs:487`).
6. The harness executes the model call itself through `execute_background_model_call()` (`crates/harness/src/worker.rs:425`, `crates/harness/src/model_gateway.rs:204`).
7. The worker returns structured unconscious outputs. The harness applies canonical proposals, retrieval updates, diagnostic alerts, and wake signals itself (`crates/harness/src/background_execution.rs:358` through `crates/harness/src/background_execution.rs:543`).
8. The worker process terminates. The harness updates terminal job state, run state, execution state, and audit events (`crates/harness/src/background_execution.rs:483` through `crates/harness/src/background_execution.rs:543`).

### Job Origination Paths That Exist Today

Background jobs are currently created only through these implemented entry
paths:

- Scheduler autonomous origination through `run_scheduler_planning_pass()`, which builds threshold/schedule `BackgroundPlanningRequest` values and routes each through `plan_background_job()` (`crates/harness/src/background_planning.rs:62` through `crates/harness/src/background_planning.rs:69`, `crates/harness/src/background_planning.rs:297` through `crates/harness/src/background_planning.rs:340`).
- Foreground delegation through governed actions: `RequestBackgroundJob` resolves to `execute_request_background_job()` and then `plan_background_job()` (`crates/harness/src/governed_actions.rs:2282` through `crates/harness/src/governed_actions.rs:2305`).
- Manual operator enqueue through the admin or management surface: `enqueue_background_job()` constructs a trigger and then calls `plan_background_job()` (`crates/harness/src/management.rs:2008` through `crates/harness/src/management.rs:2035`).

### Planning Behavior

`plan_background_job()` performs the current planning logic:

- Validates trigger shape and job/trigger compatibility (`crates/harness/src/background_planning.rs:101` through `crates/harness/src/background_planning.rs:102`).
- Derives the default background execution budget from config (`crates/harness/src/background_planning.rs:127`, `crates/harness/src/policy.rs:41`).
- Assembles a bounded `UnconsciousScope` from recent episodes and memory artifacts (`crates/harness/src/background_planning.rs:134` through `crates/harness/src/background_planning.rs:139`, `crates/harness/src/background_planning.rs:557` onward).
- Applies deduplication against active jobs before insert (`crates/harness/src/background_planning.rs:129`, `crates/harness/src/background_planning.rs:147` through `crates/harness/src/background_planning.rs:180`).
- Persists a `Planned` background job row and audit event (`crates/harness/src/background_planning.rs:183` through `crates/harness/src/background_planning.rs:233`).
- Records scheduler-pass telemetry and rate-limited planner diagnostics for duplicate suppression and rejection pressure (`crates/harness/src/background_planning.rs:433` through `crates/harness/src/background_planning.rs:554`).

### Model Routing Reality

The model-gateway implementation now has separate route configuration for
foreground and unconscious execution. `execute_background_model_call()`
validates that the request is `LoopKind::Unconscious` with
`ModelCallPurpose::BackgroundAnalysis`, then resolves the unconscious route from
`ResolvedModelGatewayConfig` before provider invocation
(`crates/harness/src/model_gateway.rs:204` through
`crates/harness/src/model_gateway.rs:229`,
`crates/harness/src/model_gateway.rs:322` through
`crates/harness/src/model_gateway.rs:327`).

Route configuration lives in both `model_gateway.foreground.*` and
`model_gateway.unconscious.*` (`crates/harness/src/config.rs:200` through
`crates/harness/src/config.rs:202`, `crates/harness/src/config.rs:555` through
`crates/harness/src/config.rs:597`).

### Automatic Origination Gaps

The architecture allows time schedules, thresholds, drift signals, passive
events, and maintenance triggers as unconscious triggers. The current codebase
now includes autonomous producer logic for bounded threshold and time-schedule
sources, while broader producer coverage remains incomplete.

Specifically:

- `BackgroundTriggerKind` variants exist and are accepted by planning validation (`crates/harness/src/background_planning.rs:242` through `crates/harness/src/background_planning.rs:249`).
- Background threshold config exists and is validated (`crates/harness/src/config.rs:79` through `crates/harness/src/config.rs:98`, `crates/harness/src/config.rs:1215` through `crates/harness/src/config.rs:1222`).
- Autonomous producers currently emit:
  - `BackgroundTriggerKind::VolumeThreshold` requests for memory consolidation
    (`episodes` count versus `background.thresholds.episode_backlog_threshold`)
    and retrieval maintenance (`memory_artifacts` active count versus
    `background.thresholds.candidate_memory_threshold`)
    (`crates/harness/src/background_planning.rs:342` through
    `crates/harness/src/background_planning.rs:402`).
  - `BackgroundTriggerKind::TimeSchedule` requests for periodic self-model
    reflection when due under a bounded interval derived from
    `background.scheduler.poll_interval_seconds`
    (`crates/harness/src/background_planning.rs:303` through
    `crates/harness/src/background_planning.rs:340`,
    `crates/harness/src/background_planning.rs:405` through
    `crates/harness/src/background_planning.rs:430`).

> **PARTIALLY IMPLEMENTED:** Autonomous producers now cover selected threshold
> and time-schedule cases, but no producer currently emits origination requests
> from contradiction thresholds, drift metrics, passive external events, or
> maintenance scans.

### Wake-Signal Flow

Wake signals returned by the unconscious worker are persisted first, then
policy-reviewed by the harness. Accepted signals may be staged toward the
foreground only if a configured foreground conversation binding exists and the
wake-signal policy allows conversion (`crates/harness/src/background_execution.rs:701`
onward, `crates/harness/src/policy.rs:322` through `crates/harness/src/policy.rs:407`).

Important current behavior:

- No Telegram foreground binding means wake signals are rejected for foreground conversion (`crates/harness/src/policy.rs:327` through `crates/harness/src/policy.rs:333`).
- `background.wake_signals.allow_foreground_conversion = false` defers conversion even when signals are recorded (`crates/harness/src/policy.rs:336` through `crates/harness/src/policy.rs:341`).
- Queue pressure, cooldown, reliability, and identity boundaries can defer or suppress non-urgent signals (`crates/harness/src/policy.rs:344` through `crates/harness/src/policy.rs:400`).

---

## 3. Configuration & Extension

### Config Keys

| Config key | Default | Valid range | Read by |
|---|---|---|---|
| `background.scheduler.poll_interval_seconds` | `300` | integer greater than zero | `config/default.toml:21`, `crates/harness/src/runtime.rs:133` |
| `background.scheduler.max_due_jobs_per_iteration` | `4` | integer greater than zero | `config/default.toml:22`, `crates/harness/src/runtime.rs:542` |
| `background.scheduler.lease_timeout_ms` | `300000` | integer greater than zero | `config/default.toml:23`, `crates/harness/src/background_execution.rs:52` |
| `background.thresholds.episode_backlog_threshold` | `25` | integer greater than zero | `config/default.toml:26`, `crates/harness/src/config.rs:1215` |
| `background.thresholds.candidate_memory_threshold` | `10` | integer greater than zero | `config/default.toml:27`, `crates/harness/src/config.rs:1218` |
| `background.thresholds.contradiction_alert_threshold` | `3` | integer greater than zero | `config/default.toml:28`, `crates/harness/src/config.rs:1221` |
| `background.execution.default_iteration_budget` | `2` | integer greater than zero | `config/default.toml:31`, `crates/harness/src/policy.rs:43` |
| `background.execution.default_wall_clock_budget_ms` | `120000` | integer greater than zero | `config/default.toml:32`, `crates/harness/src/policy.rs:44` |
| `background.execution.default_token_budget` | `6000` | integer greater than zero | `config/default.toml:33`, `crates/harness/src/policy.rs:45` |
| `background.wake_signals.allow_foreground_conversion` | `true` | `true` or `false` | `config/default.toml:36`, `crates/harness/src/policy.rs:336` |
| `background.wake_signals.max_pending_signals` | `8` | integer greater than zero | `config/default.toml:37`, `crates/harness/src/policy.rs:388` |
| `background.wake_signals.cooldown_seconds` | `900` | integer greater than zero | `config/default.toml:38`, `crates/harness/src/background_execution.rs` wake-signal persistence path |
| `worker.timeout_ms` | `10000` | integer greater than zero | `config/default.toml:101`, `crates/harness/src/worker.rs:340` |
| `model_gateway.foreground.*` | provider-specific | see `docs/internal/harness/MODEL_PROVIDERS.md` | conscious-loop provider/model route |
| `model_gateway.unconscious.*` | provider-specific | see `docs/internal/harness/MODEL_PROVIDERS.md` | unconscious-loop/background provider/model route |

### Extension Points

To add autonomous background origination:

1. Extend deterministic producer logic in `build_scheduler_planning_requests()` (`crates/harness/src/background_planning.rs:297`) so the scheduler planning stage emits all required bounded request families.
2. Read runtime state and thresholds deterministically in the harness, not in the worker.
3. Construct a `BackgroundPlanningRequest` with a specific `BackgroundTriggerKind`.
4. Reuse `plan_background_job()` so deduplication, scoping, budgeting, and audit behavior stay centralized.
5. Add component tests for planning and integration tests for end-to-end enqueue plus execution.

To extend foreground/unconscious route specialization:

1. Keep route changes inside `model_gateway.foreground.*` and `model_gateway.unconscious.*`, with fail-closed validation in `RuntimeConfig`.
2. Keep `execute_background_model_call()` on the unconscious resolver path and `execute_foreground_model_call()` on the foreground resolver path.
3. Document any new route keys or environment overrides in `docs/internal/harness/MODEL_PROVIDERS.md`.
4. Add tests covering route selection, timeout differences, and provider-specific compatibility.

To add a new job kind:

1. Extend `contracts::UnconsciousJobKind`.
2. Update `background_planning.rs` job-kind labeling, compatibility rules, and scope assembly.
3. Update worker request shaping in `crates/workers/src/main.rs`.
4. Add execution and policy coverage in the unconscious component and integration test suites.

---

## 4. Further Reading

- `docs/LOOP_ARCHITECTURE.md` defines the canonical conscious/unconscious split and the intended set of allowed unconscious triggers.
- `docs/IMPLEMENTATION_DESIGN.md` states the canonical product posture that background work is bounded and harness-governed.
- `docs/internal/harness/MODEL_PROVIDERS.md` explains the foreground/unconscious route split and provider-specific request encoding.
- `docs/internal/harness/TRACE_EXPLORER.md` explains how background jobs, runs, and wake signals appear in traces and diagnostics.
- `crates/harness/tests/unconscious_component.rs` and `crates/harness/tests/unconscious_integration.rs` cover the current planning and execution behavior end to end.

Verified: 2026-05-15.
