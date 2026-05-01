# Blue Lagoon

Blue Lagoon is a harness-governed assistant runtime for a single Telegram user
and chat. It runs as two long-lived services:

- `runtime harness` owns scheduling, policy, recovery, audit, background
  maintenance, approvals, and management surfaces.
- `runtime telegram` ingests Telegram updates and routes them through the
  harness-governed foreground path.

If you want the deeper operational guide, use
[`docs/USER_MANUAL.md`](docs/USER_MANUAL.md). Canonical architecture and
requirements live in:

- `docs/REQUIREMENTS.md`
- `docs/LOOP_ARCHITECTURE.md`
- `docs/IMPLEMENTATION_DESIGN.md`

## What You Need

- Rust toolchain with `cargo`
- Docker with Compose support
- A Telegram bot token
- A foreground model API key
- One allowed Telegram user ID and one allowed private chat ID

The default config expects:

- PostgreSQL on `localhost:55432`
- `BLUE_LAGOON_TELEGRAM_BOT_TOKEN` in `.env`
- `BLUE_LAGOON_FOREGROUND_API_KEY` in `.env`
- Telegram binding overrides in `config/local.toml`

## Quick Start

1. Copy the local config template:

```bash
cp config/local.example.toml config/local.toml
cp .env.example .env
```

2. Edit `config/local.toml` and set your Telegram binding:

```toml
[telegram.foreground_binding]
allowed_user_id = 123456789
allowed_chat_id = 123456789
internal_principal_ref = "primary-user"
internal_conversation_ref = "telegram-primary"
```

3. Edit `.env` and set `BLUE_LAGOON_TELEGRAM_BOT_TOKEN` and
   `BLUE_LAGOON_FOREGROUND_API_KEY`. The database URL does not need to be
   changed — Docker Compose injects the correct container-network address
   automatically.

4. Start everything:

```bash
docker compose up
```

On the first run this compiles the workspace from scratch — allow several
minutes. Subsequent starts reuse the build cache.

5. Verify the runtime state:

```bash
cargo run -p runtime -- admin status
```

## Common Commands

Check readiness and health:

```bash
cargo run -p runtime -- admin status
cargo run -p runtime -- admin health summary
```

List diagnostics and recovery state:

```bash
cargo run -p runtime -- admin diagnostics list
cargo run -p runtime -- admin recovery checkpoints list
cargo run -p runtime -- admin recovery leases list
cargo run -p runtime -- admin recovery supervise --actor-ref operator:local --reason "manual review"
```

Create or update a scheduled foreground task:

```bash
cargo run -p runtime -- admin foreground schedules upsert \
  --task-key morning-checkin \
  --internal-principal-ref primary-user \
  --internal-conversation-ref telegram-primary \
  --message-text "Morning check-in" \
  --cadence-seconds 86400 \
  --cooldown-seconds 300 \
  --actor-ref operator:local
```

Inspect scheduled foreground tasks:

```bash
cargo run -p runtime -- admin foreground schedules list
cargo run -p runtime -- admin foreground schedules list --due-only
cargo run -p runtime -- admin foreground schedules show --task-key morning-checkin
```

Inspect and resolve approvals:

```bash
cargo run -p runtime -- admin approvals list
cargo run -p runtime -- admin approvals resolve \
  --approval-request-id <uuid> \
  --decision approve \
  --actor-ref operator:local \
  --reason "approved by operator"
```

Inspect and manage identity:

```bash
cargo run -p runtime -- admin identity status
cargo run -p runtime -- admin identity show
cargo run -p runtime -- admin identity history list
cargo run -p runtime -- admin identity edit list
```

Inspect or run background maintenance:

```bash
cargo run -p runtime -- admin background list
cargo run -p runtime -- admin background enqueue --job-kind memory-consolidation
cargo run -p runtime -- admin background run-next
```

Check schema compatibility:

```bash
cargo run -p runtime -- admin schema status
cargo run -p runtime -- admin schema upgrade-path
```

## One-Shot Operator Checks

Run one background job and exit:

```bash
cargo run -p runtime -- harness --once --background-once
```

Run the synthetic smoke path:

```bash
cargo run -p runtime -- harness --once --synthetic-trigger smoke
```

Replay a stored Telegram fixture:

```bash
cargo run -p runtime -- telegram --fixture crates/harness/tests/fixtures/telegram/private_text_message.json
```

Run one live Telegram poll cycle:

```bash
cargo run -p runtime -- telegram --poll-once
```

## Notes

- The default self-model seed path is `config/self_model_seed.toml`.
- If you do not use the sibling `workers` binary in `target/debug`, set
  `BLUE_LAGOON_WORKER_COMMAND` explicitly.
- Most admin commands also support `--json` for automation.
- `runtime harness` is the service that executes scheduled foreground tasks.
- `runtime telegram` is the service that ingests live Telegram messages.

## Next Reading

- Operator guide: [`docs/USER_MANUAL.md`](docs/USER_MANUAL.md)
- Requirements: [`docs/REQUIREMENTS.md`](docs/REQUIREMENTS.md)
- Loop architecture: [`docs/LOOP_ARCHITECTURE.md`](docs/LOOP_ARCHITECTURE.md)
- Implementation design: [`docs/IMPLEMENTATION_DESIGN.md`](docs/IMPLEMENTATION_DESIGN.md)
