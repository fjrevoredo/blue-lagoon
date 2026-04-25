# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

Blue Lagoon is a harness-governed assistant runtime for a single Telegram user and chat. It runs two long-lived services:

- `runtime harness` — owns scheduling, policy, recovery, audit, background maintenance, approvals, and management surfaces.
- `runtime telegram` — ingests Telegram updates and routes them through the harness-governed foreground path.

Canonical architecture and requirements live in `docs/REQUIREMENTS.md`, `docs/LOOP_ARCHITECTURE.md`, and `docs/IMPLEMENTATION_DESIGN.md`. Operational guide: `docs/USER_MANUAL.md`.

## Workspace Structure

```
crates/runtime    — thin CLI entrypoints and runtime wiring (keep thin)
crates/harness    — primary control-plane (all policy, scheduling, persistence logic)
crates/contracts  — stable shared cross-process types
crates/workers    — worker executables and worker-facing tests
migrations/       — reviewed SQL migrations (PostgreSQL 17)
config/           — default.toml (committed), local.toml + .env (untracked)
```

**Rule**: keep `crates/runtime` thin, control-plane logic in `crates/harness`, cross-process types in `crates/contracts`, worker process logic in `crates/workers`. Prefer small focused modules under `crates/harness/src/` before introducing additional top-level crates.

## Architecture: Dual-Loop System

The runtime executes a **conscious loop** (foreground) and an **unconscious loop** (background), mediated by the harness:

- **Foreground**: perception → reasoning → user interaction → action → memory recording
- **Background**: consolidation, maintenance, reflection, contradiction analysis, self-model updates
- **Harness**: central mediator, policy enforcer, context assembler, job scheduler, final authority

The conscious loop is unaware of background machinery (architectural boundary). The harness has sovereignty over all writes and policy decisions.

Key `crates/harness/src/` modules: `approval`, `audit`, `background`/`background_execution`/`background_planning`, `context`, `continuity`, `db`, `execution`, `foreground`/`foreground_orchestration`, `governed_actions`, `ingress`, `management`, `memory`, `migration`, `model_gateway`, `policy`, `proposal`, `recovery`, `retrieval`, `scheduled_foreground`, `self_model`, `telegram`, `tool_execution`, `worker`, `workspace`.

## Build and Test Commands

```bash
# Formatting and compilation
cargo fmt --all --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings

# Fast unit tests (no DB required)
cargo test --workspace --lib -- --nocapture

# PostgreSQL-backed component suites (require local postgres)
cargo test -p harness --test foreground_component -- --nocapture
cargo test -p harness --test foreground_integration -- --nocapture
cargo test -p harness --test continuity_component -- --nocapture
cargo test -p harness --test continuity_integration -- --nocapture
cargo test -p harness --test foundation_component -- --nocapture
cargo test -p harness --test foundation_integration -- --nocapture
cargo test -p harness --test migration_component -- --nocapture
cargo test -p harness --test recovery_component -- --nocapture
cargo test -p harness --test recovery_integration -- --nocapture
cargo test -p harness --test unconscious_component -- --nocapture
cargo test -p harness --test unconscious_integration -- --nocapture
cargo test -p harness --test governed_actions_component -- --nocapture
cargo test -p harness --test governed_actions_integration -- --nocapture
cargo test -p harness --test management_component -- --nocapture
cargo test -p harness --test management_integration -- --nocapture
cargo test -p harness --test artifact_naming -- --nocapture
cargo test -p runtime --test admin_cli -- --nocapture

# Full repository test surface
cargo test --workspace

# Pre-commit bundle (bash/WSL or PowerShell)
./scripts/pre-commit.sh
./scripts/pre-commit.ps1
```

**Test layering**: run unit/crate-local tests first, then the broader suite, then runtime command checks for command-surface or migration changes.

## Database Tests

Automated tests must use disposable per-test PostgreSQL databases provisioned by the test harness — never target `BLUE_LAGOON_DATABASE_URL` or any operator database.

- `with_clean_database(...)` — for unmigrated DB scenarios
- `with_migrated_database(...)` — for normal migrated persistence tests

## Running the Stack

```bash
# Start full stack (first run compiles from scratch; allow several minutes)
docker compose up

# Start just PostgreSQL
docker compose up -d postgres

# Apply migrations
cargo run -p runtime -- migrate

# Verify topology config
docker compose config
```

## One-Shot Operator Checks

```bash
cargo run -p runtime -- harness --once --idle                        # verify safe harness boot
cargo run -p runtime -- harness --once --background-once             # run one background job
cargo run -p runtime -- harness --once --synthetic-trigger smoke     # smoke path
cargo run -p runtime -- telegram --fixture crates/harness/tests/fixtures/telegram/private_text_message.json
cargo run -p runtime -- telegram --poll-once
```

## Admin CLI

```bash
cargo run -p runtime -- admin status
cargo run -p runtime -- admin health summary
cargo run -p runtime -- admin diagnostics list
cargo run -p runtime -- admin recovery checkpoints list
cargo run -p runtime -- admin recovery leases list
cargo run -p runtime -- admin schema status
cargo run -p runtime -- admin schema upgrade-path
cargo run -p runtime -- admin foreground pending
cargo run -p runtime -- admin background list
cargo run -p runtime -- admin background enqueue --job-kind <job-kind>
cargo run -p runtime -- admin background run-next
cargo run -p runtime -- admin approvals list
cargo run -p runtime -- admin approvals resolve --approval-request-id <uuid> --decision <approve|reject> --actor-ref operator:local --reason "..."
cargo run -p runtime -- admin actions list
cargo run -p runtime -- admin workspace artifacts list
cargo run -p runtime -- admin workspace scripts list
cargo run -p runtime -- admin workspace runs list
```

Most admin commands support `--json` for automation.

## Config Boundaries

| File | Purpose | Committed |
|---|---|---|
| `config/default.toml` | versioned non-secret defaults | yes |
| `config/local.example.toml` | template for operator overrides | yes |
| `config/local.toml` | local operator overrides | no |
| `config/self_model_seed.toml` | runtime self-model seed | yes |
| `.env.example` | env var template | yes |
| `.env` | secrets and env overrides | no |

Do not reintroduce `BLUE_LAGOON_CONFIG` as a public/operator workflow.

## Git and Line-Ending Note

This repository is used from both WSL and Windows. Line-ending normalization can make WSL Git report false-positive worktree changes. **Windows Git is the source of truth** for status, diff, and staging. Before treating unexpected changes as real, verify with `cmd.exe /c git status --short` and `cmd.exe /c git diff --name-only`.

## Document Hierarchy

- `PHILOSOPHY.md`, `README.md`, `AGENTS.md` — repository identity and decision principles
- `docs/REQUIREMENTS.md`, `docs/LOOP_ARCHITECTURE.md`, `docs/IMPLEMENTATION_DESIGN.md` — canonical product and architecture guidance
- `docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md` — roadmap and sequencing
- `docs/sources/` — research inputs, not canonical behavior
- `docs/archive/` — historical context only, not current guidance

Planning labels (Phase N, etc.) belong only in planning documents, never in code, tests, migrations, config, or canonical docs.
