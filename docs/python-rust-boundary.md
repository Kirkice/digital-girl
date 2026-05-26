# Python / Rust Boundary

## Current Decision

Use Python for LiveTalking and all model/media code. Use Rust for decoupled app services around the model pipeline.

This keeps the MVP practical: Python owns the fast-moving AI stack, while Rust owns stable companion-product logic.

## Keep In Python

These are tightly coupled to LiveTalking or GPU/media libraries:

- Avatar model loading and warmup.
- Wav2Lip, MuseTalk, Ultralight inference.
- TTS plugin internals.
- WebRTC media tracks through `aiortc`.
- Audio/video queueing and frame rendering.
- Avatar generation tasks until the upstream flow is understood.

## Good Rust Candidates

These can be exposed over HTTP and tested independently:

- Persona prompt assembly.
- Conversation memory and summaries.
- Long-term memory storage.
- LLM provider routing.
- API key isolation and request policy.
- User/session preferences.
- Audit/event logs.
- Mobile app backend-for-frontend endpoints.

## Suggested Integration Shape

```text
Phone Browser/App
   |
   | WebRTC + HTTP
   v
LiveTalking Python Service
   |
   | HTTP localhost call for chat/persona/memory
   v
companion-core Rust Service
   |
   | optional external/local LLM calls
   v
LLM Provider
```

## First Rust Service

`services/companion-core` starts with:

- `GET /health`
- `GET /persona`
- `POST /chat`

The current `/chat` endpoint only echoes input. Later it should own persona/memory/LLM routing, then LiveTalking's `llm.py` can become a thin bridge into it.

## Why Not Rewrite LiveTalking In Rust

LiveTalking depends on Python-native AI/media libraries and model implementations. Rewriting that path would slow down the MVP without improving the first playable experience. Rust is valuable at the edges first.
