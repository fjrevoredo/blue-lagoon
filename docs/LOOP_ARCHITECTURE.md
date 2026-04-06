# Conscious and Unconscious Loops

## Overview

This document specifies a two-loop architecture for an autonomous agent system composed of a **conscious loop** (foreground cognition and action) and an **unconscious loop** (background maintenance and consolidation). The two loops are fully isolated at the process, data, and context levels, and all interactions are mediated by a central harness.

---

## 1. Loop Definitions

### 1.1 Conscious Loop

The conscious loop is the foreground executive that handles perception, present-moment reasoning, user interaction, and explicit action.

**Responsibilities**
- Interpret incoming goals, messages, and events.
- Build a present-moment context from the self-model and selected memory.
- Plan and decide on actions.
- Invoke tools and external systems (subject to policy and safety checks).
- Emit episodic records and candidate memory events.
- Request background work when needed.

**Step sequence**
1. **Trigger received**
   - Allowed triggers are defined in section 3.1.
2. **Context assembly (via harness)**
   - Harness composes:
     - Compact self-model snapshot for this agent.
     - Selected long-term and session memories relevant to the trigger.
     - Current internal state snapshot (load, health, etc.).
3. **Budget initialization**
   - Harness sets or restores iteration, time, and compute budgets for this conscious episode (e.g., max turns, max wall-clock time, max cost).
4. **Perceive**
   - Conscious agent reads the context and current input (user message, goal, event).
5. **Plan**
   - Agent proposes a plan: high-level intent, optional subgoals, tool calls.
6. **Policy & safety check (via harness)**
   - Harness validates proposed actions against policies, permissions, and remaining budgets.
7. **Act**
   - Approved tool calls and external actions are executed by the harness.
8. **Observe**
   - Results and observations are returned to the agent through harness-managed channels.
9. **Record**
   - Agent emits:
     - Episodic entries (what happened, when, outcome).
     - Candidate memory items (facts, preferences, relationships).
     - Optional background job requests (e.g., dream, reconciliation).
10. **Loop or halt**
    - If the goal is not yet satisfied and budgets allow, continue another iteration.
    - Otherwise, halt and wait for the next trigger.

---

### 1.2 Unconscious Loop

The unconscious loop is a background maintenance and transformation system implemented as harness-managed jobs and ephemeral specialist agents.

**Responsibilities**
- Consolidate episodic memory into compact long-term representations.
- Maintain retrieval structures (indexes, graphs, summaries).
- Detect and mitigate semantic drift, contradictions, and stability issues.
- Propose updates to the self-model and long-term memory.
- Raise wake-up signals when foreground attention is required.

**Step sequence (per job)**
1. **Trigger received**
   - Allowed triggers are defined in section 3.2.
2. **Job scoping (via harness)**
   - Harness determines:
     - Job type (e.g., dream, cleanup, contradiction scan).
     - Input scope: which episodic segments, facts, or indexes are visible.
     - Execution budget (iterations, tokens, time, cost).
3. **Worker instantiation**
   - Harness spawns a deterministic job or an ephemeral specialist agent with:
     - No shared memory with the conscious loop.
     - Only the scoped inputs required for this job.
     - Explicit execution budgets.
4. **Analyze & transform**
   - Worker performs bounded processing, such as:
     - Summarization and abstraction.
     - Entity and relationship extraction.
     - Conflict detection and drift analysis.
     - Self-model delta proposal generation.
5. **Produce proposals**
   - Worker returns structured outputs only, such as:
     - Memory deltas (add/update/delete proposals with provenance).
     - Retrieval/index updates.
     - Self-model delta proposals.
     - Optional wake signals (typed reasons with reason codes).
6. **Merge & validate (via harness)**
   - Harness validates and merges accepted proposals into canonical stores.
7. **Terminate worker**
   - Worker process is destroyed; no persistent identity or context is retained.

---

## 2. Interactions Between Loops

### 2.1 Isolation guarantees

- **Process isolation**: conscious loop and unconscious jobs run in separate processes/containers/VMs.
- **Data isolation**: no shared writable memory; all reads and writes go through harness APIs.
- **Context isolation**:
  - Conscious loop sees present context plus compact self-model and selected memory.
  - Unconscious workers see only the minimal slices needed for their job.

### 2.2 Harness as sole mediator

All cross-loop interactions are mediated by the harness:

- Conscious loop to unconscious:
  - Conscious agent emits a background job request intent.
  - Harness validates, schedules, and instantiates the appropriate unconscious job.
