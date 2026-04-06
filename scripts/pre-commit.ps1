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
}

Invoke-Step "docker compose config" { docker compose config }
Invoke-Step "cargo fmt --all --check" { cargo fmt --all --check }
Invoke-Step "cargo check --workspace" { cargo check --workspace }
Invoke-Step "cargo clippy --workspace --all-targets -- -D warnings" {
    cargo clippy --workspace --all-targets -- -D warnings
}
Invoke-Step "cargo test --workspace --lib -- --nocapture" {
    cargo test --workspace --lib -- --nocapture
}
Invoke-Step "cargo test -p harness --test foreground_component -- --nocapture" {
    cargo test -p harness --test foreground_component -- --nocapture
}
Invoke-Step "cargo test -p harness --test foreground_integration -- --nocapture" {
    cargo test -p harness --test foreground_integration -- --nocapture
}
Invoke-Step "cargo test -p harness --test continuity_component -- --nocapture" {
    cargo test -p harness --test continuity_component -- --nocapture
}
Invoke-Step "cargo test -p harness --test continuity_integration -- --nocapture" {
    cargo test -p harness --test continuity_integration -- --nocapture
}

if (Get-Command markdownlint -ErrorAction SilentlyContinue) {
    Write-Host ""
    Write-Host "==> markdownlint **/*.md"
    try {
        markdownlint "**/*.md"
    } catch {
        if ($env:BLUE_LAGOON_STRICT_MARKDOWNLINT -eq "1") {
            throw
        }

        Write-Host "warning: markdownlint reported issues; continuing because strict mode is disabled"
    }
} else {
    Write-Host ""
    Write-Host "==> markdownlint **/*.md"
    Write-Host "skipped: markdownlint is not installed"
}

Write-Host ""
Write-Host "pre-commit checks passed"
