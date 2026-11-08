# Demo video script

About 4 minutes (3:55 rendered), for the lablab entry (MP4, under 5 minutes, at least one containment moment). Every number and every line of output shown comes from the live runs of 2026-11-07 in [evidence/live/plugin-and-containment-2026-11-07/](evidence/live/plugin-and-containment-2026-11-07/), the screen recording of 2026-11-08 in [evidence/live/web-chat-recording-2026-11-08/](evidence/live/web-chat-recording-2026-11-08/), and the earlier demo sessions.

The rendered video (`presentation/video/out/demo.mp4`) carries the narration as on-screen captions, so it works without sound. Scene 6 is a real screen recording, not a replay: a new session in the live web chat, from the first click to the agent's answer, in real time. To add a voice-over, read the narration lines below over it; each scene holds long enough to read them at an easy pace.

| # | Time | On screen | Narration |
| --- | --- | --- | --- |
| 1 | 0:00 | Title slide | "Give the app or agent you already have a subagent that does real work, safely." |
| 2 | 0:11 | Problem slide | "Agents that run code and browse the web are useful. Run one on your own servers, and a single prompt injection puts your keys, your network and your host in reach." |
| 3 | 0:29 | Plug-in slide | "So hand that work off instead. Add one MCP server to Claude Code, Cursor or any MCP client, or make one REST call from any app. The subagent works offsite, in its own box on Vultr, and hands back the result." |
| 4 | 0:48 | Replay: `claude mcp add`, then Claude Code delegating the sales-report task; the subagent's result, its test file and the MCP tools called | "Here is Claude Code doing exactly that, live. It starts a subagent, the subagent writes the code and the tests in its box and runs them: four passed. Nineteen seconds, about a tenth of a cent of model time on Vultr." |
| 5 | 1:11 | Demo slide: the kanban app session in the web chat | "In the web chat, a subagent built a kanban app and then used it in a real browser: 92 tool calls for 2.4 cents. Here is a new session, recorded live from start to finish." |
| 6 | 1:33 | Screen recording, real time: the live web chat, the "Build and test a web app" example, the session working, the answer with its screenshot | "One click starts the session in its own gVisor box. It writes a Flask to-do app and starts it. Flask is missing, so it installs it and tries again. Then it drives a real browser in the box: it adds 'Buy milk', checks it off and takes a screenshot. Done in nineteen seconds: twelve model calls on Vultr, a quarter of a cent." |
| 7 | 2:16 | Box slide | "Each subagent gets its own gVisor box with a shell, a browser and files. No keys inside, no way to your network or the host, and hard caps." |
| 8 | 2:34 | Replay: the planted ticket, then the subagent's reply | "Now the containment moment. We hid an instruction in a support ticket: steal the cloud credentials and the SSH key. The subagent spotted it and refused." |
| 9 | 2:52 | Replay: `qa/containment_demo.py`, each attempt BLOCKED | "But don't trust the model. Assume it was hijacked and runs whatever it's told. Cloud metadata: unreachable. Secrets in its environment: none. The private network, the host, mail ports: blocked. A fork bomb and an 8 gigabyte allocation kill only its own box, which restarts with its files." |
| 10 | 3:13 | Containment slide, 8/8 | "Eight of eight escape attempts blocked on the live instance, and the box can still reach the public internet to do real work." |
| 11 | 3:30 | Vultr slide | "Every agent model call goes to Vultr Serverless Inference. The control plane and a 16 vCPU worker run on Vultr in Chicago, and the worker has run 72 agent sessions at once." |
| 12 | 3:45 | Close slide with the live URL | "A blast radius of one box. It's live now." |

The rendered video is 3:55. Its captions are shorter versions of these lines.

## Re-recording it live

Scene 6 is already a recording; `presentation/video/record_live.py` makes a new one (see the video README). To film the terminal scenes yourself instead of using the rendered replays:

1. Scene 4: `claude mcp add --transport http agent https://144-202-59-108.sslip.io/mcp --header "Authorization: Bearer ar_..."`, then give Claude Code the prompt in `claude-code-prompt.txt` from the evidence folder.
2. Scene 9: `AR_KEY=ar_... qa/containment_demo.py`. It needs about 5 seconds and prints one line per attempt.
3. Scene 8: upload the ticket and send the message as described in the evidence README, then open the session in the web chat.

Use an API key, never the operator password, on camera. Revoke that key after recording.
