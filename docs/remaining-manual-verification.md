# Blue Lagoon

## Remaining Manual Verification

Date: 2026-04-21
Audience: Local operator validation after rerunning the automatable continuity
and background-maintenance surface

## Purpose

This document lists only the verification work that still requires an operator.
The older continuity and background-maintenance checklists have been removed
because their database and runtime behavior is now covered by automated tests
and repeatable local commands.

## What Was Revalidated Automatically

The following behavior was rechecked locally on 2026-04-21 and does not need a
separate manual checklist anymore:

- `cargo run -p runtime -- harness --once --idle`
  - returned `IdleVerified`
- `cargo run -p runtime -- harness --once --synthetic-trigger smoke`
  - returned `SyntheticCompleted { ... }`
- continuity persistence suites
  - `cargo test -p harness --test continuity_component -- --nocapture`
  - `cargo test -p harness --test continuity_integration -- --nocapture`
- background-maintenance persistence suites
  - `cargo test -p harness --test unconscious_component -- --nocapture`
  - `cargo test -p harness --test unconscious_integration -- --nocapture`
- live local runtime verification of the background one-shot path
  - a seeded `memory_consolidation` job completed with:
    - `proposals evaluated=1`
    - `accepted=1`
    - `canonical_writes=1`
    - `retrieval_updates=1`
  - a seeded `self_model_reflection` job completed with:
    - `proposals evaluated=1`
    - `accepted=1`
    - `canonical_writes=1`
    - `wake_signals=1`
- fail-closed background behavior
  - worker timeout handling
  - malformed worker-result rejection

The old background checklist was also stale in one important way: a successful
`self_model_reflection` run does produce an accepted self-model proposal and an
active `self_model_artifacts` row. The prior manual document no longer matched
the current implementation state.

## Remaining Manual Surface

Only one operator-visible check remains:

- real Telegram ingress and reply delivery in the bound chat

Everything else that used to be in the manual documents is now either:

- covered by automated tests, or
- directly validated through repeatable local runtime commands without needing a
  human in the loop

## Operator Check: Live Telegram Round Trip

### Preconditions

- Docker is available locally.
- PostgreSQL is running from `docker compose up -d postgres`.
- `.env` contains:
  - `BLUE_LAGOON_DATABASE_URL`
  - `BLUE_LAGOON_TELEGRAM_BOT_TOKEN`
  - `BLUE_LAGOON_FOREGROUND_API_KEY`
- `config/local.toml` contains a valid `[telegram.foreground_binding]` for the
  intended private chat.
- the worker binary is available through:
  - `cargo build -p runtime -p workers`

### Steps

1. Send one new message from the configured allowed Telegram user in the
   configured allowed private chat.
2. Run:

```bash
cargo run -p runtime -- telegram --poll-once
```

3. Verify:
   - the command exits successfully
   - the printed result is `PollProcessed(TelegramProcessingSummary { ... })`
   - `completed_count` increments for the live message
   - the same Telegram chat receives exactly one assistant reply

### Optional database confirmation

If you want a DB-side confirmation after the live round trip, inspect the most
recent rows for:

- `ingress_events`
- `execution_records`
- `episodes`
- `episode_messages`
- `proposals`
- `merge_decisions`

The manual requirement is still the visible user-facing outcome: one real
accepted Telegram message produces one real assistant reply in the bound chat.

## Notes

- Stored Telegram fixture replay is not part of the remaining manual surface.
  It is environment-sensitive because it depends on the local
  `telegram.foreground_binding` identity, and the continuity/background suites
  already cover the persistence-critical behavior more reliably.
- Background-maintenance verification no longer needs a standalone operator
  checklist. The remaining operator-only concern is external side effects in the
  live Telegram channel, not the internal persistence or proposal/merge paths.
