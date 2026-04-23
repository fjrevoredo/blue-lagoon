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

Invoke-Step "cargo test -p harness --test recovery_component -- --nocapture" {
    cargo test -p harness --test recovery_component -- --nocapture
}
Invoke-Step "cargo test -p harness --test recovery_integration -- --nocapture" {
    cargo test -p harness --test recovery_integration -- --nocapture
}

Write-Host ""
Write-Host "recovery-hardening checks passed"
