# Roadmap

## Phase 0 - Workspace Setup

- [x] Create project folder.
- [x] Add initial docs and scripts.
- [x] Clone LiveTalking.
- [x] Create Rust `companion-core` sidecar skeleton.
- [x] Bridge LiveTalking `llm.py` to `companion-core` with fallback.
- [x] Add backend static and HTTP smoke tests.
- [ ] Record local GPU/CUDA/Python environment.

## Phase 1 - Run LiveTalking Demo

- [ ] Download `wav2lip256.pth` and demo avatar package.
- [ ] Install Python dependencies.
- [ ] Start LiveTalking on PC.
- [ ] Open page from phone over LAN.
- [ ] Confirm text-to-avatar works.
- [ ] Confirm interrupt works.

## Phase 2 - Make It Feel Like A Companion

- [x] Connect LiveTalking `llm.py` to `companion-core`.
- [ ] Configure external LLM credentials for `companion-core`.
- [ ] Define persona prompt.
- [ ] Add conversation memory summary.
- [ ] Pick a stable TTS voice.
- [ ] Tune first response latency.

## Phase 3 - Custom Character

- [ ] Prepare avatar source video.
- [ ] Generate LiveTalking avatar package.
- [ ] Prepare reference voice material.
- [ ] Swap TTS to custom voice.
- [ ] Save repeatable generation steps.

## Phase 4 - Custom Mobile Frontend

- [ ] Decide frontend stack: web app, Flutter, React Native, or native.
- [ ] Implement WebRTC playback.
- [ ] Implement text chat panel.
- [ ] Add microphone input.
- [ ] Add settings for server URL, persona, and voice.

## Phase 5 - Privacy And Local Models

- [ ] Add local LLM option.
- [ ] Add local ASR option.
- [ ] Add local/custom TTS option.
- [ ] Add authentication and LAN-only defaults.
