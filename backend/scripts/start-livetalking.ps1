param(
    [string]$AvatarId = "wav2lip256_avatar1",
    [string]$Model = "wav2lip",
    [string]$Transport = "webrtc",
    [int]$Port = 8010
)

$ErrorActionPreference = "Stop"

$ProjectRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$LiveTalkingDir = Join-Path $ProjectRoot "backend\livetalking"
$VenvPython = Join-Path $ProjectRoot ".venv\Scripts\python.exe"
$PythonExe = if (Test-Path $VenvPython) { $VenvPython } else { "python" }

if (-not (Test-Path $LiveTalkingDir)) {
    Write-Error "LiveTalking not found at $LiveTalkingDir. Clone it first: git clone https://github.com/lipku/LiveTalking.git backend/livetalking"
}

Set-Location $LiveTalkingDir

Write-Host "Starting LiveTalking..." -ForegroundColor Cyan
Write-Host "Model: $Model"
Write-Host "Avatar: $AvatarId"
Write-Host "Transport: $Transport"
Write-Host "Port: $Port"
Write-Host "Python: $PythonExe"

& $PythonExe app.py --transport $Transport --model $Model --avatar_id $AvatarId --listenport $Port
