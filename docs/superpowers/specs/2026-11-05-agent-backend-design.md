# Agent backend: design (2026-11-05)

## Intent

Turn the sealed-sandbox runner into a general agent backend that other apps integrate with. Agents must be genuinely useful:
- research the web
- write, edit and test code in any language
- create and convert files
- run and click through apps
- extend themselves with skills, MCP servers and their own tools

The Blast Radius Zero claim must survive: everything the model drives runs inside a gVisor box on the worker VM that holds no credentials and cannot reach private networks.

The user asked for: a shell and browser in the agents; a backend with an API and an MCP server; useful tools plus the user's list (research, files, conversion, app testing, skills, MCP, remote control, interjection messages, a self-editable agent stack); optional Moonshine STT and LuxTTS voice; a quick installer and a feature-picking installer; GitHub docs in concise sections; and the Vultr hackathon goals met and exceeded.

## Goals and non-goals

**Goals**
- Multi-turn **sessions**, each with a persistent **workspace box** (gVisor) holding a shell, a browser, language runtimes and file converters.
- A REST API (`/v1`), an SSE event stream, an MCP server (`/mcp`), API keys and webhooks.
- The existing web chat becomes a client of sessions.
- Optional features install separately.

**Non-goals**
- Desktop control.
- Running MCP stdio servers on the control VM.
- Multi-tenant billing.
- A GPU deployment on Vultr (voice runs wherever the operator installs it).

**Anti-goals**
- Any model-driven action on the control VM.
- Credentials inside a box.
- A box that can reach the cloud metadata service, the private VPC, the worker's control listener, or SMTP.

## Approaches considered

1. **Full toolkit inside a per-session gVisor box with filtered internet. Chosen.** It keeps the containment story, and a shell and browser become safe to give.
2. Tools on the control VM, behind an allowlist. This is the conventional agent design: high blast radius, and it would contradict the track.
3. Keep one-shot jobs and add more job types. It can't support a browser, multi-turn work or app testing.

## Architecture

```
client app / web chat / MCP client
        │  REST /v1, SSE, /mcp  (API key or cookie)
  control VM: session engine ── inference gateway (Vultr GLM; Anthropic for dev)
        │                    ── remote MCP client (operator-configured, creds stay here)
        │                    ── skills, agent config, ledger (SQLite)
        │  mTLS + bearer (existing private link)
  worker VM: box manager ── docker + runsc, network "ar-box" (egress filter)
                          └ one container per session, volume /workspace
```

The existing one-shot job path (`/api/jobs`, 10 qualification checks) stays as is.

## Worker: box manager (`src/boxes.rs`, routes on the node router)

Contract. The worker exposes these over the existing authenticated private link, with JSON bodies:

| Method and path | Body | Returns |
| --- | --- | --- |
| `POST /v1/boxes` | `{session_id, owner, memory_mb, cpu_millis}` | `{box_id}` |
| `POST /v1/boxes/{id}/exec` | `{command, cwd?, timeout_seconds?, stdin?}` | `{exit_code, stdout, stderr, timed_out, truncated}` |
| `GET /v1/boxes/{id}/files?path=` | | file bytes (≤ 25 MiB) or a directory listing JSON |
| `PUT /v1/boxes/{id}/files?path=` | raw bytes | `{bytes, sha256}` |
| `DELETE /v1/boxes/{id}` | | `{removed: true, volume_removed: true}` |
| `GET /v1/boxes` | | the list, for reconciliation |

How the commands behave:
- **exec** runs `bash -lc <command>` as uid 1000 in `/workspace`, using `docker exec`. The default timeout is 120 s and the maximum is 600 s. Output is capped at 256 KiB per stream.
- **Paths** must resolve under `/workspace` (or `/tmp`). The worker refuses `..` escapes and symlinks that lead outside.

Container settings:
- `--runtime=runsc`, non-root, `--cap-drop=ALL`, `no-new-privileges`, `--pids-limit 512`, and the memory and CPU limits.
- The root filesystem is read-only, apart from tmpfs mounts for `/tmp` and `/home/agent`.
- The named volume `arbox-<id>` is mounted at `/workspace`.
- The container attaches to network `ar-box`, and the image is the pinned workspace image.

Limits and cleanup:
- The live scaler is reused: start at 1024 MiB and grow toward the operator box ceiling (default 4096 MiB, 2 vCPU).
- An idle reaper destroys a box after `AR_BOX_IDLE_SECONDS` (default 1800) with no exec.
- Worker capacity admission counts the boxes alongside jobs.

### Egress filter (`deploy/box-network.sh`, run by the worker install)

