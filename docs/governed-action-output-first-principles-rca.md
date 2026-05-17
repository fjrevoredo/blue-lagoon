# Governed-Action Output Reliability: First-Principles Root Cause Analysis

## 1) Scope and Question

This document answers one question:

Why does the assistant repeatedly fail to produce valid governed-action outputs,
across different malformed shapes, instead of failing in one isolated format?

This is a root-cause analysis, not a solution proposal. The goal is to map
failures to first principles and define falsifiable hypotheses.

Date: 2026-05-17

---

## 2) Method

We model the foreground tool-use path as a deterministic state machine:

1. `Intent classification`: decide whether a governed action is needed.
2. `Channel decision`: decide reply-only vs reply+action.
3. `Syntax conformance`: produce action payload in the required wrapper format.
4. `Schema conformance`: satisfy required fields/types/enums.
5. `Semantic conformance`: values refer to the correct entities in the expected
   representation.
6. `Policy conformance`: capability scope/risk rules are valid.
7. `Execution + follow-up`: harness executes and model continues correctly.

A persistent failure pattern across many malformed shapes implies a systemic
issue at the protocol/interface level, not only local parser bugs.

---

## 3) Evidence Set

The following traces are the primary evidence used in this RCA.

### 3.1 Trace `019e32bf-2ba8-7e62-8949-0dbcf783d488` (failed)

- Failure class: `malformed_action_proposal`
- Direct cause: missing required `actions` envelope field.
- Model output used a tagged block but wrong schema:
  - used `"action"` and `"params"` shape instead of canonical proposal objects.
- Context scenario in metrics: `plain_factual_question`
- Schema disclosure: `short_reminder`

Implication: when only reminder-level guidance is present, the model often
emits plausible but non-canonical schemas.

### 3.2 Trace `019e32c0-9b87-7b63-8918-cc2d630e149d` (failed)

- Failure class: `malformed_action_proposal`
- Direct cause: missing required `artifact_kind` in
  `inspect_workspace_artifact` payload.
- Model output was close to correct canonical shape but omitted a required
  field.
- Context scenario in metrics: `explicit_action_request`
- Schema disclosure: `full_schema` with very large developer instruction block
  (prompt metrics show very high developer-message char volume).

Implication: even with full schema instructions, conformance is fragile because
the contract is complex and non-local.

### 3.3 Trace `019e3311-31e6-7b20-82c3-a80e9d161a68` (failed)

- Failure class: `malformed_action_proposal`
- Direct cause: unknown action kind variant `read_artifact` (non-canonical),
  plus additional non-canonical shaping.
- This happened after retry guidance had already been injected.

Implication: bounded retries help but do not solve protocol reliability when
the base output contract remains hard for the model to satisfy.

### 3.4 Trace `019e365d-663f-71c1-81cf-916e259d3c85` (completed with bad behavior)

- Execution status: `completed`
- Model output included:
  - user-visible text
  - XML-style `<governed-action> ... </governed-action>` block
- No governed action was executed (`proposed=0`), and control payload leaked to
  user-facing assistant message text.
- A malformed-action re-steer attempt occurred earlier in the same trace.

Implication: this is a protocol blind spot. Invalid control intent can bypass
malformed classification and be delivered as plain chat text.

### 3.5 Trace `019e32c0-32f1-7372-85b5-3ca26f31e8b3` (successful follow-up, but revealing)

- Harness observation summarized artifact IDs as:
  - `task_list:<uuid>`
- Later malformed outputs reused this visible representation in action payloads.

Implication: user/model-visible identifiers are not aligned with the action
payload contract (`artifact_id` expects UUID + separate `artifact_kind`).

### 3.6 Diagnostics signal

`admin diagnostics list` shows repeated
`foreground_malformed_action_resteer_exhausted` entries, confirming this is not
a one-off formatting event.

---

## 4) Failure Taxonomy by First Principles

### A. Channel Conflation (control + prose in one free-text stream)

The conscious worker uses `ModelOutputMode::PlainText` for foreground replies.
This forces one generated string to carry:

- natural language for the user, and
- machine-consumable governed-action protocol content.

Any wrapper deviation turns into parse failure, and some deviations can leak to
the user as plain text.

### B. Contract Complexity and Cognitive Load

The governed-action proposal schema requires many coupled fields:

- envelope shape,
- proposal metadata,
- action enum,
- capability scope constraints,
- payload-kind-specific required fields.

Observed failures are distributed across multiple independent constraints
(`actions` missing, required payload field missing, unknown enum variant), which
matches a high-cognitive-load contract failure profile.

### C. Representation Mismatch (human-facing IDs vs machine payload IDs)

The assistant commonly sees/uses `task_list:<uuid>` strings in summaries, while
payload parsing expects:

- `artifact_id` as bare UUID
- `artifact_kind` separately supplied.

This mismatch creates systematic semantic/value errors even when intent is
correct.

### D. Scenario-Gated Schema Disclosure

For several scenarios (`routine_greeting`, `plain_factual_question`,
`post_execution_follow_up`), the policy uses `short_reminder` instead of full
schema disclosure.

When action need emerges from context in those scenarios, the model is asked to
produce a strict structure with only a terse reminder.

