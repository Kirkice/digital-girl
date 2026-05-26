# Architecture

## Product Shape

The first version is a private LAN prototype:

- PC runs the real-time digital human backend.
- Phone opens the web page served by the backend.
- Phone sends text or audio commands.
- PC returns WebRTC audio/video stream.

## Runtime Components

### Mobile Frontend

Initial version uses LiveTalking's built-in web page:

- URL: `http://<pc-ip>:8010/index.html`
- Responsibilities:
  - Establish WebRTC session.
  - Display digital human video stream.
  - Send text to `/human`.
  - Upload audio to `/humanaudio` later.

Future custom app:

- Flutter, React Native, or native Android/iOS.
- WebRTC player.
- Chat UI.
- Microphone capture.
- Local persona settings.

### Backend

LiveTalking provides:

- Web server on port `8010`.
- WebRTC offer endpoint: `POST /offer`.
- Text driver endpoint: `POST /human`.
- Audio driver endpoint: `POST /humanaudio`.
- Interrupt endpoint: `POST /interrupt_talk`.
- Recording endpoints.
- Avatar rendering pipeline.

### Companion Core

`services/companion-core` is a Rust sidecar for app-level logic that should stay decoupled from model/media inference:

- Persona settings.
- Conversation memory.
- LLM provider routing.
- Local policy and privacy checks.
- Stable backend-for-frontend APIs for the future mobile app.

The first version exposes `GET /health`, `GET /persona`, and `POST /chat`. LiveTalking can call it from `llm.py` later.

### Model Layer

Recommended first pass:

- Avatar: `wav2lip` because it is faster and easier to run.
- TTS: EdgeTTS or another easy remote/local TTS.
- LLM: external OpenAI-compatible model for best conversation quality.

Future upgrades:

- Avatar: `musetalk` for better quality.
- TTS: custom cloned voice with GPT-SoVITS, CosyVoice, IndexTTS, or Fish Speech.
- LLM: local model on PC for privacy and cost control.
- Memory: local database + summaries + vector retrieval.

## Network

For WebRTC on LAN:

- PC must allow inbound TCP `8010`.
- WebRTC may need UDP ports depending on transport and browser/network behavior.
- Same Wi-Fi/LAN is the easiest starting point.

For cloud deployment:

- Add authentication before exposing any endpoint.
- Consider TURN/SRS for NAT traversal.
- Lock down upload endpoints.