A Docker bridge network `ar-box` with inter-container traffic disabled. `DOCKER-USER` iptables rules apply to traffic from its subnet:
- **Drop** 10/8, 172.16/12, 192.168/16, 169.254/16, 100.64/10, 127/8, 224/4 and the host's own addresses.
- **Drop** TCP 25, 465 and 587.
- **Rate-limit** new connections with hashlimit.
- **Allow** everything else, plus established traffic.
- **IPv6:** disabled on the network.
- **DNS:** public resolvers (1.1.1.1, 8.8.8.8) are passed as `--dns`.

### Box qualification

`agent-rush qualify-box` starts a box and proves eight things:
- metadata (169.254.169.254) is unreachable
- the private VPC and the worker's own listener are unreachable
- SMTP is unreachable
- a public HTTPS site is reachable
- there are no credentials in the environment or files
- it runs as a non-root uid
- the root filesystem is read-only
- the volume is gone after destroy

The worker refuses `POST /v1/boxes` without a fresh passing box report. This mirrors the job qualification and fails closed.

### Workspace image (`deploy/workspace.Dockerfile`)

Ubuntu 24.04 with:
- bash, coreutils, git, curl, wget, jq, ripgrep, fd, tree, build-essential
- python3, pip and venv; Node 22 and npm; the Rust toolchain (minimal)
- pandoc, LibreOffice (headless writer, calc and impress), ImageMagick, ffmpeg, poppler-utils, qpdf
- Playwright with Chromium

Helpers in `/opt/box/bin`, on the PATH:
- **`browse`:** a JSON-in, JSON-out CLI. It talks to a persistent Chromium launched on demand through Playwright's CDP, keeping its state under `/tmp/browser`. Actions: `goto`, `snapshot` (an accessibility-tree text with refs), `click`, `type`, `press`, `select`, `scroll`, `screenshot` (a PNG saved to `/workspace/.screenshots`), `eval`, `back`, `tabs`, `console`, `close`.
- **`websearch`:** DuckDuckGo HTML results as JSON (title, url, snippet), with no key.
- **`fetch`:** a URL turned into readable text or markdown (readability-style, with a cap).
- **`convert`:** one front end over pandoc, LibreOffice, ImageMagick and ffmpeg, chosen by the extensions.

The image is built on the worker and its digest is recorded in `deploy/images.lock.json`.

## Control: session engine (`src/session.rs`, `src/tools.rs`)

**Data** (new ledger tables):
- `sessions`: id, owner, title, status idle/running/failed/closed, box_id, created and updated, spend
- `session_events`: a seq per session; kind user/assistant/tool_call/tool_result/interjection/status/error; JSON payload; time
- `pending_messages`
- `skills`: owner, name, description, body, updated
- `agent_configs`: owner, version, JSON, reason, author user|agent, created
- `api_keys`: owner, name, hash, prefix, created, last_used, revoked
- `webhooks`: owner, url, secret

**Loop.**
- `POST /v1/sessions/{id}/messages` appends a user event.
  - If the session is idle, it starts a turn task.
  - If a turn is running, the message is stored as an **interjection**. It is injected as a user message before the next model call and recorded as an `interjection` event.
- One turn is a model↔tool loop until the model calls `reply` (the final text for this turn) or a limit is hit: turns (default 60), wall time, or the spend cap.
- **Cancel** stops after the current tool call.
- **Context:** the existing byte and message budget and compaction are reused, and tool results are truncated to fit.
- **Inference:** the existing gateway and the existing JSON one-tool-per-turn protocol, which is proven on GLM. Role `Solo`, with its model and effort.

