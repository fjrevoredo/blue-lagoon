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
| `crates/harness/src/background_planning.rs` | `plan_background_job()` (`crates/harness/src/background_planning.rs:52`), `validate_background_trigger()` (`crates/harness/src/background_planning.rs:198`), `assemble_scope()` (`crates/harness/src/background_planning.rs:253`) |
| `crates/harness/src/background_execution.rs` | `lease_next_due_job()` (`crates/harness/src/background_execution.rs:47`), `execute_next_due_job()` (`crates/harness/src/background_execution.rs:219`), `execute_leased_job()` (`crates/harness/src/background_execution.rs:234`) |
| `crates/harness/src/background.rs` | background job, run, and wake-signal persistence helpers including `insert_job()`, `lease_due_job()`, `insert_job_run()`, and `insert_wake_signal()` |
| `crates/harness/src/worker.rs` | `launch_unconscious_worker()` (`crates/harness/src/worker.rs:329`), `launch_unconscious_worker_with_timeout()` (`crates/harness/src/worker.rs:345`) |
| `crates/workers/src/main.rs` | `run_unconscious_worker()` (`crates/workers/src/main.rs:316`), `build_unconscious_model_call_request()` (`crates/workers/src/main.rs:497`) |
| `crates/harness/src/model_gateway.rs` | `execute_background_model_call()` (`crates/harness/src/model_gateway.rs:204`), `execute_model_call_unchecked()` (`crates/harness/src/model_gateway.rs:213`) |
| `crates/harness/src/policy.rs` | `default_background_budget()` (`crates/harness/src/policy.rs:41`), `evaluate_wake_signal()` (`crates/harness/src/policy.rs:307`) |
| `crates/harness/src/governed_actions.rs` | `execute_request_background_job()` (`crates/harness/src/governed_actions.rs:1406`) |
| `crates/harness/src/management.rs` | `enqueue_background_job()` (`crates/harness/src/management.rs:1954`), `run_next_background_job()` (`crates/harness/src/management.rs:2004`) |
| `config/default.toml` | background scheduler, thresholds, execution budgets, and wake-signal policy defaults |

### Execution Flow

The current end-to-end execution path is:

1. The harness service loop polls every `background.scheduler.poll_interval_seconds` and calls `run_background_scheduler_iteration()` (`crates/harness/src/runtime.rs:133`, `crates/harness/src/runtime.rs:152`, `crates/harness/src/runtime.rs:520`).
2. The scheduler checks whether any due rows already exist in `background_jobs`. If none exist, it returns without creating new jobs (`crates/harness/src/runtime.rs:530`).
3. For each due job up to `background.scheduler.max_due_jobs_per_iteration`, the harness leases one row, constructs a `WorkerRequest::unconscious(...)`, inserts an execution record, and records a `background_job_runs` lease row (`crates/harness/src/background_execution.rs:60` through `crates/harness/src/background_execution.rs:123`).
4. The harness spawns the `unconscious-worker` subprocess and speaks the two-step worker protocol: request from harness, one model-call request from worker, model-call response from harness, final worker response (`crates/harness/src/worker.rs:381` through `crates/harness/src/worker.rs:487`).
5. The harness executes the model call itself through `execute_background_model_call()` (`crates/harness/src/worker.rs:425`, `crates/harness/src/model_gateway.rs:204`).
6. The worker returns structured unconscious outputs. The harness applies canonical proposals, retrieval updates, diagnostic alerts, and wake signals itself (`crates/harness/src/background_execution.rs:358` through `crates/harness/src/background_execution.rs:543`).
7. The worker process terminates. The harness updates terminal job state, run state, execution state, and audit events (`crates/harness/src/background_execution.rs:483` through `crates/harness/src/background_execution.rs:543`).

### Job Origination Paths That Exist Today

Background jobs are currently created only through these implemented entry
paths:

