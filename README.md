# Digital Girl

A personal virtual girlfriend app prototype built around a PC-hosted LiveTalking backend and a mobile browser frontend.

## Goal

Build a private, playable MVP where a phone connects to a PC/server, sends text or voice input, and receives a real-time talking digital human stream.

## Initial Architecture

```text
Mobile Browser / Mobile App
  - WebRTC video playback
  - Text input
  - Optional microphone input
        |
        | HTTP + WebRTC
        v
PC Backend
  - LiveTalking server
  - Avatar rendering: wav2lip first, musetalk later
  - TTS service
  - Optional ASR service
  - Optional LLM adapter
  - Rust companion-core sidecar for decoupled app logic
        |
        v
Models / APIs
  - External LLM first
  - Local LLM later
  - TTS can be EdgeTTS / QwenTTS / GPT-SoVITS / CosyVoice
```

## MVP Choice

- Backend: LiveTalking on PC or GPU server
- Frontend: phone browser opens LiveTalking web page
- Avatar model: start with `wav2lip`
- LLM: external OpenAI-compatible API first
- TTS: start with low-friction TTS, then replace with custom voice

## Folder Layout

```text
Digital-Girl/
  backend/
    livetalking/          # clone LiveTalking here
    scripts/              # local helper scripts
    config/               # local env/config templates
  docs/
    architecture.md
    roadmap.md
    runbook.md
  frontend/
    mobile-web-notes.md   # phone browser integration notes
  services/
    companion-core/       # Rust sidecar for persona, memory, and LLM routing
  assets/
    avatars/              # future avatar source files
    voices/               # future reference audio
```

## First Run Plan

1. Clone LiveTalking into `backend/livetalking`.
2. Download the demo `wav2lip` model and avatar package.
3. Create the Python environment on the PC.
4. Start LiveTalking with WebRTC.
5. Open `http://<pc-ip>:8010/index.html` from the phone.
6. Send text through `/human` and verify real-time playback.

Model asset placement is documented in `docs/model-assets.md`.

## Notes

This repo is intentionally a wrapper/project workspace, not a fork of LiveTalking yet. Keep upstream LiveTalking isolated under `backend/livetalking` so it is easy to update or replace.

For future setup/debug context, start with [PROJECT_MEMORY.md](PROJECT_MEMORY.md). The longer environment record is in [docs/environment-notes.md](docs/environment-notes.md).

## Python And Rust Boundary

Keep GPU/model/media code in Python through LiveTalking. Use Rust for independent services that benefit from strong typing, low overhead, and clean HTTP contracts: persona state, memory, configuration, local storage, request policy, and LLM routing.
