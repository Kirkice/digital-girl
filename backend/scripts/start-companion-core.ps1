$ErrorActionPreference = "Stop"

$ProjectRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$ServiceDir = Join-Path $ProjectRoot "services\companion-core"

if (-not (Test-Path $ServiceDir)) {
    Write-Error "companion-core not found at $ServiceDir"
}

Set-Location $ServiceDir
cargo run
