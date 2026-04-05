# blue-lagoon

Blue Lagoon is a Rust workspace for a harness-governed assistant runtime. The
repository centers on a control-plane architecture where the harness owns
budgets, policy, context assembly, canonical writes, auditability, and runtime
boundaries, while worker processes provide bounded foreground reasoning.

Project philosophy lives in `PHILOSOPHY.md`. Canonical product and architecture
specification lives in:

- `docs/REQUIREMENTS.md`
- `docs/LOOP_ARCHITECTURE.md`
- `docs/IMPLEMENTATION_DESIGN.md`

Implementation plans and detailed planning ledgers under `docs/` are useful
project history, but they are not the stable description of repository
behavior.

Planning labels belong only in planning documents. Deliverable artifacts such
as code, tests, migrations, config, workflow steps, and canonical behavior
docs should be named by capability or domain, not by sequencing labels from a
project plan.

## Workspace Layout

- `crates/runtime`: thin CLI entrypoints
- `crates/harness`: control-plane logic, policy, persistence, orchestration
- `crates/contracts`: typed cross-process and cross-boundary contracts
- `crates/workers`: worker executables and worker-facing tests
- `migrations/`: reviewed SQL migrations
- `config/default.toml`: versioned non-secret runtime configuration
- `compose.yaml`: local PostgreSQL and a minimal runtime topology

## Runtime Commands

Inspect the command surface:

```bash
cargo run -p runtime -- --help
```

Apply reviewed migrations:

```bash
cargo run -p runtime -- migrate
```

Verify the harness can boot safely and return to idle:

```bash
cargo run -p runtime -- harness --once --idle
```

Run the synthetic harness smoke path:

```bash
cargo run -p runtime -- harness --once --synthetic-trigger smoke
```

Replay one stored Telegram update through the foreground path:

```bash
cargo run -p runtime -- telegram --fixture crates/harness/tests/fixtures/telegram/private_text_message.json
```

Run one live Telegram poll cycle:

```bash
cargo run -p runtime -- telegram --poll-once
```

The `telegram` command is intentionally one-shot. Live Telegram and live model
provider checks are operator-run tasks because they require real credentials, a
bound chat, and side-effect-aware execution.

## Local Development

Start PostgreSQL:

```bash
docker compose up -d postgres
```

The default local PostgreSQL mapping is `localhost:55432`.

Validate the compose topology:

```bash
docker compose config
```

If `BLUE_LAGOON_TEST_POSTGRES_ADMIN_URL` is unset, the test support code starts
local PostgreSQL through `docker compose up -d postgres`. Automated tests do
not run against `BLUE_LAGOON_DATABASE_URL`; they create disposable databases
through the test support fixtures. In CI or other managed environments, set
`BLUE_LAGOON_TEST_POSTGRES_ADMIN_URL` to a PostgreSQL admin connection that can
create and drop per-test databases.

## Configuration

Default non-secret settings live in `config/default.toml`. Runtime secrets are
resolved through environment variables referenced by that file.

Important runtime inputs include:

- `BLUE_LAGOON_DATABASE_URL`: PostgreSQL connection string
- `BLUE_LAGOON_CONFIG`: optional config file override
- `BLUE_LAGOON_LOG`: optional tracing filter override
- `BLUE_LAGOON_WORKER_COMMAND`: explicit worker executable path when a sibling
  `workers` binary is not used
- `BLUE_LAGOON_WORKER_ARGS`: worker arguments as a JSON array of strings
- `BLUE_LAGOON_WORKER_TIMEOUT_MS`: generic worker timeout override for smoke and
  non-foreground worker launches
- `BLUE_LAGOON_TELEGRAM_BOT_TOKEN`: Telegram bot token
- `BLUE_LAGOON_FOREGROUND_ROUTE`: optional foreground route override in
  `<provider>/<exact-model>` form
- `BLUE_LAGOON_FOREGROUND_API_BASE_URL`: optional foreground provider API base
  URL override for operator/debug use
- `BLUE_LAGOON_FOREGROUND_API_KEY`: foreground model provider API key
- `BLUE_LAGOON_TEST_POSTGRES_ADMIN_URL`: optional automated-test-only PostgreSQL
  admin connection used to create and drop disposable per-test databases

The harness expects either:

- a packaged sibling `workers` binary next to the runtime binary, or
- an explicit worker command supplied in config or through
  `BLUE_LAGOON_WORKER_COMMAND`

The Telegram foreground path also requires:

- a configured single allowed Telegram user and private chat binding
- a valid self-model seed file
- a configured foreground model route

Live Telegram foreground timeout behavior is derived from the harness budget:

- foreground budget uses `harness.default_wall_clock_budget_ms`
- model-call timeout is `min(model_gateway.foreground.timeout_ms, harness.default_wall_clock_budget_ms)`
- conscious worker timeout is derived from the same harness budget plus a fixed
  grace window
- `worker.timeout_ms` and `BLUE_LAGOON_WORKER_TIMEOUT_MS` do not shorten or
  extend that live Telegram foreground worker timeout

Provider-specific foreground settings live under `model_gateway.<provider>`.
For Z.ai, the stable config is `[model_gateway.z_ai]` with:

- `api_surface = "general" | "coding"`
- optional `api_base_url` only when a nonstandard endpoint override is needed

`model_gateway.foreground.api_base_url` remains a compatibility fallback, but
provider-specific sections are the preferred repository-facing configuration.

Foreground Telegram intake is treated as an atomic accepted-trigger write path:

- execution start, binding reconciliation, ingress persistence, and acceptance
  audit commit together or not at all
- rebinding preserves the canonical internal conversation binding row and
  rewires historical ingress references before removing superseded duplicates
- live Telegram fetch failures are durably audited as ingress failures

Automated DB tests follow one repository-wide rule:

- DB-using tests must provision disposable databases from reviewed migrations
- DB-using tests must not target existing operator databases
- live manual Telegram E2E uses the normal local app config and database, not a
  dedicated test profile
- use `with_clean_database(...)` when a test needs an unmigrated DB
- use `with_migrated_database(...)` when a test needs the latest reviewed schema

## Verification

The core repository verification commands are:

```bash
cargo fmt --all --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
docker compose config
```

Matched pre-commit helper scripts are available for the same validation set:

- bash/WSL: `./scripts/pre-commit.sh`
- PowerShell: `./scripts/pre-commit.ps1`

If `markdownlint` is installed locally, the scripts run it in warning-only mode
by default because the repository-wide Markdown baseline is not yet fully
clean. Set `BLUE_LAGOON_STRICT_MARKDOWNLINT=1` to make Markdown lint failures
blocking.

Useful command-surface checks:

```bash
cargo run -p runtime -- --help
cargo run -p runtime -- harness --once --idle
cargo run -p runtime -- harness --once --synthetic-trigger smoke
cargo run -p runtime -- telegram --fixture crates/harness/tests/fixtures/telegram/private_text_message.json
```

Repository-hosted CI lives in `.github/workflows/ci.yml` and should remain a
stable workspace verification gate. Live-network Telegram and provider checks
remain intentionally outside repository-hosted CI.