- Unconscious to conscious:
  - Unconscious worker may emit a wake signal with a typed reason and a reason code (e.g., `critical_conflict`, `proactive_briefing_ready`, `self_state_anomaly`).
  - Harness applies policy and decides whether to convert it into a foreground trigger, possibly throttling or dropping low-priority signals.
- Both loops to memory and self-model:
  - Both may emit proposals (episodic entries, memory deltas, self-model deltas).
  - Only the harness can commit changes to canonical storage.

No direct calls or shared references between the conscious and unconscious loops are allowed.

---

## 3. Allowed Triggers

### 3.1 Conscious Loop Triggers

The conscious loop may start or resume only when the harness emits one of the following trigger types:

1. **User input**
   - New message, command, or interaction from the user or external caller.
2. **Scheduled foreground task**
   - Pre-approved proactive events (reminders, check-ins, routine reports).
3. **Approved wake signal**
   - A wake signal from the unconscious layer that has been accepted by harness policy (based on reason code, priority, user settings, and rate limits).
4. **Supervisor recovery event**
   - Restart after crash, timeout, or interrupted active task using a checkpoint.
5. **Approval resolution**
   - A previously pending operation receives human approval or rejection, requiring replanning or completion.

### 3.2 Unconscious Loop Triggers

The unconscious loop may start a job only when the harness sees one of the following triggers:

1. **Time-based schedule**
   - Cron-like intervals (e.g., nightly dream, hourly cleanup, weekly self-model refresh).
2. **Volume thresholds**
   - Episodic log size, number of unresolved candidate memories, index size, or working-memory pressure exceeding configured thresholds.
3. **Drift or anomaly signals**
   - Metrics indicating semantic drift, rising contradiction rates, retrieval degradation, repeated failures, or self-state anomalies.
4. **Foreground delegation**
   - Explicit request from the conscious loop to perform a background task (e.g., dream on todayâ€™s episodes).
5. **External passive events**
   - New documents, emails, telemetry, or environment changes that can be processed silently.
6. **Maintenance triggers**
   - Checkpoint compaction, index rebuild, failed-merge retry, or stalled-worker cleanup.

---

## 4. Allowed Results

### 4.1 Conscious Loop Results

The conscious loop is allowed to produce:

- **User-facing outputs**
  - Natural language responses, explanations, UI updates.
- **Plans and delegations**
  - Proposed actions, subgoals, and tool invocations.
- **Tool actions**
  - Requests to execute tools and external operations (via harness and policy).
- **Episodic records**
  - Structured logs of inputs, internal state snapshots, actions, and outcomes.
- **Candidate memory events**
  - Proposed long-term memory items (facts, preferences, relationships).
- **Background job requests**
  - Requests to start unconscious jobs such as dream, summarization, reconciliation, or cleanup.

The conscious loop is **not** allowed to:

- Directly mutate canonical long-term memory in bulk.
- Rewrite the self-model artifact in-place without going through harness merge.
- Start or control unconscious jobs directly (all delegation is via the harness).

### 4.2 Unconscious Loop Results

The unconscious loop is allowed to produce:

- **Memory delta proposals**
  - Add/update/delete proposals with provenance and confidence scores.
- **Retrieval/index updates**
  - New or updated indexes, summaries, graph edges, weights, or archival moves.
- **Self-model delta proposals**
  - Suggested updates to the compact self-model (traits, preferences, constraints).
- **Diagnostics and alerts**
  - Drift indicators, contradiction reports, stability assessments.
- **Wake signals**
  - Typed requests to wake the conscious loop for specific reasons, always including a reason code and optional payload reference.

The unconscious loop is **not** allowed to:

- Produce user-facing outputs.
- Execute external actions or tools with side effects.
- Directly mutate canonical memory or self-model artifacts.
- Directly wake or control the conscious loop.

---

## 5. How Everything Plays Together

### 5.1 Primary components

- **Conscious loop**
  - Foreground cognition and action.
- **Unconscious loop**
  - Background maintenance and consolidation.
- **Harness**
  - Sole mediator, scheduler, policy engine, and storage controller.
- **Canonical stores**
  - Episodic log, long-term memory, retrieval/index structures, self-model artifacts.

### 5.2 Control flow summary

1. **Event arrives or schedule fires**
   - Harness classifies the event as a foreground trigger, background job trigger, or both.
2. **Foreground and/or background work is scheduled**
   - For foreground: harness assembles context, initializes budgets, and wakes the conscious loop.
   - For background: harness scopes inputs, sets budgets, and launches unconscious jobs.
3. **Loops run in isolation**
   - Each loop operates within its process and scoped data view.
