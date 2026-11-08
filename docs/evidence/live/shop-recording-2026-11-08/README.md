# Inventory app session, two turns and a mid-run message, screen-recorded live (2026-11-08 UTC)

This is the live demo in the entry video: `presentation/video/out/demo.mp4`, from 1:18. On its own it is `presentation/video/out/live-shop.mp4`. The interactive replay, `presentation/replay/out/index.html`, puts the recording beside every event below.

It shows one new session on the public instance, `https://144-202-59-108.sslip.io` (binary `8f9d5b65`), with three messages from us:

1. The brief: [tasks/shop.txt](../../../../presentation/video/tasks/shop.txt), typed and sent.
2. While the agent was still working: [tasks/shop-steer.txt](../../../../presentation/video/tasks/shop-steer.txt).
3. After the agent's first answer: [tasks/shop-followup.txt](../../../../presentation/video/tasks/shop-followup.txt).

At the end, the recording opens the Files panel and steps into `shop/`.

## How it was recorded

- `presentation/video/record_live.py` drove headless Chromium against the live web chat.
- It signed in with an operator sign-in made for this recording: a row added straight to the control database, so the operator password was never typed. The row expired after 2 hours and was deleted after the recording.
- `presentation/video/compose_live.py` puts the recording beside a narration panel, built from [narration/shop.json](../../../../presentation/video/narration/shop.json). Its step times are the event times in `session-events.json`.
- The clip plays in real time. Where a step needs longer to read than it took, the picture freezes and the badge reads "PAUSED TO EXPLAIN". There are 8 such freezes, 20.4 s in all.
- The clip stops 1.5 s after the Files panel opens. Nothing is sped up, cut or reordered.

## What the session did

Times are seconds after the first message arrived:

| Time | Step |
| --- | --- |
| 0.6 s | Checks for Flask, finds it missing, and installs Flask and pytest. |
| 6.1 s | Writes `app.py` (SQLite, `GET /api/products`, `POST /api/orders`, 409 when stock is short), `templates/index.html` and `test_app.py`. 6 tests pass. |
| 13.0 s | Starts the server and checks the API answers. |
| 15.7 s | Opens the page in the box's browser, then picks a product and orders it through the form. |
| 21.7 s | The mid-run message arrives: add a red "Low stock" badge, test it, restart, and check the page. |
| 23.5 s | Reads its own files. It adds the badge and its style, then looks for `tests/test_api.py`, which doesn't exist (a failed step). It finds `test_app.py` instead and adds a seventh test. 7 pass. |
| 32.4 s | Restarts the server. On the page, "Low stock" is not there yet. It orders 20 keyboards through the form (stock 24 to 4), then checks again: "Low stock" is now on the page. |
| 40.9 s | Takes `screenshot.png`, re-runs the tests (7 passed) and answers. |
| 50.5 s | The follow-up arrives: a one-page A4 PDF report. |
| 51.6 s | Tries the `sqlite3` command, which isn't in the box (a failed step), and queries with Python instead: 2 orders, $1,889.79. |
| 57.0 s | Its first script needs `reportlab`, which isn't installed. It draws the page with matplotlib instead. |
| 60.6 s | Opens the PDF with PyMuPDF. The page is A4 and the content ends at x 547.8 of 595.4 and y 758.7 of 841.7 points. |
| 70.0 s | Answers, with `report.pdf` attached. |

Model use (`session.json`): 39 calls to `glm-5.3-flash` on Vultr Serverless Inference, 129,486 input and 7,945 output tokens, $0.0157. The session made 37 tool calls.

Results: the agent's `screenshot.png`, and `report.pdf` with its first page rendered as `report-page1.png`. The PDF fits its page, but the order table's columns are cramped, and the chart has one bar, because both orders were for the same product. The brief asked to attach the test results; the agent reported them in its answer but attached only the screenshot.

## A first take that was not used

The same brief was recorded once before, as session `b03dbfe2`, with a mid-run message that did not mention restarting. That agent added the badge to the code, but the server it checked was still running the old code. It then said the badge showed in its screenshot. `take1-screenshot.png` shows that it does not. Its PDF also overflowed the page on the right.

The mid-run message was then made to ask for a restart and a check on the page, the follow-up was made to ask for everything to fit the page, and the session was recorded again. Take 1's event log is `take1-session-events.json`.
