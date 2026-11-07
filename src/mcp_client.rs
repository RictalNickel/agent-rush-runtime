//! Client for operator-configured remote MCP servers (streamable HTTP, JSON-RPC 2.0).
//! Credentials stay on the control plane: header values never reach a prompt, a box,
//! a log line or an error message.
//!
//! An https server gets the webhooks' SSRF guard on every request: its host must resolve
//! only to public addresses, the request connects to the address that was checked (no
//! second lookup), ignores proxy settings and never follows redirects. Plain http is
//! allowed only to a loopback host, for a local server the operator runs on this machine.
use crate::webhooks::{UrlGuard, is_forbidden_ip, pinned_client};
use anyhow::{Context, Result, anyhow, bail, ensure};
use futures_util::StreamExt;
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

pub const PROTOCOL_VERSION: &str = "2025-06-18";
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const MAX_SERVERS: usize = 32;
/// Owner sessions remembered per server before idle ones are forgotten.
const MAX_OWNER_SESSIONS: usize = 4096;
/// The owner used by the owner-less `list_tools` and `call`.
const SHARED_OWNER: &str = "";
const SESSION_HEADER: &str = "mcp-session-id";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpServerConfig {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// When set, only these tools are listed and callable.
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
}
impl std::fmt::Debug for McpServerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpServerConfig")
            .field("name", &self.name)
            .field("headers", &self.headers.keys().collect::<Vec<_>>())
            .field("allowed_tools", &self.allowed_tools)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ConfigFile {
    List(Vec<McpServerConfig>),
    Wrapped { servers: Vec<McpServerConfig> },
}

#[derive(Clone)]
struct Session {
    id: Option<String>,
    protocol: String,
}

/// One owner's MCP session with one server. The lock is held only while the session is
/// initialized or read, never across a tool request, so calls run concurrently.
type SessionSlot = Arc<tokio::sync::Mutex<Option<Session>>>;

struct Server {
    config: McpServerConfig,
    /// Plain http to a loopback host: an operator's local server, reached without the guard.
    local: bool,
    headers: HeaderMap,
    /// Sessions by owner, so one owner's server-side state never serves another's.
    sessions: Mutex<HashMap<String, SessionSlot>>,
}
impl Server {
    fn slot(&self, owner: &str) -> SessionSlot {
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        if !sessions.contains_key(owner) && sessions.len() >= MAX_OWNER_SESSIONS {
            // Forget idle sessions first; a forgotten one is simply initialized again.
            sessions.retain(|_, slot| Arc::strong_count(slot) > 1);
        }
        sessions.entry(owner.to_string()).or_default().clone()
    }
}

pub struct McpRegistry {
    /// For local (loopback http) servers only; https servers get a pinned client per request.
    client: reqwest::Client,
    guard: UrlGuard,
    servers: Vec<Server>,
    next_id: AtomicU64,
}

fn valid_server_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 40
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-' || *b == b'_')
}

