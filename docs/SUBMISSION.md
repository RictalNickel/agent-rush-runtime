# lablab.ai entry (draft)

Draft text for the Vultr hackathon entry, Blast Radius Zero track. The form's limits come from lablab's submission guidelines: title 5 to 50 characters, short description 50 to 255, long description 600 to 2000. The lead feature is the plug-in subagent: any app or agent hands work to a contained, offsite subagent over MCP or REST.

Not submitted. The repository is still private, and the demo video is not recorded yet.

## Title

Vultr-Placeholder-Test123

(The product name is a placeholder. Swap it here, in `README.md` and in `PRODUCT` at the top of `presentation/manim/deck.py`.)

## Short description

A contained, offsite subagent for the app or agent you already have. Plug it in over MCP or REST; it writes and runs code and drives a real browser inside its own gVisor box on Vultr, with no keys inside.

## Long description

Teams want agents that do real work: run code, test a web app in a browser, research with sources, turn data into a report. Running that agent on your own servers puts your keys, your network and your host one prompt injection away.

Vultr-Placeholder-Test123 lets the app or agent you already have hand that work to a contained subagent that runs offsite. Add it to Claude Code, Cursor or any MCP client as one remote server, or start a session from any app with one POST. Your app sends the task, can steer it mid-task, watches it over a live event stream, gets a signed webhook when it finishes, and downloads the files.

Each subagent session gets its own gVisor box on a private Vultr worker: a shell, headless Chromium, web search, language runtimes and file converters. The box holds no credentials, cannot reach cloud metadata, private networks, the host or other boxes, and has caps on memory, disk, time and spend. A subagent that goes wrong damages one box. The workers refuse to run until they pass their containment checks (9 for boxes, 10 for one-shot jobs), and 21 live security probes of the public instance all held.

Every agent model call goes to Vultr Serverless Inference (GLM 5.3 Flash). The control VM and a 16 vCPU / 64 GB worker are Vultr instances in Chicago. Through the public URL the worker has run 72 agent sessions at once, and real tasks cost cents: building and then using a kanban app took 92 tool calls for $0.024.

It is a Rust backend with a REST API, an MCP server with 15 tools, a web chat, skills, self-editable agent config and one-line installers. Limits are stated plainly: one trusted operator per deployment, and no independent audit.

## Technologies and categories

Vultr, Vultr Serverless Inference, GLM, gVisor, Docker, Rust, MCP (Model Context Protocol), Chromium, AI agents, sandboxing, security, developer tools, API

## Other fields

- **Demo application platform:** Vultr (two VMs in Chicago)
- **Demo application URL:** https://144-202-59-108.sslip.io
- **GitHub repository:** https://github.com/RictalNickel/agent-rush-runtime (private until the owner approves making it public)
- **Slide presentation:** `presentation/manim/out/vultr-placeholder-test123-deck.pdf`
- **Video presentation:** not recorded yet. It must show at least one containment moment.
- **Participation:** online or onsite, to be chosen by the team.
