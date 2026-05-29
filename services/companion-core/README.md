# companion-core

Rust sidecar service for app logic that should not be coupled to LiveTalking's Python model/runtime process.

## Responsibility

Good candidates for this service:

- Persona configuration.
- Conversation memory summaries.
- Long-term memory storage and retrieval.
- LLM provider routing.
- Safety/privacy policy for requests.
- Session metadata.
- App-level configuration.

Keep these in Python/LiveTalking for now:

- Avatar model loading.
- GPU inference.
- TTS streaming internals.
- WebRTC media tracks.
- Audio/video frame processing.

## Run

From this crate, `cargo run` opens the egui control panel for the full local stack:

```powershell
Set-Location F:\Project\Digital-Girl\services\companion-core
cargo run
```

To run only the HTTP sidecar service in the current terminal:

```powershell
Set-Location F:\Project\Digital-Girl\services\companion-core
cargo run --bin companion-core
```

Use the control panel for normal local startup. Running the HTTP sidecar directly is only for focused debugging and does not manage the LiveTalking process lifecycle.

Health check:

```powershell
Invoke-RestMethod http://127.0.0.1:8787/health
```

Test chat placeholder:

```powershell
Invoke-RestMethod http://127.0.0.1:8787/chat `
  -Method Post `
  -ContentType 'application/json' `
  -Body '{"session_id":"local","message":"hello"}'
```

## Local Config

Preferred local config file:

```powershell
Set-Location F:\Project\Digital-Girl
Copy-Item .\backend\config\companion-core.toml.example .\backend\config\companion-core.toml
notepad .\backend\config\companion-core.toml
```

The real `backend/config/companion-core.toml` file is ignored by git. Override the default path with:

```powershell
$env:COMPANION_CORE_CONFIG_FILE = "F:\path\to\companion-core.toml"
```

Legacy `.env` config files are still supported through `COMPANION_CORE_ENV_FILE`, but TOML is the preferred local format.

## Future Integration

LiveTalking's `llm.py` can call this service first. The Rust service can decide whether to:

- return a direct reply,
- call an external LLM,
- call a local LLM,
- enrich prompts with memory,
- or reject/reshape requests based on local policy.
