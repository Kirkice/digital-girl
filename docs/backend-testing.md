# Backend Testing

## Static Checks

Run:

```powershell
Set-Location F:\Project\Digital-Girl
.\backend\scripts\test-backend.ps1
```

This checks:

- Rust `companion-core` compiles.
- LiveTalking `llm.py` bridge has valid Python syntax.
- Required LiveTalking folders and files exist.

## Asset Checks

Run:

```powershell
Set-Location F:\Project\Digital-Girl
.\backend\scripts\check-livetalking-assets.ps1
```

This checks for:

- `backend/livetalking/models/wav2lip.pth`
- `backend/livetalking/data/avatars/wav2lip256_avatar1`

These assets must be downloaded manually from the official LiveTalking links. See [model-assets.md](model-assets.md).

## companion-core HTTP Checks

Start the service:

```powershell
Set-Location F:\Project\Digital-Girl\services\companion-core
cargo run
```

In the Rust control panel, click `Start` on `companion-core`.

In another terminal:

```powershell
Invoke-RestMethod http://127.0.0.1:8787/health
Invoke-RestMethod http://127.0.0.1:8787/persona
Invoke-RestMethod http://127.0.0.1:8787/llm/status
Invoke-RestMethod http://127.0.0.1:8787/chat `
  -Method Post `
  -ContentType 'application/json' `
  -Body '{"session_id":"local","message":"你好"}'
```

Expected behavior without LLM credentials:

- `/health` returns `status = ok`.
- `/persona` returns configured persona text.
- `/llm/status` returns whether `LLM_BASE_URL` and `LLM_API_KEY` are configured without exposing the API key.
- `/chat` returns a local placeholder reply with `source = local`.

## Real LLM Configuration

companion-core supports an OpenAI-compatible chat completion provider. The preferred local setup is:

```powershell
Set-Location F:\Project\Digital-Girl
Copy-Item .\backend\config\companion-core.toml.example .\backend\config\companion-core.toml
notepad .\backend\config\companion-core.toml
```

Set these values in `backend/config/companion-core.toml`:

```toml
[llm]
base_url = "https://your-openai-compatible-endpoint/v1"
model = "your-model-name"
api_key = "your-api-key"
```

The real `companion-core.toml` file is ignored by git. Process environment variables still take priority over the file. To use a different config path, set:

```powershell
$env:COMPANION_CORE_CONFIG_FILE = "F:\path\to\companion-core.toml"
```

Legacy `.env` files still work through `COMPANION_CORE_ENV_FILE`, but TOML is now the preferred local secret format.

After starting `companion-core` from the control panel, verify safe status:

```powershell
Invoke-RestMethod http://127.0.0.1:8787/llm/status
```

Then test chat:

```powershell
Invoke-RestMethod http://127.0.0.1:8787/chat `
  -Method Post `
  -ContentType 'application/json' `
  -Body '{"session_id":"llm-test","message":"用一句话介绍你自己"}'
```

Expected behavior with valid LLM credentials:

- `/llm/status` returns `configured = true`.
- `/chat` returns `source = llm`.

Expected behavior with invalid or unreachable LLM credentials:

- `/chat` returns a local placeholder reply with `source = fallback`.
- The response includes a `detail` field describing the provider error.

The egui control panel also has an `LLM` section with a `Reload Config` button and a safe `Test Chat` button. The API key is only shown as configured or missing; the secret value is never displayed.

## LiveTalking Bridge Behavior

LiveTalking `llm.py` now calls:

```text
POST http://127.0.0.1:8787/chat
```

Set a different URL with:

```powershell
$env:COMPANION_CORE_URL = "http://127.0.0.1:8787"
```

If `companion-core` is unavailable, LiveTalking falls back to its original DashScope/Qwen path.

Bridge-only smoke test without loading avatar models:

```powershell
Set-Location F:\Project\Digital-Girl\backend\livetalking
@'
import llm

class Opt:
  sessionid = 'bridge-test'

class FakeAvatar:
  opt = Opt()
  def __init__(self):
    self.messages = []
  def put_msg_txt(self, text, datainfo={}):
    self.messages.append(text)

avatar = FakeAvatar()
ok = llm._try_companion_core('backend bridge test', avatar, {})
print({'ok': ok, 'messages': avatar.messages})
'@ | python -
```

Expected result: `ok` is `True` when `companion-core` is running, and `messages` contains one or more emitted reply chunks.

## Full LiveTalking Runtime Test

This still requires model assets:

- `backend/livetalking/models/wav2lip.pth`
- demo avatar folder under `backend/livetalking/data/avatars/wav2lip256_avatar1`

Once assets are present:

```powershell
Set-Location F:\Project\Digital-Girl\services\companion-core
cargo run
```

In the Rust control panel, click `Start` on `LiveTalking`.

## Current Verified State

Verified on 2026-05-26:

- `cargo check` passes for `services/companion-core`.
- `python -m py_compile llm.py` passes for LiveTalking bridge.
- `GET /health` returns `status = ok`.
- `GET /persona` returns the default persona.
- `GET /llm/status` returns safe LLM configuration status.
- `POST /chat` returns a local reply with `source = local` when no LLM credentials are configured.
- LiveTalking `llm.py` bridge can call `companion-core` with a fake avatar session.
