#!/usr/bin/env bash
set -euo pipefail

run_cargo() {
  # In mixed WSL/Windows setups, prefer Windows cargo when it is reachable so
  # the bash helper matches the normal repository toolchain.
  if command -v cmd.exe >/dev/null 2>&1; then
    cmd.exe /c cargo "$@"
    return
  fi
  if command -v cargo >/dev/null 2>&1; then
    cargo "$@"
    return
  fi
  if [[ -x "${HOME}/.cargo/bin/cargo" ]]; then
    "${HOME}/.cargo/bin/cargo" "$@"
    return
  fi

  echo "cargo was not found on PATH and no compatible fallback was available" >&2
  exit 127
}

run_step() {
  local label="$1"
  shift

  echo
  echo "==> $label"
  "$@"
}

run_step "cargo test -p harness --test migration_component -- --nocapture" \
  run_cargo test -p harness --test migration_component -- --nocapture
run_step "cargo test -p harness --test foundation_integration synthetic_trigger_runs_end_to_end_and_persists_outputs -- --nocapture" \
  run_cargo test -p harness --test foundation_integration synthetic_trigger_runs_end_to_end_and_persists_outputs -- --nocapture
run_step "cargo test -p harness --test foundation_integration timed_out_worker_is_terminated -- --nocapture" \
  run_cargo test -p harness --test foundation_integration timed_out_worker_is_terminated -- --nocapture
run_step "cargo test -p harness --test foundation_integration timed_out_foreground_run_is_marked_failed_and_audited -- --nocapture" \
  run_cargo test -p harness --test foundation_integration timed_out_foreground_run_is_marked_failed_and_audited -- --nocapture
run_step "cargo test --workspace --lib recovery_decision_abandons_rejected_approval_fail_closed -- --nocapture" \
  run_cargo test --workspace --lib recovery_decision_abandons_rejected_approval_fail_closed -- --nocapture
run_step "cargo test --workspace --lib ensure_supported_reports_missing_pending_too_old_and_too_new_variants -- --nocapture" \
  run_cargo test --workspace --lib ensure_supported_reports_missing_pending_too_old_and_too_new_variants -- --nocapture
run_step "cargo test -p runtime --bin runtime phase_six_admin_parser_rejects_invalid_recovery_thresholds -- --nocapture" \
  run_cargo test -p runtime --bin runtime phase_six_admin_parser_rejects_invalid_recovery_thresholds -- --nocapture
run_step "cargo test -p runtime --test admin_cli phase_seven_admin_scheduled_foreground_commands_run_against_a_real_database -- --nocapture" \
  run_cargo test -p runtime --test admin_cli phase_seven_admin_scheduled_foreground_commands_run_against_a_real_database -- --nocapture
run_step "cargo test -p harness --test foreground_integration scheduled_foreground_runtime_run_executes_due_task_through_worker_binary -- --nocapture" \
  run_cargo test -p harness --test foreground_integration scheduled_foreground_runtime_run_executes_due_task_through_worker_binary -- --nocapture

echo
echo "release-readiness checks passed"
