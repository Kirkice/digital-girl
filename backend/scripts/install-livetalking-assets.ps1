param(
    [Parameter(Mandatory=$true)]
    [string]$Wav2LipModelPath,

    [Parameter(Mandatory=$true)]
    [string]$AvatarArchiveOrFolderPath,

    [string]$AvatarId = "wav2lip256_avatar1"
)

$ErrorActionPreference = "Stop"

$ProjectRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$LiveTalkingDir = Join-Path $ProjectRoot "backend\livetalking"
$ModelsDir = Join-Path $LiveTalkingDir "models"
$AvatarsDir = Join-Path $LiveTalkingDir "data\avatars"
$TargetModel = Join-Path $ModelsDir "wav2lip.pth"
$TargetAvatarDir = Join-Path $AvatarsDir $AvatarId

if (-not (Test-Path $LiveTalkingDir)) {
    throw "LiveTalking not found: $LiveTalkingDir"
}
if (-not (Test-Path $Wav2LipModelPath)) {
    throw "Model file not found: $Wav2LipModelPath"
}
if (-not (Test-Path $AvatarArchiveOrFolderPath)) {
    throw "Avatar archive/folder not found: $AvatarArchiveOrFolderPath"
}

New-Item -ItemType Directory -Force $ModelsDir | Out-Null
New-Item -ItemType Directory -Force $AvatarsDir | Out-Null

Write-Host "Installing model..." -ForegroundColor Cyan
Copy-Item -Force $Wav2LipModelPath $TargetModel
Write-Host "  copied to $TargetModel" -ForegroundColor Green

$item = Get-Item $AvatarArchiveOrFolderPath
if ($item.PSIsContainer) {
    Write-Host "Installing avatar from folder..." -ForegroundColor Cyan
    if (Test-Path $TargetAvatarDir) {
        Remove-Item -Recurse -Force $TargetAvatarDir
    }
    Copy-Item -Recurse -Force $item.FullName $TargetAvatarDir
    Write-Host "  copied to $TargetAvatarDir" -ForegroundColor Green
} else {
    Write-Host "Installing avatar from archive..." -ForegroundColor Cyan
    $tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("digital-girl-avatar-" + [System.Guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Force $tempDir | Out-Null
    try {
        $archive = $item.FullName
        if ($archive.EndsWith(".zip", [System.StringComparison]::OrdinalIgnoreCase)) {
            Expand-Archive -Force $archive $tempDir
        } elseif ($archive.EndsWith(".tar.gz", [System.StringComparison]::OrdinalIgnoreCase) -or $archive.EndsWith(".tgz", [System.StringComparison]::OrdinalIgnoreCase)) {
            tar -xzf $archive -C $tempDir
            if ($LASTEXITCODE -ne 0) {
                throw "tar failed with exit code $LASTEXITCODE"
            }
        } else {
            throw "Unsupported avatar archive format. Use .tar.gz, .tgz, .zip, or pass an extracted folder."
        }

        $candidate = Join-Path $tempDir $AvatarId
        if (-not (Test-Path $candidate)) {
            $dirs = Get-ChildItem $tempDir -Directory
            if ($dirs.Count -eq 1) {
                $candidate = $dirs[0].FullName
            } else {
                throw "Could not find extracted avatar folder '$AvatarId' in archive."
            }
        }

        if (Test-Path $TargetAvatarDir) {
            Remove-Item -Recurse -Force $TargetAvatarDir
        }
        Copy-Item -Recurse -Force $candidate $TargetAvatarDir
        Write-Host "  extracted to $TargetAvatarDir" -ForegroundColor Green
    } finally {
        if (Test-Path $tempDir) {
            Remove-Item -Recurse -Force $tempDir
        }
    }
}

& (Join-Path $PSScriptRoot "check-livetalking-assets.ps1") -AvatarId $AvatarId
