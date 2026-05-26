# Decisions

## 2026-05-26 - Initial Technical Direction

Decision:

- Use LiveTalking as the backend service foundation.
- Use phone browser as the first frontend.
- Keep LiveTalking as an upstream checkout under `backend/livetalking`.
- Start with `wav2lip` for lower runtime cost.
- Use external LLM first for conversation quality and speed of development.
- Defer full phone-local inference until after the MVP works.

Why:

- The target is a playable personal virtual girlfriend app, which needs real-time interaction more than offline video generation.
- LiveTalking already provides WebRTC, text/audio driving APIs, interrupt support, and multiple avatar backends.
- Keeping the upstream repo isolated makes updates easier.

Open Questions:

- Which LLM provider to use first?
- Which TTS voice gives the best latency and personality fit?
- Whether the PC GPU is strong enough for `musetalk`, or whether `wav2lip` should stay as default.
- Whether to build the custom mobile frontend as PWA, Flutter, React Native, or native app.
