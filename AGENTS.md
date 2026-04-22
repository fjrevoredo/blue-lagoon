# Repository Guidelines

## Project Structure & Module Organization

`blue-lagoon` is a Rust workspace with implementation code plus canonical
design docs. Root documents such as `README.md` and `PHILOSOPHY.md` define
repository identity and decision principles. Authoritative product and
architecture guidance lives in `docs/`, especially `docs/REQUIREMENTS.md`,
`docs/LOOP_ARCHITECTURE.md`, and `docs/IMPLEMENTATION_DESIGN.md`.

Planning material also lives under `docs/`, but it should be treated according
to purpose:

- `docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md`: repository roadmap and sequencing
- detailed implementation plans under `docs/`: implementation ledgers and
  historical execution records, not canonical runtime behavior

Runtime code lives under `crates/`:

- `crates/runtime`: thin CLI entrypoints and runtime wiring
- `crates/harness`: primary control-plane crate
- `crates/contracts`: stable shared cross-process types
- `crates/workers`: worker executables and worker-facing tests

Operational assets live at the repository root:

- `migrations/`: reviewed SQL migrations
- `config/default.toml`: versioned non-secret config
- `config/local.example.toml`: template for untracked local operator overrides
- `compose.yaml`: local PostgreSQL plus runtime topology

Use `docs/sources/` for research inputs and external references. Historical
handover documents live in `docs/archive/` and should be treated as archived
context rather than current canonical guidance.

Planning labels belong in planning documents only. Code, tests, migrations,
config, workflow steps, and canonical behavior docs must use domain or
capability names rather than project-sequencing labels.

## Build, Test, and Development Commands

Typical implementation workflow is:

- `rg --files` to inspect the repository quickly.
- `cargo fmt --all --check` to verify formatting.
- `cargo check --workspace` to verify compilation.
- `cargo clippy --workspace --all-targets -- -D warnings` to keep linting clean.
- `cargo test --workspace --lib -- --nocapture` to run the fast workspace
  library and unit verification surface.
- `cargo test -p harness --test foreground_component -- --nocapture` to run
  the PostgreSQL-backed foreground component suite.
- `cargo test -p harness --test foreground_integration -- --nocapture` to run
  the foreground runtime integration suite.
- `cargo test -p harness --test continuity_component -- --nocapture` to run
  the canonical continuity component suite.
- `cargo test -p harness --test continuity_integration -- --nocapture` to run
  the canonical continuity integration suite.
- `cargo test -p harness --test unconscious_component -- --nocapture` to run
  the PostgreSQL-backed background-maintenance component suite.
- `cargo test -p harness --test unconscious_integration -- --nocapture` to run
  the architecture-critical background-maintenance integration suite.
- `cargo test -p runtime --test admin_cli -- --nocapture` to run the runtime
  management CLI surface tests.
- `cargo test -p runtime --bin runtime -- --nocapture` to run the runtime
  command-surface unit tests, including the Phase 5 management admin parsers
  and text formatters.
- `cargo test -p harness --test management_component -- --nocapture` to run
  the PostgreSQL-backed management CLI component suite.
- `cargo test -p harness --test management_integration -- --nocapture` to run
  the architecture-critical management CLI integration suite.
- `cargo test -p harness --test governed_actions_component -- --nocapture` to
  run the PostgreSQL-backed governed-action component suite for workspace,
  approvals, capability scoping, and blocked-action diagnostics.
- `cargo test -p harness --test governed_actions_integration -- --nocapture`
  to run the governed-action integration suite for proposal, approval,
  execution, and blocked-action foreground flows.
- `cargo test --workspace` to run the full repository test surface when a
  broader local check is warranted.
- `docker compose config` to verify the local runtime topology.
- `./scripts/pre-commit.sh` to run the standard pre-commit verification bundle
  from bash/WSL.
- `./scripts/pre-commit.ps1` to run the same pre-commit verification bundle
  from PowerShell.
- `BLUE_LAGOON_STRICT_MARKDOWNLINT=1 ./scripts/pre-commit.sh` or
  `BLUE_LAGOON_STRICT_MARKDOWNLINT=1 ./scripts/pre-commit.ps1` to make the
  optional Markdown lint step blocking once the repository Markdown baseline is
  ready for it.
- `docker compose up -d postgres` to start local PostgreSQL.
- `cargo run -p runtime -- migrate` to apply reviewed migrations.
- `cargo run -p runtime -- --help` to inspect the stable CLI surface.
- `cargo run -p runtime -- harness --once --idle` to verify safe harness boot.
- `cargo run -p runtime -- harness --once --background-once` to execute one
  due background-maintenance job through the harness one-shot path.
- `cargo run -p runtime -- harness --once --synthetic-trigger smoke` to run the
  synthetic harness smoke path.
- `cargo run -p runtime -- telegram --fixture <fixture-path>` to replay one
  stored Telegram update through the foreground path.
- `cargo run -p runtime -- telegram --poll-once` to perform one live Telegram
  poll cycle.
- `cargo run -p runtime -- admin --help` to inspect the durable management CLI
  surface.
- `cargo run -p runtime -- admin status` to inspect runtime readiness and
  pending-work state without raw SQL.
- `cargo run -p runtime -- admin foreground pending` to inspect pending or
  recoverable foreground work.
- `cargo run -p runtime -- admin background list` to inspect recent background
  job state and latest run outcomes.