**Tools** (the model's allowlist; validated and typed in trusted code):

| Tool | Runs | Purpose |
| --- | --- | --- |
| `bash` | box | any shell command (build, test, install, run servers in background) |
| `read_file`, `write_file`, `edit_file` (exact replace, `replace_all`), `list_files`, `grep`, `glob` | box | file work |
| `browser` | box (`browse`) | drive Chromium: open and test apps at localhost inside the box, or the web |
| `web_search`, `web_fetch` | box (`websearch`/`fetch`) | research |
| `convert_file` | box (`convert`) | any-to-any conversion |
| `skill` (`list`/`load`/`save`/`delete`) | control | Anthropic `SKILL.md` format; loaded skills are also copied to `/workspace/.skills` |
| `agent_config` (`get`/`update`/`history`/`revert`) | control | self-editing: system-prompt addendum, enabled tools, custom tools, preferred model effort, MCP servers enabled |
| custom tools | box | defined by the agent in config: `{name, description, command_template}`, run through bash |
| `mcp` (`list_tools`, `call`) | control | operator-configured remote MCP servers (streamable HTTP); credentials never enter the prompt or the box |
| `reply` | control | end the turn with the answer and optional attachment paths |

The agent may edit its stack, but trusted code always enforces these:
- the turn, spend, memory and box ceilings
- the egress filter and credentials
- which MCP servers the operator allows
- the tool registry itself

The agent's config edits are versioned and revertible, and each one is shown as an event.

**Webhooks:** each turn end and error is sent to the owner's webhooks, signed with HMAC-SHA256 (`X-Agent-Signature`).

## Control: API (`src/api_v1.rs`) and MCP server (`src/mcp_server.rs`)

**Auth:** `Authorization: Bearer ar_<key>`, or the existing cookie together with CSRF. Keys are created through `POST /v1/keys`, which returns the key once (only its hash is stored), and revoked through `DELETE /v1/keys/{id}`.

**REST:**

| Area | Endpoints |
| --- | --- |
| Sessions | `POST/GET /v1/sessions`; `GET/DELETE /v1/sessions/{id}`; `POST /v1/sessions/{id}/messages`; `POST /v1/sessions/{id}/cancel` |
| Events | `GET /v1/sessions/{id}/events?after=` (JSON); `GET /v1/sessions/{id}/stream` (SSE) |
| Files | `GET/PUT /v1/sessions/{id}/files?path=` |
| Direct shell | `POST /v1/sessions/{id}/exec` |
| Skills | `/v1/skills` CRUD |
| Agent config | `/v1/agent-config` (GET/PUT, history, revert) |
| Webhooks | `/v1/webhooks` |
| Status | `GET /v1/status` |
| Voice | `POST /v1/voice/transcribe`; `POST /v1/voice/speak` |

The `/v1/voice/*` endpoints return 404 unless voice is configured. The whole surface is documented in `docs/API.md`, with an OpenAPI-ish table.

**MCP:** `POST /mcp` speaks streamable HTTP (JSON-RPC 2.0) with `initialize`, `tools/list` and `tools/call`, using the same bearer keys. It exposes these tools:
- `start_session`, `send_message`, `wait_for_reply` (long-polls until idle, up to 120 s)
- `get_events`, `run_command`, `read_file`, `write_file`, `list_files`
- `cancel`, `close_session`, `list_sessions`, `manage_skills`

It works with `claude mcp add --transport http`, Cursor, and others.

## Voice (optional, `voice/`)

A Python FastAPI service on localhost:
- **STT:** Moonshine through ONNX (`useful-moonshine-onnx`, base model, CPU).
- **TTS:** LuxTTS (Apache-2.0; a GPU is preferred, CPU works). It uses a default reference voice and accepts an operator voice clip.

The control VM proxies to it through `AR_VOICE_URL`. The flow:
1. The web chat records audio from the mic and sends it to transcribe.
2. The text goes in as a message.
3. The agent's `reply` text is sent to speak and played back.

The LLM writes what is spoken, and STT feeds the LLM.

## Installers

**`scripts/install.sh`** is the quick, non-interactive installer. Its flags are `--role all|control|worker` and `--with browser,voice,tls`. The default is an all-in-one single host, which keeps gVisor and the egress filter but is documented as a weaker separation than two VMs.

**`scripts/setup.sh`** is the interactive feature picker:
- Features: core, workspace browser image, voice STT, voice TTS, TLS (Caddy and sslip), a systemd unit, an MCP example config.
- Settings: inference provider and key (Vultr by default), and the admin password.
- Output: an env file with mode 0600.

Both are idempotent and print what they changed.

## Docs

- **README:** short sections (What it is, Quick start, Use it from your app, MCP, Tools, Security model, Optional features, Configuration, Hackathon fit).
- **New:** `docs/API.md`, `docs/MCP.md`, `docs/INSTALL.md`, `docs/VOICE.md`.
- **Updated:** `docs/DESIGN.md` and `docs/SECURITY.md`. The "deliberately not included" list changes: the shell and browser now exist, inside the box.

## Hackathon fit (Blast Radius Zero)

- Every agent LLM call goes to Vultr Serverless Inference.
- Everything runs on Vultr VMs.
- The containment proofs are live and public:
  - the 10 job checks
  - the 8 box checks
  - receipts
  - a published egress policy
  - an "agent tried to reach metadata, blocked" event visible in the stream

## Calls made by the builder (Claude), not the user

Each of these is a judgment call by the builder; the user can overrule any of them.
- The egress policy: public internet allowed; private, metadata and SMTP blocked.
- The JSON one-tool-per-turn protocol is kept instead of native function calling.
- MCP stdio servers are not run on control.
- Voice stays off the Vultr VMs by default.
- Default limits: 60 turns, a 30-minute idle reaper, and a 4 GiB box ceiling.
- The product name is unchanged until the user picks Cofferdam or Glovebox.
