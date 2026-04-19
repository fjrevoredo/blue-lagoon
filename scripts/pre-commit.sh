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

run_step "docker compose config" docker compose config
run_step "cargo fmt --all --check" run_cargo fmt --all --check
run_step "cargo check --workspace" run_cargo check --workspace
run_step "cargo clippy --workspace --all-targets -- -D warnings" \
  run_cargo clippy --workspace --all-targets -- -D warnings
run_step "cargo test --workspace --lib -- --nocapture" \
  run_cargo test --workspace --lib -- --nocapture
run_step "cargo test -p harness --test foreground_component -- --nocapture" \
  run_cargo test -p harness --test foreground_component -- --nocapture
run_step "cargo test -p harness --test foreground_integration -- --nocapture" \
  run_cargo test -p harness --test foreground_integration -- --nocapture
run_step "cargo test -p harness --test continuity_component -- --nocapture" \
  run_cargo test -p harness --test continuity_component -- --nocapture
run_step "cargo test -p harness --test continuity_integration -- --nocapture" \
  run_cargo test -p harness --test continuity_integration -- --nocapture

if command -v markdownlint >/dev/null 2>&1; then
  echo
  echo "==> markdownlint **/*.md"
  if markdownlint "**/*.md"; then
    :
  elif [[ "${BLUE_LAGOON_STRICT_MARKDOWNLINT:-0}" == "1" ]]; then
    echo "markdownlint failed and strict mode is enabled" >&2
    exit 1
  else
    echo "warning: markdownlint reported issues; continuing because strict mode is disabled"
  fi
else
  echo
  echo "==> markdownlint **/*.md"
  echo "skipped: markdownlint is not installed"
fi

echo
echo "pre-commit checks passed"
