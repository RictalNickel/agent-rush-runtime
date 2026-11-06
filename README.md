# Agent Rush

Agent Rush is a backend for AI agents that do real work. Each session gets its own workspace box: a gVisor container on a private worker VM with a shell, a headless browser, language runtimes and file converters. Your app drives the agent through a REST API, a live event stream or an MCP server.

Everything the model runs stays inside the box. The box holds no credentials, and an egress filter keeps it away from cloud metadata, private networks and mail ports.

Status: the public instance is live on Vultr. See [Live demo](#live-demo).

## What it is

```
 your app, web chat or MCP client
        |  REST /v1, SSE, /mcp (API key)
        v
 +--------------------------------------------+   control VM (public HTTPS)
 | session engine   skills   agent config     |-----> Vultr Serverless Inference
 | ledger (SQLite)  remote MCP client         |       (GLM 5.3 Flash)
 +--------------------------------------------+
        |  mTLS + bearer token, private network
        v
 +--------------------------------------------+   worker VM (private)
 | box manager: Docker + gVisor (runsc)       |
 |   one box per session, /workspace volume   |-----> public internet only
 +--------------------------------------------+       (egress filter)
```

The control VM runs sessions, the inference gateway and the API. It never runs a command the model chose. The worker VM runs the boxes. The one-shot job path (`/api/jobs`) also runs on the worker, as described in [the design](docs/DESIGN.md).

## Why it's safe

- **One box per session.** Each session has its own gVisor container. It runs as uid 1000 with a read-only root filesystem, no Linux capabilities and no-new-privileges, with limits on processes, memory and CPU. It is destroyed when the session closes or after 30 idle minutes.
- **No credentials in the box.** API keys, the worker token, the inference key and remote MCP credentials stay on the control or worker side. The box qualification checks the box's environment and mounts for them.
- **Egress filter.** Boxes can reach the public internet. They cannot reach cloud metadata (169.254.169.254), private and CGNAT ranges, loopback, the host's own addresses, or SMTP ports 25, 465 and 587. Boxes cannot reach each other. Details are in [SECURITY.md](docs/SECURITY.md#egress-policy).
- **Fail-closed qualification.** A worker runs one-shot jobs only with a passing report of its 10 containment checks, and creates boxes only with a passing report of 8 box checks. Reports expire after two hours.
- **Owner isolation.** Keys, sessions, files and skills belong to one owner. Another owner's session returns 404, the same as a session that does not exist.
- **Spend caps.** Each model call reserves its worst-case cost before it is sent. The operator budget defaults to $5 (maximum $25). A turn stops before its next model call at $0.50, and a session at $5. A turn also ends after 60 steps or 30 minutes.
- **The agent cannot widen its own limits.** It can change its prompt addendum, tool choices, custom tools and skills. Trusted code still enforces the limits, the egress rules and the operator's list of MCP servers. See [what the agent can edit](docs/SECURITY.md#what-the-agent-can-and-cannot-edit).

## Quick start

On Ubuntu 24.04 (x86-64), from a checkout of this repository:

```sh
sudo scripts/install.sh --role all --yes
```

- `--role all` runs control and worker on one host. That is a weaker separation than two VMs, because the control plane and the boxes share a kernel and a disk. For public use, install `--role control` on one VM and `--role worker` on a private one.
- Preview first, which changes nothing: `scripts/install.sh --dry-run --role all --with browser,voice-stt`.
- Guided setup asks for the role, features, inference provider, key and admin password: `sudo scripts/setup.sh`.
- Session boxes need a workspace image. `--with browser` builds it on the worker. `scripts/build-images.sh --workspace` prints its reference. The installer does not write `AR_BOX_IMAGE` into `worker.env`, so set that variable to the printed reference before the worker's box qualification runs. See [INSTALL.md](docs/INSTALL.md).

Local development, without a worker. Sessions return 503 until a worker is connected:

```sh
cargo build --locked
export AR_OPERATOR_PASSWORD='choose-a-local-password-of-at-least-16-characters'
export AR_SECURE_COOKIES=false   # loopback development only
cargo run --locked -- serve      # http://127.0.0.1:8080
cargo test --locked
```

## Use it from your app

The REST API is documented in full in [docs/API.md](docs/API.md). A typical integration:

1. **Create an API key.** Sign in once with the operator password, then create a key. The key is shown once.

   ```sh
   HOST=https://agent.example.com
   curl -sc jar -H 'content-type: application/json' \
     -d '{"password":"<operator password>"}' $HOST/api/login         # returns {"csrf": "..."}
   curl -sb jar -H 'x-csrf-token: <csrf>' -H 'content-type: application/json' \
     -d '{"name":"laptop"}' $HOST/v1/keys                              # returns {"key": "ar_..."}
   export AR_KEY=ar_...
   ```

2. **Create a session and send the first message.** A message to an idle session starts a turn.

   ```sh
   curl -s -H "authorization: Bearer $AR_KEY" -H 'content-type: application/json' \
     -d '{"message":"Create a Flask hello-world app in /workspace/app and test it"}' \
     $HOST/v1/sessions
   # {"session": {"id": "6f1c...", "status": "running", ...}, "sent": {...}}
   ```

3. **Steer it while it works.** A message sent during a turn is an interjection. The agent sees it before its next step.

   ```sh
   curl -s -H "authorization: Bearer $AR_KEY" -H 'content-type: application/json' \
     -d '{"text":"Also add a /health route"}' $HOST/v1/sessions/$SID/messages
   ```

4. **Stream events.** The stream first replays events after your cursor, then sends new ones. Each event's `id` is its sequence number, `event` is its kind (`user`, `interjection`, `status`, `tool_call`, `tool_result`, `assistant`, `config_change`, `error`) and `data` is the event as JSON.

   ```sh
   curl -N -H "authorization: Bearer $AR_KEY" "$HOST/v1/sessions/$SID/stream?after=0"
   ```

5. **Download a file.** Downloads are always sent as attachments.

   ```sh
   curl -s -H "authorization: Bearer $AR_KEY" -o report.pdf \
     "$HOST/v1/sessions/$SID/files?path=report.pdf"
   ```

   Files live in the box. A box is destroyed after 30 idle minutes, and its `/workspace` files go with it. Download results before a session goes quiet.

6. **Get notified.** Each turn end is POSTed to your webhooks, signed with HMAC-SHA256 in `X-Agent-Signature`. Create one with `POST /v1/webhooks`. The URL must be a public `https` address. See [API.md](docs/API.md#webhooks).

## Use it from an MCP client

Agent Rush is also an MCP server at `/mcp` (streamable HTTP, same API keys):

```sh
claude mcp add --transport http agent https://<host>/mcp --header "Authorization: Bearer ar_..."
```

The tools are `start_session`, `send_message`, `wait_for_reply`, `get_events`, `run_command`, `read_file`, `write_file`, `list_files`, `cancel`, `close_session`, `list_sessions` and `manage_skills`. Cursor and other clients that take a remote HTTP server work the same way. See [docs/MCP.md](docs/MCP.md).

## What agents can do

These are the agent's built-in tools. Its configuration can disable any of them except `reply`.

| Tool | Runs in | What it does |
| --- | --- | --- |
| `bash` | box | Any shell command: build, test, install, and start servers in the background |
| `read_file`, `write_file`, `edit_file` | box | Read, create and exactly edit files under `/workspace` |
| `list_files`, `grep`, `glob` | box | Find files and search their contents |
| `browser` | box | Drive headless Chromium: open pages, snapshot, click, type, scroll, screenshot |
| `web_search`, `web_fetch` | box | Search the web without an API key, and read a page as text |
| `convert_file` | box | Convert between documents, images, audio and video (pandoc, LibreOffice, ImageMagick, ffmpeg) |
| `skill` | control | List, load, save and delete skills |
| `agent_config` | control | Read or change its own configuration, review its history and revert it |
| `mcp` | control | List and call tools on the operator's remote MCP servers |
| `reply` | control | End the turn with the answer and optional attachments |

Each model call makes one tool call, and the result goes back to the model. The loop continues until `reply`, a limit or a cancel. The box publishes no ports, so a web app the agent starts on `localhost` is reachable only from inside the box.

## Extend the agent

- **Skills.** Owner-scoped skills in the `SKILL.md` format: a name, a description and a body of up to 64 KiB, up to 200 per owner. Three built-in skills (`web-research`, `app-testing`, `file-conversion`) are always available. Manage yours with `/v1/skills`, or let the agent save them.
- **Custom tools.** Define up to 20 tools as bash command templates with `{{arg}}` placeholders. Each value is shell-quoted before the command runs in the box. Custom tools cannot reuse built-in names.
- **Agent config.** `system_addendum` (up to 8 KiB), `disabled_tools`, `custom_tools`, `mcp_servers` (only servers the operator allows) and `reasoning_effort` (`low`, `medium` or `high`). Every change is a new version, and any version can be restored. See [API.md](docs/API.md#endpoints).
- **Remote MCP servers.** The operator lists servers in a JSON file named by `AR_MCP_SERVERS_FILE`: a name, a streamable HTTP URL, optional headers and an optional `allowed_tools` list. Headers hold credentials, which stay on the control VM. Up to 32 servers. MCP stdio servers are not run on the control VM.

## Optional features

| Feature | Install with | What it adds |
| --- | --- | --- |
| Workspace image | `--with browser` | Builds the box image: shell, Chromium, converters. Boxes need it. |
| Voice input | `--with voice-stt` | Speech to text with Moonshine on CPU. A short clip took about 0.6 s on a 4 vCPU VM. |
| Voice output | `--with voice-tts` | Speech output with LuxTTS on CPU. Needs one reference clip. About 1 s of compute per second of speech. |
| HTTPS | `--with tls` | Caddy with an `sslip.io` name on ports 80 and 443. |

Voice is off by default. The `/v1/voice/*` routes return 404 until `AR_VOICE_URL` is set. See [docs/VOICE.md](docs/VOICE.md).

## Configuration

Settings are environment variables. The most important are below. The full list, with defaults taken from the code, is in [docs/CONFIGURATION.md](docs/CONFIGURATION.md).

| Variable | Default | Purpose |
| --- | --- | --- |
| `AR_OPERATOR_PASSWORD` | none | Operator sign-in. At least 16 characters. |
| `AR_INFERENCE_PROVIDER` | `vultr` | `vultr` for Vultr Serverless Inference, or `anthropic` for development. |
| `VULTR_INFERENCE_API_KEY` | none | Vultr inference key, read by the control VM only. |
| `AR_INFERENCE_MODEL` | none | Vultr catalog model ID. The public instance uses `glm-5.3-flash`. |
| `AR_INFERENCE_BUDGET_USD` | `5` | Total model spend the ledger allows. Range 0 to 25. |
| `AR_WORKER_URL`, `AR_WORKER_TOKEN` | none | Private worker link. The token is at least 32 characters. |
| `AR_BOX_IMAGE` | none | Workspace image reference the worker runs boxes from. |
| `AR_BOX_IDLE_SECONDS` | `1800` | Idle seconds before a box and its `/workspace` are destroyed. |
| `AR_BOX_CEILING_MB` | `4096` | Largest memory a box may grow to. |
| `AR_MCP_SERVERS_FILE` | none | JSON file of remote MCP servers the agent may use. |
| `AR_VOICE_URL` | none | Local voice service, for example `http://127.0.0.1:8790`. |

## Live demo

- **Public instance:** https://144-202-59-108.sslip.io (Let's Encrypt certificate through Caddy).
- **Where it runs:** two Vultr VMs in Chicago (`ord`): a control VM (2 vCPU, 4 GB) and a private worker VM (4 vCPU, 8 GB), about $0.082 per hour in total. See [LIVE_DEPLOYMENT.md](docs/LIVE_DEPLOYMENT.md).
- **Inference:** the public instance sends every agent call to Vultr Serverless Inference, using GLM 5.3 Flash.
- **Evidence:** [VERIFICATION.md](docs/VERIFICATION.md) lists what was observed and when. Raw receipts are in [docs/evidence/live/](docs/evidence/live/), including the box qualification (`box-qualification/`), the workspace image build (`workspace-image/`), a box round trip (`boxes/`) and the voice service run (`voice/`).

What is not yet done is listed in [LIVE_DEPLOYMENT.md](docs/LIVE_DEPLOYMENT.md) and [VERIFICATION.md](docs/VERIFICATION.md).

## Hackathon fit (Blast Radius Zero)

- **Every agent LLM call goes to Vultr.** Job `7d466fa9` on the public instance made 5 model calls, all to `glm-5.3-flash`. It finished in 58 s for $0.0058, and its reviewer approved the result. Receipt: [docs/evidence/live/vultr-public/](docs/evidence/live/vultr-public/).
- **Everything runs on Vultr.** The control and worker VMs are both Vultr instances in Chicago.
- **Containment evidence:**
  - 8 of 8 box checks pass: [box-qualification.json](docs/evidence/live/box-qualification/box-qualification.json).
  - The egress rules as applied: [egress-policy.txt](docs/evidence/live/box-qualification/egress-policy.txt).
  - The 10 job containment checks: [worker-qualification-2026-11-05.json](docs/evidence/live/worker-qualification-2026-11-05.json) (nine at first deploy) and [VERIFICATION.md](docs/VERIFICATION.md) (10 of 10 after the scaling check was added).
  - A containment probe receipt, runaway runs killed on timeout, and an overlapping run refused with HTTP 429: [containment/](docs/evidence/live/containment/) and [load/](docs/evidence/live/load/).
- **Limits, stated plainly.** One operator per deployment. No independent security audit. No multi-tenant claim. The box checks prove eight specific things, not escape resistance in general. See [SECURITY.md](docs/SECURITY.md).

## Documentation

- [API](docs/API.md), [MCP](docs/MCP.md), [install](docs/INSTALL.md), [voice](docs/VOICE.md)
- [Design](docs/DESIGN.md) and [security](docs/SECURITY.md)
- [Configuration](docs/CONFIGURATION.md) and [deployment](docs/DEPLOYMENT.md)
- [Live deployment](docs/LIVE_DEPLOYMENT.md) and [verification](docs/VERIFICATION.md)

## License

MIT. See [LICENSE](LICENSE). Third-party components and their licenses are listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
