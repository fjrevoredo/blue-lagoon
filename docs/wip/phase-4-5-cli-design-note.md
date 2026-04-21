# Blue Lagoon

## Phase 4.5 CLI Design Note

Date: 2026-04-21
Status: Temporary design note for pre-implementation review
Scope: Narrow operator and verification CLI only

## Purpose

This note proposes the minimum operator-facing CLI slice that should be added
before Phase 5.

The goal is not to introduce a second control plane or a general-purpose admin
shell. The goal is to remove the current reliance on raw SQL, log archaeology,
and temporary manual verification documents for routine local inspection and
verification tasks.

This document is intentionally temporary and belongs under `docs/wip/` until
the command surface is reviewed and translated into a proper detailed
implementation plan.

## Why now

Phase 4 exposed a concrete operator gap:

- background-maintenance verification required raw SQL seeding of due jobs
- state inspection depended on direct database queries
- `telegram --poll-once` behavior is correct but not obvious from the command
  output alone
- temporary verification documents drifted because the runtime is now easier to
  exercise through code than through the current operator surface

If this is deferred until after Phase 5, the governed-action surface will be
broader and the operator tooling will be harder to introduce coherently.

## Design constraints

The Phase 4.5 CLI should follow these constraints:

- stay under the existing `runtime` binary
- expose explicit, narrow, operator-safe commands
- never bypass harness-owned validation or policy
- prefer domain actions over raw storage access
- support concise human-readable output first
- add machine-readable output only where it clearly helps automation
- avoid interactive shells, REPLs, TUIs, or broad arbitrary mutation commands

## Proposed command shape

The suggested top-level shape is:

```text
runtime admin ...
```

Initial subcommands should be:

- `runtime admin status`
- `runtime admin telegram status`
- `runtime admin foreground pending`
- `runtime admin background list`
- `runtime admin background enqueue`
- `runtime admin background run-next`
- `runtime admin wake-signals list`
- `runtime admin verify summary`

## Proposed commands

### `runtime admin status`

Purpose:
- quick operator snapshot of runtime readiness

Should report:
- expected schema version and current schema version
- worker resolution mode
- Telegram binding presence
- foreground model route summary
- whether required secrets are present by name, without printing secret values

This should replace the current need to mentally combine `migrate`, config
inspection, and worker-path assumptions before running local checks.

### `runtime admin telegram status`

Purpose:
- inspect recent Telegram intake state without querying tables directly

Should report:
- latest accepted ingress summary
- recent duplicate count summary
- recent normalization rejections
- any recoverable foreground conversations

This is inspection only. It should not fetch from Telegram or send messages.

### `runtime admin foreground pending`

Purpose:
- show whether any staged or recoverable foreground work exists

Should report:
- internal conversation ref
- number of pending ingress rows
- whether the conversation currently qualifies for backlog recovery

This removes one of the more awkward DB inspection tasks during local runtime
debugging.

### `runtime admin background list`

Purpose:
- inspect background jobs and recent runs

Should report:
- job id
- job kind
- trigger kind
- status
- available time
- last completion time
- most recent run status

Filtering by status and job kind would be useful, but the first version should
stay minimal.

### `runtime admin background enqueue`

Purpose:
- create one due background job safely without raw SQL

This is the highest-value operator action in the Phase 4.5 slice.

Minimum arguments should include:
- `--job-kind`
- `--conversation-ref` or another explicit scope selector
- optional `--trigger-kind`
- optional `--reason`
- optional `--available-now`

This command must call the same planning and validation path the harness uses.
It should not write directly to background tables with ad hoc SQL.

### `runtime admin background run-next`

Purpose:
- explicit operator-triggered execution of one due job

This should be a clearer operator alias around the existing one-shot background
execution path, with a more focused summary for local verification.

It should not introduce new execution semantics. It should remain a thin
operator surface over the current harness behavior.

### `runtime admin wake-signals list`

Purpose:
- inspect pending, deferred, accepted, suppressed, or rejected wake signals

Should report:
- signal id
- reason code
- priority
- status
- decision kind
- reviewed timestamp

This removes another recurring DB query pattern from local verification.

### `runtime admin verify summary`

Purpose:
- provide a compact verification snapshot for local operator use

Should report:
- recent synthetic execution result
- recent foreground execution summary
- recent background execution summary
- whether any pending foreground or background work remains

This should stay informational. It is not a replacement for the automated test
surface.

## Explicit non-goals

Phase 4.5 should not include:

- arbitrary SQL execution
- arbitrary filesystem or network tool execution
- an interactive shell or TUI
- bulk destructive admin commands
- bypasses around proposal validation, merge logic, or policy
- Phase 5 approval or governed tool execution behavior

## Output posture

The first implementation should default to readable text.

Where useful, selected commands may add:

- `--json` for machine-readable output

That should be limited to commands where structured output materially improves
automation or debugging. It should not become a blanket requirement for every
command before the operator surface is stable.

## Recommended implementation order

Suggested order:

1. `runtime admin status`
2. `runtime admin background list`
3. `runtime admin background enqueue`
4. `runtime admin background run-next`
5. `runtime admin wake-signals list`
6. `runtime admin foreground pending`
7. `runtime admin telegram status`
8. `runtime admin verify summary`

This order removes the raw-SQL background verification dependency first, which
is the most immediate operator pain point.

## Testing expectations

Phase 4.5 should add:

- CLI parsing tests for new admin subcommands
- harness-level tests for background enqueue routing through the planning path
- persistence-backed tests for listing and summary commands where semantics
  matter
- regression tests proving admin commands do not bypass policy or canonical
  validation paths

## Open questions

- whether `runtime admin background run-next` should remain a separate command
  or simply wrap `harness --once --background-once`
- whether `runtime admin telegram status` should include recent outbound reply
  summaries or stay ingress-only
- whether the first version should expose `--json` immediately or add it after
  the text output stabilizes
