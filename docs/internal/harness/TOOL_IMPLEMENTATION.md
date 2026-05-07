# Tool Implementation

---

## 1. Overview

This guide describes how to add one architecture-compliant conscious-loop tool
end to end. In this repository, a model-usable tool is normally a governed
action: the conscious worker proposes JSON, and the harness validates, audits,
routes approval, executes, and returns a bounded observation.

Use context injection instead of a governed action only when a small bounded
summary is safer than letting the model query state.

---

## 2. Implementation

### Source Files

| File | Relevant symbol |
|---|---|
| `crates/contracts/src/lib.rs` | `GovernedActionKind` (line 714), `GovernedActionPayload` (line 867) |
| `crates/harness/src/governed_actions.rs` | `execute_governed_action()` (line 442), `validate_capability_scope()` (line 1370), `CanonicalGovernedActionPayload` (line 2917) |
| `crates/harness/src/policy.rs` | `classify_governed_action_risk()` (line 168) |
| `crates/workers/src/main.rs` | `governed_action_schema_message()` (line 592), `governed_action_kind_as_str()` (line 718) |
| `crates/harness/tests/governed_actions_component.rs` | governed-action component tests |
| `migrations/` | reviewed SQL migrations for constrained action-kind columns |

### E2E Steps

1. Define the contract.
   Add a `GovernedActionKind` variant, one payload struct, and one
   `GovernedActionPayload` variant. Use explicit fields and stable serde names.

2. Add persistence compatibility.
   If `action_kind` is stored in constrained `TEXT` columns, add the next
   reviewed migration that drops and recreates checks on
   `governed_action_executions` and `approval_requests`. Never rely on editing
   an already-applied migration to add a new action kind; existing operator
   databases need a forward migration.

3. Add parsing and canonicalization.
   Update `governed_action_kind_as_str()`, `parse_governed_action_kind()`, and
   `CanonicalGovernedActionPayload`. Canonicalization must normalize strings
   that affect duplicate detection or approval fingerprints.

4. Add validation.
   Extend `validate_proposal_shape()` for structural checks and
   `validate_capability_scope()` for policy checks. Reject mismatched
   `action_kind` and payload variants.

5. Classify risk and approval posture.
   Add an arm in `classify_governed_action_risk()`. Read-only inspection should
   normally be Tier 0. Future user contact, script authoring, external network
   access, filesystem writes, or background work should be Tier 1 or higher.

6. Implement the backend.
   Add a dispatch arm in `execute_governed_action()`. Use existing harness
   services where possible. Do not let the conscious worker directly mutate
   canonical memory, self-model state, schedules, workspace tables, background
   jobs, or wake signals.

7. Format observations.
   Return bounded, model-facing summaries and store full audit details in the
   execution record or subsystem tables. Include truncation metadata when output
   is shortened.

8. Update worker schema exposure.
   Add the action to `governed_action_schema_message()`. Include a complete
   payload example and a complete `capability_scope` posture so the model can
   produce deserializable JSON.

9. Update every exhaustive match.
   Typical files include `approval.rs`, `management.rs`, `recovery.rs`,
   `foreground_orchestration.rs`, `workers/src/main.rs`, and tests. Do not add
   wildcard arms for action kinds.

   In recovery, classify only read-only or provably idempotent actions as
   replay-safe. Any action that can create, update, schedule, delegate, execute,
   fetch, or otherwise produce side effects must be ambiguous or nonrepeatable
   unless its backend has explicit durable idempotency semantics.

10. Add tests.
    Cover contract serialization, DB constraints, planning, validation failure,
    successful execution, approval routing where relevant, and recovery or
    management visibility if the action creates new state.

11. Update docs.
    Update `docs/internal/conscious_loop/GOVERNED_ACTIONS.md`, any affected
    context assembly docs, and user/operator docs if behavior is user-visible.
    Re-stamp verified dates after line references are checked.

### Philosophy Checklist

- Harness-heavy: the harness owns validation, execution, storage, and audit.
- Conscious boundary: the model proposes; it does not mutate canonical state.
- Traceability: proposal, risk, approval, execution, and output are persisted.
- Bounded context: observations are concise and never dump unbounded output.
- Explicit recovery: stalled or ambiguous execution has a recovery posture.
- No raw operator dependency: operators can inspect state without SQL when the
  tool creates durable operational state.

---

## 3. Configuration & Extension

Add config only when the tool has a real operator-tunable limit or policy knob.
Defaults belong in `config/default.toml`; local overrides belong in untracked
`config/local.toml`. Update `RuntimeConfig::validate()` and all test config
constructors when a new field is required.

When choosing a risk tier, treat delayed user contact, future execution,
background job creation, network access, filesystem writes, and script
authoring as side effects even if nothing externally visible happens during the
current turn.

Required validation commands for most tool changes:

- `cargo test -p contracts --lib -- --nocapture`
- `cargo test -p harness --test governed_actions_component -- --nocapture`
- `cargo test -p harness --test governed_actions_integration -- --nocapture`
- `cargo test -p workers -- --nocapture`
- `cargo test -p harness --test migration_component -- --nocapture`
- `cargo check --workspace`

---

## 4. Further Reading

- `docs/internal/conscious_loop/GOVERNED_ACTIONS.md`: live governed-action
  schema, action list, validation, and risk table.
- `docs/internal/conscious_loop/CONTEXT_ASSEMBLY.md`: where action schemas and
  observations enter conscious context.
- `docs/LOOP_ARCHITECTURE.md`: canonical boundary between conscious,
  unconscious, and harness responsibilities.
- `docs/IMPLEMENTATION_DESIGN.md`: canonical implementation constraints.

---

*Last verified: branch `usage-improvements`, session 2026-04-29.*
