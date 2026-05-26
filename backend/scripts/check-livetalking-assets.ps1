param(
    [string]$AvatarId = "wav2lip256_avatar1"
)

$ErrorActionPreference = "Stop"

$ProjectRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$LiveTalkingDir = Join-Path $ProjectRoot "backend\livetalking"
$ModelPath = Join-Path $LiveTalkingDir "models\wav2lip.pth"
$AvatarDir = Join-Path $LiveTalkingDir "data\avatars\$AvatarId"

Write-Host "Checking LiveTalking demo assets..." -ForegroundColor Cyan
Write-Host "LiveTalking: $LiveTalkingDir"

$ok = $true

if (Test-Path $ModelPath) {
    $model = Get-Item $ModelPath
    Write-Host "  ok model: $($model.FullName) ($([math]::Round($model.Length / 1MB, 2)) MB)" -ForegroundColor Green
} else {
    Write-Host "  missing model: $ModelPath" -ForegroundColor Red
    Write-Host "    Download wav2lip256.pth, copy it to models, and rename it to wav2lip.pth"
    $ok = $false
}

if (Test-Path $AvatarDir) {
    $requiredAvatarPaths = @(
        "coords.pkl",
        "full_imgs",
        "face_imgs"
    )
    $missingAvatarPaths = @()
    foreach ($path in $requiredAvatarPaths) {
        if (-not (Test-Path (Join-Path $AvatarDir $path))) {
            $missingAvatarPaths += $path
        }
    }

    if ($missingAvatarPaths.Count -gt 0) {
        Write-Host "  invalid avatar: $AvatarDir" -ForegroundColor Red
        Write-Host "    Missing expected extracted content: $($missingAvatarPaths -join ', ')"
        Write-Host "    It may still be compressed. Extract wav2lip256_avatar1.tar.gz into data\avatars."
        $ok = $false
    } else {
        $files = Get-ChildItem $AvatarDir -Recurse -File -ErrorAction SilentlyContinue
        $fullImages = Get-ChildItem (Join-Path $AvatarDir "full_imgs") -File -ErrorAction SilentlyContinue
        $faceImages = Get-ChildItem (Join-Path $AvatarDir "face_imgs") -File -ErrorAction SilentlyContinue
        Write-Host "  ok avatar: $AvatarDir ($($files.Count) files, full_imgs=$($fullImages.Count), face_imgs=$($faceImages.Count))" -ForegroundColor Green
    }
} else {
    Write-Host "  missing avatar: $AvatarDir" -ForegroundColor Red
    Write-Host "    Extract wav2lip256_avatar1.tar.gz, then place the extracted wav2lip256_avatar1 folder under data\avatars"
    $ok = $false
}

if (-not $ok) {
    Write-Host ""
    Write-Host "Download sources from LiveTalking README:" -ForegroundColor Yellow
    Write-Host "  Quark:        https://pan.quark.cn/s/83a750323ef0"
    Write-Host "  Google Drive: https://drive.google.com/drive/folders/1FOC_MD6wdogyyX_7V1d4NDIO7P9NlSAJ?usp=sharing"
    exit 1
}

Write-Host "LiveTalking demo assets are ready." -ForegroundColor Green
