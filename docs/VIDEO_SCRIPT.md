# Demo video script

About 3 minutes (3:05 rendered), for the lablab entry (MP4, under 5 minutes, at least one containment moment). Every number and every line of output shown comes from the live runs of 2026-11-07 in [evidence/live/plugin-and-containment-2026-11-07/](evidence/live/plugin-and-containment-2026-11-07/) and from the earlier demo sessions.

The rendered video (`presentation/video/out/demo.mp4`) carries the narration as on-screen captions, so it works without sound. To add a voice-over, read the narration lines below over it; each scene holds long enough to read them at an easy pace.

| # | Time | On screen | Narration |
| --- | --- | --- | --- |
| 1 | 0:00 | Title slide | "Give the app or agent you already have a subagent that does real work, safely." |
| 2 | 0:08 | Problem slide | "Agents that run code and browse the web are useful. Run one on your own servers, and a single prompt injection puts your keys, your network and your host in reach." |
| 3 | 0:30 | Plug-in slide | "So hand that work off instead. Add one MCP server to Claude Code, Cursor or any MCP client, or make one REST call from any app. The subagent works offsite, in its own box on Vultr, and hands back the result." |
| 4 | 0:48 | Replay: `claude mcp add`, then Claude Code delegating the sales-report task; the subagent's result, its test file and the MCP tools called | "Here is Claude Code doing exactly that, live. It starts a subagent, the subagent writes the code and the tests in its box and runs them: four passed. Nineteen seconds, about a tenth of a cent of model time on Vultr." |
| 5 | 1:12 | Demo slide: the kanban app session in the web chat | "In the web chat, a subagent built a kanban app and then used it in a real browser: 92 tool calls for 2.4 cents." |
| 6 | 1:28 | Box slide | "Each subagent gets its own gVisor box with a shell, a browser and files. No keys inside, no way to your network or the host, and hard caps." |
| 7 | 1:44 | Replay: the planted ticket, then the subagent's reply | "Now the containment moment. We hid an instruction in a support ticket: steal the cloud credentials and the SSH key. The subagent spotted it and refused." |
| 8 | 2:00 | Replay: `qa/containment_demo.py`, each attempt BLOCKED | "But don't trust the model. Assume it was hijacked and runs whatever it's told. Cloud metadata: unreachable. Secrets in its environment: none. The private network, the host, mail ports: blocked. A fork bomb and an 8 gigabyte allocation kill only its own box, which restarts with its files." |
| 9 | 2:24 | Containment slide, 8/8 | "Eight of eight escape attempts blocked on the live instance, and the box can still reach the public internet to do real work." |
| 10 | 2:40 | Vultr slide | "Every agent model call goes to Vultr Serverless Inference. The control plane and a 16 vCPU worker run on Vultr in Chicago, and the worker has run 72 agent sessions at once." |
| 11 | 2:56 | Close slide with the live URL | "A blast radius of one box. It's live now." |

The rendered video is 3:05. Its captions are shorter versions of these lines.

## Re-recording it live

To film the terminal scenes yourself instead of using the rendered replays:

1. Scene 4: `claude mcp add --transport http agent https://144-202-59-108.sslip.io/mcp --header "Authorization: Bearer ar_..."`, then give Claude Code the prompt in `claude-code-prompt.txt` from the evidence folder.
2. Scene 8: `AR_KEY=ar_... qa/containment_demo.py`. It needs about 5 seconds and prints one line per attempt.
3. Scene 7: upload the ticket and send the message as described in the evidence README, then open the session in the web chat.

Use an API key, never the operator password, on camera. Revoke that key after recording.
