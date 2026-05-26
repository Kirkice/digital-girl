# Mobile Web Notes

## First Version

Use LiveTalking's built-in page directly:

```text
http://<pc-ip>:8010/index.html
```

This gives us:

- WebRTC connection.
- Avatar video playback.
- Text input through `/human`.
- Audio upload through `/humanaudio`.
- Interrupt support.
- Recording controls.

## Later Custom Frontend

A custom phone UI can keep the same backend contract:

- `POST /offer` to establish WebRTC.
- `POST /human` to send text.
- `POST /humanaudio` to send audio.
- `POST /interrupt_talk` to interrupt speech.
- `POST /is_speaking` to poll speaking status.

## UX Ideas

- Full-screen portrait video.
- Bottom chat input.
- Hold-to-talk microphone button.
- Interrupt button.
- Persona selector.
- Memory on/off switch.
- Server URL settings.

## First Test Checklist

- [ ] Phone and PC are on same Wi-Fi.
- [ ] PC browser can open `http://127.0.0.1:8010/index.html`.
- [ ] Phone can open `http://<pc-ip>:8010/index.html`.
- [ ] WebRTC starts without firewall errors.
- [ ] Text message triggers avatar speech.
