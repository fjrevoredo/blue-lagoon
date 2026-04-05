# Blue Lagoon

## Phase 1.1 Detailed Implementation Plan

Date: 2026-04-05
Status: Repository implementation landed, hosted-run evidence pending
Scope: High-level plan Phase 1.1 only
Audience: LLM-assisted implementation work and human review

## Purpose

This document defines the detailed implementation plan for Phase 1.1 of Blue
Lagoon.

It translates the approved Phase 1.1 scope from
`docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md` into concrete, trackable, and
LLM-executable work items.

Phase 1.1 is intentionally narrow. Its purpose is to establish the first
permanent GitHub Actions baseline that runs the implemented Phase 1
verification on the repository host before Phase 2 expands the runtime
surface.

## Canonical inputs

This plan is subordinate to the following canonical documents:

- `PHILOSOPHY.md`
- `docs/REQUIREMENTS.md`
- `docs/LOOP_ARCHITECTURE.md`
- `docs/IMPLEMENTATION_DESIGN.md`
- `docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md`
- `docs/PHASE_1_DETAILED_IMPLEMENTATION_PLAN.md`

If this document conflicts with the canonical documents, the canonical
documents win.

## Phase 1.1 target

Phase 1.1 is complete only when Blue Lagoon has a minimal repository-hosted
CI/CD baseline that proves the following:

- GitHub Actions runs automatically for pull requests and pushes to the default
  integration branch
- the minimum automated Phase 1.1 verification subset runs in
  repository-hosted automation:
  `cargo fmt --all --check`, `cargo check --workspace`, and
  `cargo test --workspace`
- persistence-critical tests that require PostgreSQL run in CI rather than
  being skipped silently
- the first required CI gate names are documented clearly enough for branch
  protection or equivalent repository settings
- local verification commands and repository-hosted workflow steps are aligned
- the baseline workflow and required check identities are stable enough to
  remain in place as later phases extend CI
- commands not yet automated in Phase 1.1 are documented explicitly as deferred
- at least one successful repository-hosted workflow run is recorded as phase
  evidence

## Settled implementation clarifications

The following Phase 1.1 decisions are treated as settled unless later canonical
documents intentionally change them:

- GitHub Actions is the repository-hosted automation system for the initial
  CI/CD baseline.
- Phase 1.1 is a CI/CD bootstrap phase, not a general release-automation or
  deployment-automation phase.
- The workflow and required check names established in Phase 1.1 should be
  stable repository identities that later phases extend rather than
  phase-specific names that would need to be replaced.
- The minimum workflow must run on `pull_request` and on pushes to the default
  integration branch.
- `cargo test --workspace` is mandatory in the minimum workflow.
- `cargo fmt --all --check` and `cargo check --workspace` should be part of the
  same baseline unless a temporary blocker is documented explicitly.
- Disposable PostgreSQL must be provisioned in GitHub Actions for
  persistence-critical tests.
- Phase 1.1 does not need to automate every recurring Phase 1 verification
  command. The minimum required automated scope is `cargo fmt --all --check`,
  `cargo check --workspace`, and `cargo test --workspace` with PostgreSQL
  available for the persistence-critical tests.
- `docker compose config`, `cargo run -p runtime -- migrate`,
  `cargo run -p runtime -- --help`,
  `cargo run -p runtime -- harness --once --idle`, and
  `cargo run -p runtime -- harness --once --synthetic-trigger smoke` remain
  documented local verification commands until a later CI-expansion phase
  chooses to automate them.
- The Phase 1.1 workflow should avoid unnecessary secrets and should rely on
  repository-local configuration wherever feasible.
- If branch-protection or required-check configuration cannot be expressed
  fully in the repository, the required manual repository settings must be
  documented as part of the phase output.
- Completion of Phase 1.1 requires at least one successful GitHub Actions run
  of the minimum workflow to be recorded in the task evidence.
- No production deployment, package publishing, release tagging, or environment
  promotion is required in Phase 1.1.

## LLM execution rules

The plan should be executed under the following rules:

- Work one task at a time unless a task is explicitly marked as parallel-safe.
- Do not start a task until all of its dependencies are marked `DONE`.
- No task is complete without the verification listed for it.
- Prefer the narrowest workflow that still proves the required repository-hosted
  gates.
- Keep local command names and CI step names intentionally aligned.
- Use disposable PostgreSQL for persistence-critical verification.
- Update this document immediately after finishing each task.

## Progress tracking protocol

This document is the progress ledger for Phase 1.1.

Each task contains:

- a stable task ID
- a `Status` field
- explicit dependencies
- concrete deliverables
- verification commands or checks
- an `Evidence` field to update when done

Use only these status values:

- `TODO`
- `IN PROGRESS`
- `BLOCKED`
- `DONE`

## Progress snapshot

