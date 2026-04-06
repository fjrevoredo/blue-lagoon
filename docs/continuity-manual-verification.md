# Blue Lagoon

## Continuity Manual Verification

Date: 2026-04-06
Audience: Local operator verification after the continuity and backlog-recovery slice

## Purpose

This checklist defines the local manual verification steps for the continuity,
retrieval, canonical proposal-and-merge, self-model persistence, and
backlog-aware foreground recovery behavior now present in Blue Lagoon.

This is an operator-facing checklist. It complements automated tests and CI; it
does not replace them.

## Preconditions

Before starting, make sure the following are true:

- Docker is available locally.
- PostgreSQL can be started with `docker compose up -d postgres`.
- `config/local.toml` exists and contains the local Telegram foreground binding.
- `.env` exists and contains `BLUE_LAGOON_DATABASE_URL`,
  `BLUE_LAGOON_TELEGRAM_BOT_TOKEN`, and `BLUE_LAGOON_FOREGROUND_API_KEY`.
- The local database URL points at the disposable local PostgreSQL from
  `compose.yaml`, normally `postgres://blue_lagoon:blue_lagoon@localhost:55432/blue_lagoon`.
- The runtime can find the sibling worker binary, usually by running
  `cargo build -p runtime -p workers` before the manual checks.

## Step 1: Verify the local topology and schema baseline

Run:

```bash
docker compose up -d postgres
docker compose config
cargo run -p runtime -- migrate
```

Verify:

- PostgreSQL becomes healthy.
- `docker compose config` renders without errors.
- the migration command reports version `4` as discovered and applied
- the continuity tables exist in PostgreSQL:

```sql
SELECT tablename
FROM pg_tables
WHERE schemaname = 'public'
  AND tablename IN (
    'proposals',
    'merge_decisions',
    'memory_artifacts',
    'self_model_artifacts',
    'retrieval_artifacts',
    'execution_ingress_links'
  )
ORDER BY tablename;
```

## Step 2: Verify safe harness boot

Run:

```bash
cargo run -p runtime -- harness --once --idle
```

Verify:

- the command exits successfully
- the printed outcome is `IdleVerified`
- no schema-compatibility error is reported

## Step 3: Verify the synthetic harness path still works

Run:

```bash
cargo run -p runtime -- harness --once --synthetic-trigger smoke
```

Verify:

- the command exits successfully
- the printed outcome is `SyntheticCompleted { ... }`
- a new `execution_records` row exists with `trigger_kind = 'synthetic'`

Example query:

```sql
SELECT execution_id, status, trigger_kind, synthetic_trigger
FROM execution_records
WHERE trigger_kind = 'synthetic'
ORDER BY created_at DESC
LIMIT 3;
```

## Step 4: Verify normal foreground fixture replay

Run:

```bash
cargo run -p runtime -- telegram --fixture crates/harness/tests/fixtures/telegram/private_preference_message.json
```

Verify:

- the command exits successfully
- the printed summary reports `completed_count: 1`
- the allowed Telegram chat receives one assistant reply
- one new `episodes` row and at least one new `episode_messages` row are present
- one or more `proposals` rows and matching `merge_decisions` rows are present
- at least one active `memory_artifacts` row exists for the conversation subject
- at least one active `self_model_artifacts` row exists
- retrieval rows exist for the episode and any accepted memory artifact

Useful queries:

```sql
SELECT proposal_kind, canonical_target, status, created_at
FROM proposals
ORDER BY created_at DESC
LIMIT 10;
```

```sql
SELECT decision_kind, decision_reason, created_at
FROM merge_decisions
ORDER BY created_at DESC
LIMIT 10;
```

```sql
SELECT artifact_kind, subject_ref, status, created_at
FROM memory_artifacts
ORDER BY created_at DESC
LIMIT 10;
```

```sql
SELECT artifact_origin, status, created_at
FROM self_model_artifacts
ORDER BY created_at DESC
LIMIT 10;
```

