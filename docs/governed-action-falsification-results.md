# Governed-Action Falsification/Confirmation Results

Date: 2026-05-17

## 1) Scope And Run Configuration

- Runner: `scripts/governed-action-falsification.ps1`
- Command:
  - `powershell -ExecutionPolicy Bypass -File scripts/governed-action-falsification.ps1 -TraceLimit 1500 -LeakScanLimit 20`
- Selection policy:
  - sampled `trace recent` window size: 1500
  - measured set restricted to `telegram_pending_ingress` traces
- Artifacts:
  - `docs/analysis/governed-action-falsification-latest.json`
  - `docs/analysis/governed-action-falsification-latest.md`

## 2) Baseline (E0)

- Recent traces returned: 1500
- Selected traces analyzed: 252
- Malformed-action traces: 36
- Malformed rate: 0.1429 (14.29%)

Failure-class distribution (selected highlights):

- `malformed_action_proposal`: 36
- `worker_protocol_failure`: 18
- `persistence_failure`: 16
- `model_gateway_transport_failure`: 6
- `scheduled_foreground_validation_failure`: 5

## 3) Experiment Results

### E1: Channel-Conflation Evidence

Measured signals:

- Malformed reason distribution:
  - `invalid_block_other`: 31 / 36 malformed traces
  - `missing_actions_envelope`: 2 / 36
  - `missing_proposal_id`: 1 / 36
  - `unknown_enum_variant`: 1 / 36
  - `missing_required_payload_field`: 1 / 36
- Malformed output shape:
  - `plain_text_or_other`: 35 / 36
  - `tool_wrapper_json`: 1 / 36
- Control leak scan on completed traces:
  - leaks detected: 1 / 20
  - concrete leak: trace `019e365d-663f-71c1-81cf-916e259d3c85` (`xml_wrapper`)

Interpretation:

- Confirmed: malformed behavior is not a single parser edge case.
- Confirmed: mixed prose/control output is the dominant observed malformed shape.
- Confirmed: control payload can leak into user-visible messages.

### E2: Schema-Disclosure Correlation

Malformed rate by `schema_disclosure`:

- `short_reminder`: 4 / 8 (0.50)
- `full_schema`: 1 / 4 (0.25)
- `<none>`: 31 / 239 (0.1297)

Malformed rate by `context_scenario`:

- `plain_factual_question`: 2 / 3 (0.6667)
- `explicit_action_request`: 1 / 2 (0.50)
- `routine_greeting`: 2 / 5 (0.40)
- `<none>`: 31 / 239 (0.1297)

Interpretation:

- Directional support for disclosure/scenario effects exists, but sample sizes
  in labeled buckets are very small; this is suggestive, not conclusive.

### E3: Identifier-Representation Mismatch

Measured signals:

- malformed outputs containing `task_list:` prefix: 2
- representative traces include:
  - `019e3318-0576-7070-8849-a4e1dc5b0089`
  - `019e365d-663f-71c1-81cf-916e259d3c85` (control leak case)

Interpretation:

- Confirmed as a recurring contributor, but not the majority failure mode in
  this sample.

### E4: Contract-Complexity Failure Surface

Observed malformed causes span multiple independent contract dimensions:

- envelope-level (`missing_actions_envelope`)
- required metadata/field (`missing_proposal_id`, `missing_required_payload_field`)
- enum/action naming (`unknown_enum_variant`)
- wrapper/shape-level (`invalid_block_other`)

Interpretation:

- Confirmed: failure surface is multi-dimensional, consistent with contract
  complexity under free-text emission.

### E5: Model-Sensitivity Directional Test

Measured signals:

- distinct models in sample: 3
  - `deepseek/deepseek-v4-flash`
  - `nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free`
  - `glm-5-turbo`

Interpretation:

- Evidence is sufficient to reject "single-model-only" explanation.
- This run is not sufficient to estimate causal per-model effect sizes because
  scenario mix and prompt conditions are uncontrolled.

## 4) Hypothesis Status

- H1 (channel conflation primary root): **Confirmed**
- H2 (contract complexity is structurally brittle in free text): **Confirmed**
- H3 (ID representation mismatch contributes recurrently): **Confirmed (secondary)**
- H4 (scenario/disclosure gating materially affects malformed rate): **Partially confirmed, low confidence**
- H5 (context overhang drives off-target actions): **Not falsified; still inconclusive from this run**

## 5) Falsified Alternatives

- "This is one isolated parser bug": **Falsified**
- "Retries alone are enough": **Falsified** (malformed remains recurrent and multi-shape)
- "Model swap alone should solve it": **Falsified as primary explanation**

## 6) Decision Matrix (What To Do Next)

| Option | Description | Fit Against Measured Failures | Operational Risk | Expected Reliability Gain |
|---|---|---|---|---|
| A | Keep mixed prose/control channel; add more parser tolerance/checks | Poor (addresses symptoms, not root) | High (new blind spots) | Low |
| B | Structured-only control channel + separate user-facing message channel | Strong (directly targets H1/H2/H4) | Medium (protocol migration work) | High |
| C | Hybrid: structured channel primary, legacy text parser fallback temporarily | Strong near-term, safer rollout than B | Medium | High (with deprecation plan) |

Recommendation:

- Proceed with **Option C** as transition, with a strict deadline to remove
  legacy text control parsing after staged validation.
- Do not invest further in broad parser-tolerance expansion as primary strategy.

## 7) Known Limits Of This Run

- Correlation tables have small labeled buckets (`short_reminder`, `full_schema`,
  scenario labels), so effect sizes are directional.
- Leak scan used bounded sample (`LeakScanLimit=20`), intentionally fast.
- No controlled A/B in this execution; this is production-trace observational evidence.
