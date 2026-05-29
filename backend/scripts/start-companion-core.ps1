$ErrorActionPreference = "Stop"

$ProjectRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$ServiceDir = Join-Path $ProjectRoot "services\companion-core"
$HealthUrl = "http://127.0.0.1:8787/health"

if (-not (Test-Path $ServiceDir)) {
    Write-Error "companion-core not found at $ServiceDir"
}

Set-Location $ServiceDir

$Host.UI.RawUI.WindowTitle = "Digital-Girl companion-core"
Write-Host "Starting companion-core..." -ForegroundColor Cyan
Write-Host "Project: $ProjectRoot"
Write-Host "Service: $ServiceDir"
Write-Host "Health: $HealthUrl"
Write-Host "Close this server window to stop companion-core." -ForegroundColor Yellow
Write-Host ""

cargo run --bin companion-core
