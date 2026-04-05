# Blue Lagoon Phase 2 Telegram E2E Checklist

Use this sequence.

## 1. Start PostgreSQL

```bash
docker compose up -d postgres
```

## 2. Export the required env vars

```bash
export BLUE_LAGOON_DATABASE_URL='postgres://blue_lagoon:blue_lagoon@localhost:55432/blue_lagoon'
export BLUE_LAGOON_TELEGRAM_BOT_TOKEN='...'
export BLUE_LAGOON_FOREGROUND_ROUTE='zai/glm-5-turbo'
export BLUE_LAGOON_FOREGROUND_API_KEY='...'
```

## 3. Configure the Telegram binding and self-model in `config/default.toml`

Set these correctly:

- `telegram.allowed_user_id`
- `telegram.allowed_chat_id`
- `self_model.seed_path`
- optionally `model_gateway.foreground.*` if you need a non-default route

## 4. Build the binaries once

```bash
cargo build -p workers
cargo build -p runtime
```

## 5. Apply migrations

```bash
cargo run -p runtime -- migrate
```

## 6. Sanity-check the harness boot

```bash
cargo run -p runtime -- harness --once --idle
```

## 7. If you do not already know your Telegram IDs, send a message to the bot in the target private chat, then inspect Telegram `getUpdates` manually

Copy these values:

- `message.from.id` -> `telegram.allowed_user_id`
- `message.chat.id` -> `telegram.allowed_chat_id`

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
cargo test -p harness --test phase2_component -- --nocapture
cargo test -p harness --test phase2_integration -- --nocapture
```

If needed, also prepare a minimal known-good local config in
`config/default.toml`.