- `cargo run -p runtime -- admin background enqueue --job-kind <job-kind>` to
  enqueue one background-maintenance job through the harness-owned planning
  path.
- `cargo run -p runtime -- admin background run-next` to execute one due
  background-maintenance job through the focused management surface.
- `cargo run -p runtime -- admin wake-signals list` to inspect recent
  wake-signal state.
- `cargo run -p runtime -- admin approvals list` to inspect approval request
  state without raw SQL.
- `cargo run -p runtime -- admin approvals resolve --approval-request-id <uuid> --decision <approve|reject>` to
  resolve one approval request through the canonical approval path when bounded
  operator intervention is required.
- `cargo run -p runtime -- admin actions list` to inspect recent governed
  action execution state, including blocked and approval-gated actions.
- `cargo run -p runtime -- admin workspace artifacts list` to inspect recent
  workspace artifact summaries.
- `cargo run -p runtime -- admin workspace scripts list` to inspect recent
  workspace script summaries.
- `cargo run -p runtime -- admin workspace runs list` to inspect recent
  workspace script run history, optionally filtered by script.
- `cp config/local.example.toml config/local.toml` to prepare local Telegram
  binding overrides.
- `cp .env.example .env` to prepare local runtime secrets and env overrides.
- `cargo build -p runtime -p workers` to ensure the default sibling worker
  lookup works before manual runtime verification.
- `git diff -- docs/ PHILOSOPHY.md README.md AGENTS.md` to review documentation
  changes before commit.
- `git log --oneline` to match existing commit style.
- `markdownlint "**/*.md"` if available locally, to catch heading and spacing issues.
Git environment rule:

- This repository is commonly used from both WSL and Windows, and line-ending
  normalization can make WSL Git report false-positive worktree changes.
- Windows Git is the source of truth for repository status, diff, and staging
  decisions when WSL Git and Windows Git disagree.
- Before treating unexpected worktree changes as real, verify with
  `cmd.exe /c git status --short` and `cmd.exe /c git diff --name-only`.
- Prefer Windows Git output when deciding what actually changed, what should be
  committed, and whether unrelated modifications are present.

## Coding Style & Naming Conventions

Rust code should preserve the current workspace boundary posture: keep
`crates/runtime` thin, keep control-plane logic in `crates/harness`, keep
cross-process types in `crates/contracts`, and keep worker process logic in
`crates/workers`. Prefer small focused modules under `crates/harness/src/`
before introducing additional top-level crates.

Write Markdown with clear ATX headings (`#`, `##`, `###`) and short paragraphs.
Keep language precise, technical, and directive. In formal specs, preserve
normative terms such as `MUST`, `SHOULD`, and `MAY`. Follow existing naming
patterns: top-level canonical documents use uppercase names like
`PHILOSOPHY.md`, while supporting material under `docs/sources/` and archived
handovers under `docs/archive/` use descriptive lowercase kebab-case filenames.

## Testing Guidelines

Automated testing is required for implementation work. Fast unit tests should
cover local logic, while persistence-critical behavior must be verified against
disposable real PostgreSQL through the harness component and integration tests.

Database-using automated tests must follow the repository fixture pattern:

- provision a disposable per-test PostgreSQL database
- apply reviewed migrations inside test support, not by pointing at an existing
  operator database
- never target `BLUE_LAGOON_DATABASE_URL` or another shared operator database
  from automated tests
- reserve the regular local app config and database for manual runtime and
  Telegram E2E validation
- use `with_clean_database(...)` for unmigrated DB scenarios
- use `with_migrated_database(...)` for normal migrated persistence tests

Local operator config must stay separated from repository config:

- keep committed repo-safe defaults in `config/default.toml`
- keep local non-secret operator overrides in untracked `config/local.toml`
- keep local secrets and env-style overrides in untracked `.env`
- do not reintroduce `BLUE_LAGOON_CONFIG` as a public/operator workflow

When modifying code, prefer to run the lowest effective layer first, then rerun
the relevant broader suite:

- unit and crate-local tests for pure logic
- `cargo test --workspace` for repository-level validation
- runtime command checks for command-surface or migration changes

Documentation testing remains manual. Verify that section hierarchy is
consistent, terminology matches `PHILOSOPHY.md`, `docs/REQUIREMENTS.md`,
`docs/LOOP_ARCHITECTURE.md`, and `docs/IMPLEMENTATION_DESIGN.md`, and
cross-document claims do not conflict. `README.md` and `AGENTS.md` should stay
repository-oriented and stable rather than being written as handoff notes or
temporary execution status reports. Re-read modified files in rendered
Markdown when possible. Treat broken links, contradictory definitions, and
unclear scope boundaries as defects that must be fixed before merge.

## Commit & Pull Request Guidelines

Existing history uses short, direct subjects such as `initial docs`. Keep
commit messages brief and imperative, and avoid bundling unrelated document
changes together. Pull requests should state which documents changed, why the
change is needed, and any open questions or follow-up work. Include screenshots
only when a rendered artifact or visual document output materially changes.

## Source Material Handling

Do not treat `docs/sources/` as canonical product behavior; it is evidence and
research context. Do not treat `docs/archive/` as current product guidance; it
is retained for historical traceability. Promote conclusions into
`docs/REQUIREMENTS.md`, `docs/LOOP_ARCHITECTURE.md`,
`docs/IMPLEMENTATION_DESIGN.md`, or other canonical docs only after they are
cleaned up, reconciled, and stated as repository-approved guidance.
