# Agent Backend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Multi-turn agent sessions, each with a persistent gVisor workspace box (shell, browser, runtimes, converters). They are driven through a REST API, SSE and an MCP server, with skills, a self-editable config, interjections, an MCP client, optional voice, and installers.

**Architecture:**
- The worker VM gains a box manager behind the existing mTLS link. The contract is `src/sandbox_api.rs`, which is already committed.
- The control VM gains a session engine that drives the existing inference gateway and calls box tools through `Arc<dyn BoxClient>`.
- The API, MCP server and web chat are thin layers over `Sessions`.

**Tech Stack:**
- Rust 1.90: Axum 0.8, Tokio, reqwest, rusqlite, futures-util `BoxFuture`
- Docker with gVisor `runsc`
- Workspace image based on `mcr.microsoft.com/playwright:v1.64.1-noble`
- Python 3 for the in-box helpers and the optional voice service

**Spec:** `docs/superpowers/specs/2026-11-05-agent-backend-design.md`

## Global Constraints

- **Building and testing:** there is no local Rust or Docker. Build and test on the worker VM (`ssh -F ~/.config/agent-rush/ssh_config ar-worker`):
  - Each track uses its own directory `/root/build-<track>` and its own `CARGO_TARGET_DIR=/root/target-<track>`, seeded by copying `/root/agent-rush/target`.
  - Run `source /root/.cargo/env`. Gate on `cargo fmt --all --check && cargo clippy --locked -j2 --all-targets -- -D warnings && cargo test --locked -j2`.
  - Copy formatted files back to the local worktree.
- **Live services:** never touch the live services or paths: `agent-rush-worker`, `agent-rush-control`, `/opt/agent-rush`, `/etc/agent-rush` and the `ar-control` host. Never restart Docker, never print or commit secrets, and never push.
- **Existing behaviour stays green:** the one-shot jobs path and its 10 qualification checks keep working, and all 111 existing tests still pass.
- **Table names:** the existing `sessions` table holds login sessions. Agent sessions use the table `agent_sessions`.
- **Each track keeps its migrations in its own module**, as `pub(crate) fn migrate(db: &rusqlite::Connection) -> anyhow::Result<()>`. `Ledger::init` calls it as a one-line addition.
- **Copy style:** plain words and sentence case. No all-caps labels and no middle-dot metadata strings.
- **New dependencies** only where named in a task.

## Review Focus

1. **Path escape.** `read_file("../../etc/passwd")` or a symlink inside `/workspace` that points to `/` must be refused with a clear error, never served. Owned by Task A1.
2. **A box disappears** (reaped, OOM or worker restart) while a session still references it. The next tool call should recreate a box, keep the session and record an `error` event. It must never hang. Owned by Task B2.
3. **An interjection arrives just as a turn ends.** It must start a new turn, not be lost. Owned by Task B2.
4. **Large or binary tool output** (`cat` of a PNG, 10 MB of logs) must be truncated to the context budget as UTF-8-lossy text. It must never panic or blow the context. Owned by Tasks A1 and B2.
5. **A bearer key from another owner** must get 404 on every `/v1/sessions/{id}` route and every MCP tool, never 403 with existence leaked. Owned by Task C1.

---

### Track A (worker): boxes, image, egress, qualification

#### Task A1: Box manager and node routes

**Files:**
- Create: `src/boxes.rs`, `src/box_client.rs`, `tests/boxes.rs`
- Modify: `src/node.rs` (routes and capacity), `src/lib.rs`, `src/main.rs` (worker args `--box-image`, `--box-network`, env `AR_BOX_IDLE_SECONDS`, `AR_BOX_CEILING_MB`)

**Interfaces:**
- Consumes: `crate::sandbox_api::*`, the existing `worker` scaler and docker runner helpers.
- Produces:
  - `boxes::BoxManager` (worker side), mounted by `node_router` at `/v1/boxes...` behind the existing bearer and mTLS.
  - `box_client::HttpBoxClient::new(http: reqwest::Client, base_url: String, token: String) -> Self`, which implements `BoxClient`.
  - Pure helpers, tested in `tests/boxes.rs`:
    - `boxes::resolve_path(p: &str) -> Result<String>`
    - `boxes::truncate_utf8(bytes: &[u8], max: usize) -> (String, bool)`
    - `boxes::docker_create_args(spec: &BoxSpec, image: &str, network: &str, box_id: &str) -> Vec<String>`