4. **Loops produce results as proposals**
   - Conscious loop emits user responses, plans, episodic records, candidate memories, and background job intents.
   - Unconscious jobs emit memory deltas, index updates, self-model deltas, diagnostics, and wake signals with reason codes.
5. **Harness validates and commits**
   - Harness enforces policies, merges accepted proposals into canonical stores, and logs all changes.
6. **Harness may wake the conscious loop**
   - If allowed wake signals (by reason code and policy) or external events justify it, the harness creates a new foreground trigger.
7. **The system returns to idle**
   - Loops halt until new triggers arrive.

---

## 6. Example Workflows

### 6.1 Nightly Dream Cycle

**Scenario**: A scheduled dream cycle consolidates the dayâ€™s experiences into long-term memory and updates the self-model.

1. **Trigger**
   - A cron-like schedule fires at 03:00 local time.
   - Harness treats this as a background job trigger of type `dream`.
2. **Job scoping**
   - Harness identifies:
     - Episodic log entries from the last 24 hours.
     - Current long-term memory slice relevant to those episodes.
     - Current self-model snapshot.
3. **Worker instantiation**
   - Harness spawns an ephemeral dream worker with:
     - Read-only access to the scoped data.
     - A fixed budget (tokens/time/iterations).
4. **Analyze & transform**
   - Worker:
     - Summarizes the day into a few narrative episodes.
     - Extracts or updates stable facts (e.g., new preferences, recurring tasks).
     - Detects contradictions or anomalies in memory.
     - Proposes self-model deltas (e.g., updated preference weights).
5. **Produce proposals**
   - Worker returns:
     - A set of memory delta proposals.
     - Updated summaries and retrieval/index artifacts.
     - Self-model delta proposals.
     - Optionally, a wake signal with reason code `critical_conflict` if a serious contradiction or anomaly is detected.
6. **Merge & validate**
   - Harness validates and merges accepted proposals into canonical memory and self-model stores.
7. **Potential wake-up**
   - If the worker emitted a high-priority wake signal:
     - Harness policy evaluates the reason code and may create a foreground trigger for the conscious loop, so that when the user next appears (or immediately, if policy allows), the agent can explain and/or ask for guidance.
8. **Completion**
   - Worker terminates; no persistent worker state is kept.

### 6.2 Proactive Calendar & Weather Check

**Scenario**: A scheduled job checks the userâ€™s calendar for the next meeting, looks up expected weather for that time and location, and surfaces a concise summary to the user.

1. **Trigger**
   - A scheduled event fires every morning at 07:00.
   - Harness classifies this as both:
     - A background job trigger (`calendar_weather_check`).
     - A potential foreground trigger depending on policy.
2. **Background job scoping**
   - Harness scopes inputs for the unconscious job:
     - Read-only access to calendar API results for the next 24 hours.
     - Location preferences and travel habits from long-term memory.
3. **Worker instantiation**
   - Harness spawns an ephemeral worker to:
     - Find the next meeting requiring travel or significant preparation.
     - Call a weather API (via harness tools) for that time and location, within a bounded budget.
4. **Analyze & transform**
   - Worker computes:
     - Meeting details (time, place, duration).
     - Expected weather and any implications (e.g., rain, heat).
     - A structured summary payload for the user.
5. **Produce proposals**
   - Worker returns:
     - A small memory delta capturing the upcoming meeting + expected conditions.
     - A wake signal with reason code `proactive_briefing_ready` and a payload reference.
6. **Merge & validate**
   - Harness merges the small memory delta into long-term memory.
7. **Foreground trigger**
   - Harness policy determines that proactive briefings with reason code `proactive_briefing_ready` are allowed in the morning.
   - Harness converts the wake signal into a `foreground_trigger` with input such as:
     > "Prepare a short briefing for the user about their next meeting and the expected weather. Use payload X from memory."
8. **Conscious loop execution**
   - Conscious loop wakes, receives the context (self-model, relevant memory, payload X), and:
     - Generates a user-facing message summarizing the next meeting and the weather.
     - Optionally offers actions (e.g., suggest what to bring, propose leaving time).
   - The conscious loop runs within its iteration/compute budget.
9. **Record & halt**
   - Conscious loop logs the episode and any candidate memories, then goes idle until the next trigger.

---

## 7. Summary

This architecture cleanly separates foreground cognition and background maintenance into two isolated loops, with a harness that mediates all interactions, enforces policies, manages budgets, and owns canonical storage. Conscious and unconscious processes communicate only through structured proposals and harness-managed triggers with typed reason codes, ensuring safety, observability, and long-term stability while supporting rich autonomous behavior.
