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

Implementation plans and phase ledgers under `docs/PHASE_*` are useful project
history, but they are not the stable description of repository behavior.

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

If `BLUE_LAGOON_DATABASE_URL` is unset, the test support code starts local
PostgreSQL through `docker compose up -d postgres`. In CI or other managed
environments, set `BLUE_LAGOON_DATABASE_URL` explicitly instead.

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
- `BLUE_LAGOON_TELEGRAM_BOT_TOKEN`: Telegram bot token
- `BLUE_LAGOON_ZAI_API_KEY`: foreground model provider API key

The harness expects either:

- a packaged sibling `workers` binary next to the runtime binary, or
- an explicit worker command supplied in config or through
  `BLUE_LAGOON_WORKER_COMMAND`

The Telegram foreground path also requires:

- a configured single allowed Telegram user and private chat binding
- a valid self-model seed file
- a configured foreground model route

## Verification

The core repository verification commands are:

```bash
cargo fmt --all --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
docker compose config
```

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
