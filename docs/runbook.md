# Runbook

## 1. Clone LiveTalking

Current environment notes and pitfalls are recorded in [environment-notes.md](environment-notes.md). Read that first when restoring this project on the same PC or continuing a future setup/debug session.

From `F:\Project\Digital-Girl`:

```powershell
Set-Location F:\Project\Digital-Girl
New-Item -ItemType Directory -Force backend | Out-Null
Set-Location backend
git clone https://github.com/lipku/LiveTalking.git livetalking
```

Or use the helper script:

```powershell
Set-Location F:\Project\Digital-Girl
.\backend\scripts\clone-livetalking.ps1
```

## 2. Create Python Environment

```powershell
conda create -n livetalking python=3.10
conda activate livetalking
```

Install PyTorch according to local CUDA version. For CUDA 12.4, LiveTalking README suggests:

```powershell
conda install pytorch==2.5.0 torchvision==0.20.0 torchaudio==2.5.0 pytorch-cuda=12.4 -c pytorch -c nvidia
```

Then:

```powershell
Set-Location F:\Project\Digital-Girl\backend\livetalking
pip install -r requirements.txt
```

## 3. Download Demo Models

Follow LiveTalking README:

- Copy `wav2lip256.pth` into `backend/livetalking/models/`.
- Rename it to `wav2lip.pth`.
- Extract demo avatar package into `backend/livetalking/data/avatars/`.

Expected demo avatar id:

```text
wav2lip256_avatar1
```

Detailed asset notes: [model-assets.md](model-assets.md)

Check the files are in the right place:

```powershell
Set-Location F:\Project\Digital-Girl
.\backend\scripts\check-livetalking-assets.ps1
```

## 4. Start Backend

```powershell
Set-Location F:\Project\Digital-Girl\backend\livetalking
conda activate livetalking
python app.py --transport webrtc --model wav2lip --avatar_id wav2lip256_avatar1 --listenport 8010
```

## 5. Open From Phone

Find PC LAN IP:

```powershell
ipconfig
```

On phone browser:

```text
http://<pc-ip>:8010/index.html
```

If it does not connect:

- Ensure phone and PC are on same LAN.
- Allow Python through Windows Firewall.
- Allow TCP port `8010`.
- Test from PC browser first: `http://127.0.0.1:8010/index.html`.

## 6. MVP Acceptance Criteria

- Phone loads the LiveTalking page.
- WebRTC connection starts.
- Digital human video appears.
- Text sent from phone causes the avatar to speak.
- Interrupt button stops current speech.

## 7. Optional: Start Rust Companion Core

The Rust sidecar is for persona, memory, and LLM routing. It is not required for the first LiveTalking demo.

```powershell
Set-Location F:\Project\Digital-Girl
.\backend\scripts\start-companion-core.ps1
```

Health check:

```powershell
Invoke-RestMethod http://127.0.0.1:8787/health
```