- Current milestone: `Milestone B`
- Current active task: `P1.1-06`
- Completed tasks: `7/9`
- Milestone A status: `DONE`
- Milestone B status: `BLOCKED`
- Milestone C status: `BLOCKED`

Repository sequencing note:

- Tasks `P1.1-07` and `P1.1-08` were completed in the same patch set as the
  workflow changes so the repository documentation would remain aligned with
  the committed CI baseline while `P1.1-06` waits on the first GitHub-hosted
  run.

## Expected Phase 1.1 verification commands

These are the intended recurring verification commands for this phase:

- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo test --workspace`
- review of at least one successful GitHub Actions run for the minimum workflow
- manual review that the GitHub Actions workflow triggers, step names, and
  required check names match the documented CI baseline

## Phase 1.1 milestones

- Milestone A: workflow baseline and trigger definition
- Milestone B: repository-hosted Rust and PostgreSQL verification
- Milestone C: CI gate documentation and completion gate

## Milestone A quality gate

Milestone A is green only if:

- a GitHub Actions workflow file exists under `.github/workflows/`
- the workflow has the minimum required triggers
- the workflow name and required check names are defined clearly
- the workflow and required check names are suitable as long-term repository
  gates rather than one-phase-only labels
- workflow permissions are explicit and minimal

## Milestone B quality gate

Milestone B is green only if:

- the workflow installs the required Rust toolchain
- the workflow provisions disposable PostgreSQL for persistence-critical tests
- the minimum required verification commands are executed in the workflow
- repository-hosted failures would fail the workflow rather than being skipped
- at least one repository-hosted workflow run has completed successfully and
  its evidence is recorded

## Milestone C quality gate

Milestone C is green only if:

- local verification commands are mapped to the repository-hosted workflow
- commands intentionally left outside the Phase 1.1 automation scope are
  documented explicitly
- required check names and any manual repository settings are documented
- the baseline workflow is documented as the starting point for later CI
  expansion rather than as a disposable one-off
- canonical status docs are ready to move from Phase 1.1 to Phase 2 after
  completion
- this document reflects the final task status and evidence

## Task list

### Task P1.1-01: Define the minimum CI gate surface

- Status: `DONE`
- Depends on: none
- Parallel-safe: no
- Deliverables:
  - the minimum workflow name
  - the minimum trigger set
  - the required Phase 1.1 commands to run in CI
  - the list of recurring Phase 1 verification commands intentionally deferred
    from CI in Phase 1.1
  - the first required check names for repository settings
- Verification:
  - manual review against `docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md`
- Evidence:
  - Minimum gate defined as workflow `CI` with required check
    `workspace-verification`, triggers `pull_request` and push to `master`,
    and required commands `cargo fmt --all --check`,
    `cargo check --workspace`, and `cargo test --workspace`; deferred commands
    and baseline-extension posture documented in `README.md`.

### Task P1.1-02: Create the initial GitHub Actions workflow scaffold

- Status: `DONE`
- Depends on: `P1.1-01`
- Parallel-safe: no
- Deliverables:
  - `.github/workflows/ci.yml` or equivalent canonical workflow file
  - explicit workflow triggers
  - explicit workflow permissions
  - a clear job structure for the minimum CI gate
- Verification:
  - manual review that the workflow structure matches the agreed scope
- Evidence:
  - `.github/workflows/ci.yml` created with explicit `pull_request` and push
    to `master` triggers, explicit `contents: read` permissions, and a single
    minimum-gate job named `workspace-verification`.

### Task P1.1-03: Add Rust setup and repository checkout steps

- Status: `DONE`
- Depends on: `P1.1-02`
- Parallel-safe: yes
- Deliverables:
  - repository checkout step
  - Rust toolchain setup step
  - any minimal cache posture judged worthwhile for the baseline
- Verification:
  - manual review that the workflow can execute Rust commands deterministically
- Evidence:
  - `.github/workflows/ci.yml` includes `actions/checkout@v4`,
    `dtolnay/rust-toolchain@stable` with `rustfmt`, and
    `Swatinem/rust-cache@v2`.

### Task P1.1-04: Add disposable PostgreSQL service wiring

- Status: `DONE`
- Depends on: `P1.1-02`
- Parallel-safe: yes
- Deliverables:
  - GitHub Actions PostgreSQL service or equivalent disposable database posture
  - CI environment wiring for `BLUE_LAGOON_DATABASE_URL`
  - readiness handling sufficient for repository-hosted test execution
- Verification:
  - manual review that the workflow can run persistence-critical tests without
    manual operator setup
- Evidence:
  - `.github/workflows/ci.yml` provisions a PostgreSQL 17 service with health
    checks and sets `BLUE_LAGOON_DATABASE_URL` for the job;
    `crates/harness/tests/support/mod.rs` now reuses
    `BLUE_LAGOON_DATABASE_URL` when present and skips local `docker compose`
    bootstrap in that case.

### Task P1.1-05: Add formatting and compile verification to CI

- Status: `DONE`
- Depends on: `P1.1-03`
- Parallel-safe: yes
- Deliverables:
  - `cargo fmt --all --check` workflow step
  - `cargo check --workspace` workflow step
  - fail-fast or equivalent clear failure behavior
- Verification:
  - local commands match the workflow steps exactly
- Evidence:
  - `.github/workflows/ci.yml` runs named steps `cargo fmt --all --check` and
    `cargo check --workspace`; verified locally on 2026-04-05 with
    `cmd.exe /c cargo fmt --all --check` and
    `cmd.exe /c cargo check --workspace`.

### Task P1.1-06: Add repository-hosted workspace test execution

- Status: `BLOCKED`
- Depends on: `P1.1-03`, `P1.1-04`
- Parallel-safe: no
- Deliverables:
  - `cargo test --workspace` workflow step
  - CI execution posture that includes persistence-critical PostgreSQL tests
  - no silent skip of the required Phase 1 automated coverage
- Verification:
  - at least one successful GitHub Actions run of the minimum workflow is
    recorded
  - manual review that the workflow would fail if the workspace tests fail
- Evidence:
  - `.github/workflows/ci.yml` runs `cargo test --workspace` against the
    disposable PostgreSQL service and local verification passed on 2026-04-05
    with `cmd.exe /c cargo test --workspace`; the remaining blocker is that a
    real GitHub-hosted workflow run cannot be executed or recorded from the
    current environment.

### Task P1.1-07: Document local-to-CI command mapping

- Status: `DONE`
- Depends on: `P1.1-05`, `P1.1-06`
- Parallel-safe: yes
- Deliverables:
  - documentation update describing the minimum CI workflow
  - mapping between local verification commands and the workflow steps
  - explicit list of recurring Phase 1 verification commands deferred from
    Phase 1.1 automation
  - any repository-level assumptions needed for contributors
- Verification:
  - manual review that the documented commands and workflow steps match
- Evidence:
  - `README.md` documents `.github/workflows/ci.yml`, maps the three Phase 1.1
    local commands to the workflow step names, and lists the recurring Phase 1
    commands intentionally deferred from automation while stating that the
    baseline workflow is the foundation for later CI expansion.

### Task P1.1-08: Document required repository settings follow-up

- Status: `DONE`
- Depends on: `P1.1-07`
- Parallel-safe: yes
- Deliverables:
  - documented required check names
  - documented branch-protection or equivalent repository-setting follow-up if
    it cannot be encoded directly in the repository
  - explicit note that Phase 1.1 does not yet include release or deployment
    automation
- Verification:
  - manual review that a maintainer could configure the required repository gate
- Evidence:
  - `README.md` documents the required check name `workspace-verification`, the
    follow-up to require it on `master` branch protection or an equivalent
    ruleset, the expectation that later phases extend this baseline, and the
    explicit deferment of release or deployment automation.

### Task P1.1-09: Close the phase and update the progress ledger

- Status: `BLOCKED`
- Depends on: `P1.1-08`
- Parallel-safe: no
- Deliverables:
  - this document updated with final statuses
  - milestone state updated to `DONE` where justified
  - `docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md` updated to reflect Phase 1.1
    completion and Phase 2 as the next phase
  - any follow-on notes required for Phase 2 CI/CD extension planning
- Verification:
  - manual review that all completed tasks contain evidence
- Evidence:
  - Final closure is blocked until `P1.1-06` records at least one successful
    GitHub Actions run; `docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md` was updated to
    reflect repository implementation progress, but Phase 1.1 is not yet marked
    complete.

## Recommended execution order

Execute Phase 1.1 in this order unless a justified change is written into this
document first:

1. `P1.1-01`
2. `P1.1-02`
3. `P1.1-03`
4. `P1.1-04`
5. `P1.1-05`
6. `P1.1-06`
7. `P1.1-07`
8. `P1.1-08`
9. `P1.1-09`

## Phase 1.1 definition of done

Phase 1.1 is done only when all of the following are true:

- all tasks required for Milestones A through C are marked `DONE`
- all milestone quality gates are green
- the minimum GitHub Actions workflow exists and is documented
- the minimum required Phase 1.1 verification subset is represented in
  repository-hosted automation
- persistence-critical tests are accounted for in the CI posture
- at least one successful repository-hosted workflow run is recorded in the
  evidence for this phase
- the progress ledger in this document is up to date
- canonical status docs are updated so planning can proceed to Phase 2
- the repository state is good enough to begin Phase 2 without re-opening the
  minimum CI/CD baseline

## Next document after this phase

Once Phase 1.1 is complete and the progress ledger is current, the next
planning document should be the detailed implementation plan for Phase 2.
