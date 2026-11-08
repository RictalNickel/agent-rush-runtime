# Web chat session, screen-recorded live (2026-11-08 UTC)

A screen recording of one new session on the public instance, `https://144-202-59-108.sslip.io` (binary `8f9d5b65`), from the first click to the agent's answer. It was the live clip in an earlier cut of the demo video; on its own it is [presentation/video/out/live-session.mp4](../../../../presentation/video/out/live-session.mp4). The entry video now uses the [inventory app recording](../shop-recording-2026-11-08/).

How it was made:

- `presentation/video/record_live.py` drove headless Chromium against the live web chat. It clicked the "Build and test a web app" example, pressed send, and waited until the session went back to Ready. The screen was captured throughout and resampled to 30 fps at real speed.
- The web chat signs in with the operator cookie. For the recording, a sign-in that expired after 2 hours was added straight to the control database, so the operator password was never typed. That sign-in was deleted after the recording.
- `presentation/video/cut_live.sh` holds the opening frame for 5 s so the first caption can be read, then burns in the captions from `live_captions.ass`. Nothing in the session is sped up, cut or reordered.

What the session did (`session.json`, `session-events.json`), with times after the message arrived:

| Time | Step |
| --- | --- |
| 0.8 s | wrote `todo/app.py` |
| 2.1 s | wrote `todo/templates/index.html` |
| 2.7 s | started the app: it failed, because Flask was not installed |
| 5.2 s | installed Flask and started it again; the page answered |
| 9.1 s | opened `localhost:5000` in the box's browser and read the page |
| 13.1 s | typed "Buy milk" and submitted it |
| 15.4 s | clicked the item's complete button |
| 16.8 s | took the screenshot `todo/result.png` (also saved here as `result.png`) |
| 19.3 s | answered, with the screenshot attached |

Model use: 12 calls to `glm-5.3-flash` on Vultr Serverless Inference, 21,560 input and 939 output tokens, $0.0025.

One inaccuracy in the agent's answer: it says the items are "stored in a JSON file", but `app.py` keeps them in a Python list in memory. The video's captions do not repeat that claim.
