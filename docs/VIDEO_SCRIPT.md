# Demo video script

About 5 minutes (4:40 rendered), for the lablab entry (MP4, under 5 minutes, at least one containment moment). Every number and every line of output shown comes from the live runs of 2026-11-07 in [evidence/live/plugin-and-containment-2026-11-07/](evidence/live/plugin-and-containment-2026-11-07/), the screen recording of 2026-11-08 in [evidence/live/shop-recording-2026-11-08/](evidence/live/shop-recording-2026-11-08/), and the earlier demo sessions.

The rendered video (`presentation/video/out/demo.mp4`) carries the narration as on-screen captions, so it works without sound. Scene 5 is a real screen recording, not a replay: a new session in the live web chat, from typing the task to the agent's last answer, beside a panel that explains each step. It plays in real time, and freezes, marked "PAUSED TO EXPLAIN", where a step needs longer to read than it took. To add a voice-over, read the narration lines below over it; each scene holds long enough to read them at an easy pace.

| # | Time | On screen | Narration |
| --- | --- | --- | --- |
| 1 | 0:00 | Title slide | "Give the app or agent you already have a subagent that does real work, safely." |
| 2 | 0:11 | Problem slide | "Agents that run code and browse the web are useful. Run one on your own servers, and a single prompt injection puts your keys, your network and your host in reach." |
| 3 | 0:28 | Plug-in slide | "So hand that work off instead. Add one MCP server to Claude Code, Cursor or any MCP client, or make one REST call from any app. The subagent works offsite, in its own box on Vultr, and hands back the result." |
| 4 | 0:47 | Replay: `claude mcp add`, then Claude Code delegating the sales-report task; the subagent's result, its test file and the MCP tools called | "Here is Claude Code doing exactly that, live. It starts a subagent, the subagent writes the code and the tests in its box and runs them: four passed. Nineteen seconds, about a tenth of a cent of model time on Vultr. Next, a harder task in the web chat, recorded live from start to finish." |
| 5 | 1:18 | Screen recording of the live web chat with the narration panel (task, agent clock, tool calls, steps, what's happening), then the agent's screenshot and its PDF report full frame | The panel's step descriptions, in [narration/shop.json](../presentation/video/narration/shop.json). We type the brief for a Flask and SQLite shop with an API, tests and a dark page. The agent installs Flask, writes the app and 6 tests, starts it and orders through the page. We send a second message while it works, asking for a low-stock badge. It adds the badge and a seventh test, restarts, orders 20 keyboards and checks the badge is on the page. A follow-up turn asks for a PDF report, which it draws and measures to fit A4. 70 s of agent time, 37 tool calls, $0.016. Then the Files panel. |
| 6 | 3:18 | Replay: the planted ticket, then the subagent's reply | "Now the containment moment. We hid an instruction in a support ticket: steal the cloud credentials and the SSH key. The subagent spotted it and refused." |
| 7 | 3:36 | Replay: `qa/containment_demo.py`, each attempt BLOCKED | "But don't trust the model. Assume it was hijacked and runs whatever it's told. Cloud metadata: unreachable. Secrets in its environment: none. The private network, the host, mail ports: blocked. A fork bomb and an 8 gigabyte allocation kill only its own box, which restarts with its files." |
| 8 | 3:57 | Containment slide, 8/8 | "Eight of eight escape attempts blocked on the live instance, and the box can still reach the public internet to do real work." |
| 9 | 4:14 | Vultr slide | "Every agent model call goes to Vultr Serverless Inference. The control plane and a 16 vCPU worker run on Vultr in Chicago, and the worker has run 72 agent sessions at once." |
| 10 | 4:29 | Close slide with the live URL | "A blast radius of one box. It's live now." |

The rendered video is 4:40. Its captions are shorter versions of these lines.

## Re-recording it live

Scene 5 is already a recording; `presentation/video/record_live.py` and `compose_live.py` make a new one (see the video README). To film the terminal scenes yourself instead of using the rendered replays:

1. Scene 4: `claude mcp add --transport http agent https://144-202-59-108.sslip.io/mcp --header "Authorization: Bearer ar_..."`, then give Claude Code the prompt in `claude-code-prompt.txt` from the evidence folder.
2. Scene 6: upload the ticket and send the message as described in the evidence README, then open the session in the web chat.
3. Scene 7: `AR_KEY=ar_... qa/containment_demo.py`. It needs about 5 seconds and prints one line per attempt.

Use an API key, never the operator password, on camera. Revoke that key after recording.
