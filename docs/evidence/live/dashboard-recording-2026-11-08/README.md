# Dashboard session with a mid-run message, screen-recorded live (2026-11-08 UTC)

The live demo in an earlier cut of the entry video; on its own it is `presentation/video/out/live-dash.mp4`. The entry video now uses the [inventory app recording](../shop-recording-2026-11-08/). One new session on the public instance, `https://144-202-59-108.sslip.io` (binary `8f9d5b65`), from typing the task to the agent's last answer.

## How it was recorded

- `presentation/video/record_live.py` drove headless Chromium against the live web chat. It typed the brief in [presentation/video/tasks/dashboard.txt](../../../../presentation/video/tasks/dashboard.txt) and sent it. Once the thread showed the agent's first browser step, it typed and sent [dashboard-steer.txt](../../../../presentation/video/tasks/dashboard-steer.txt) while the agent was still working. It then waited for the session to go back to Ready. The screen was captured throughout.
- The web chat signs in with the operator cookie. A sign-in that expired after 2 hours was added straight to the control database, so the operator password was never typed. That sign-in was deleted after the recording.
- `presentation/video/compose_live.py` puts the recording next to a narration panel built from [narration/dashboard.json](../../../../presentation/video/narration/dashboard.json). The step times in that file are the event times in `session-events.json`.
- The recording plays in real time. Where a step's description needs more reading time than the step took, the picture freezes at the start of that step and the badge reads "PAUSED TO EXPLAIN", with the agent clock stopped. There are 9 such pauses, 51.0 s in all. Nothing is sped up, cut or reordered.

## What the session did

Times are seconds after the first message arrived (`session-events.json`):

| Time | Step |
| --- | --- |
| 3.7 s | One shell command writes `generate.py` (2,000 orders, seed 42), `analyze.py` (pandas into `summary.json`) and `app.py` (Flask and Chart.js), then runs them. Total: $6,414,795 from 2,000 orders. The server fails: Flask is not installed. |
| 7.7 s | Installs Flask, restarts, and checks the page contains the total and the months Jan and Dec. |
| 12.6 s | Opens the page in the box's browser, reads the KPI cards from the page, and takes `dashboard.png`. |
| 19.1 s | The mid-run message arrives: add a region filter, check West against the CSV, and send a second screenshot. |
| 19.7 s | Posts the first answer, then starts on the new request. It computes West's figures from the CSV with pandas. |
| 23.1 to 28.5 s | Adds per-region data, then patches `app.py` with the dropdown, the filtered cards, monthly chart and table, and a highlighted bar for the chosen region. It checks the file parses and restarts the server. |
| 31.2 to 40.7 s | Selects West in the browser. Its first `select` uses a CSS selector instead of a snapshot ref and fails, so it takes a snapshot and selects again. Then it reads the cards. |
| 41.2 s | Takes `west.png`. |
| 43.8 s | Runs an independent pandas check: 489 orders, $1,480,535, average $3,027.68, which matches the page. |
| 45.4 s | Final answer, with `west.png` attached. |

Model use (`session.json`): 26 calls to `glm-5.3-flash` on Vultr Serverless Inference, 95,934 input and 5,257 output tokens, $0.0114. The run made 24 tool calls.

## A first take that was not used

The same brief was recorded once before, as session `f4ab55f4`. That dashboard drew its monthly chart with the months in alphabetical order (Apr, Aug, Dec, ...), and its region chart did not respond to the filter, although the agent said every chart did. `take1-dashboard.png` shows it. The brief was then made explicit ("revenue by month in calendar order (Jan to Dec)", "check ... the months run Jan to Dec", and which parts the filter should change), and the session was recorded again. The take above is the second one, shown as it ran.
