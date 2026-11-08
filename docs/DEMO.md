# Demonstration

## One-sentence pitch

Boxoro gives the app or agent you already have a contained, offsite subagent: it writes and runs code and drives a browser inside its own gVisor box on Vultr, and the blast radius of anything that goes wrong is that one box.

## The recorded demo

The entry video is `presentation/video/out/demo.mp4` (4:40). Its scene list and narration are in [VIDEO_SCRIPT.md](VIDEO_SCRIPT.md). Every output it replays is in [evidence/live/plugin-and-containment-2026-11-07/](evidence/live/plugin-and-containment-2026-11-07/). Its screen recording of a live web chat session is in [evidence/live/shop-recording-2026-11-08/](evidence/live/shop-recording-2026-11-08/): an inventory app built and used in a browser, changed by a message sent mid-run, then a PDF report in a follow-up turn. To step through that session, open the interactive replay, [presentation/replay/out/index.html](../presentation/replay/out/index.html).

## A live demo in three steps

1. **Plug it in.** Add the public instance to Claude Code as an MCP server (`claude mcp add --transport http agent https://144-202-59-108.sslip.io/mcp --header "Authorization: Bearer ar_..."`) and give it a task to delegate, such as the one in the evidence folder's `claude-code-prompt.txt`.
2. **Watch it.** Open the web chat, where the session shows each tool call, its files and its model cost.
3. **Try to break out.** Run `AR_KEY=ar_... qa/containment_demo.py`. It runs eight escape attempts inside a live box and prints one line each; all eight are blocked.

Use an API key on camera, never the operator password, and revoke the key afterwards.
