$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Invoke-Step {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Label,

        [Parameter(Mandatory = $true)]
        [scriptblock]$Action
    )

    Write-Host ""
    Write-Host "==> $Label"
    & $Action
    if ($LASTEXITCODE -ne 0) {
        throw "step '$Label' failed with exit code $LASTEXITCODE"
    }
}

Invoke-Step "cargo test -p harness --test migration_component -- --nocapture" {
    cargo test -p harness --test migration_component -- --nocapture
}
Invoke-Step "cargo test -p harness --test foundation_integration synthetic_trigger_runs_end_to_end_and_persists_outputs -- --nocapture" {
    cargo test -p harness --test foundation_integration synthetic_trigger_runs_end_to_end_and_persists_outputs -- --nocapture
}
Invoke-Step "cargo test -p harness --test foundation_integration timed_out_worker_is_terminated -- --nocapture" {
    cargo test -p harness --test foundation_integration timed_out_worker_is_terminated -- --nocapture
}
Invoke-Step "cargo test -p harness --test foundation_integration timed_out_foreground_run_is_marked_failed_and_audited -- --nocapture" {
    cargo test -p harness --test foundation_integration timed_out_foreground_run_is_marked_failed_and_audited -- --nocapture
}
Invoke-Step "cargo test --workspace --lib recovery_decision_abandons_rejected_approval_fail_closed -- --nocapture" {
    cargo test --workspace --lib recovery_decision_abandons_rejected_approval_fail_closed -- --nocapture
}
Invoke-Step "cargo test --workspace --lib ensure_supported_reports_missing_pending_too_old_and_too_new_variants -- --nocapture" {
    cargo test --workspace --lib ensure_supported_reports_missing_pending_too_old_and_too_new_variants -- --nocapture
}
Invoke-Step "cargo test -p runtime --bin runtime phase_six_admin_parser_rejects_invalid_recovery_thresholds -- --nocapture" {
    cargo test -p runtime --bin runtime phase_six_admin_parser_rejects_invalid_recovery_thresholds -- --nocapture
}

Write-Host ""
Write-Host "release-readiness checks passed"