- [ ] Write tests:
  - `resolve_path`: accepts `src/a.rs` → `/workspace/src/a.rs` and `/tmp/x`; rejects `../etc`, `/etc/passwd`, `/workspace/../root` and NUL.
  - `truncate_utf8`: lossy, and the cap is respected.
  - `docker_create_args` contains `--runtime=runsc`, `--read-only`, `--cap-drop=ALL`, `--security-opt=no-new-privileges`, `--pids-limit=512`, `--user=1000:1000`, `--network=<network>`, `-v arbox-<id>:/workspace`, and `--tmpfs` for `/tmp` and `/home/agent`.
- [ ] Implement the manager:
  - **Create:** `docker volume create`, then `docker run -d … sleep infinity`.
  - **Exec:** `docker exec -i -u 1000 -w <cwd> <c> bash -lc <cmd>` with a timeout and kill.
  - **Symlink check:** resolve the path inside the box with `realpath -m`, then re-check the prefix.
  - **Files:** read with `docker exec cat` (size checked first with `stat`). List directories with `find -maxdepth 1 -printf`. Write by piping into `docker exec -i … sh -c 'mkdir -p "$(dirname "$1")" && cat > "$1"' _ <path>`.
  - **Destroy:** `docker rm -f` plus `docker volume rm`.
  - **Bookkeeping:** keep an in-memory map, the idle reaper and live scaling, starting at 1024 MiB with a ceiling of `AR_BOX_CEILING_MB` (default 4096).
- [ ] Gate on the worker VM (fmt, clippy, test). Then smoke-test it live with a scratch binary run on a spare port, against the image from A2, and record the transcript in `docs/evidence/live/boxes/`.
- [ ] Commit.

#### Task A2: Workspace image and in-box helpers

**Files:**
- Create: `deploy/workspace.Dockerfile`, `deploy/box-tools/browse` (Python, Playwright CDP), `deploy/box-tools/websearch`, `deploy/box-tools/fetch`, `deploy/box-tools/convert`, `deploy/box-tools/README.md`
- Modify: `deploy/images.lock.json` (add the `workspace` digest), `scripts/build-images.sh`

**Interfaces:**
- **`browse '<json>'`:** prints one JSON object to stdout.
  - Actions: `goto{url}`, `snapshot{}` → `{url, title, text}` (an accessibility tree with `[ref=N]`, ≤ 40 KB), `click{ref|selector}`, `type{ref|selector,text,submit?}`, `press{key}`, `select{ref,value}`, `scroll{dy}`, `screenshot{path?}` → `{path}`, `eval{js}`, `back`, `tabs`, `console`, `close`.
  - The browser persists between calls through a CDP endpoint at `127.0.0.1:9222`, started on first use with `--no-sandbox`. Under gVisor this is acceptable: the box is the sandbox.
- **`websearch '<query>' [n]`:** prints `[{title,url,snippet}]` from DuckDuckGo HTML.
- **`fetch <url> [--max N]`:** prints readable text.
- **`convert <in> <out>`:** chooses pandoc, LibreOffice headless, ImageMagick or ffmpeg by the extensions, and exits non-zero with a reason when unsupported.

- [ ] Build the image on the worker. Verify in a `runsc` container on network `ar-box`:
  - `browse goto https://example.com` then `snapshot` contains "Example Domain"
  - `websearch rust` returns at least 3 results
  - `convert a.md a.docx`, `convert a.docx a.pdf`, `convert a.png a.jpg` and `convert a.wav a.mp3` each work
  - python3, node, npm, git, rg and cargo are present
- [ ] Commit, with the evidence in `docs/evidence/live/workspace-image/`.

#### Task A3: Egress filter and box qualification

**Files:**
- Create: `deploy/box-network.sh`, `qualification/box_checks.sh`
- Modify: `src/qualification.rs` (a `BoxReport`, with fail-closed gating in `BoxManager::create`), `src/main.rs` (subcommand `qualify-box`), `tests/qualification.rs`
- Modify: `deploy/install-worker.sh` (run `box-network.sh` and `qualify-box`), and a systemd timer next to the existing requalify unit (`deploy/`)

