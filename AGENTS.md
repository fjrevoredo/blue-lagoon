# Repository Guidelines

## Project Structure & Module Organization
`blue-lagoon` is now a Rust workspace with implementation code plus canonical
design docs. Root documents such as `README.md` and `PHILOSOPHY.md` define
project identity and decision principles. Authoritative design material lives in
`docs/`, especially `docs/REQUIREMENTS.md`, `docs/LOOP_ARCHITECTURE.md`,
`docs/IMPLEMENTATION_DESIGN.md`, `docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md`, and
`docs/PHASE_1_DETAILED_IMPLEMENTATION_PLAN.md`.

Runtime code lives under `crates/`:

- `crates/runtime`: thin CLI entrypoints and runtime wiring
- `crates/harness`: primary control-plane crate
- `crates/contracts`: stable shared cross-process types
- `crates/workers`: worker executables and worker-facing tests

Operational assets live at the repository root:

- `migrations/`: reviewed SQL migrations
- `config/default.toml`: versioned non-secret config
- `compose.yaml`: local PostgreSQL plus runtime topology

Use `docs/sources/` for research inputs and external references. Historical
handover documents live in `docs/archive/` and should be treated as archived
context rather than current canonical guidance.

## Build, Test, and Development Commands
Typical implementation workflow is:

- `rg --files` to inspect the repository quickly.
- `cargo fmt --all --check` to verify formatting.
- `cargo check --workspace` to verify compilation.
- `cargo clippy --workspace --all-targets -- -D warnings` to keep linting clean.
- `cargo test --workspace` to run unit, component, and integration tests.
- `docker compose config` to verify the local runtime topology.
- `docker compose up -d postgres` to start disposable local PostgreSQL.
- `cargo run -p runtime -- migrate` to apply reviewed migrations.
- `cargo run -p runtime -- harness --once --idle` to verify safe harness boot.
- `cargo run -p runtime -- harness --once --synthetic-trigger smoke` to run the
  Phase 1 end-to-end smoke path.
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

When modifying code, prefer to run the lowest effective layer first, then rerun
the relevant broader suite:

- unit and crate-local tests for pure logic
- `cargo test --workspace` for repository-level validation
- runtime command checks for command-surface or migration changes

Documentation testing remains manual. Verify that section hierarchy is
consistent, terminology matches `PHILOSOPHY.md`, `docs/REQUIREMENTS.md`,
`docs/LOOP_ARCHITECTURE.md`, and `docs/IMPLEMENTATION_DESIGN.md`, and
cross-document claims do not conflict. Re-read modified files in rendered
Markdown when possible. Treat broken links, contradictory definitions, and
unclear scope boundaries as defects that must be fixed before merge.

## Commit & Pull Request Guidelines
Existing history uses short, direct subjects such as `initial docs`. Keep commit messages brief and imperative, and avoid bundling unrelated document changes together. Pull requests should state which documents changed, why the change is needed, and any open questions or follow-up work. Include screenshots only when a rendered artifact or visual document output materially changes.

## Source Material Handling
Do not treat `docs/sources/` as canonical product behavior; it is evidence and research context. Do not treat `docs/archive/` as current product guidance; it is retained for historical traceability. Promote conclusions into `docs/REQUIREMENTS.md`, `docs/LOOP_ARCHITECTURE.md`, `docs/IMPLEMENTATION_DESIGN.md`, or other canonical docs only after they are cleaned up, reconciled, and stated as repository-approved guidance.
