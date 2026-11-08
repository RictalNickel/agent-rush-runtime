# Interactive session replay

`out/index.html` is a single page about one live session: the inventory app recorded on 2026-11-08 ([evidence](../../docs/evidence/live/shop-recording-2026-11-08/)). Open it in a browser next to `out/recording.mp4`. It shows:

- the screen recording, at 1×, 2× or 4×;
- the session's totals;
- chapters, with a description of the current step that follows the video;
- every message, tool call and output from the session's event log, in order. Each one opens to show the exact command or file and what came back, and jumps the video to that moment. The log can be filtered to messages, shell, files, browser or failed steps, and follows the video as it plays.

Build it from a recording (`../video/record_live.py`), its narration file and the session's own JSON:

```bash
python3 build_replay.py ../video/out/live-shop ../video/narration/shop.json \
  ../../docs/evidence/live/shop-recording-2026-11-08/session.json \
  ../../docs/evidence/live/shop-recording-2026-11-08/session-events.json out/
```

`template.html` is the page; `build_replay.py` fills in the data and re-encodes the recording at 1200x900.
