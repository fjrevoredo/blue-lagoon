# Blue Lagoon User Manual

## Purpose

This manual describes the normal operator workflows for the shipped Blue Lagoon
runtime.

Use this document when you want to:

- set up the runtime locally
- start or stop the services
- manage scheduled foreground tasks
- review approvals, diagnostics, and recovery state
- run safe one-shot checks
- troubleshoot common local problems

## Runtime Model

Blue Lagoon runs as three main local pieces:

- PostgreSQL stores canonical runtime state.
- `runtime harness` owns policy, scheduling, recovery, approvals, background
  maintenance, and the management CLI.
- `runtime telegram` polls Telegram and routes incoming updates into the
  foreground path.

The normal live setup is one Telegram bot, one allowed user, and one allowed
private chat.

## First-Time Setup

### 1. Prepare local config

Create local operator files:

```bash
cp config/local.example.toml config/local.toml
cp .env.example .env
```

Edit `config/local.toml`:

```toml
[telegram.foreground_binding]
allowed_user_id = 123456789
allowed_chat_id = 123456789
internal_principal_ref = "primary-user"
internal_conversation_ref = "telegram-primary"
```

Edit `.env` and set:

- `BLUE_LAGOON_DATABASE_URL`
- `BLUE_LAGOON_TELEGRAM_BOT_TOKEN`
- `BLUE_LAGOON_FOREGROUND_API_KEY`

The default checked-in self-model seed is `config/self_model_seed.toml`. The
default provider route comes from `config/default.toml`.

### 2. Start PostgreSQL

```bash
docker compose up -d postgres
```

Validate the compose file if needed:

```bash
docker compose config
```

### 3. Apply migrations

```bash
cargo run -p runtime -- migrate
```

### 4. Build the runtime and worker binaries

```bash
cargo build -p runtime -p workers
```

The harness expects either:

- a sibling `workers` binary in the normal target directory, or
- an explicit `BLUE_LAGOON_WORKER_COMMAND`

### 5. Verify safe startup

```bash
cargo run -p runtime -- harness --once --idle
```

## Starting the System

Start the harness service:

```bash
cargo run -p runtime -- harness
```

Start the Telegram service in another terminal:

```bash
cargo run -p runtime -- telegram
```

Verify readiness:

```bash
cargo run -p runtime -- admin status
```

Useful quick checks:

```bash
cargo run -p runtime -- admin health summary
cargo run -p runtime -- admin diagnostics list
```

## Normal Daily Workflows

### Talking to the assistant

Once both services are running, send a Telegram message from the configured
allowed user and private chat. The Telegram service ingests the message, and
the harness-governed foreground path produces the response.

For one-shot live ingestion without running the long-lived poller:

```bash
cargo run -p runtime -- telegram --poll-once
```

For fixture replay:

```bash
cargo run -p runtime -- telegram --fixture crates/harness/tests/fixtures/telegram/private_text_message.json
```

### Assistant-managed tools

The assistant can now use the harness-governed foreground path to inspect,
create, and update workspace notes, runbooks, scratchpads, task lists, and
scripts. It can also list script run history, propose scheduled foreground
tasks, request bounded background maintenance, run approved workspace scripts,
and fetch web pages through the approval-gated `web_fetch` action.

These capabilities are not native provider tool calls. The model proposes a
governed action, the harness validates and audits it, and higher-risk actions
are routed through the normal approval flow before execution. Operators can
inspect the resulting state with:

```bash
cargo run -p runtime -- admin actions list
cargo run -p runtime -- admin workspace artifacts list
cargo run -p runtime -- admin workspace scripts list
cargo run -p runtime -- admin workspace runs list
cargo run -p runtime -- admin foreground schedules list
cargo run -p runtime -- admin background list
```

### Managing scheduled foreground tasks

Create or update a schedule:

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

Important schedule fields:

- `--task-key` identifies the task
- `--cadence-seconds` controls the recurrence interval
- `--cooldown-seconds` delays retries after suppression or failure
- `--status` can be `active`, `paused`, or `disabled`
- `--next-due-at` can be used to set an explicit first run time

Inspect schedules:

```bash
cargo run -p runtime -- admin foreground schedules list
cargo run -p runtime -- admin foreground schedules list --status active
cargo run -p runtime -- admin foreground schedules list --due-only
cargo run -p runtime -- admin foreground schedules show --task-key morning-checkin
```

Pending or stalled foreground ingestion state is separate:

```bash
cargo run -p runtime -- admin foreground pending
```

### Reviewing approvals