**Interfaces:**
- **`box-network.sh`** is idempotent. It creates network `ar-box` (subnet 172.30.0.0/24, `com.docker.network.bridge.enable_icc=false`, IPv6 off). It inserts `DOCKER-USER` rules matched on `-s 172.30.0.0/24` only, per the spec:
  - drop the private, metadata, CGNAT, loopback and multicast ranges and the host's IPs
  - drop TCP 25, 465 and 587
  - hashlimit new connections at 50/s
  - RETURN for everything else

  It must not touch other rules.
- **The box report** has 8 named checks: `metadata_blocked`, `private_blocked`, `worker_listener_blocked`, `smtp_blocked`, `public_https_allowed`, `no_credentials`, `non_root_readonly`, `volume_destroyed`. It also carries `host`, `image`, `created_at` and `expires_at` (+2 h). `BoxManager::create` returns an error when the report is missing, stale or failing.
  - Dated note (2026-11-05): this step describes the first build, and the 8 in this item and the next checkbox refer to it. The report now has nine checks: `host_services_blocked` replaces `worker_listener_blocked`, and `disk_quota_enforced` is new. It is also bound to the host's machine and boot IDs and to the box network, and every box start needs the guard marker. Workspaces are now per-box disk images, and idle boxes are parked instead of destroyed. The current behavior is in [SECURITY.md](../../SECURITY.md#session-boxes).
- [ ] Write the tests: report assessment (all pass, one failing, stale, wrong image), each rejected.
- [ ] Run it live on the worker; all 8 must pass. Save the report to `docs/evidence/live/box-qualification/`.
- [ ] Commit.

### Track B (control): session engine

#### Task B1: Stores (sessions, events, skills, agent config)

**Files:**
- Create: `src/session_store.rs`, `src/skills.rs`, `src/agent_config.rs`, `tests/session_store.rs`
- Modify: `src/ledger.rs` (one migrate call per module), `src/lib.rs`

**Interfaces (produced):**

```rust
pub enum SessionStatus { Idle, Running, Failed, Closed }   // serde lowercase
pub struct SessionView { pub id: String, pub owner: String, pub title: String, pub status: SessionStatus,
    pub box_id: Option<String>, pub created_at: i64, pub updated_at: i64, pub spend_usd: f64, pub last_seq: i64 }
pub struct SessionEvent { pub session_id: String, pub seq: i64, pub kind: String, pub payload: serde_json::Value, pub at: i64 }
// kinds: user, assistant, tool_call, tool_result, interjection, status, config_change, error
impl Ledger {
  fn create_agent_session(&self, owner:&str, title:&str) -> Result<SessionView>;
  fn agent_session(&self, owner:&str, id:&str) -> Result<Option<SessionView>>;
  fn agent_sessions(&self, owner:&str) -> Result<Vec<SessionView>>;
  fn set_session_status(&self, id:&str, s: SessionStatus) -> Result<()>;
  fn set_session_box(&self, id:&str, box_id: Option<&str>) -> Result<()>;
  fn add_session_spend(&self, id:&str, usd:f64) -> Result<()>;
  fn append_event(&self, id:&str, kind:&str, payload: Value) -> Result<SessionEvent>;
  fn session_events(&self, owner:&str, id:&str, after:i64, limit:usize) -> Result<Vec<SessionEvent>>;
  fn push_pending(&self, id:&str, text:&str) -> Result<()>; fn take_pending(&self, id:&str) -> Result<Vec<String>>;
}
pub struct Skill { pub name: String, pub description: String, pub body: String, pub updated_at: i64 }
impl Ledger { fn skills(&self, owner)->Result<Vec<Skill>>; fn skill(&self, owner, name)->Result<Option<Skill>>;
  fn save_skill(&self, owner, name, description, body)->Result<Skill>; fn delete_skill(&self, owner, name)->Result<bool>; }
pub struct CustomTool { pub name: String, pub description: String, pub command_template: String } // {{arg}} placeholders, shell-quoted on expansion
pub struct AgentConfig { pub system_addendum: String, pub disabled_tools: Vec<String>, pub custom_tools: Vec<CustomTool>,
    pub mcp_servers: Vec<String>, pub reasoning_effort: Option<String> }
impl AgentConfig { pub fn validate(&self, builtin_tools:&[&str], allowed_mcp:&[String]) -> Result<()> }
impl Ledger { fn agent_config(&self, owner)->Result<(i64, AgentConfig)>; fn update_agent_config(&self, owner, cfg:&AgentConfig, reason:&str, author:&str)->Result<i64>;
  fn agent_config_history(&self, owner)->Result<Vec<(i64, String, String, i64)>>; fn revert_agent_config(&self, owner, version:i64)->Result<i64>; }
```

