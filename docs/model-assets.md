# LiveTalking Model Assets

LiveTalking's first demo needs two assets that are not stored in this repository:

1. `wav2lip256.pth`
2. `wav2lip256_avatar1.tar.gz`

Official download links from LiveTalking README:

- Quark: <https://pan.quark.cn/s/83a750323ef0>
- Google Drive: <https://drive.google.com/drive/folders/1FOC_MD6wdogyyX_7V1d4NDIO7P9NlSAJ?usp=sharing>

## Required Local Layout

After downloading, arrange files like this:

```text
F:\Project\Digital-Girl\backend\livetalking\
  models\
    wav2lip.pth
  data\
    avatars\
      wav2lip256_avatar1\
        ...avatar files...
```

Important:

- `wav2lip256.pth` must be renamed to `wav2lip.pth`.
- `wav2lip256_avatar1.tar.gz` must be extracted.
- The extracted folder itself should be named `wav2lip256_avatar1` and placed directly under `data\avatars`.

## Check Assets

Run:

```powershell
Set-Location F:\Project\Digital-Girl
.\backend\scripts\check-livetalking-assets.ps1
```

Expected success:

```text
LiveTalking demo assets are ready.
```

## Install Assets With Helper Script

After downloading the two official files, you can copy/extract them into the correct LiveTalking folders with:

```powershell
Set-Location F:\Project\Digital-Girl
.\backend\scripts\install-livetalking-assets.ps1 `
  -Wav2LipModelPath "D:\Downloads\wav2lip256.pth" `
  -AvatarArchiveOrFolderPath "D:\Downloads\wav2lip256_avatar1.tar.gz"
```

You can also pass an already extracted `wav2lip256_avatar1` folder:

```powershell
.\backend\scripts\install-livetalking-assets.ps1 `
  -Wav2LipModelPath "D:\Downloads\wav2lip256.pth" `
  -AvatarArchiveOrFolderPath "D:\Downloads\wav2lip256_avatar1"
```

## Start After Assets Are Ready

```powershell
Set-Location F:\Project\Digital-Girl
.\backend\scripts\start-livetalking.ps1
```

## If You Cannot Download From Google Drive

Use the Quark link from the official README, or use the official cloud image mentioned by LiveTalking for a quick environment. Avoid downloading model files from random mirrors because model files can be tampered with.

## Later Custom Avatar

For the MVP, use the official `wav2lip256_avatar1` demo avatar. Custom character/avatar generation should wait until the base backend can start reliably.