impl McpRegistry {
    pub fn empty() -> Self {
        Self::new(Vec::new()).expect("empty registry is valid")
    }
    /// Loads `[{name,url,headers,allowed_tools}]` or `{"servers":[...]}`.
    pub fn from_file(path: &Path) -> Result<Self> {
        let raw = std::fs::read(path)
            .with_context(|| format!("reading MCP servers file {}", path.display()))?;
        let file: ConfigFile = serde_json::from_slice(&raw).map_err(|e| {
            anyhow!(
                "invalid MCP servers file (line {}, column {})",
                e.line(),
                e.column()
            )
        })?;
        Self::new(match file {
            ConfigFile::List(servers) | ConfigFile::Wrapped { servers } => servers,
        })
    }
    pub fn new(configs: Vec<McpServerConfig>) -> Result<Self> {
        ensure!(
            configs.len() <= MAX_SERVERS,
            "at most {MAX_SERVERS} MCP servers"
        );
        let mut servers: Vec<Server> = Vec::new();
        for config in configs {
            ensure!(
                valid_server_name(&config.name),
                "MCP server name {:?} must match ^[a-z0-9][a-z0-9_-]{{0,39}}$",
                config.name
            );
            ensure!(
                servers.iter().all(|s| s.config.name != config.name),
                "duplicate MCP server {:?}",
                config.name
            );
            let url = reqwest::Url::parse(&config.url)
                .map_err(|_| anyhow!("MCP server {:?}: invalid url", config.name))?;
            let local = url.scheme() == "http"
                && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
            ensure!(
                url.scheme() == "https" || local,
                "MCP server {:?}: url must be https (http only on loopback)",
                config.name
            );
            // A literal address can be refused now; names are checked at every request.
            let literal = url
                .host_str()
                .map(|h| h.trim_start_matches('[').trim_end_matches(']'))
                .and_then(|h| h.parse::<std::net::IpAddr>().ok());
            ensure!(
                local || !literal.is_some_and(is_forbidden_ip),
                "MCP server {:?}: an https url must not point at a private or reserved address",
                config.name
            );
            let mut headers = HeaderMap::new();
            for (key, value) in &config.headers {
                let name = HeaderName::from_bytes(key.as_bytes())
                    .map_err(|_| anyhow!("MCP server {:?}: invalid header name", config.name))?;
                let mut value = HeaderValue::from_str(value).map_err(|_| {
                    anyhow!(
                        "MCP server {:?}: invalid value for header {key}",
                        config.name
                    )
                })?;
                value.set_sensitive(true);
                headers.insert(name, value);
            }
            servers.push(Server {
                config,
                local,
                headers,
                sessions: Mutex::new(HashMap::new()),
            });
        }
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .connect_timeout(Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            guard: UrlGuard::public(),
            servers,
            next_id: AtomicU64::new(1),
        })
    }
    pub fn names(&self) -> Vec<String> {
        self.servers.iter().map(|s| s.config.name.clone()).collect()
    }
    fn server(&self, name: &str) -> Result<&Server> {
        self.servers
            .iter()
            .find(|s| s.config.name == name)
            .ok_or_else(|| anyhow!("unknown MCP server {name:?}"))
    }
    /// `list_tools_for_owner` with a session shared by every owner-less caller.
    pub async fn list_tools(&self, server: &str) -> Result<Vec<McpTool>> {
        self.list_tools_for_owner(SHARED_OWNER, server).await
    }
    /// `call_for_owner` with a session shared by every owner-less caller.
    pub async fn call(&self, server: &str, tool: &str, args: Value) -> Result<Value> {
        self.call_for_owner(SHARED_OWNER, server, tool, args).await
    }
    /// Lists the server's (allowed) tools in `owner`'s MCP session.
    pub async fn list_tools_for_owner(&self, owner: &str, server: &str) -> Result<Vec<McpTool>> {
        let server = self.server(server)?;
        let result = async {
            let mut tools = Vec::new();
            let mut cursor: Option<String> = None;
            for _ in 0..20 {
                let params = match &cursor {
                    Some(c) => json!({"cursor": c}),
                    None => json!({}),
                };
                let page = self.rpc(server, owner, "tools/list", params).await?;
                for tool in page["tools"].as_array().into_iter().flatten() {
                    let Some(name) = tool["name"].as_str() else {
                        continue;
                    };
                    if !allowed(&server.config, name) {
                        continue;
                    }
                    tools.push(McpTool {
                        name: name.into(),
                        description: tool["description"].as_str().unwrap_or("").into(),
                        input_schema: tool
                            .get("inputSchema")
                            .cloned()
                            .unwrap_or_else(|| json!({"type":"object"})),
                    });
                }
                cursor = page["nextCursor"].as_str().map(String::from);
                if cursor.is_none() {
                    break;
                }
            }
            Ok(tools)
        }
        .await;
        result.map_err(|e| redacted(server, e))
    }
    /// Calls a tool in `owner`'s MCP session and returns the JSON-RPC `result`
    /// (`{content, isError?, ...}`).
    pub async fn call_for_owner(
        &self,
        owner: &str,
        server: &str,
        tool: &str,
        args: Value,
    ) -> Result<Value> {
        let server = self.server(server)?;
        ensure!(
            allowed(&server.config, tool),
            "tool {tool:?} is not allowed on MCP server {:?}",
            server.config.name
        );
        let args = if args.is_null() { json!({}) } else { args };
        ensure!(args.is_object(), "MCP tool arguments must be a JSON object");
        self.rpc(
            server,
            owner,
            "tools/call",
            json!({"name": tool, "arguments": args}),
        )
        .await
        .map_err(|e| redacted(server, e))
    }

    /// `owner`'s session with `server`, initialized on first use. When `stale` is given,
    /// that session expired: it is replaced unless a concurrent call already did so.
    async fn session(
        &self,
        server: &Server,
        owner: &str,
        stale: Option<&Session>,
    ) -> Result<Session> {
        let slot = server.slot(owner);
        let mut current = slot.lock().await;
        let expired = match (stale, current.as_ref()) {
            (Some(stale), Some(now)) => stale.id == now.id,
            _ => false,
        };
        if current.is_none() || expired {
            *current = Some(self.initialize(server).await?);
        }
        Ok(current.clone().expect("session was just set"))
    }

    async fn rpc(
        &self,
        server: &Server,
        owner: &str,
        method: &str,
        params: Value,
    ) -> Result<Value> {
        let session = self.session(server, owner, None).await?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let body = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
        match self.post(server, Some(&session), &body, Some(id)).await {
            // The server forgot our session: start a new one and retry once.
            Err(e) if e.downcast_ref::<SessionExpired>().is_some() => {
                let session = self.session(server, owner, Some(&session)).await?;
                let id = self.next_id.fetch_add(1, Ordering::Relaxed);
                let body = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
                let (value, _) = self.post(server, Some(&session), &body, Some(id)).await?;
                value.context("server sent no response")
            }
            other => other?.0.context("server sent no response"),
        }
    }
    async fn initialize(&self, server: &Server) -> Result<Session> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let body = json!({"jsonrpc":"2.0","id":id,"method":"initialize","params":{
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {"name":"boxoro","version":env!("CARGO_PKG_VERSION")}
        }});
        let (result, session_id) = self.post(server, None, &body, Some(id)).await?;
        let result = result.context("initialize returned no result")?;
        let session = Session {
            id: session_id,
            protocol: result["protocolVersion"]
                .as_str()
                .unwrap_or(PROTOCOL_VERSION)
                .into(),
        };
        let note = json!({"jsonrpc":"2.0","method":"notifications/initialized"});
        self.post(server, Some(&session), &note, None).await?;
        Ok(session)
    }
    /// Sends one message. Returns the matching `result` (None for a notification) and the
    /// session id the server assigned, if any.
    async fn post(
        &self,
        server: &Server,
        session: Option<&Session>,
        body: &Value,
        id: Option<u64>,
    ) -> Result<(Option<Value>, Option<String>)> {
        let mut request = if server.local {
            self.client.post(&server.config.url)
        } else {
            let (url, addr) = self.guard.check(&server.config.url).await?;
            let host = url.host_str().unwrap_or_default().to_string();
            pinned_client(&host, addr, REQUEST_TIMEOUT)?.post(url)
        }
        .headers(server.headers.clone())
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json, text/event-stream")
        .json(body);
        if let Some(session) = session {
            request = request.header("mcp-protocol-version", &session.protocol);
            if let Some(sid) = &session.id {
                request = request.header(SESSION_HEADER, sid);
            }
        }
        let response = request
            .send()
            .await
            .map_err(|e| anyhow!("request failed: {}", e.without_url()))?;
        let status = response.status();
        let session_id = response
            .headers()
            .get(SESSION_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        if status == reqwest::StatusCode::NOT_FOUND && session.is_some_and(|s| s.id.is_some()) {
            return Err(SessionExpired.into());
        }
        let Some(id) = id else {
            ensure!(
                status.is_success(),
                "notification rejected with HTTP {}",
                status.as_u16()
            );
            return Ok((None, session_id));
        };
        let event_stream = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("text/event-stream"));
        if !status.is_success() {
            let bytes = read_bounded(response, 64 * 1024).await.unwrap_or_default();
            let detail = String::from_utf8_lossy(&bytes);
            bail!(
                "HTTP {}: {}",
                status.as_u16(),
                crate::agent::clip(detail.trim(), 500)
            );
        }
        let message = if event_stream {
            read_sse_response(response, id).await?
        } else {
            let bytes = read_bounded(response, MAX_RESPONSE_BYTES).await?;
            let value: Value =
                serde_json::from_slice(&bytes).context("server returned invalid JSON")?;
            match value {
                Value::Array(batch) => batch
                    .into_iter()
                    .find(|m| m["id"] == json!(id))
                    .context("no response with a matching id")?,
                single => single,
            }
        };
        ensure!(message["id"] == json!(id), "response id does not match");
        if let Some(error) = message.get("error") {
            bail!(
                "error {}: {}",
                error["code"].as_i64().unwrap_or(0),
                crate::agent::clip(error["message"].as_str().unwrap_or("unknown"), 500)
            );
        }
        Ok((Some(message["result"].clone()), session_id))
    }
}