Limits that `validate` enforces:
- Skill names match `^[a-z0-9][a-z0-9-]{0,63}$`, and the body is ≤ 64 KiB. Each owner may have at most 200 skills.
- `system_addendum` is ≤ 8 KiB.
- An owner may have at most 20 custom tools. Their names match `^[a-z][a-z0-9_]{0,39}$` and must not clash with built-in names.
- `mcp_servers` must be a subset of the operator's allowed list.

- [ ] Test each of these:
  - event seq is monotonic per session
  - owner isolation (another owner gets `None` or empty)
  - pending messages are taken exactly once
  - skill validation
  - config versioning, history and revert
  - `validate` rejects a custom tool named `bash`, an unknown MCP server and an oversized addendum
- [ ] Implement, gate, commit.

#### Task B2: Session engine, tools, interjections

**Files:**
- Create: `src/session.rs`, `src/tools.rs`, `tests/sessions.rs`
- Modify: `src/inference.rs` (add `GatewayModel`), `src/lib.rs`

**Interfaces (produced):**

```rust
pub struct ModelReply { pub text: String, pub cost_usd: f64 }
pub trait SessionModel: Send + Sync { fn complete<'a>(&'a self, system:&'a str, messages:&'a [Value]) -> BoxFuture<'a, Result<ModelReply>>; }
pub struct GatewayModel { pub gateway: InferenceGateway }   // impl SessionModel; role Solo; existing spend reservation
pub struct SessionLimits { pub max_turn_steps: u32 /*60*/, pub max_turn_seconds: u64 /*1800*/, pub box_memory_mb: u32 /*1024*/, pub box_cpu_millis: u32 /*1000*/ }
#[derive(Clone)] pub struct Sessions { /* Arc inner */ }
impl Sessions {
  pub fn new(ledger: Arc<Ledger>, model: Arc<dyn SessionModel>, boxes: Arc<dyn BoxClient>, mcp: Arc<McpRegistry>, limits: SessionLimits) -> Self;
  pub fn create(&self, owner:&str, title: Option<&str>) -> Result<SessionView>;
  pub fn list(&self, owner:&str) -> Result<Vec<SessionView>>;
  pub fn get(&self, owner:&str, id:&str) -> Result<Option<SessionView>>;
  pub fn send(&self, owner:&str, id:&str, text:&str) -> Result<SendOutcome>;  // SendOutcome { interjection: bool, seq: i64 }
  pub fn events(&self, owner:&str, id:&str, after:i64, limit:usize) -> Result<Vec<SessionEvent>>;
  pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<SessionEvent>;
  pub fn cancel(&self, owner:&str, id:&str) -> Result<()>;
  pub async fn close(&self, owner:&str, id:&str) -> Result<()>;           // destroys box, status closed
  pub async fn wait_idle(&self, owner:&str, id:&str, timeout: Duration) -> Result<SessionView>;
  pub async fn exec(&self, owner:&str, id:&str, req: ExecRequest) -> Result<ExecResult>;     // ensures box
  pub async fn read_file(&self, owner:&str, id:&str, path:&str) -> Result<FileRead>;
  pub async fn write_file(&self, owner:&str, id:&str, path:&str, bytes:&[u8]) -> Result<WriteResult>;
}
```

