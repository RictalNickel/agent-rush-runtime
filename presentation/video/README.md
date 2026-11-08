# Demo video

`out/demo.mp4` is the entry video: 3:55, 1920x1080, H.264 with a silent AAC track, about 10 MB. It plays the pitch deck's slides, replays of the 2026-11-07 live runs, and a real screen recording of a new web chat session from 2026-11-08, with captions taken from [docs/VIDEO_SCRIPT.md](../../docs/VIDEO_SCRIPT.md). Every line of terminal output in the replays is copied from [the evidence](../../docs/evidence/live/plugin-and-containment-2026-11-07/); the recording's evidence is in [web-chat-recording-2026-11-08](../../docs/evidence/live/web-chat-recording-2026-11-08/).

`out/live-session.mp4` is that recording on its own: 0:44, real time, captioned.

- `video.py` holds the replay scenes and the scene list. It imports the deck (`../manim/deck.py`), so the style and the product name come from there.
- `./render.sh` renders it and splices `out/live-session.mp4` in after the demo slide. It needs the deck's virtualenv (`../manim/render.sh` makes it). `QUALITY=l ./render.sh` makes a fast 480p draft.

## Recording a live session

`record_live.py` drives headless Chromium (Playwright) against the live web chat: it clicks one of the example tasks, sends it, and records the screen until the session is Ready again, then writes `out/live/raw.mp4` at 30 fps in real time. `cut_live.sh` holds the first frame 5 s for the opening caption and burns in `live_captions.ass`, giving `out/live-session.mp4`. The captions are written for the 2026-11-08 session, so a new recording needs new caption times.

```bash
pip install playwright && playwright install chromium
AR_SESSION=... python3 record_live.py --example webapp
./cut_live.sh
```

The web chat signs in with the operator cookie. `AR_SESSION` is a short-lived `ar_session` token: add a row to the control database's `sessions` table (owner `operator`, the token's SHA-256 as `hash`, a random `csrf`, and an `expires` a couple of hours ahead) and delete it after recording. That way the operator password is never typed on camera.

To add a voice-over, read the narration in the script over the video; each scene holds long enough for it.