- Foreground delegation through governed actions: `RequestBackgroundJob` resolves to `execute_request_background_job()` and then `plan_background_job()` (`crates/harness/src/governed_actions.rs:1406` through `crates/harness/src/governed_actions.rs:1429`).
- Manual operator enqueue through the admin or management surface: `enqueue_background_job()` constructs a trigger and then calls `plan_background_job()` (`crates/harness/src/management.rs:1954` through `crates/harness/src/management.rs:1981`).

### Planning Behavior

`plan_background_job()` performs the current planning logic:

- Validates trigger shape and job/trigger compatibility (`crates/harness/src/background_planning.rs:57` through `crates/harness/src/background_planning.rs:59`).
- Derives the default background execution budget from config (`crates/harness/src/background_planning.rs:83`, `crates/harness/src/policy.rs:41`).
- Assembles a bounded `UnconsciousScope` from recent episodes and memory artifacts (`crates/harness/src/background_planning.rs:90` through `crates/harness/src/background_planning.rs:95`, `crates/harness/src/background_planning.rs:253` onward).
- Applies deduplication against active jobs before insert (`crates/harness/src/background_planning.rs:85`, `crates/harness/src/background_planning.rs:102` through `crates/harness/src/background_planning.rs:135`).
- Persists a `Planned` background job row and audit event (`crates/harness/src/background_planning.rs:138` through `crates/harness/src/background_planning.rs:188`).

### Model Routing Reality

The current model-gateway implementation does distinguish foreground versus
background request validation, but it does not provide separate route
configuration. `execute_background_model_call()` validates that the request is
`LoopKind::Unconscious` with `ModelCallPurpose::BackgroundAnalysis`, then calls
the same shared `execute_model_call_unchecked()` path that resolves the single
configured route from `ResolvedModelGatewayConfig` (`crates/harness/src/model_gateway.rs:204` through `crates/harness/src/model_gateway.rs:220`).

> **NOT IMPLEMENTED:** There is no separate background provider/model route in
> `RuntimeConfig`. The only route config is `model_gateway.foreground`
> (`crates/harness/src/config.rs:200` through `crates/harness/src/config.rs:218`),
> and unconscious jobs currently use that same resolved provider/model.

### Automatic Origination Gaps

The architecture allows time schedules, thresholds, drift signals, passive
events, and maintenance triggers as unconscious triggers. The current codebase
contains enums, validation logic, and persistence support for those trigger
kinds, but not a general autonomous producer that scans runtime state and
enqueues jobs from them.

Specifically:

- `BackgroundTriggerKind` variants exist and are accepted by planning validation (`crates/harness/src/background_planning.rs:198` through `crates/harness/src/background_planning.rs:205`).
- Background threshold config exists and is validated (`crates/harness/src/config.rs:79` through `crates/harness/src/config.rs:98`, `crates/harness/src/config.rs:957` through `crates/harness/src/config.rs:964`).
- The scheduler executes already-planned due jobs, but does not originate new ones during `run_background_scheduler_iteration()` (`crates/harness/src/runtime.rs:520` through `crates/harness/src/runtime.rs:553`).

> **NOT IMPLEMENTED:** No autonomous planner currently enqueues background jobs
> from `background.thresholds.*`, periodic schedules, drift metrics, passive
> external events, or maintenance scans. The current live creation paths are
> foreground delegated requests and manual management enqueue only.

### Wake-Signal Flow

Wake signals returned by the unconscious worker are persisted first, then
policy-reviewed by the harness. Accepted signals may be staged toward the
foreground only if a configured foreground conversation binding exists and the
wake-signal policy allows conversion (`crates/harness/src/background_execution.rs:701`
onward, `crates/harness/src/policy.rs:307` through `crates/harness/src/policy.rs:392`).

Important current behavior:

- No Telegram foreground binding means wake signals are rejected for foreground conversion (`crates/harness/src/policy.rs:312` through `crates/harness/src/policy.rs:318`).
- `background.wake_signals.allow_foreground_conversion = false` defers conversion even when signals are recorded (`crates/harness/src/policy.rs:321` through `crates/harness/src/policy.rs:326`).
- Queue pressure, cooldown, reliability, and identity boundaries can defer or suppress non-urgent signals (`crates/harness/src/policy.rs:329` through `crates/harness/src/policy.rs:385`).

