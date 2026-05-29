$ErrorActionPreference = "Stop"

$ProjectRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$ServiceDir = Join-Path $ProjectRoot "services\companion-core"

if (-not (Test-Path $ServiceDir)) {
    Write-Error "companion-core not found at $ServiceDir"
}

Set-Location $ServiceDir

Write-Host "Starting Digital-Girl Control Panel..." -ForegroundColor Cyan
Write-Host "Project: $ProjectRoot"
Write-Host "Panel: cargo run"
Write-Host ""

cargo run