#[derive(Debug)]
struct SessionExpired;
impl std::fmt::Display for SessionExpired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MCP session expired")
    }
}
impl std::error::Error for SessionExpired {}

fn allowed(config: &McpServerConfig, tool: &str) -> bool {
    config
        .allowed_tools
        .as_ref()
        .is_none_or(|list| list.iter().any(|t| t == tool))
}

/// Prefixes the server name and strips every configured header value (and each long
/// word of it, such as the token after `Bearer`) from the message.
fn redacted(server: &Server, error: anyhow::Error) -> anyhow::Error {
    let mut text = format!("MCP server {:?}: {error:#}", server.config.name);
    let mut secrets: Vec<&str> = Vec::new();
    for value in server.config.headers.values() {
        secrets.push(value.as_str());
        secrets.extend(value.split_whitespace().filter(|w| w.len() >= 6));
    }
    secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
    for secret in secrets {
        if secret.len() >= 4 {
            text = text.replace(secret, "[redacted]");
        }
    }
    anyhow!(text)
}

async fn read_bounded(response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| anyhow!("reading response: {}", e.without_url()))?;
        ensure!(
            bytes.len() + chunk.len() <= limit,
            "response exceeds {limit} bytes"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// Reads server-sent events until the first `message` event whose JSON carries `id`.
async fn read_sse_response(response: reqwest::Response, id: u64) -> Result<Value> {
    let mut stream = response.bytes_stream();
    let mut buffer: Vec<u8> = Vec::new();
    let mut parser = SseParser::default();
    let mut total = 0usize;
    loop {
        let finished = match stream.next().await {
            Some(chunk) => {
                let chunk =
                    chunk.map_err(|e| anyhow!("reading event stream: {}", e.without_url()))?;
                total += chunk.len();
                ensure!(total <= MAX_RESPONSE_BYTES, "event stream exceeds bound");
                buffer.extend_from_slice(&chunk);
                false
            }
            // Flush a final event that lacks its trailing blank line.
            None => {
                buffer.extend_from_slice(b"\n\n");
                true
            }
        };
        while let Some(end) = buffer.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = buffer.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line);
            if let Some(message) = parser.line(line.trim_end_matches(['\n', '\r']))
                && message["id"] == json!(id)
            {
                return Ok(message);
            }
        }
        if finished {
            bail!("event stream ended without a response");
        }
    }
}

#[derive(Default)]
struct SseParser {
    event: String,
    data: String,
}
impl SseParser {
    /// Feeds one line; returns a parsed JSON message when a `message` event completes.
    fn line(&mut self, line: &str) -> Option<Value> {
        if line.is_empty() {
            let event = std::mem::take(&mut self.event);
            let data = std::mem::take(&mut self.data);
            if data.is_empty() || !(event.is_empty() || event == "message") {
                return None;
            }
            return serde_json::from_str(&data).ok();
        }
        if line.starts_with(':') {
            return None;
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "event" => self.event = value.into(),
            "data" => {
                if !self.data.is_empty() {
                    self.data.push('\n');
                }
                self.data.push_str(value);
            }
            _ => {}
        }
        None
    }
}
