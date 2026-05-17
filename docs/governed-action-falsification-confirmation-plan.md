# Governed-Action Falsification And Confirmation Plan

## Metadata

- Plan Status: COMPLETED
- Created: 2026-05-17
- Last Updated: 2026-05-17
- Owner: Coding agent
- Approval: APPROVED (user requested immediate execution)

## Status Legend

- Plan Status values: DRAFT, QUESTIONS PENDING, READY FOR APPROVAL, APPROVED, IN PROGRESS, COMPLETED, BLOCKED
- Task/Milestone Status values: TO BE DONE, IN PROGRESS, COMPLETED, BLOCKED, SKIPPED

## Goal

Execute a first-principles falsification/confirmation program for governed-action
output reliability so the next architectural decision is based on measured
evidence, not on a single error case or assumptions.

## Scope

- Define measurable hypotheses from the RCA.
- Build a reproducible experiment runner against real trace data.
- Execute experiments E1-E5 at the highest feasible fidelity in the current
  local environment.
- Produce decision-ready evidence in Markdown with concrete metrics.

## Non-Goals

- Implementing the final protocol redesign in this plan.
- Expanding governed-action capabilities or policy semantics.
- Tuning provider/model prompts as the final solution.

## Assumptions

- Local runtime DB has enough recent traces to produce meaningful directional
  evidence.
- Admin trace surfaces provide sufficient detail for classifier-level analysis.
- Experiments requiring production A/B rollout will be approximated locally via
  deterministic replay/measurement where needed.

## Open Questions

None.

## Milestones

### Milestone 1: Measurement Harness

- Status: COMPLETED
- Purpose: Create reproducible instrumentation for hypothesis testing.
- Exit Criteria: A scripted experiment runner exists, executes successfully, and
  emits structured artifacts for analysis.

#### Task 1.1: Define Experiment Metrics and Hypothesis Mapping

- Status: COMPLETED
- Objective: Convert RCA hypotheses into explicit measurable signals.
- Steps:
  1. Define metric dictionary for E1-E5 in the plan artifacts.
  2. Map each metric to concrete trace fields.
- Validation: Metric dictionary appears in analysis artifacts with field mapping.
- Notes: Reuse terminology from `docs/governed-action-output-first-principles-rca.md`.

#### Task 1.2: Implement Trace Experiment Runner

- Status: COMPLETED
- Objective: Add a reusable script that samples traces and computes failure
  taxonomy metrics.
- Steps:
  1. Add PowerShell script under `scripts/` to collect recent traces and
     `trace explain`/`trace show` evidence.
  2. Compute malformed reason categories, scenario/schema correlations, wrapper
     shape categories, and control-leak signals.
  3. Emit JSON artifact plus concise console summary.
- Validation: Script runs successfully and writes artifact files.
- Notes: Script must fail fast on command errors.

#### Task 1.3: Smoke-Validate Harness Outputs

- Status: COMPLETED
- Objective: Confirm script correctness on a small sample before full run.
- Steps:
  1. Run script with a low trace limit.
  2. Inspect output shape and category counts for obvious misclassification.
- Validation: Manual inspection confirms output structure and plausible counts.
- Notes: Adjust classifiers before full execution if needed.

### Milestone 2: Execute Falsification/Confirmation Experiments

- Status: COMPLETED
- Purpose: Produce measured evidence for each hypothesis from real traces.
- Exit Criteria: E1-E5 each have recorded measurements, observations, and
  confidence notes.

#### Task 2.1: Run Baseline Reliability Measurement (E0)

- Status: COMPLETED
- Objective: Measure current failure profile over recent traces.
- Steps:
  1. Execute script on a broad recent window.
  2. Record verdict/failure-class distribution and malformed share.
- Validation: Baseline numbers present in generated artifact and report.
- Notes: Include sampled time window and trace count.

#### Task 2.2: Run Channel-Conflation Evidence Test (E1)

- Status: COMPLETED
- Objective: Quantify control/prose channel-conflict manifestations.
- Steps:
  1. Measure frequency of non-canonical control wrappers in model outputs.
  2. Measure user-visible control-payload leak events in completed traces.
- Validation: Wrapper/leak counts captured with trace references.
- Notes: This is a falsification test for “single-case parser bug” hypothesis.

#### Task 2.3: Run Disclosure-Policy Correlation Test (E2)

- Status: COMPLETED
- Objective: Measure malformed rate by schema disclosure mode and scenario.
- Steps:
  1. Compute malformed distribution by `schema_disclosure`.
  2. Compute malformed distribution by `context_scenario`.
- Validation: Correlation tables appear in report.
- Notes: Correlation does not imply causation; report limits explicitly.

#### Task 2.4: Run Identifier-Representation Mismatch Test (E3)

- Status: COMPLETED
- Objective: Quantify errors tied to human-facing vs machine-facing ID shapes.
- Steps:
  1. Detect malformed outputs containing prefixed IDs like `task_list:<uuid>`.
  2. Detect related payload-field mismatch reasons.
- Validation: Counts and concrete trace examples recorded.
- Notes: Include representative payload excerpts in paraphrased form.

#### Task 2.5: Run Contract-Complexity Failure Surface Test (E4)

- Status: COMPLETED
- Objective: Measure how many independent schema dimensions fail.
- Steps:
  1. Bucket malformed causes into envelope/required-field/enum/wrapper/value
     families.
  2. Compute distribution across families.