---

## 3. Configuration & Extension

### Config Keys

| Config key | Default | Valid range | Read by |
|---|---|---|---|
| `background.scheduler.poll_interval_seconds` | `300` | integer greater than zero | `config/default.toml:21`, `crates/harness/src/runtime.rs:133` |
| `background.scheduler.max_due_jobs_per_iteration` | `4` | integer greater than zero | `config/default.toml:22`, `crates/harness/src/runtime.rs:525` |
| `background.scheduler.lease_timeout_ms` | `300000` | integer greater than zero | `config/default.toml:23`, `crates/harness/src/background_execution.rs:52` |
| `background.thresholds.episode_backlog_threshold` | `25` | integer greater than zero | `config/default.toml:26`, `crates/harness/src/config.rs:957` |
| `background.thresholds.candidate_memory_threshold` | `10` | integer greater than zero | `config/default.toml:27`, `crates/harness/src/config.rs:960` |
| `background.thresholds.contradiction_alert_threshold` | `3` | integer greater than zero | `config/default.toml:28`, `crates/harness/src/config.rs:963` |
| `background.execution.default_iteration_budget` | `2` | integer greater than zero | `config/default.toml:31`, `crates/harness/src/policy.rs:43` |
| `background.execution.default_wall_clock_budget_ms` | `120000` | integer greater than zero | `config/default.toml:32`, `crates/harness/src/policy.rs:44` |
| `background.execution.default_token_budget` | `6000` | integer greater than zero | `config/default.toml:33`, `crates/harness/src/policy.rs:45` |
| `background.wake_signals.allow_foreground_conversion` | `true` | `true` or `false` | `config/default.toml:36`, `crates/harness/src/policy.rs:321` |
| `background.wake_signals.max_pending_signals` | `8` | integer greater than zero | `config/default.toml:37`, `crates/harness/src/policy.rs:373` |
| `background.wake_signals.cooldown_seconds` | `900` | integer greater than zero | `config/default.toml:38`, `crates/harness/src/background_execution.rs` wake-signal persistence path |
| `worker.timeout_ms` | `10000` | integer greater than zero | `config/default.toml:80`, `crates/harness/src/worker.rs:340` |
| `model_gateway.foreground.*` | provider-specific | see `docs/internal/harness/MODEL_PROVIDERS.md` | shared foreground and unconscious model route |

### Extension Points

To add autonomous background origination:

1. Add a producer stage to the harness service loop before execution, or a dedicated planner function called from `run_background_scheduler_iteration()`.
2. Read runtime state and thresholds deterministically in the harness, not in the worker.
3. Construct a `BackgroundPlanningRequest` with a specific `BackgroundTriggerKind`.
4. Reuse `plan_background_job()` so deduplication, scoping, budgeting, and audit behavior stay centralized.
5. Add component tests for planning and integration tests for end-to-end enqueue plus execution.

To add a separate background model route:

1. Extend `ModelGatewayConfig` and `ResolvedModelGatewayConfig` with an unconscious route instead of reusing `foreground`.
2. Split route resolution in `model_gateway.rs` so `execute_background_model_call()` does not call the foreground route resolver.
3. Document any new config keys and environment overrides in `docs/internal/harness/MODEL_PROVIDERS.md`.
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
- `docs/internal/harness/MODEL_PROVIDERS.md` explains the single current model route and provider-specific request encoding.
- `docs/internal/harness/TRACE_EXPLORER.md` explains how background jobs, runs, and wake signals appear in traces and diagnostics.
- `crates/harness/tests/unconscious_component.rs` and `crates/harness/tests/unconscious_integration.rs` cover the current planning and execution behavior end to end.

Verified: 2026-05-07.