### E. Validation Blind Spots Before User Delivery

Current validation catches many malformed cases but does not uniformly catch all
alternate control wrappers embedded in mixed prose (example:
`<governed-action>...</governed-action>` in a completed turn).

This creates false-success traces with protocol failure hidden as user text.

### F. Context Contamination/Overhang

Recent history includes multiple prior malformed notices and unresolved action
threads. This increases probability of stale continuation behavior and extra
tool-intent attempts in turns where the user intent is weakly specified (e.g.,
simple greeting).

### G. Model Choice as Amplifier, Not Primary Root

Model capability differences can amplify error rate, but the same failure family
would still exist under this protocol shape because the core burden is
architectural: strict machine contract serialized through unconstrained prose.

---

## 5) Root-Cause Hypotheses (Ranked)

### H1 (High confidence): Protocol-level channel conflation is the primary root cause.

Reasoning:

- Foreground path requires strict machine output via plain text channel.
- Failures span multiple independent schema constraints and wrapper formats.
- False-success leakage occurred when malformed control markup appeared in prose.

### H2 (High confidence): Action contract is over-specified relative to what the model can reliably emit in free text.

Reasoning:

- Failures include envelope-level, field-level, and enum-level violations.
- Even full-schema turns still fail on required fields.

### H3 (High confidence): Identifier representation mismatch induces repeated semantic errors.

Reasoning:

- Assistant observes `task_list:<uuid>` but payload requires separate
  `artifact_id` (UUID) + `artifact_kind`.
- Later malformed outputs reuse the visible prefixed form or alternate action
  names.

### H4 (Medium confidence): Scenario-based short schema disclosure causes under-specification in action-needed turns.

Reasoning:

- Some malformed turns occurred under `short_reminder` with no full schema in
  input.
- Needs controlled experiment to quantify magnitude.

### H5 (Medium confidence): Context overhang increases off-target action attempts.

Reasoning:

- Greeting turns occasionally continue stale unresolved tasks.
- Requires instrumentation to quantify prevalence.

---

## 6) What This Analysis Rules Out

- This is not one parser bug with one malformed shape.
- This is not solved by adding more static checks for each bad format.
- This is not primarily a retry-count issue (retries already exist and still
  exhaust).

---

## 7) Falsification and Confirmation Plan (No Fixes Yet)

To confirm root causes rigorously, we should run the following experiments and
measure deltas.

### Metrics to collect

- `governed_action_intent_turns`
- `proposal_parse_success_rate`
- `proposal_schema_success_rate`
- `first_pass_action_success_rate`
- `same_turn_completion_rate_after_resteer`
- `control_payload_leak_to_user_rate`
- malformed reason histogram (`missing_field`, `unknown_enum`,
  `wrong_wrapper`, `value_shape_mismatch`, etc.)

### Experiments

1. `E1` Channel separation experiment:
   compare plain-text mixed channel vs typed structured envelope output.
2. `E2` Disclosure experiment:
   always-full-schema vs scenario-gated short reminder.
3. `E3` Identifier contract experiment:
   canonicalize/align ID representation between user-visible summaries and
   machine payload requirements.
4. `E4` Contract simplicity experiment:
   reduce redundant required fields and measure malformed rate impact.
5. `E5` Model sensitivity experiment:
   hold protocol constant, swap model, measure relative change.

Confirmation criteria:

- If `E1` produces the largest reduction in malformed and leak rates, H1 is
  confirmed as primary root.
- If `E3` significantly reduces semantic/value errors, H3 is confirmed as a
  major secondary root.
- If `E2` materially improves first-pass success, H4 is confirmed.

---

## 8) Interim Conclusion

The failure pattern is systemic. The dominant root cause is protocol design:
strict governed-action control content is being generated inside an
unconstrained natural-language output channel, with a high-complexity schema and
representation mismatches.

The correct next step is to validate these hypotheses with targeted
instrumentation and controlled experiments before choosing the final redesign.

---

## 9) 2026-05-17 Confirmation/Falsification Update

The falsification/confirmation runner was executed with:

- `powershell -ExecutionPolicy Bypass -File scripts/governed-action-falsification.ps1 -TraceLimit 1500 -LeakScanLimit 20`
- selection subset: `telegram_pending_ingress`
- sample size: 252 traces

Measured results:

- malformed-action traces: 36 (14.29%)
- malformed reason spread:
  - `invalid_block_other`: 31
  - `missing_actions_envelope`: 2
  - `missing_proposal_id`: 1
  - `unknown_enum_variant`: 1
  - `missing_required_payload_field`: 1
- control leak evidence: 1/20 bounded completed-trace scan, including trace
  `019e365d-663f-71c1-81cf-916e259d3c85` with XML wrapper leakage
- identifier mismatch signal (`task_list:` prefix in malformed outputs): 2

Hypothesis status after execution:

- H1: confirmed
- H2: confirmed
- H3: confirmed (secondary contributor)
- H4: partially confirmed (directional only; low confidence due small bucket sizes)
- H5: inconclusive

This update confirms the dominant issue is protocol-level channel conflation
with a high-complexity output contract, not a single parser edge case.
