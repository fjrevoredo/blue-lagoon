# Use Case Catalog

This catalog defines the 8 most important use cases for the Blue Lagoon assistant runtime. Each entry maps to an acceptance test in `crates/harness/tests/use_case_scenarios.rs`.

---

## UC-1: Basic Conversation

**Goal**: The user sends a message and receives a coherent, personality-consistent reply.

**Trigger**: User sends a private Telegram text message.

**Acceptance Criteria**:
1. Exactly one Telegram message is delivered.
2. The first message in the model request contains the self-model seed's `communication_style` ("be direct").
3. One `execution_records` row with `status = 'completed'` exists after the run.

**Current Status**: Partial

**Gap**: No assertion that the self-model seed appears in the model prompt. The system assembles context, but this is untested at the scenario level.

**Test**: `uc1_basic_conversation_delivers_reply_with_self_model_in_prompt`

---

## UC-2: Multi-Turn Continuity

**Goal**: The second message in a session includes the prior turn's user text in the model context, enabling a coherent multi-turn conversation.

**Trigger**: User sends a second message after an initial exchange.

**Acceptance Criteria**:
1. Two model requests are made (`transport.seen_requests().len() == 2`).
2. The second request's messages array contains the first session's user message text ("hello from telegram").
3. Two Telegram messages are delivered (`delivery.sent_messages().len() == 2`).

**Current Status**: Partial

**Gap**: No integration test asserting that prior episode text appears in the second model request. Recent history retrieval is implemented, but the scenario-level assertion is missing.

**Test**: `uc2_second_message_receives_prior_episode_in_context`

---

## UC-3: Cross-Session Memory

**Goal**: A preference stated in one session is remembered and placed in context in the next session.

**Trigger**: User states a preference; a follow-up message in a later session is sent.

**Acceptance Criteria**:
1. One `memory_artifacts` row with `status = 'active'` exists after the first session.
2. The second request's messages contain "Retrieved canonical context:" with the preference text.
3. Both sessions complete successfully.

**Current Status**: Working

**Gap**: None. Covered by existing `continuity_integration` and `foreground_integration` tests. This test is a thin anchor.

**Test**: `uc3_preference_from_session_1_appears_in_session_2_context`

---

## UC-4: Governed Action with Approval

**Goal**: When the model proposes an action requiring approval, the user sees the approval prompt; after approval, the action executes.

**Trigger**: Model output contains a governed-action block with a write-scope that exceeds Tier1.

**Acceptance Criteria**:
1. After Run 1: one Telegram message delivered (approval prompt) and one `governed_action_executions` row with `status = 'awaiting_approval'`.
2. After manual approval resolution: one `governed_action_executions` row with `status IN ('executed', 'failed')`.
3. The `approval_requests` row is resolved.

**Current Status**: Partial

**Gap**: No two-run test covering the full flow: model output → approval prompt → user approves → execution. The approval resolution and execution paths are tested in isolation in `governed_actions_integration.rs`.

**Test**: `uc4_governed_action_requires_approval_then_executes_after_user_approves`

**Manual verification**: Verify that the approval prompt message in Telegram contains an inline keyboard or fallback text (cannot be checked with `FakeTelegramDelivery`).

---

## UC-5: Proactive Scheduled Message

**Goal**: A due scheduled task fires and delivers a proactive message to the user without any user-initiated trigger.

**Trigger**: A `scheduled_foreground_tasks` row with `next_due_at` in the past is present when the harness runs its scheduled iteration.

**Acceptance Criteria**:
1. `run_scheduled_foreground_iteration_with` returns `handled == 1`.
2. One Telegram message is delivered.
3. `scheduled_foreground_tasks.last_outcome = 'completed'` and `current_execution_id IS NULL`.

**Current Status**: Working

**Gap**: None. Covered by `foreground_integration::scheduled_foreground_runtime_run_executes_due_task_through_worker_binary`. This test is a thin anchor.

**Test**: `uc5_scheduled_task_fires_and_delivers_proactive_message`

---

## UC-6: Background-Initiated Notification

**Goal**: A background job (e.g. self-model reflection) produces a wake signal that is staged and then delivered to the user on the next foreground pickup.

**Trigger**: A `SelfModelReflection` background job runs; its wake signal is staged as a pending ingress event; a subsequent foreground pass processes it.

**Acceptance Criteria**:
1. After Phase 1 (background): one `wake_signals` row with `status = 'accepted'`; one `ingress_events` row with `external_event_id LIKE 'wake-signal:%'` and `foreground_status = 'pending'`.
2. After Phase 2 (foreground pickup): one Telegram message delivered; the `ingress_events` row has `foreground_status = 'processed'`.

**Current Status**: Partial

**Gap**: The background→staging path is tested in `unconscious_integration`. The staging→Telegram delivery path is not tested end-to-end in a single test.

**Test**: `uc6_background_wake_signal_stages_then_delivers_notification`

---

## UC-7: Backlog Recovery

**Goal**: A burst of messages is condensed into a single coherent reply using backlog recovery.

**Trigger**: A fixture containing 3+ messages within the backlog threshold window is ingested.

**Acceptance Criteria**:
1. `summary.backlog_recovery_count == 1`.
2. One Telegram message is delivered.
3. All `ingress_events` for the execution have `foreground_status = 'processed'`.

**Current Status**: Working

**Gap**: None. Covered by `foreground_integration::telegram_fixture_runtime_batch_activates_backlog_recovery`. This test is a thin anchor.

**Test**: `uc7_backlog_of_messages_batched_into_single_coherent_reply`

---

## UC-8: Worker Failure Recovery

**Goal**: A scheduled task whose worker process crashed is detected, marked failed with a checkpoint, and the task can be rescheduled for a clean re-run.

**Trigger**: A claimed scheduled task with a stale `current_run_started_at` (simulating a crash) is present when recovery runs.

**Acceptance Criteria**:
1. `recover_interrupted_scheduled_foreground_tasks` returns `recovered == 1`.
2. `scheduled_foreground_tasks.last_outcome = 'failed'` and `current_execution_id IS NULL`.
3. One `recovery_checkpoints` row with `recovery_decision IN ('retry', 'abandon')` exists for the execution.
4. (Extended) After re-upserting the task as due, `run_scheduled_foreground_iteration_with` returns `handled == 1`.

**Current Status**: Partial

**Gap**: Each mechanism (claim, recovery, re-execution) is tested in isolation. No single test covers the full acquire→crash→recover→re-execute lifecycle.

**Test**: `uc8_worker_crash_creates_checkpoint_and_task_is_clean_after_recovery`
