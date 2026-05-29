# Digital-Girl Project Memory

This is the short context file for future setup/debug sessions. For detailed notes, see [docs/environment-notes.md](docs/environment-notes.md). For the next feature roadmap, see [docs/development-plan.md](docs/development-plan.md).

## Project Shape

- Root: `F:/Project/Digital-Girl`
- LiveTalking checkout: `backend/livetalking/`
- Rust sidecar: `services/companion-core/`
- Main backend page: `http://127.0.0.1:8010/index.html`
- companion-core health: `http://127.0.0.1:8787/health`

## Environment To Reuse

- Reuse `F:/Project/Digital-Girl/.venv`.
- Do not create another Python env by default.
- Known working stack:
  - Python `3.10.11`
  - `torch==2.7.1+cu118`
  - `torchvision==0.22.1+cu118`
  - `torchaudio==2.7.1+cu118`
  - GPU: `NVIDIA GeForce RTX 4070`

## Startup

```powershell
Set-Location F:/Project/Digital-Girl/services/companion-core
cargo run
```

This opens the Rust egui control panel. Use the panel buttons to start/stop companion-core and LiveTalking, inspect status, open URLs, and view logs. Closing the panel stops any server process it started.

This is the only supported startup entrypoint so server lifetime stays owned by the Rust process.

## Required Assets

```text
backend/livetalking/models/wav2lip.pth
backend/livetalking/data/avatars/wav2lip256_avatar1/coords.pkl
backend/livetalking/data/avatars/wav2lip256_avatar1/full_imgs/
backend/livetalking/data/avatars/wav2lip256_avatar1/face_imgs/
```

Run:

```powershell
Set-Location F:/Project/Digital-Girl
./backend/scripts/check-livetalking-assets.ps1
```

## Pitfalls Already Hit

- Copied `.venv` had working CUDA torch but missed LiveTalking dependencies such as `flask`.
- `torchaudio` must match the existing torch CUDA build: `torchaudio==2.7.1+cu118`.
- `onnxruntime-gpu` is large; `gevent` may build slowly on Windows.
- Git repos do not include Python wheels, model weights, or extracted avatar data.
- `backend/livetalking/` is ignored by the root repo because it is an upstream clone.
- The LiveTalking `llm.py` bridge to companion-core is preserved as [patches/livetalking-companion-core-llm.patch](patches/livetalking-companion-core-llm.patch).
- `services/companion-core/` once had a nested `.git/`; it was removed so the Rust service source can be committed in the root repo.

## Git Ignore Intent

The root repo should commit project scripts, docs, patches, and Rust service code. It should not commit:

- `.venv/`
- `backend/livetalking/`
- model weights
- extracted avatars
- generated media files
- logs and build caches