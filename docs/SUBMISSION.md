# lablab.ai entry

Text for the Vultr hackathon entry, Blast Radius Zero track. The form's limits come from lablab's submission guidelines: title 5 to 50 characters, short description 50 to 255, long description 600 to 2000. The lead feature is the plug-in subagent: any app or agent hands work to a contained, offsite subagent over MCP or REST.

Not submitted yet; the deadline is 2026-11-08, 5:00 PM CST. The repository is public (since 2026-11-07). Every field below is ready to paste, and each file is also linked on GitHub for fields that take a URL.

## Title

Boxoro

## Short description

A contained, offsite subagent for the app or agent you already have. Plug it in over MCP or REST; it writes and runs code and drives a real browser inside its own gVisor box on Vultr, with no keys inside.

## Long description

Teams want agents that do real work: run code, test a web app in a browser, research with sources, turn data into a report. Running that agent on your own servers puts your keys, your network and your host one prompt injection away.

Boxoro lets the app or agent you already have hand that work to a contained subagent that runs offsite. Add it to Claude Code, Cursor or any MCP client as one remote server, or start a session from any app with one POST. Your app sends the task, can steer it mid-task, watches it over a live event stream, gets a signed webhook when it finishes, and downloads the files.

Each subagent session gets its own gVisor box on a private Vultr worker: a shell, headless Chromium, web search, language runtimes and file converters. The box holds no credentials, cannot reach cloud metadata, private networks, the host or other boxes, and has caps on memory, disk, time and spend. A subagent that goes wrong damages one box. On the live instance, 8 of 8 escape attempts from inside a box were blocked, a planted prompt injection was refused, and 21 earlier security probes all held.

Every agent model call goes to Vultr Serverless Inference (GLM 5.3 Flash). The control VM and a 16 vCPU / 64 GB worker are Vultr instances in Chicago. Through the public URL the worker has run 72 agent sessions at once, and real tasks cost cents: building and then using a kanban app took 92 tool calls for $0.024.

It is a Rust backend with a REST API, an MCP server with 15 tools, a web chat, skills, self-editable agent config and one-line installers. Its known limits are one trusted operator per deployment and no independent audit.

## Technologies and categories

Vultr, Vultr Serverless Inference, GLM, gVisor, Docker, Rust, MCP (Model Context Protocol), Chromium, AI agents, sandboxing, security, developer tools, API

## Other fields

- **Cover image (optional, 16:9):** `presentation/manim/out/previews/slide-01.png`, the title slide at 1920x1080 ([on GitHub](https://github.com/RictalNickel/agent-rush-runtime/blob/work/presentation/manim/out/previews/slide-01.png))
- **Demo application platform:** Vultr (two VMs in Chicago)
- **Demo application URL:** https://144-202-59-108.sslip.io
- **GitHub repository:** https://github.com/RictalNickel/agent-rush-runtime (public)
- **Slide presentation:** `presentation/manim/out/boxoro-deck.pdf` (8 slides, [on GitHub](https://github.com/RictalNickel/agent-rush-runtime/blob/work/presentation/manim/out/boxoro-deck.pdf))
- **Video presentation:** `presentation/video/out/demo.mp4` (4:40, 1080p, about 13 MB, captioned, [on GitHub](https://github.com/RictalNickel/agent-rush-runtime/blob/work/presentation/video/out/demo.mp4)). At 1:18 it shows a real screen recording of a new session in the live web chat, beside a panel explaining each step. The agent builds a Flask and SQLite shop with an API, tests and a dark page, and uses it in a browser. A second message sent mid-run asks for a low-stock badge, which it adds, tests and proves on the page. A follow-up turn asks for a PDF report. 70 s of agent time, $0.016. Its containment moments are the refused prompt injection and 8 of 8 escape attempts blocked, both run live on 2026-11-07. Script: `docs/VIDEO_SCRIPT.md`.
- **Interactive replay (optional extra):** `presentation/replay/out/index.html` ([on GitHub](https://github.com/RictalNickel/agent-rush-runtime/blob/work/presentation/replay/out/index.html); download it with `recording.mp4` beside it and open it in a browser) plays the same session's recording beside its full event log. Every message, command and output is clickable to jump the video.
- **Participation:** online or onsite, to be chosen by the team.
