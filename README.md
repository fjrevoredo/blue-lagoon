# blue-lagoon

Phase 1 establishes a runnable Rust workspace with a harness-owned migration
path, schema safety checks, a no-trigger idle boot path, and a synthetic
end-to-end worker flow.

## Phase 1 Commands

Start PostgreSQL:

```bash
docker compose up -d postgres
```

The default local host mapping is `localhost:55432`.

Apply reviewed migrations:

```bash
cargo run -p runtime -- migrate
```

Verify the harness can boot safely and return to idle:

```bash
cargo run -p runtime -- harness --once --idle
```

Before running the synthetic smoke trigger, make sure the harness can locate a
worker executable. The intended v1 posture is an explicit worker binary rather
than an implicit `cargo run` fallback. For local development, either:

- build the worker binary first with `cargo build -p workers`, or
- set `BLUE_LAGOON_WORKER_COMMAND` explicitly to the worker executable path

If `BLUE_LAGOON_WORKER_ARGS` is needed, provide it as a JSON array of strings
rather than as shell-split text.

Run the Phase 1 synthetic smoke trigger:

```bash
cargo run -p runtime -- harness --once --synthetic-trigger smoke
```

## Phase 2 Foreground Commands

Phase 2 adds the first Telegram foreground runtime slice. The runtime crate
stays thin and delegates foreground orchestration, provider routing, policy, and
canonical writes to `crates/harness`.

Before running the Phase 2 foreground commands, make sure:

- PostgreSQL is running and the reviewed migrations have been applied
- the harness can locate a worker executable, either through a sibling
  `workers` binary or `BLUE_LAGOON_WORKER_COMMAND`
- `BLUE_LAGOON_TELEGRAM_BOT_TOKEN` is set for the configured bot
- `BLUE_LAGOON_ZAI_API_KEY` is set for the configured foreground model route
- the configured Telegram user or chat binding points at the intended private
  1:1 conversation
- the configured self-model seed path resolves to a real file

Run a fixture-driven Telegram foreground execution:

```bash
cargo run -p runtime -- telegram --fixture crates/harness/tests/fixtures/telegram/private_text_message.json
```

Run a one-shot live Telegram poll:

```bash
cargo run -p runtime -- telegram --poll-once
```

`runtime telegram` is intentionally one-shot. `--fixture` replays a stored
Telegram update through the full foreground path and still uses the configured
model gateway and Telegram delivery boundary, so it should be run only against
the intended bound chat. `--poll-once` fails closed when Phase 2 Telegram
configuration is absent.

Run the baseline verification suite:

```bash
cargo fmt --all --check
cargo check --workspace
cargo test --workspace
docker compose config
```

`cargo test --workspace` boots local PostgreSQL with `docker compose up -d
postgres` when `BLUE_LAGOON_DATABASE_URL` is unset. Repository-hosted CI sets
`BLUE_LAGOON_DATABASE_URL` explicitly and uses a disposable GitHub Actions
PostgreSQL service instead.

## Phase 1.1 CI Baseline

The baseline repository-hosted workflow is `.github/workflows/ci.yml`. This is
the starting point for repository CI, not a one-off Phase 1 workflow. Later
phases should extend this baseline or add adjacent stable gates without
replacing its role as the core workspace verification path.

- Workflow name: `CI`
- Required check name: `workspace-verification`
- Triggers: `pull_request` and pushes to `master`

The Phase 1.1 local-to-CI command mapping is:

- `cargo fmt --all --check` -> `cargo fmt --all --check`
- `cargo check --workspace` -> `cargo check --workspace`
- `cargo test -p harness --test phase2_component -- --nocapture` ->
  `cargo test -p harness --test phase2_component -- --nocapture`
- `cargo test -p harness --test phase2_integration -- --nocapture` ->
  `cargo test -p harness --test phase2_integration -- --nocapture`
- `cargo test --workspace` -> `cargo test --workspace`

The following recurring Phase 1 commands remain intentionally outside the Phase
1.1 automation scope and must still be run locally when relevant:

- `docker compose config`
- `cargo run -p runtime -- migrate`
- `cargo run -p runtime -- --help`
- `cargo run -p runtime -- harness --once --idle`
- `cargo run -p runtime -- harness --once --synthetic-trigger smoke`
- `cargo run -p runtime -- telegram --fixture <fixture-path>`
- `cargo run -p runtime -- telegram --poll-once`

Live Telegram-network and live provider-network verification remain intentionally
outside repository-hosted CI. The required automated suites use fake Telegram
and provider boundaries, while live `telegram` command checks stay operator-run
because they require bound credentials, a real chat, and side-effect-aware
execution posture.

Repository follow-up after the workflow lands:

- require the `workspace-verification` check on `master` branch protection or the
  repository's equivalent ruleset
- keep this workflow as the foundation and extend the CI surface in later
  phases with additional stable checks as new runtime capabilities land
- note that no repository-hosted live-network run evidence is recorded from the
  current implementation environment; live Telegram or provider checks remain
  documented local operator tasks
- release, deployment, and publish automation remain deferred
