# Digital-Girl Development Plan

This document captures the next development direction for the local Digital-Girl prototype. It is intended as a concise handoff for future sessions: read this first when deciding what to build next.

## Current Baseline

The project currently provides a local digital human demo stack:

- LiveTalking runs the Python media pipeline, Wav2Lip avatar rendering, WebRTC page, TTS/ASR integrations, and GPU-heavy work.
- companion-core runs as a Rust HTTP sidecar for persona, chat routing, memory, and app-level logic.
- The Rust egui control panel launches and monitors companion-core and LiveTalking, shows status, URLs, runtime paths, and logs.
- LiveTalking's `llm.py` bridge can call `POST http://127.0.0.1:8787/chat` and fall back to the original LiveTalking LLM path if companion-core is unavailable.
- Required demo assets are available for `wav2lip256_avatar1` and `wav2lip.pth`.

Primary local entry points:

```powershell
Set-Location F:/Project/Digital-Girl/services/companion-core
cargo run
```

## Guiding Direction

Move the project from a runnable demo toward a stable local digital human application:

1. Make startup, checks, and failure recovery obvious from the control panel.
2. Make the digital human genuinely conversational by wiring a real LLM path.
3. Give the character a configurable persona, voice, avatar, and memory.
4. Polish the web/mobile interaction so the user experience feels like Digital-Girl, not a raw LiveTalking demo.
5. Keep Python focused on media/GPU/runtime work and Rust focused on app logic, configuration, memory, and orchestration.

## Phase 1: Stabilize Startup And Diagnostics

Goal: make the system easy to start and easy to debug without terminal archaeology.

Status: initial implementation added to the egui control panel. The panel now has startup diagnostics for project paths, Python, PyTorch/CUDA, LiveTalking imports, model/avatar assets, and ports. It also supports per-service restart, log clearing, and local log file output.

Recommended tasks:

- Add an environment check section to the egui control panel.
- Check that `F:/Project/Digital-Girl/.venv` exists.
- Check Python version and show the selected Python path.
- Check `torch`, `torchvision`, `torchaudio`, CUDA availability, and GPU name.
- Check LiveTalking imports that commonly fail, such as `flask`, `flask_sockets`, `aiortc`, `cv2`, `onnxruntime`, `face_alignment`, `edge_tts`, `librosa`, and `soundfile`.
- Check model and avatar assets:
  - `backend/livetalking/models/wav2lip.pth`
  - `backend/livetalking/data/avatars/wav2lip256_avatar1/coords.pkl`
  - `backend/livetalking/data/avatars/wav2lip256_avatar1/full_imgs/`
  - `backend/livetalking/data/avatars/wav2lip256_avatar1/face_imgs/`
- Check whether ports `8787` and `8010` are available before starting.
- If a port is occupied, show the owning process name and PID.
- Add one-click restart for each service.
- Add a clear log button per service.
- Save service logs to a `logs/` directory, while keeping generated logs ignored by git.
- Make LiveTalking startup failures easier to understand by surfacing the last relevant error lines.

Priority: high. This should be the next implementation target.

## Phase 2: Wire A Real LLM Path

Goal: make the digital human hold real conversations through companion-core.

Status: initial implementation added. companion-core can load local file-backed config, expose safe `/llm/status`, call an OpenAI-compatible provider from `/chat`, report clearer fallback details, and the egui panel can show LLM config status plus run a safe test chat.

Recommended tasks:

- Define a clear LLM configuration format and keep it stable across launcher, docs, and runtime:
  - `LLM_BASE_URL`
  - `LLM_API_KEY`
  - `LLM_MODEL`
- Keep OpenAI-compatible chat completion support in companion-core.
- Add an egui panel section showing whether LLM config is present without displaying secrets.
- Add a safe test-chat button in the control panel.
- Improve `/chat` error handling:
  - authentication failures
  - model not found
  - timeout
  - network failure
  - malformed response
- Preserve graceful fallback to local replies when the LLM is unavailable.
- Document a tested configuration path in `docs/backend-testing.md`.

Priority: high after Phase 1.

## Phase 3: Persona System

Goal: make Digital-Girl feel like a specific character rather than a generic assistant.

Recommended tasks:

- Add a persona config file, for example `backend/config/persona.toml` or `services/companion-core/config/persona.toml`.
- Include fields such as:
  - name
  - short summary
  - speaking style
  - emotional tone
  - boundaries
  - default greeting
  - system prompt
- Load persona config in companion-core instead of relying only on env vars.
- Expose persona through `/persona`.
- Add a read-only persona preview in the egui panel.
- Later, add simple editing support in the panel.

Priority: medium-high.

## Phase 4: Memory

Goal: allow the digital human to remember recent context and useful long-term facts.

Recommended tasks:

- Start with short-term memory per `session_id`.
- Store the last N turns of conversation.
- Add summarization hooks for longer sessions.
- Add long-term memory storage, preferably SQLite for local durability and queryability.
- Support memory operations:
  - remember a fact
  - retrieve relevant facts
  - forget a fact
  - list recent memories
- Inject relevant memory into LLM prompts through companion-core.
- Keep private/local storage explicit in documentation.

Priority: medium.

## Phase 5: Digital Human Web Experience

Goal: make the browser UI feel like this project instead of a raw upstream demo.

Recommended tasks:

- Create a custom Digital-Girl web page or wrap the existing LiveTalking page.
- Provide a clean chat input.
- Show connection state: disconnected, connecting, ready, thinking, speaking.
- Show subtitles or recent transcript.
- Keep a visible interrupt/stop-speaking control.
- Hide or simplify demo-only controls.
- Polish mobile layout for local LAN use.
- Document phone access and Windows Firewall requirements.

Priority: medium.

## Phase 6: Voice, Avatar, And Character Profiles

Goal: let the user choose who the digital human is and how she sounds.

Recommended tasks:

- Discover available avatar folders under `backend/livetalking/data/avatars/`.
- Show avatar completeness checks in the egui panel.
- Allow selecting `avatar_id` before starting LiveTalking.
- Add TTS provider and voice selection.
- Bind persona, avatar, voice, and model options into a character profile.
- Support multiple local profiles over time.

Priority: medium-low until the basic chat loop is stable.

## Phase 7: Packaging And Productization

Goal: make the project easy to run outside a development session.

Recommended tasks:

- Build a release binary for the egui control panel.
- Add a Windows shortcut or launcher script.
- Add a first-run checklist.
- Keep generated media, logs, model files, avatar data, and caches ignored by git.
- Document a fresh setup path for another Windows machine.
- Consider local asset download helpers only after URLs and licenses are clear.

Priority: later.

## Suggested Immediate Next Task

Implement Phase 1 environment and asset checks inside the egui control panel.

A good first version should show a compact checklist with pass/fail states for:

- project root
- `.venv` Python path
- Python version
- CUDA torch availability
- required LiveTalking imports
- `wav2lip.pth`
- demo avatar folder completeness
- ports `8787` and `8010`

This will make every future debugging session much easier and will provide the foundation for a smoother end-user startup flow.
