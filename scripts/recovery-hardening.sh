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

run_step "cargo test -p harness --test recovery_component -- --nocapture" \
  run_cargo test -p harness --test recovery_component -- --nocapture
run_step "cargo test -p harness --test recovery_integration -- --nocapture" \
  run_cargo test -p harness --test recovery_integration -- --nocapture
run_step "cargo test -p harness --test foreground_component scheduled_foreground_recovery_clears_stranded_in_progress_task -- --nocapture" \
  run_cargo test -p harness --test foreground_component scheduled_foreground_recovery_clears_stranded_in_progress_task -- --nocapture
run_step "cargo test -p harness --test foreground_integration scheduled_foreground_runtime_recovery_finalizes_stranded_execution -- --nocapture" \
  run_cargo test -p harness --test foreground_integration scheduled_foreground_runtime_recovery_finalizes_stranded_execution -- --nocapture

echo
echo "recovery-hardening checks passed"
