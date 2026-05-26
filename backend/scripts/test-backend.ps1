$ErrorActionPreference = "Stop"

$ProjectRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$CompanionDir = Join-Path $ProjectRoot "services\companion-core"
$LiveTalkingDir = Join-Path $ProjectRoot "backend\livetalking"

Write-Host "[1/4] Checking companion-core build..." -ForegroundColor Cyan
Set-Location $CompanionDir
cargo check
if ($LASTEXITCODE -ne 0) {
    throw "cargo check failed with exit code $LASTEXITCODE"
}

Write-Host "[2/4] Checking LiveTalking LLM bridge syntax..." -ForegroundColor Cyan
Set-Location $LiveTalkingDir
python -m py_compile llm.py
if ($LASTEXITCODE -ne 0) {
    throw "python py_compile failed with exit code $LASTEXITCODE"
}

Write-Host "[3/4] Checking required LiveTalking folders..." -ForegroundColor Cyan
$requiredPaths = @(
    "app.py",
    "config.py",
    "server\routes.py",
    "web\index.html",
    "models",
    "data\avatars"
)
foreach ($path in $requiredPaths) {
    $fullPath = Join-Path $LiveTalkingDir $path
    if (-not (Test-Path $fullPath)) {
        throw "Missing required path: $fullPath"
    }
    Write-Host "  ok $path"
}

Write-Host "[4/4] Backend static checks passed." -ForegroundColor Green
Write-Host "To run HTTP checks, start companion-core and call: Invoke-RestMethod http://127.0.0.1:8787/health"
