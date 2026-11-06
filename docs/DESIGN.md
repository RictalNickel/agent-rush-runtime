# Design

Vultr-Placeholder-Test123 is an original Rust system. Its core rule: authority stays in trusted code. Models choose only a validated tool call from an allowlist. They never hold credentials, Docker access or the power to exceed a limit.

The system has two paths that share one control plane and one worker:

- **Sessions** are multi-turn conversations with an agent that works in its own workspace box, with a shell, a browser, files and converters. This is the agent backend that apps integrate with.
- **One-shot jobs** (`/api/jobs`) are a team of typed roles that writes and runs a single Rust program and returns a receipt. The job path is described in [How the one-shot job agent works](#how-the-one-shot-job-agent-works).

## Components

| Component | Runs on | Code | Role |
| --- | --- | --- | --- |
| Control API | control VM | `src/api.rs`, `src/api_v1.rs` | Operator login, `/v1` REST and SSE, API keys, webhooks |
| MCP server | control VM | `src/mcp_server.rs` | The same sessions for MCP clients, at `/mcp` |
| Session engine | control VM | `src/session.rs`, `src/tools.rs` | The model and tool loop for each turn, interjections, cancel, limits |
| Inference gateway | control VM | `src/inference.rs` | The only path to a model. Reserves worst-case cost before each call, settles to actual usage |
| Skills and agent config | control VM | `src/skills.rs`, `src/agent_config.rs` | Owner-scoped skills, and versioned, revertible agent configuration |
| Remote MCP client | control VM | `src/mcp_client.rs` | Calls operator-configured remote MCP servers. Credentials stay here |
| Ledger | control VM | `src/ledger.rs`, `src/session_store.rs` | SQLite: sessions, events, keys, skills, config versions, webhooks, spend |
| Box manager | worker VM | `src/boxes.rs`, `src/node.rs` | One gVisor container per session, the egress filter and host input guard, a per-box workspace quota, live memory growth, idle parking and workspace retention |
| Box tools | inside each box | `deploy/box-tools/` | `browse`, `websearch`, `fetch`, `convert` |
| One-shot runner | worker VM | `src/worker.rs`, `src/engine.rs`, `src/sandbox_api.rs` | Team roles and fixed gVisor runs for the job path |

## Sessions

A session is a conversation with one persistent box. The box is created on the first turn, and its `/workspace` is a per-box disk image with a hard size limit.

**Turn loop.** A message to an idle session starts a turn. A message to a running session is an **interjection**: it is recorded and shown to the model before its next call.

1. The model returns one JSON object that names one tool. Trusted code decodes it strictly. Unknown fields and unknown tools fail, and the error goes back to the model.
2. The call is checked against the agent's configuration (disabled tools, custom tools, MCP allowlist) and then run: in the box for shell, file, browser, search, fetch and conversion calls, and in the control plane for skill, config, MCP and `reply` calls.
3. The call and its result are recorded as events.
4. The turn ends when the model calls `reply`, when it hits a limit, or when it is cancelled. A cancel stops the turn after the current tool call.

**Limits** (`SessionLimits::default()` in `src/session.rs`, fixed in code and not set by environment variables):

| Limit | Value |
| --- | --- |
| Model calls per turn | 60 |
| Turn wall time | 30 minutes |
| Spend per turn | $0.50, checked before each model call |
| Spend per session | $5.00, checked before each model call |
| Box memory at creation | 1024 MiB, growing live to the box ceiling (`AR_BOX_CEILING_MB`, default 4096) |
| Box CPU | 1000 millicores at creation, up to 2000 |
| Open sessions per owner | 20 |
| Live event streams per owner | 8 |
| Long requests per owner (exec and file calls, MCP waits) | 8 |
| Box exec and file calls in flight | 16 across sessions, 4 per session |
| JSON-RPC messages per MCP batch | 16 |

**Context.** The model sees the turn's request and the newest steps that fit a 28 KB, 14-message window. Older steps of the turn are left out and a note says so. The event log keeps everything. Tool output is truncated before it goes back to the model.

**Recovery.** Parked boxes are still listed, and a call restarts one with its files. If the worker no longer has a session's box, because its workspace was deleted or lost, the engine creates a new box and continues. Files from the old box are gone, and the session records an error event that says so.

## Boxes

A box is one gVisor container (`--runtime=runsc`) per session, created by the worker's box manager.

- Non-root uid 1000, read-only root filesystem, `--cap-drop=ALL`, `no-new-privileges`, `--pids-limit=512`, `--user=1000`, and memory and CPU limits.
- tmpfs mounts for `/tmp` and `/home/agent`. `/workspace` is the box's own disk image (the Workspace bullet below).
- **Workspace.** A sparse ext4 image of `AR_BOX_DISK_MB` (default 4096 MiB), attached to a loop device and mounted by Docker. Writes past its size fail with ENOSPC. Creating a box needs twice the quota plus 2 GiB free, after the headroom that existing workspaces may still grow into.
- No published ports. `--log-driver=none`. `/etc/resolv.conf` points at public resolvers.
- Attached to the `ar-box` network, which has inter-container traffic disabled, an egress filter in `DOCKER-USER`, and a host input guard in `INPUT`. See [SECURITY.md](SECURITY.md#session-boxes).
- Exec runs `bash -lc` as uid 1000 in `/workspace`, with a 120 s default timeout (600 s maximum) and 256 KiB of output per stream.
- Paths are checked lexically and again after `realpath`. They must stay under `/workspace` or `/tmp`.
- **Parked, not destroyed.** After `AR_BOX_IDLE_SECONDS` (default 1800) with no call, a box's container is removed and its workspace kept. The next call restarts it. A parked workspace is deleted when its session is closed (`DELETE /v1/sessions/{id}`) or after `AR_BOX_RETENTION_SECONDS` (default 604800, seven days) with no call.

Boxes start only with a fresh passing box qualification report (nine checks) for this host's machine and boot IDs, the image and the box network, and only while the guard marker for this boot is present. Creating a box and restarting a parked one both check this.

## Tools

The built-in tool registry (`BUILTIN_TOOLS` in `src/agent_config.rs`) is:

`bash`, `read_file`, `write_file`, `edit_file`, `list_files`, `grep`, `glob`, `browser`, `web_search`, `web_fetch`, `convert_file`, `skill`, `agent_config`, `mcp`, `reply`.

Custom tools (from the agent config) are bash command templates that run in the box. They are added to the built-in set, and they cannot reuse a built-in name.

## The self-editable stack

The agent may change its own stack through `skill` and `agent_config`:

- Its system prompt addendum (up to 8 KiB).
- Which built-in tools are disabled. `reply` cannot be disabled.
- Its custom tools (up to 20, each a command template of up to 4 KiB).
- Which operator-approved MCP servers it uses.
- Its reasoning effort (`low`, `medium` or `high`).
- Its skills.

Every change is a new version that is recorded as a `config_change` event and can be reverted. Trusted code enforces what the agent cannot change: the turn, spend, memory and box limits, the egress filter, credentials, the operator's MCP list, and the tool registry. See [SECURITY.md](SECURITY.md#what-the-agent-can-and-cannot-edit).

## MCP in and out

- **MCP in.** The operator names remote MCP servers in a JSON file (`AR_MCP_SERVERS_FILE`). Each is reached over streamable HTTP through the `mcp` tool. The server list, its headers and any `allowed_tools` filter are operator-only. Tool calls are owner-scoped MCP sessions, and error messages never include header values.
- **MCP out.** `POST /mcp` exposes sessions as tools to MCP clients (`start_session`, `send_message`, `wait_for_reply`, and others), with the same API keys as `/v1`.

MCP stdio servers are not run on the control VM. The MCP client speaks only streamable HTTP.

## Design choices

- **Shell and browser in a box, not on the control VM.** The control VM runs no command the model chose. The box is the place where shell, browser and file work happen, and it is the blast radius.
- **Public internet yes, private networks no.** Research needs the public web. Private ranges, cloud metadata, the host's own addresses and SMTP are blocked by the egress filter, so the protection does not depend on the model obeying a prompt.
- **One tool call per model turn.** Each model answer is one JSON object that calls one tool. This keeps the protocol simple to validate, and it is the protocol that was measured on GLM 5.3 Flash.
- **Owner scope everywhere.** Keys, sessions, files, skills, webhooks and config versions belong to one owner. A session of another owner returns 404, the same as one that does not exist.
- **Spend is reserved before it is spent.** Every model call reserves its worst case in the ledger. A failed call still settles at its actual cost.
- **Separate machines.** A control VM never runs generated code. A worker VM is reachable only over a private network with mutual TLS and a bearer token.
- **Receipts and qualification.** The one-shot path returns receipts for each run. Both paths refuse to run work without a fresh passing qualification report for their host and image.
- **Webhooks are signed.** Each turn end is sent with an `X-Agent-Signature` HMAC-SHA256 header, and webhook URLs must resolve to public addresses.

## Deliberately not included

These are still excluded:

- **Desktop control.** No screen capture or input outside a box.
- **MCP stdio servers on the control plane.** Servers run only on the remote side of an HTTP connection.
- **Credentials inside a box.** No keys, tokens, host mounts or Docker socket in any box.
- **Model-driven actions on the control host.** Every tool a model drives either runs in a box or changes only the agent's own state in the ledger (skills, config, and MCP calls on the operator's list).
- **A general plugin system.**
- **Embedding memory.** One-shot owner notes use literal search only.
- **Unlimited subordinate depth.** The one-shot team has depth one.
- **Parallel tool calls in one turn.**

Shell and browser are no longer excluded. They exist inside the box.

## How the one-shot job agent works

Jobs submitted to `/api/jobs` use typed roles and a single sandbox runner.

| Concern | Design |
| --- | --- |
| Team | Trusted code assigns typed roles: orchestrator, coder, worker and reviewer. Depth is limited to one, with at most two subordinates. `delegate` creates a subordinate and `follow_up` continues it. Subordinates share the job's lease and call budget. |
| Loop | An explicit stack of frames (`src/engine.rs`) with turn, call and lifetime budgets. Each tool is checked against the role's allowlist (`Role::allows`) before it runs, and every start and finish is a durable ledger event. |
| Finishing | `response` ends the job only when the independent worker receipt shows exit 0, passing checks and verified cleanup. In team mode a reviewer must also approve; a final rejection fails the job. |
| Code execution | No shell. The coder submits one standard-library Rust program, which runs in a fresh gVisor container on a separate worker VM: no network or credentials, non-root, read-only root, capped PIDs, time, output and scratch. The container is destroyed and its removal confirmed. |
| Resources | The orchestrator sets a ceiling with `set_limits`; trusted code clamps it. Each run starts at 256 MiB and 0.5 vCPU and the worker grows it live, up to the ceiling. |
| Memory | Owner-scoped SQLite notes with literal search and quotas, treated as untrusted data and never as policy. |
| Context | A per-context byte and message budget. Haiku roles (Anthropic provider) use server-side threshold compaction; the context then resets to its policy, task and summary turn. |
| Models | One trusted inference gateway with a per-role model, effort and cache table, and a durable worst-case cost reservation settled to provider usage. |

The one-shot path also has cloud lifecycle tooling: exact-resource tracking, teardown and a watchdog. These are trusted-runner commands and are not part of the web or agent runtime.

## Tests that pin the design

Sessions and boxes:

- `tests/sessions.rs`: one turn's events; interjections before the next model call; cancel; the step cap; spend caps before the next model call; recovery after a lost box; exact-match `edit_file`; versioned config changes; custom-tool quoting; owner isolation; output truncation.
- `tests/boxes.rs`: paths stay under `/workspace` or `/tmp`; the container arguments (`runsc`, read-only root, dropped capabilities); refusal without a passing box report; `live_box_round_trip`, which is ignored by default and needs a real worker.
- `tests/api_v1.rs`: every key-scoped route rejects a missing key; keys are shown once; another owner gets 404; SSE replays after `Last-Event-ID`; the webhook signature matches RFC 4231 and the webhook target guard refuses private addresses.
- `tests/mcp_server.rs`, `tests/mcp_client.rs`: JSON-RPC protocol errors; owner-scoped MCP sessions; the operator's `allowed_tools` filter; error messages that never include header values.

One-shot jobs:

- `tests/agent_runtime.rs`: only the native tool schema is accepted; only the orchestrator can set resource ceilings, and only within policy.
- `tests/team.rs`: the orchestrator and worker roles cannot execute code; review approval, rejection and outage; `follow_up`; compaction.
- `tests/orchestration.rs`: ceiling clamping, defaults, and restarting at the ceiling after a memory kill.
- `tests/qualification.rs`: the `scaling` check accepts only live growth that stops at the ceiling.
