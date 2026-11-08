# lablab.ai entry

Text for the Vultr hackathon entry, Blast Radius Zero track. The form's limits come from lablab's submission guidelines: title 5 to 50 characters, short description 50 to 255, long description 600 to 2000. The lead feature is the plug-in subagent: any app or agent hands work to a contained, offsite subagent over MCP or REST.

Not submitted yet; the deadline is 2026-11-08, 5:00 PM CST. The repository is public (since 2026-11-07). Every field below is ready to paste, and each file is also linked on GitHub for fields that take a URL.

## Title

Boxoro

## Short description

A contained subagent your app or agent calls over MCP or REST. It runs code and a real browser in its own gVisor box on Vultr, with no keys inside.

## Long description

Agents that run code and browse the web are one prompt injection away from your keys, network and host. Boxoro moves that work offsite.

Your app or agent hands a task to a Boxoro subagent over MCP (one remote server in Claude Code, Cursor or any MCP client) or REST (one POST). It can steer the task mid-run, stream events, get a signed webhook and download the files.

Each session gets its own gVisor box on a private Vultr worker, with a shell, headless Chromium, web search and file converters. The box holds no credentials, can't reach cloud metadata, private networks, the host or other boxes, and has caps on memory, disk, time and spend. On the live instance, 8 of 8 escape attempts were blocked and a planted prompt injection was refused.

It runs on Vultr: two VMs in Chicago, with every model call on Vultr Serverless Inference (DeepSeek V4.1 Flash by default, GLM selectable). It has run 72 sessions at once, and real tasks cost cents.

Rust backend, REST API, 15-tool MCP server, web chat. Limits: one operator per deployment, no independent audit.

## Technologies and categories

Vultr, Vultr Serverless Inference, DeepSeek, gVisor, Rust, MCP, AI agents, sandboxing, security

## Other fields

- **Cover image (optional, 16:9):** `presentation/manim/out/previews/slide-01.png`, the title slide at 1920x1080 ([on GitHub](https://github.com/RictalNickel/agent-rush-runtime/blob/work/presentation/manim/out/previews/slide-01.png))
- **Demo application platform:** Vultr (two VMs in Chicago)
- **Demo application URL:** https://144-202-59-108.sslip.io
- **GitHub repository:** https://github.com/RictalNickel/agent-rush-runtime (public)
- **Slide presentation:** `presentation/manim/out/boxoro-deck.pdf` (8 slides, [on GitHub](https://github.com/RictalNickel/agent-rush-runtime/blob/work/presentation/manim/out/boxoro-deck.pdf))
- **Video presentation:** `presentation/video/out/demo.mp4` (4:40, 1080p, about 13 MB, captioned, [on GitHub](https://github.com/RictalNickel/agent-rush-runtime/blob/work/presentation/video/out/demo.mp4)). From 1:18, a live session builds and tests a small shop app, takes a mid-run change and writes a PDF report. Script: `docs/VIDEO_SCRIPT.md`.
- **Interactive replay (optional extra):** `presentation/replay/out/index.html` ([on GitHub](https://github.com/RictalNickel/agent-rush-runtime/blob/work/presentation/replay/out/index.html); download it with `recording.mp4` beside it and open it in a browser) plays the same session's recording beside its full event log. Every message, command and output is clickable to jump the video.
- **Participation:** online or onsite, to be chosen by the team.
