# Demo video

`out/demo.mp4` is the entry video: 4:51, 1920x1080, H.264 with a silent AAC track, about 13 MB. It is made of three kinds of material, with captions taken from [docs/VIDEO_SCRIPT.md](../../docs/VIDEO_SCRIPT.md):

- the pitch deck's slides;
- replays of the 2026-11-07 live runs, with every line of terminal output copied from [the evidence](../../docs/evidence/live/plugin-and-containment-2026-11-07/);
- at 1:18, a real screen recording of a new web chat session on the live instance, beside a panel that explains each step ([evidence](../../docs/evidence/live/dashboard-recording-2026-11-08/)).

In the recording, the agent builds a sales analytics dashboard from data it generates, then checks it in a browser. Midway through, it gets a second message asking for a region filter, adds one and proves the filtered numbers.

Other files:

- `out/live-dash.mp4` is that recording on its own: 2:11, with the panel and the agent's two screenshots.
- `out/live-session.mp4` is an earlier, simpler recording (a to-do app, 0:44, [evidence](../../docs/evidence/live/web-chat-recording-2026-11-08/)). It is not in the entry video.
- `video.py` holds the replay scenes and the scene list. It imports the deck (`../manim/deck.py`), so the style and the product name come from there.
- `./render.sh` renders the video and splices `out/live-dash.mp4` in after the Claude Code replay. It needs the deck's virtualenv (`../manim/render.sh` makes it). `QUALITY=l ./render.sh` makes a fast 480p draft.

## Recording a live session

```bash
pip install playwright && playwright install chromium
AR_SESSION=... python3 record_live.py --prompt-file tasks/dashboard.txt \
  --steer-file tasks/dashboard-steer.txt --size 1200x900 --out out/live-dash
python3 compose_live.py out/live-dash narration/dashboard.json
```

1. `record_live.py` drives headless Chromium (Playwright) against the live web chat.
   - It types the task (or clicks an example with `--example`) and sends it.
   - With `--steer-file`, it sends a second message while the agent is still working, once the thread shows `--steer-when` (default: the agent's first "Opened" browser step).
   - It records until the session is Ready again. The output, `out/<name>/raw.mp4`, is 30 fps and real time, and the frame times go in `marks.json`.
2. `compose_live.py` builds the final clip from the frames and a narration file.
   - It places the recording beside a panel: the task, the agent's clock and tool-call count, the steps, and a description of the current step.
   - Step times in `narration/dashboard.json` are the session's event times (`GET /v1/sessions/{id}/events`). A new recording needs a new narration file.
   - Where a description needs longer to read than its step took, the picture freezes and the panel says "PAUSED TO EXPLAIN". Nothing is sped up or cut.
   - It ends with the agent's own screenshots.

`AR_SESSION` is a short-lived operator sign-in, because the web chat signs in with the operator cookie. To make one, add a row to the control database's `sessions` table: owner `operator`, the token's SHA-256 as `hash`, a random `csrf`, and `expires` a couple of hours ahead. Delete the row after recording. This way the operator password is never typed on camera.

To add a voice-over, read the narration in the script over the video; each scene holds long enough for it.
