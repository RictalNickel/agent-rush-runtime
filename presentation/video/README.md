# Demo video

`out/demo.mp4` is the entry video: 3:05, 1920x1080, H.264 with a silent AAC track, about 8 MB. It plays the pitch deck's slides and replays of the 2026-11-07 live runs, with captions taken from [docs/VIDEO_SCRIPT.md](../../docs/VIDEO_SCRIPT.md). Every line of terminal output in it is copied from [the evidence](../../docs/evidence/live/plugin-and-containment-2026-11-07/).

- `video.py` holds the replay scenes and the scene list. It imports the deck (`../manim/deck.py`), so the style and the product name come from there.
- `./render.sh` renders it. It needs the deck's virtualenv (`../manim/render.sh` makes it). `QUALITY=l ./render.sh` makes a fast 480p draft.

To add a voice-over, read the narration in the script over the video; each scene holds long enough for it.
