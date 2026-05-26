$ErrorActionPreference = "Stop"

$ProjectRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$BackendDir = Join-Path $ProjectRoot "backend"
$LiveTalkingDir = Join-Path $BackendDir "livetalking"

New-Item -ItemType Directory -Force $BackendDir | Out-Null

if (Test-Path $LiveTalkingDir) {
    Write-Host "LiveTalking already exists: $LiveTalkingDir" -ForegroundColor Yellow
    exit 0
}

Set-Location $BackendDir
git clone https://github.com/lipku/LiveTalking.git livetalking

Write-Host "LiveTalking cloned to $LiveTalkingDir" -ForegroundColor Green