List current approvals:

```bash
cargo run -p runtime -- admin approvals list
```

Resolve an approval:

```bash
cargo run -p runtime -- admin approvals resolve \
  --approval-request-id <uuid> \
  --decision approve \
  --actor-ref operator:local \
  --reason "approved by operator"
```

Use `reject` instead of `approve` when needed.

### Background maintenance

Inspect recent jobs:

```bash
cargo run -p runtime -- admin background list
```

Enqueue one job:

```bash
cargo run -p runtime -- admin background enqueue --job-kind memory-consolidation
```

Run one due background job immediately:

```bash
cargo run -p runtime -- admin background run-next
```

Supported `--job-kind` values are:

- `memory-consolidation`
- `retrieval-maintenance`
- `contradiction-and-drift-scan`
- `self-model-reflection`

### Recovery and diagnostics

List recent diagnostics:

```bash
cargo run -p runtime -- admin diagnostics list
```

List recovery checkpoints:

```bash
cargo run -p runtime -- admin recovery checkpoints list
```

List active worker leases:

```bash
cargo run -p runtime -- admin recovery leases list
```

Run manual lease supervision:

```bash
cargo run -p runtime -- admin recovery supervise \
  --soft-warning-threshold-percent 80 \
  --actor-ref operator:local \
  --reason "manual supervision pass"
```

### Schema and upgrades

Check current schema compatibility:

```bash
cargo run -p runtime -- admin schema status
cargo run -p runtime -- admin schema upgrade-path
```

Apply reviewed migrations:

```bash
cargo run -p runtime -- migrate
```

## Safe One-Shot Verification

Verify harness startup only:

```bash
cargo run -p runtime -- harness --once --idle
```

Run one due background-maintenance job:

```bash
cargo run -p runtime -- harness --once --background-once
```

Run the synthetic smoke path:

```bash
cargo run -p runtime -- harness --once --synthetic-trigger smoke
```

## Updating the Runtime

When updating local code:

1. Pull the latest changes.
2. Rebuild binaries:

```bash
cargo build -p runtime -p workers
```

3. Reapply migrations if needed:

```bash
cargo run -p runtime -- migrate
```

4. Restart `runtime harness` and `runtime telegram`.
5. Recheck:

```bash
cargo run -p runtime -- admin status
```

## Troubleshooting

### The assistant does not reply in Telegram

Check:

- PostgreSQL is running
- `runtime harness` is running
- `runtime telegram` is running
- `BLUE_LAGOON_TELEGRAM_BOT_TOKEN` is set
- `BLUE_LAGOON_FOREGROUND_API_KEY` is set
- `config/local.toml` has the correct `allowed_user_id` and `allowed_chat_id`
- the `workers` binary exists or `BLUE_LAGOON_WORKER_COMMAND` is set

Then run:

```bash
cargo run -p runtime -- admin status
cargo run -p runtime -- admin diagnostics list
```

### Scheduled tasks are not firing

Check:

```bash
cargo run -p runtime -- admin foreground schedules list --due-only
cargo run -p runtime -- admin diagnostics list
cargo run -p runtime -- admin recovery checkpoints list
cargo run -p runtime -- admin recovery leases list
```

Also confirm that `runtime harness` is running, because scheduled foreground
tasks are executed by the harness service, not by the Telegram poller.

### Approval requests are stuck

Use:

```bash
cargo run -p runtime -- admin approvals list
cargo run -p runtime -- admin actions list
```

If needed, resolve the request explicitly through `admin approvals resolve`.

### Schema or migration problems

Use:

```bash
cargo run -p runtime -- admin schema status
cargo run -p runtime -- admin schema upgrade-path
```

Then apply migrations:

```bash
cargo run -p runtime -- migrate
```

### Runtime starts but workers do not launch

Build the binaries again:

```bash
cargo build -p runtime -p workers
```

If you run the binary from a nonstandard location, set
`BLUE_LAGOON_WORKER_COMMAND` explicitly.

## Automation-Friendly Output

Most admin commands support `--json`. Use that when:

- feeding output into scripts
- building local automation around approvals or diagnostics
- verifying state in CI or local guard scripts

## Reference

- Quick start: [`../README.md`](../README.md)
- Requirements: [`REQUIREMENTS.md`](REQUIREMENTS.md)
- Loop architecture: [`LOOP_ARCHITECTURE.md`](LOOP_ARCHITECTURE.md)
- Implementation design: [`IMPLEMENTATION_DESIGN.md`](IMPLEMENTATION_DESIGN.md)
