# Environment Notes And Pitfalls

This file records the setup path that actually worked on the current Windows PC, plus the problems hit along the way. Keep it short and practical so future sessions can recover context quickly.

Use forward slashes in commands here because PowerShell accepts them for paths and they avoid accidental escape characters in documentation.

## Known Good Local Environment

- Project root: `F:/Project/Digital-Girl`
- Python environment to reuse: `F:/Project/Digital-Girl/.venv`
- Python version: `3.10.11`
- pip version seen during setup: `26.0`
- GPU: `NVIDIA GeForce RTX 4070`
- CUDA through PyTorch: `11.8`
- PyTorch stack:
  - `torch==2.7.1+cu118`
  - `torchvision==0.22.1+cu118`
  - `torchaudio==2.7.1+cu118`

Do not create a second LiveTalking env by default. The user copied a working `.venv` into the project and wants reuse instead of repeated installs.

## Working Startup Flow

From the project root:

```powershell
Set-Location F:/Project/Digital-Girl
./backend/scripts/start-companion-core.ps1
./backend/scripts/start-livetalking.ps1
```

The LiveTalking startup script should print this Python path:

```text
Python: F:\Project\Digital-Girl\.venv\Scripts\python.exe
```

LiveTalking should then log:

```text
Using cuda for inference.
Load checkpoint from: ./models/wav2lip.pth
start http server; http://<serverip>:8010/index.html
```

Verify from the PC:

```powershell
Invoke-WebRequest -UseBasicParsing http://127.0.0.1:8010/index.html -TimeoutSec 5
Invoke-WebRequest -UseBasicParsing http://127.0.0.1:8787/health -TimeoutSec 5
```

Expected URLs:

- LiveTalking: `http://127.0.0.1:8010/index.html`
- companion-core health: `http://127.0.0.1:8787/health`

## Asset Layout That Passed Checks

Required files under the LiveTalking checkout:

```text
backend/livetalking/models/wav2lip.pth
backend/livetalking/data/avatars/wav2lip256_avatar1/coords.pkl
backend/livetalking/data/avatars/wav2lip256_avatar1/full_imgs/
backend/livetalking/data/avatars/wav2lip256_avatar1/face_imgs/
```

The verified demo avatar had:

- `550` files in `full_imgs`
- `550` files in `face_imgs`
- `1101` files total in the avatar directory
- `wav2lip.pth` around `204.73 MB`

Common asset mistakes:

- The downloaded model may be named `wav2lip256.pth`; LiveTalking expects `wav2lip.pth` for this startup command.
- The avatar archive must be extracted so `coords.pkl`, `full_imgs`, and `face_imgs` are directly under `wav2lip256_avatar1`.
- A folder existing is not enough; run the asset checker:

```powershell
Set-Location F:/Project/Digital-Girl
./backend/scripts/check-livetalking-assets.ps1
```

## Dependency Pitfalls

- Initial startup failed with `ModuleNotFoundError: No module named 'flask'` because the copied `.venv` had PyTorch but not LiveTalking's web/media dependencies.
- Install missing dependencies into the existing project `.venv`; do not reinstall torch unless explicitly needed.
- `torchaudio` needed to match the existing CUDA PyTorch stack:

```powershell
Set-Location F:/Project/Digital-Girl
./.venv/Scripts/python.exe -m pip install torchaudio==2.7.1+cu118 --index-url https://download.pytorch.org/whl/cu118
./.venv/Scripts/python.exe -m pip install -r ./backend/livetalking/requirements.txt
```

- `onnxruntime-gpu` is large and slow to download.
- `gevent` may build locally on Windows and take a while. In this run it eventually built successfully.
- pip cache is reused automatically, but source repos do not include Python wheels or model weights. For faster future rebuilds, consider a local wheelhouse after the stack stabilizes.

Dependency import check used after install:

```powershell
Set-Location F:/Project/Digital-Girl
./.venv/Scripts/python.exe -c "import importlib.util as u; mods=['torch','torchvision','torchaudio','flask','flask_sockets','aiortc','aiohttp_cors','cv2','onnxruntime','face_alignment','edge_tts','soundfile','librosa','numpy','scipy','numba','resampy','python_speech_features','configargparse','ffmpeg','openai','websockets']; [print(f'{m}=' + ('ok' if u.find_spec(m) else 'missing')) for m in mods]; import torch; print(torch.__version__, torch.cuda.is_available(), torch.version.cuda, torch.cuda.get_device_name(0) if torch.cuda.is_available() else 'none')"
```

## Git And Repo Layout Pitfalls

- `backend/livetalking/` is an upstream clone and is ignored by the root repo.
- The local LiveTalking bridge change is saved in `patches/livetalking-companion-core-llm.patch` so it is not lost when the checkout is ignored.
- `services/companion-core/` accidentally had its own `.git/` directory. That caused `git add .` to fail with `does not have a commit checked out`. The nested `.git/` was removed so the Rust service source can be committed with the main repo.
- Root `.gitignore` intentionally excludes `.venv/`, model weights, extracted avatar data, media files, and LiveTalking checkout content.

Before committing, preview files with:

```powershell
Set-Location F:/Project/Digital-Girl
git add -n .
git status --ignored --short
```

## Windows Terminal Pitfalls

- If `start-livetalking.ps1` is run from `C:/Users/admin`, PowerShell reports that `./backend/scripts/start-livetalking.ps1` cannot be found. Always `Set-Location F:/Project/Digital-Girl` first.
- VS Code terminal output may show an old failed startup even while a later process is running.
- To verify the actual listener:

```powershell
$c = Get-NetTCPConnection -LocalPort 8010 -State Listen -ErrorAction SilentlyContinue | Select-Object -First 1
if ($null -ne $c) { Get-Process -Id $c.OwningProcess }
```

- A visible server window can be opened with:

```powershell
Start-Process powershell.exe -ArgumentList @('-NoExit','-Command','$host.UI.RawUI.WindowTitle = "LiveTalking Server - Digital-Girl"; Set-Location "F:/Project/Digital-Girl"; ./backend/scripts/start-livetalking.ps1')
```

## Architecture Decisions From Setup

- Keep LiveTalking's media, WebRTC, Wav2Lip, TTS, and GPU path in Python.
- Use Rust for decoupled app logic: persona, memory, model routing, and future API/backend features.
- LiveTalking calls Rust companion-core first through `COMPANION_CORE_URL`, then falls back to the original built-in LLM path if unavailable.
- Mobile browser connectivity is intentionally deferred; backend stability comes first.
