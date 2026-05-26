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

```powershell
Set-Location F:\Project\Digital-Girl\services\companion-core
cargo run
```

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

## Future Integration

LiveTalking's `llm.py` can call this service first. The Rust service can decide whether to:

- return a direct reply,
- call an external LLM,
- call a local LLM,
- enrich prompts with memory,
- or reject/reshape requests based on local policy.