```sql
SELECT source_kind, status, relevance_timestamp, created_at
FROM retrieval_artifacts
ORDER BY created_at DESC
LIMIT 10;
```

## Step 5: Verify later continuity retrieval and self-model carry-forward

Run:

```bash
cargo run -p runtime -- telegram --fixture crates/harness/tests/fixtures/telegram/private_preference_followup.json
```

Verify:

- the command exits successfully
- the printed summary reports `completed_count: 1`
- the allowed Telegram chat receives one assistant reply
- a new `foreground_context_assembled` audit event exists for the latest run
- the latest audit payload shows:
  - `foreground_execution_mode = "normal"`
  - `self_model_source_kind = "canonical_artifact"`
  - at least one retrieved context item when prior memory is relevant, including
    semantically related phrasing rather than only exact token overlap where the
    retrieval baseline has a meaningful synonym or concept match

Useful query:

```sql
SELECT payload
FROM audit_events
WHERE event_kind = 'foreground_context_assembled'
ORDER BY created_at DESC
LIMIT 1;
```

## Step 6: Verify backlog-aware foreground recovery

Run:

```bash
cargo run -p runtime -- telegram --fixture crates/harness/tests/fixtures/telegram/private_text_backlog_batch.json
```

Verify:

- the command exits successfully
- the printed summary reports `completed_count: 1`
- the printed summary reports `backlog_recovery_count: 1`
- the allowed Telegram chat receives one assistant reply, not one reply per
  delayed ingress
- the selected ingress rows end in `foreground_status = 'processed'`
- the execution has multiple rows in `execution_ingress_links`
- the latest `foreground_context_assembled` audit payload shows:
  - `foreground_execution_mode = "backlog_recovery"`
  - `recovery_ingress_count > 1`
  - multiple `recovery_ingress_ids`

The same recovery mode should also apply when a prior foreground attempt left
multiple ingress rows in stale `processing` state and a later Telegram cycle
resumes that conversation after degraded operation.

Useful queries:

```sql
SELECT ingress_id, foreground_status, occurred_at, last_processed_at
FROM ingress_events
WHERE internal_conversation_ref IS NOT NULL
ORDER BY occurred_at DESC
LIMIT 10;
```

```sql
SELECT execution_id, ingress_id, link_role, sequence_index
FROM execution_ingress_links
ORDER BY created_at DESC
LIMIT 20;
```

```sql
SELECT payload
FROM audit_events
WHERE event_kind = 'foreground_context_assembled'
ORDER BY created_at DESC
LIMIT 1;
```

## Step 7: Verify canonical-write auditability

Run:

```sql
SELECT event_kind, payload
FROM audit_events
WHERE event_kind IN (
  'proposal_evaluated',
  'merge_decision_recorded',
  'canonical_write_applied'
)
ORDER BY created_at DESC
LIMIT 20;
```

Verify:

- each accepted proposal has a matching merge decision
- `canonical_write_applied` events appear only for accepted canonical writes
- rejected proposals remain visible in audit history without producing accepted
  canonical targets

## Step 8: Re-run the automated local verification surface

Run:

```bash
./scripts/pre-commit.sh
```

or on Windows:

```powershell
./scripts/pre-commit.ps1
```

Verify:

- the script completes successfully
- the script runs format, compile, clippy, unit, foreground, and continuity
  verification without requiring manual edits between steps

## Expected outcome

The continuity slice is locally verified when all of the following are true:

- the continuity schema is applied and queryable
- the harness boots safely and the smoke path still succeeds
- normal Telegram foreground replay creates proposals, merge decisions, memory
  artifacts, retrieval artifacts, and self-model state
- later replay uses canonical continuity state in assembled context
- backlog replay collapses delayed ingress into one recovery-aware foreground
  execution with one reply
- audit history exposes proposal evaluation, merge outcomes, canonical writes,
  and recovery-aware context metadata
- the local pre-commit verification bundle completes successfully