**Tools** (`src/tools.rs`): a `ToolCall` enum decoded from the model's single JSON object `{"tool":"…", …}`, with unknown fields rejected. The variants are those in the spec: `bash`, `read_file`, `write_file`, `edit_file` (`old`, `new`, `replace_all`), `list_files`, `grep`, `glob`, `browser{action,…}`, `web_search`, `web_fetch`, `convert_file`, `skill`, `agent_config`, `mcp`, custom tools, and `reply{text, attachments?}`. Each one maps to a box exec, a file call or a control-side store call. A malformed call produces an `error` tool result back to the model, not a failed turn.

**System prompt:** the base prompt, the tool list with one example each (minus `disabled_tools`, plus custom tools), the skills index (name and description), `system_addendum`, and the rule: "Work in /workspace. Test what you build. Use reply once to finish."

**Loop:**
1. `take_pending` happens before every model call, and each pending message is appended as a user message plus an `interjection` event.
2. Box loss: on an exec error matching not-found, recreate the box, record an `error` event and continue.
3. When the turn ends, `take_pending` runs again; if it is non-empty, start the next turn immediately.

- [ ] Write the tests, using a scripted `FakeModel` and `FakeBoxes`:
  - one turn of bash then reply produces the event order user, tool_call, tool_result, assistant, status idle
  - an interjection sent mid-turn is seen by the next model call
  - an interjection sent after the final model call starts a new turn
  - cancel stops the turn
  - the step cap ends with a `failed` status
  - a box lost mid-turn is recreated
  - `edit_file` replaces an exact unique match, refuses an ambiguous match unless `replace_all`, and reports when there is no match
  - an `agent_config` update by the model is versioned with author `agent` and a disabled tool is then refused
  - a custom tool expands with shell-quoted args
  - a 10 MB tool output is truncated
  - another owner can't `get`, `send` or `exec`
- [ ] Implement, gate, commit.

#### Task B3: MCP client (operator-configured remote servers)

**Files:**
- Create: `src/mcp_client.rs`, `tests/mcp_client.rs`
- Modify: `src/config.rs` (`AR_MCP_SERVERS_FILE`)

**Interfaces:**
- `McpServerConfig { name, url, headers: BTreeMap<String,String>, allowed_tools: Option<Vec<String>> }`, loaded from JSON.
- `McpRegistry::from_file(path) / empty()`, plus `names()`, `list_tools(server) -> Vec<{name, description, input_schema}>` and `call(server, tool, args) -> Value`.
- Transport: streamable HTTP JSON-RPC. It sends `initialize` and then `notifications/initialized`, keeps `Mcp-Session-Id`, and accepts both `application/json` and `text/event-stream` responses (it takes the first `message` event that carries a matching id).
- Header values are never logged and never returned in errors.

- [ ] Tests against an in-process Axum fake MCP server: init, list, call, an SSE response, an `allowed_tools` refusal, and an error that redacts the header values.
- [ ] Implement, gate, commit.

### Track C (control): API, MCP server, webhooks, wiring. Starts after B2.

#### Task C1: API keys, `/v1` REST and SSE, webhooks

**Files:**
- Create: `src/api_v1.rs`, `src/api_keys.rs`, `src/webhooks.rs`, `tests/api_v1.rs`
- Modify: `src/api.rs` (`AppState` gains `sessions: Option<Sessions>`, plus bearer auth in `owner()`), `src/main.rs` (build `Sessions` with `HttpBoxClient` and `GatewayModel`), `src/config.rs`

**Interfaces:**
- **Keys:** `ar_` followed by 48 hex characters; only the SHA-256 is stored, and the key is shown once.
- **Endpoints:** the routes from the spec's API section, with JSON errors `{error}`.
- **SSE:** `GET /v1/sessions/{id}/stream` replays from `Last-Event-ID` or `?after`, then streams live events; the SSE `id` is the event's `seq`.
- **Webhooks:** POST `{session_id, status, last_reply}` on each turn end, signed `X-Agent-Signature: sha256=<hmac>`. Two retries, with a 5 s timeout.
- **Voice proxy:** `/v1/voice/transcribe` and `/v1/voice/speak` stream to `AR_VOICE_URL` and return 404 when it isn't set.

- [ ] Write the tests (using fakes):
  - each key-scoped route rejects a missing key with 401
  - a cross-owner request gets 404 (Review Focus 5)
  - a revoked key gets 401
  - SSE replays after `Last-Event-ID`
  - a webhook signature verifies
  - the voice routes return 404 when unconfigured
