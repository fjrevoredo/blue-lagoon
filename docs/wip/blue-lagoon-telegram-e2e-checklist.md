# Blue Lagoon Telegram E2E Checklist

Use this sequence.

## 1. Start PostgreSQL

```bash
docker compose up -d postgres
```

## 2. Prepare local config and secrets

```bash
cp config/local.example.toml config/local.toml
cp .env.example .env
```

Set these correctly in `config/local.toml`:

- `telegram.foreground_binding.allowed_user_id`
- `telegram.foreground_binding.allowed_chat_id`
- `telegram.foreground_binding.internal_principal_ref`
- `telegram.foreground_binding.internal_conversation_ref`

Set the required secrets and env values in `.env`:

```bash
BLUE_LAGOON_DATABASE_URL=postgres://blue_lagoon:blue_lagoon@localhost:55432/blue_lagoon
BLUE_LAGOON_TELEGRAM_BOT_TOKEN=...
BLUE_LAGOON_FOREGROUND_ROUTE=zai/glm-5-turbo
BLUE_LAGOON_FOREGROUND_API_KEY=...
```

If the regular local database does not exist yet:

```bash
docker compose exec -T postgres psql -U blue_lagoon -d postgres -c "CREATE DATABASE blue_lagoon OWNER blue_lagoon;"
```

Automated tests now provision disposable per-test databases through the harness
test support fixtures, so manual Telegram E2E should run against the normal
local app config rather than a dedicated E2E profile.

## 3. Verify the repository baseline config

Confirm these are right in `config/default.toml`:

- `self_model.seed_path`
- optionally `model_gateway.foreground.*` if you need a non-default route
- use provider-specific config such as `[model_gateway.z_ai] api_surface = "coding"` when the provider exposes multiple API surfaces

## 4. Build the binaries once

```bash
cargo build -p workers
cargo build -p runtime
```

## 5. Apply migrations

```bash
cargo run -p runtime -- migrate
```

Timeout note:

- tune live foreground duration through `harness.default_wall_clock_budget_ms`
  and `model_gateway.foreground.timeout_ms`
- `worker.timeout_ms` and `BLUE_LAGOON_WORKER_TIMEOUT_MS` are not the live
  Telegram foreground timeout control

## 6. Sanity-check the harness boot

```bash
cargo run -p runtime -- harness --once --idle
```

## 7. If you do not already know your Telegram IDs, send a message to the bot in the target private chat, then inspect Telegram `getUpdates` manually

Copy these values:

- `message.from.id` -> `telegram.foreground_binding.allowed_user_id`
- `message.chat.id` -> `telegram.foreground_binding.allowed_chat_id`

Example:

```bash
curl "https://api.telegram.org/bot$BLUE_LAGOON_TELEGRAM_BOT_TOKEN/getUpdates"
```

## 8. Send a fresh test message to the bot in that exact private 1:1 chat

## 9. Run one live foreground cycle

```bash
cargo run -p runtime -- telegram --poll-once
```

Expected result:

- stdout prints a `PollProcessed(...)` summary
- `completed_count` should be `1`
- the bot should send a reply in Telegram
- the worker should have gone through the conscious path end to end

Failure handling expectations:

- if Telegram `getUpdates` fails, the run should fail closed and write a
  durable `telegram_fetch_failed` audit event
- if a foreground trigger is accepted, execution start, binding reconciliation,
  ingress persistence, and acceptance audit should commit together
- conversation rebinding keeps the canonical internal conversation binding and
  rewires historical ingress rows before removing superseded duplicate bindings

## 10. Verify persistence if you want to inspect the run

```bash
psql "$BLUE_LAGOON_DATABASE_URL" -c "select execution_id,status,trigger_kind,created_at,completed_at from execution_records order by created_at desc limit 5;"
psql "$BLUE_LAGOON_DATABASE_URL" -c "select episode_id,status,trigger_source,started_at,completed_at from episodes order by started_at desc limit 5;"
psql "$BLUE_LAGOON_DATABASE_URL" -c "select event_kind,severity,created_at from audit_events order by created_at desc limit 10;"
```

## 11. Re-run `--poll-once` without sending a new message only if you want to check idempotence

Because offset persistence is not implemented yet, Telegram may return the same
update again. The harness should reject re-execution via deduplication rather
than replying twice.

Useful extra checks:

```bash
cargo test -p harness --test foreground_component -- --nocapture
cargo test -p harness --test foreground_integration -- --nocapture
```

If needed, prepare a minimal known-good local override in `config/local.toml`,
not in `config/default.toml`, and do not rely on a dedicated test-only E2E
profile.
