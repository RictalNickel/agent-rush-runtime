# Vultr-Placeholder-Test123

_Previously called Agent Rush. Older evidence, links and internal names (the `agent-rush` binary and services, `AR_*` settings) still use that name._

**Give the app or agent you already have a contained, offsite subagent.** Plug Vultr-Placeholder-Test123 in over MCP or one REST call, hand it a task, and get back the result. The subagent writes and runs code, drives a real browser and converts files. It does all of that in its own sealed box on a Vultr worker, never on your servers.

Each session gets its own workspace box: a gVisor container on a private worker VM with a shell, a headless browser, language runtimes and file converters. Everything the model runs stays inside the box. The box holds no credentials, and an egress filter keeps it away from cloud metadata, private networks and mail ports.

Status: the public instance is live on Vultr. See [Live demo](#live-demo).

## Plug it into the app you already have

```
 your app, your agent             Vultr-Placeholder-Test123 (on Vultr)
 (Claude Code, Cursor,   MCP /    +--------------------------------------+
  a web app, a script)  -- REST ->| subagent session -> its own gVisor   |
        ^                         | box: shell, browser, files, runtimes |
        +--- reply, files, -------+--------------------------------------+
             events, webhooks       blast radius: that one box
```

- **From an AI agent, over MCP.** Add one remote server to Claude Code, Claude Desktop, Cursor or any MCP client. Your agent gets 15 tools to start a subagent, hand it a task, wait for the reply, steer it mid-task, and run commands or move files in its box. See [Use it from an MCP client](#use-it-from-an-mcp-client).

  ```sh
  claude mcp add --transport http agent https://<host>/mcp --header "Authorization: Bearer ar_..."
  ```

- **From any app, over REST.** One `POST /v1/sessions` starts a subagent with its task. Watch it work over a live event stream, get a signed webhook when it finishes, and download its files or the whole workspace as a `.tar.gz`. See [Use it from your app](#use-it-from-your-app).

  ```sh
  curl -H "authorization: Bearer $AR_KEY" -H 'content-type: application/json' \
    -d '{"message":"Clone the repo, run its tests and fix the failing one"}' $HOST/v1/sessions
  ```

- **What your app gains.** A worker that can run untrusted code, test a web app in a real browser, research the web with sources, or turn a CSV into a chart and a PDF report. Your app needs no sandbox, Docker host or GPU of its own.
- **What it risks.** One box. A subagent that goes wrong, or is turned by a prompt injection, holds no keys and cannot reach your network, the cloud metadata service, the host or other boxes. Memory, disk, time and spend are capped. Its work and its mistakes stay offsite.
- **What it does not do.** Each deployment has one trusted operator who issues the API keys; it is not a shared service for strangers. The subagent thinks with Vultr's GLM models (Flash by default), not with your app's model. Files move in and out up to 25 MiB over REST and 8 MiB over MCP.

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

- **One box per session.** Each session has its own gVisor container. It runs as uid 1000 with a read-only root filesystem, no Linux capabilities and no-new-privileges, with limits on processes, memory, CPU and workspace size (4 GiB by default). An idle box is parked: its container is removed and its workspace kept, and it starts again on the next call. The workspace is deleted when the session closes or after seven days without a call.
- **No credentials in the box.** API keys, the worker token, the inference key and remote MCP credentials stay on the control or worker side. The box qualification checks the box's environment and mounts for them.
- **Egress filter.** Boxes can reach the public internet. They cannot reach cloud metadata (169.254.169.254), private and CGNAT ranges, loopback, SMTP ports 25, 465 and 587, or any service on the host itself, SSH included. A host input guard enforces the last part at every boot. Boxes cannot reach each other. Details are in [SECURITY.md](docs/SECURITY.md#egress-policy).
- **Fail-closed qualification.** A worker runs one-shot jobs only with a passing report of its 10 containment checks. It starts boxes only with a passing report of 9 box checks made on this host, this boot and this box network. Reports expire after two hours, and the box report is rerun every 45 minutes.
- **Owner isolation.** Keys, sessions, files and skills belong to one owner. Another owner's session returns 404, the same as a session that does not exist.
- **Spend caps.** Each model call reserves its worst-case cost before it is sent. The operator budget defaults to $5 (maximum $25). A turn stops before its next model call at $0.50, and a session at $5. A turn also ends after 30 minutes; there is no cap on the number of steps.
- **The agent cannot widen its own limits.** It can change its prompt addendum, tool choices, custom tools and skills. Trusted code still enforces the limits, the egress rules and the operator's list of MCP servers. See [what the agent can edit](docs/SECURITY.md#what-the-agent-can-and-cannot-edit).

## Quick start

On Ubuntu 24.04 (x86-64), from a checkout of this repository:

```sh
sudo scripts/install.sh --role all --yes
```

- `--role all` runs control and worker on one host. That is a weaker separation than two VMs, because the control plane and the boxes share a kernel and a disk. For public use, install `--role control` on one VM and `--role worker` on a private one.
- Preview first, which changes nothing: `scripts/install.sh --dry-run --role all --with browser,voice-stt`.
- Guided setup asks for the role, features, inference provider, key and admin password: `sudo scripts/setup.sh`.
- Session boxes need a workspace image. The worker role builds it from `deploy/workspace.Dockerfile`, writes its digest to `AR_BOX_IMAGE` in `worker.env`, and then runs box qualification. `scripts/build-images.sh --workspace` builds the same image and prints its reference. See [INSTALL.md](docs/INSTALL.md).

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

1. **Create an API key.** Sign in once with the operator password, then create a key with that browser login (the cookie and its CSRF token). The key is shown once. An API key cannot create or revoke keys or register webhooks, which need the operator login.

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

   Files live in the session's workspace. An idle box is parked after 30 minutes, and its files stay put. They are deleted when the session closes, or after seven days without a call. Download results before you close the session.

6. **Get notified.** Each turn end is POSTed to your webhooks, signed with HMAC-SHA256 in `X-Agent-Signature`. Register one with `POST /v1/webhooks` using the operator login, as in step 1. The URL must be a public `https` address. See [API.md](docs/API.md#webhooks).

7. **Compare models.** When the operator lists more than one model, pick one per session (`"model":"glm-5.3"` on create), fork a finished session onto another model with `POST /v1/sessions/$SID/fork`, download a whole workspace with `GET /v1/sessions/$SID/archive`, and compare spend and tokens per model with `GET /v1/usage`. See [API.md](docs/API.md#models-and-usage).

Each owner has caps on open sessions (20), live event streams (8) and long requests such as exec and file calls (8). Over a cap the API returns 429 and says which one was reached. The limits are listed in [API.md](docs/API.md#limits).

## Use it from an MCP client

Vultr-Placeholder-Test123 is also an MCP server at `/mcp` (streamable HTTP, same API keys):

```sh
claude mcp add --transport http agent https://<host>/mcp --header "Authorization: Bearer ar_..."
```

The tools are `start_session`, `send_message`, `wait_for_reply`, `get_events`, `run_command`, `read_file`, `write_file`, `list_files`, `cancel`, `close_session`, `list_sessions`, `list_models`, `fork_session`, `get_usage` and `manage_skills`. Cursor and other clients that take a remote HTTP server work the same way. See [docs/MCP.md](docs/MCP.md).

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
| Workspace image | Worker role (built by default) | Builds the box image: shell, Chromium, converters. Boxes need it. |
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
| `AR_INFERENCE_MODELS` | none | Optional models a session may choose, with prices: `glm-5.3-flash:0.10:0.35,glm-5.3:0.75:3.00`. |
| `AR_INFERENCE_BUDGET_USD` | `5` | Total model spend the ledger allows. Range 0 to 25. |
| `AR_WORKER_URL`, `AR_WORKER_TOKEN` | none | Private worker link. The token is at least 32 characters. |
| `AR_BOX_IMAGE` | none | Workspace image reference the worker runs boxes from. |
| `AR_BOX_IDLE_SECONDS` | `1800` | Idle seconds before a box is parked: its container goes, its `/workspace` stays. |
| `AR_BOX_CEILING_MB` | `4096` | Largest memory a box may grow to. |
| `AR_BOX_DISK_MB` | `4096` | Hard size of each box's `/workspace`, in MiB. |
| `AR_BOX_RETENTION_SECONDS` | `604800` | Seconds a parked workspace is kept without a call, then deleted (7 days). |
| `AR_MCP_SERVERS_FILE` | none | JSON file of remote MCP servers the agent may use. |
| `AR_VOICE_URL` | none | Local voice service, for example `http://127.0.0.1:8790`. |

## Live demo

- **Public instance:** https://144-202-59-108.sslip.io (Let's Encrypt certificate through Caddy).
- **Where it runs:** two Vultr VMs in Chicago (`ord`): a control VM (2 vCPU, 4 GB) and a private worker VM (16 vCPU, 64 GB since 2026-11-06, for large demos), about $0.74 per hour in total. The worker has run 64 sandboxes and 72 agent sessions at once through the public URL. See [LIVE_DEPLOYMENT.md](docs/LIVE_DEPLOYMENT.md).
- **Inference:** the public instance sends every agent call to Vultr Serverless Inference, using GLM 5.3 Flash.
- **Evidence:** [VERIFICATION.md](docs/VERIFICATION.md) lists what was observed and when. Raw receipts are in [docs/evidence/live/](docs/evidence/live/), including the box qualification ([box-qualification-v2/](docs/evidence/live/box-qualification-v2/) and the earlier [box-qualification/](docs/evidence/live/box-qualification/)), the workspace image build ([workspace-image/](docs/evidence/live/workspace-image/)), a box round trip ([boxes/](docs/evidence/live/boxes/)), the session demo ([session-demo/](docs/evidence/live/session-demo/)), the MCP run ([mcp/](docs/evidence/live/mcp/)) and the voice service run ([voice/](docs/evidence/live/voice/)).

What is not yet done is listed in [LIVE_DEPLOYMENT.md](docs/LIVE_DEPLOYMENT.md) and [VERIFICATION.md](docs/VERIFICATION.md).

## Hackathon fit (Blast Radius Zero)

- **Business value: a contained subagent for apps that already exist.** Any MCP client or HTTP app can hand work to it, so teams get agents that run code and browse without giving them their own servers. See [Plug it into the app you already have](#plug-it-into-the-app-you-already-have) and the live MCP run in [docs/evidence/live/mcp/](docs/evidence/live/mcp/).
- **Every agent LLM call goes to Vultr.** Job `7d466fa9` on the public instance made 5 model calls, all to `glm-5.3-flash`. It finished in 58 s for $0.0058, and its reviewer approved the result. Receipt: [docs/evidence/live/vultr-public/](docs/evidence/live/vultr-public/).
- **Everything runs on Vultr.** The control and worker VMs are both Vultr instances in Chicago.
- **Containment evidence:**
  - 9 of 9 box checks pass on a scratch box network, which left the live `ar-box` network untouched: [box-qualification-v2/](docs/evidence/live/box-qualification-v2/). The earlier 8-check report, [box-qualification.json](docs/evidence/live/box-qualification/box-qualification.json), predates the host guard and the disk quota, and the worker no longer accepts it.
  - An earlier dump of the applied egress rules: [egress-policy.txt](docs/evidence/live/box-qualification/egress-policy.txt). The current rules are in [deploy/box-network.sh](deploy/box-network.sh).
  - The 10 job containment checks: [worker-qualification-2026-11-05.json](docs/evidence/live/worker-qualification-2026-11-05.json) (nine at first deploy) and [VERIFICATION.md](docs/VERIFICATION.md) (10 of 10 after the scaling check was added).
  - A containment probe receipt, runaway runs killed on timeout, and an overlapping run refused with HTTP 429: [containment/](docs/evidence/live/containment/) and [load/](docs/evidence/live/load/).
  - A 2026-11-06 security test: a source review, a clean `cargo audit`, and 21 live probes of the public instance (metadata, VPC, box-to-box, private DNS names, the browser, fork bombs, path traversal, auth, CSRF, TLS). Two low findings and three smaller issues were fixed: [security-audit-2026-11-06/](docs/evidence/live/security-audit-2026-11-06/).
- **Benchmarks.** Box throughput on 4 and 8 vCPU workers (16 sessions at once with no refusals on 8 vCPU / 32 GB), and GLM 5.3 Flash against full GLM 5.3 on six graded tasks (35/36 against 34/36; full GLM costs about 7× more and is slower): [BENCHMARKS.md](docs/BENCHMARKS.md).
- **Limits, stated plainly.** One operator per deployment. No independent third-party security audit. No multi-tenant claim. The box checks prove nine specific things, not escape resistance in general. See [SECURITY.md](docs/SECURITY.md).

## Documentation

- [API](docs/API.md), [MCP](docs/MCP.md), [install](docs/INSTALL.md), [voice](docs/VOICE.md)
- [Design](docs/DESIGN.md) and [security](docs/SECURITY.md)
- [Configuration](docs/CONFIGURATION.md) and [deployment](docs/DEPLOYMENT.md)
- [Live deployment](docs/LIVE_DEPLOYMENT.md), [verification](docs/VERIFICATION.md) and [benchmarks](docs/BENCHMARKS.md)
- [Hackathon entry draft](docs/SUBMISSION.md)

## License

MIT. See [LICENSE](LICENSE). Third-party components and their licenses are listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