- Validation: Cause-family histogram present in report.
- Notes: This supports or falsifies “single dominant parser edge” theory.

#### Task 2.6: Run Model-Sensitivity Directional Test (E5)

- Status: COMPLETED
- Objective: Evaluate whether model choice is primary root vs amplifier.
- Steps:
  1. Use trace evidence to identify whether failures happen across many error
     families under one model.
  2. Record whether evidence is sufficient/inconclusive for cross-model claims.
- Validation: Report includes explicit confidence level and gaps.
- Notes: No production model swap in this plan.

### Milestone 3: Decision Package

- Status: COMPLETED
- Purpose: Convert measurements into an actionable decision artifact.
- Exit Criteria: Decision memo states confirmed roots, falsified alternatives,
  uncertainty, and next-step options with evidence.

#### Task 3.1: Write Experiment Results Report

- Status: COMPLETED
- Objective: Publish experiment outputs in Markdown for review.
- Steps:
  1. Add a results doc under `docs/` with all experiment outputs.
  2. Include metric definitions, sample scope, and trace references.
- Validation: Report is complete and internally consistent with JSON artifact.
- Notes: Keep claims tied to measured evidence only.

#### Task 3.2: Update RCA With Confirmed/Falsified Findings

- Status: COMPLETED
- Objective: Fold measured evidence into the first-principles RCA.
- Steps:
  1. Add a “confirmed/falsified” section to the RCA doc.
  2. Update hypothesis confidence levels.
- Validation: RCA reflects executed experiment outcomes.
- Notes: Separate evidence from proposed remediation.

#### Task 3.3: Produce Decision Matrix For Next Architecture Step

- Status: COMPLETED
- Objective: Provide decision-ready options with tradeoffs.
- Steps:
  1. Summarize options (retain mixed channel vs structured-only vs hybrid).
  2. Score each option against measured failure modes and operational risk.
- Validation: Matrix appears in results report and references experiment data.
- Notes: No implementation selection in this task.

### Milestone 4: Cleanup And Final Verification

- Status: COMPLETED
- Purpose: Ensure artifacts are intentional and reproducible.
- Exit Criteria: Intermediate files are cleaned up, verification passes, and
  plan status is COMPLETED.

#### Task 4.1: Cleanup Intermediate Artifacts

- Status: COMPLETED
- Objective: Remove temporary outputs not needed for repository history.
- Steps:
  1. Keep durable script and final docs.
  2. Remove transient scratch outputs if created.
- Validation: `cmd.exe /c git status --short` contains only intentional files.
- Notes: Do not remove user-provided logs.

#### Task 4.2: Final Verification

- Status: COMPLETED
- Objective: Validate script + docs + compile/test surfaces touched by this work.
- Steps:
  1. Run formatting and targeted tests/checks.
  2. Rerun experiment script to verify reproducibility.
- Validation: Commands in `Final Verification Commands` pass.
- Notes: If any step is skipped, record reason.

## Final Verification Commands

1. `cargo fmt --all --check`
2. `cargo check -p runtime -p workers -p harness`
3. `cargo test -p runtime --test admin_cli -- --nocapture`
4. `cargo test -p workers --bin workers -- --nocapture`
5. `powershell -ExecutionPolicy Bypass -File scripts/governed-action-falsification.ps1 -TraceLimit 120`

## Approval Gate

User approved immediate execution in-thread. No additional approval gate is
required for this plan.

## Plan Self-Check

- [x] Plan location follows the default location rule.
- [x] Scope, non-goals, assumptions, and open questions are explicit.
- [x] Any unresolved open questions have been surfaced to the user.
- [x] Tasks are grouped into milestones because the plan has more than 10 tasks.
- [x] Every task has concrete steps and validation.
- [x] Every milestone has exit criteria.
- [x] Cleanup and final verification are included.
- [x] The plan avoids vague actions without concrete targets.
- [x] The plan can be executed by a coding agent without reading the original conversation.

## Execution Notes

- 2026-05-17: User requested immediate execution of falsification/confirmation
  work; plan created directly in `IN PROGRESS`.
- 2026-05-17: Task 1.1 completed from the existing RCA experiment set and
  trace-field mapping.
- 2026-05-17: Implemented `scripts/governed-action-falsification.ps1`, including
  trace sampling, malformed taxonomy extraction, correlation tables, leak scan,
  and JSON/Markdown artifact emission.
- 2026-05-17: Executed full experiment run (`TraceLimit=1500`,
  `LeakScanLimit=20`) and generated final analysis artifacts.
- 2026-05-17: Fixed runner robustness for single-item selection under strict
  mode and corrected markdown escaping for literal ``task_list:`` output.
- 2026-05-17: Published decision package docs:
  `docs/governed-action-falsification-results.md` and updated
  `docs/governed-action-output-first-principles-rca.md` with
  confirmed/falsified findings.
- 2026-05-17: Final verification commands completed successfully:
  `cargo fmt --all --check`,
  `cargo check -p runtime -p workers -p harness`,
  `cargo test -p runtime --test admin_cli -- --nocapture`,
  `cargo test -p workers --bin workers -- --nocapture`,
  `powershell -ExecutionPolicy Bypass -File scripts/governed-action-falsification.ps1 -TraceLimit 120`.