- [ ] Implement, gate, commit.

#### Task C2: MCP server `/mcp`

**Files:**
- Create: `src/mcp_server.rs`, `tests/mcp_server.rs`
- Modify: `src/api.rs` (the route)

**Interfaces:** JSON-RPC 2.0 over POST, with `application/json` responses.
- `initialize` returns protocolVersion `2025-06-18` and capabilities `{tools:{}}`.
- `tools/list` and `tools/call` cover the tools named in the spec, with JSON-schema inputs.
- `notifications/initialized` returns 202.
- Bearer auth uses the same keys.

- [ ] Tests:
  - initialize, list, then `start_session`, `send_message`, `wait_for_reply` against fakes
  - an unknown tool returns error -32602
  - no key returns 401
- [ ] Implement, gate, commit.

### Track D: optional voice, installers, web chat, docs

#### Task D1: Voice service (optional)

**Files:**
- Create: `voice/server.py` (FastAPI): `POST /transcribe` (audio of any format, converted by ffmpeg to 16 kHz mono) → `{text}` using Moonshine ONNX base; `POST /speak {text}` → `audio/wav` using LuxTTS with the reference voice `voice/ref.wav`. Also `GET /health`.
- Create: `voice/requirements-stt.txt`, `voice/requirements-tts.txt`, `voice/install.sh` (`--stt`, `--tts`, `--device cpu|cuda`), `voice/voice.service`, `docs/VOICE.md`

STT and TTS install independently. When one is not installed, its route returns 501.

- [ ] Verify on the worker VM in a venv with a CPU build: transcribe a TTS-generated wav, and the round-trip text should match apart from case and punctuation. Record the timings in `docs/evidence/live/voice/`. Remove the venv afterwards if it is over 3 GB.
- [ ] Commit.

#### Task D2: Installers

**Files:**
- Create: `scripts/install.sh` (rewritten), `scripts/setup.sh` (interactive, using whiptail when present, else numbered prompts), `docs/INSTALL.md`

**`install.sh`:**
- Flags: `--role all|control|worker`, `--with browser,voice-stt,voice-tts,tls`, `--yes`.
- What it does: installs Docker, gVisor, the Rust build or a prebuilt binary, the box network, the workspace image, systemd units and qualification.
- It is idempotent.
- `setup.sh` collects the answers and then calls `install.sh`.

- [ ] Verify with `bash -n` and `shellcheck` (if present), plus `install.sh --dry-run --role all --with browser,voice-stt`, which prints the plan.
- [ ] Commit.

#### Task D3: Web chat on sessions

**Files:**
- Modify: `web/index.html`, `web/app.js`, `web/app.css`, `qa/workspace.spec.cjs`

Behaviour:
- A session list in a slim drawer.
- Each thread is one session. Sending while the agent is running becomes an interjection, marked "Sent while working".
- Tool steps render as compact rows (icon, a one-line summary, expandable output). Screenshots from the browser tool render inline through the files API.
- A files panel offers downloads.
- The mic and speaker buttons appear only when `/v1/status` reports voice.
- The live updates come from SSE.

- [ ] Playwright: send, see tool rows, interject, download a file, mobile at 390 px.
- [ ] Commit.

#### Task D4: Docs and the final README

Rewrite the README in short sections. Write `docs/API.md` and `docs/MCP.md` (`claude mcp add --transport http agent https://<host>/mcp --header "Authorization: Bearer ar_…"`). Update DESIGN and SECURITY.

### Track E: integration

#### Task E1: Deploy, live proof, review

- Deploy both VMs, from a release build on the worker:
  1. stop the old worker
  2. install
  3. `box-network.sh`
  4. `qualify-box`
  5. `requalify`
  6. start
- Run a live session on Vultr GLM that:
  - researches a topic
  - builds a small web app in the box
  - tests it with `browser`
  - converts a report to PDF

  Save the events and receipts in `docs/evidence/live/session-demo/`.
- Run a live probe with a prompt asking the agent to `curl 169.254.169.254`; the stream must show it blocked.
- Whole-branch review by a fresh reviewer; fix Important and Critical findings. Secret-scan, commit and push the branch.
