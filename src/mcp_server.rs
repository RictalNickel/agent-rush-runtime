//! MCP server at `POST /mcp` (streamable HTTP transport, JSON responses, stateless).
//! JSON-RPC 2.0, single or batch: `initialize`, `ping`, `tools/list`, `tools/call`, and
//! notifications (answered with 202). Auth is the same bearer API keys as `/v1`; a
//! session that is missing or belongs to another owner is reported as not found.
//! A batch holds at most `SessionLimits::max_mcp_batch` messages.
use crate::{
    api::{ApiError, AppState},
    api_v1::{check_exec, check_path, engine, input_error, lookup_error, workspace_error},
    mcp_client::PROTOCOL_VERSION,
    sandbox_api::{ExecRequest, FileRead},
    session::{SessionLimits, Sessions},
    session_store::{MAX_EVENT_PAGE, SessionEvent, SessionStatus},
    skills::{builtin_skills, parse_skill_md},
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use base64::Engine;
use serde_json::{Map, Value, json};
use std::time::Duration;

/// Largest file moved through `read_file`/`write_file` here; use the REST files endpoint
/// for anything bigger (up to 25 MiB).
pub const MCP_MAX_FILE_BYTES: usize = 8 * 1024 * 1024;
const MCP_BODY_LIMIT: usize = 12 * 1024 * 1024;
const DEFAULT_WAIT_SECONDS: u64 = 120;
const MAX_WAIT_SECONDS: u64 = 300;
const SUMMARY_EVENTS: usize = 50;

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/mcp", post(handle).get(not_allowed).delete(not_allowed))
        .layer(DefaultBodyLimit::max(MCP_BODY_LIMIT))
}

async fn not_allowed() -> Response {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        [(header::ALLOW, "POST")],
        Json(json!({"error": "This MCP server answers POST only (no server-sent stream)"})),
    )
        .into_response()
}

fn unauthorized(message: &str, invalid: bool) -> Response {
    let challenge = if invalid {
        "Bearer error=\"invalid_token\""
    } else {
        "Bearer"
    };
    (
        StatusCode::UNAUTHORIZED,
        [(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static(challenge),
        )],
        Json(json!({"error": message})),
    )
        .into_response()
}

async fn handle(State(s): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    let owner = match s.bearer_owner(&headers) {
        Ok(Some(owner)) => owner,
        Ok(None) => {
            return unauthorized("API key required: send Authorization: Bearer ar_...", false);
        }
        Err(ApiError(_, message)) => return unauthorized(&message, true),
    };
    let message: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(rpc_error(Value::Null, -32700, "Parse error")),
            )
                .into_response();
        }
    };
    match message {
        Value::Array(batch) => {
            if batch.is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(rpc_error(Value::Null, -32600, "Empty batch")),
                )
                    .into_response();
            }
            let max = s
                .sessions
                .as_ref()
                .map_or_else(SessionLimits::default, Sessions::limits)
                .max_mcp_batch;
            if batch.len() > max {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(rpc_error(
                        Value::Null,
                        -32600,
                        &format!("Batch too large: at most {max} messages per request"),
                    )),
                )
                    .into_response();
            }
            let mut replies = Vec::new();
            for item in batch {
                if let Some(reply) = dispatch(&s, &owner, item).await {
                    replies.push(reply);
                }
            }
            if replies.is_empty() {
                StatusCode::ACCEPTED.into_response()
            } else {
                Json(Value::Array(replies)).into_response()
            }
        }
        single => match dispatch(&s, &owner, single).await {
            Some(reply) => Json(reply).into_response(),
            None => StatusCode::ACCEPTED.into_response(),
        },
    }
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}
fn rpc_result(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

/// One JSON-RPC message. None for notifications and responses (nothing to answer).
async fn dispatch(s: &AppState, owner: &str, message: Value) -> Option<Value> {
    let Value::Object(message) = message else {
        return Some(rpc_error(Value::Null, -32600, "Invalid request"));
    };
    let id = message.get("id").cloned();
    let method = message.get("method").and_then(Value::as_str);
    let Some(method) = method.filter(|_| message.get("jsonrpc") == Some(&json!("2.0"))) else {
        // A response from the client (result or error with an id) needs no answer.
        if message.contains_key("result") || message.contains_key("error") {
            return None;
        }
        return Some(rpc_error(
            id.unwrap_or(Value::Null),
            -32600,
            "Invalid request",
        ));
    };
    let Some(id) = id else {
        // Notifications (`notifications/initialized`, `notifications/cancelled`, ...).
        return None;
    };
    let params = message.get("params").cloned().unwrap_or(json!({}));
    Some(match method {
        "initialize" => rpc_result(
            id,
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "vultr-placeholder-test123", "title": "Vultr-Placeholder-Test123", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Agent sessions with a persistent Linux workspace. Call start_session (optionally with a first message), then wait_for_reply. Use send_message for follow-ups; messages sent while a turn runs are delivered to the agent as interjections. run_command, read_file, write_file and list_files act directly on the session's workspace.",
            }),
        ),
        "ping" => rpc_result(id, json!({})),
        "tools/list" => rpc_result(id, json!({"tools": tool_definitions()})),
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return Some(rpc_error(id, -32602, "Missing tool name"));
            };
            let arguments = match params.get("arguments") {
                None | Some(Value::Null) => Map::new(),
                Some(Value::Object(map)) => map.clone(),
                Some(_) => return Some(rpc_error(id, -32602, "arguments must be an object")),
            };
            match call_tool(s, owner, name, Args(&arguments)).await {
                Ok(content) => rpc_result(id, json!({"content": content, "isError": false})),
                Err(ToolError::Params(message)) => rpc_error(id, -32602, &message),
                Err(ToolError::Failed(message)) => rpc_result(
                    id,
                    json!({"content": [{"type": "text", "text": message}], "isError": true}),
                ),
            }
        }
        _ => rpc_error(id, -32601, &format!("Method not found: {method}")),
    })
}

enum ToolError {
    /// Unknown tool or bad arguments: a JSON-RPC -32602 error.
    Params(String),
    /// The tool ran and failed: a result with `isError: true`.
    Failed(String),
}
impl From<ApiError> for ToolError {
    fn from(e: ApiError) -> Self {
        ToolError::Failed(e.1)
    }
}
type ToolResult = Result<Vec<Value>, ToolError>;

struct Args<'a>(&'a Map<String, Value>);
impl Args<'_> {
    fn opt_str(&self, key: &str) -> Result<Option<&str>, ToolError> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(v)) => Ok(Some(v)),
            Some(_) => Err(ToolError::Params(format!("{key} must be a string"))),
        }
    }
    fn str(&self, key: &str) -> Result<&str, ToolError> {
        self.opt_str(key)?
            .ok_or_else(|| ToolError::Params(format!("{key} is required")))
    }
    fn opt_u64(&self, key: &str) -> Result<Option<u64>, ToolError> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v
                .as_u64()
                .map(Some)
                .ok_or_else(|| ToolError::Params(format!("{key} must be a non-negative integer"))),
        }
    }
    fn opt_bool(&self, key: &str) -> Result<Option<bool>, ToolError> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Bool(v)) => Ok(Some(*v)),
            Some(_) => Err(ToolError::Params(format!("{key} must be a boolean"))),
        }
    }
}

fn text(value: impl Into<String>) -> Vec<Value> {
    vec![json!({"type": "text", "text": value.into()})]
}
fn json_text(value: &Value) -> Vec<Value> {
    text(serde_json::to_string_pretty(value).unwrap_or_default())
}

fn tool_definitions() -> Value {
    let session_id = json!({"type": "string", "description": "The session id from start_session or list_sessions."});
    let tool = |name: &str, description: &str, properties: Value, required: &[&str]| {
        json!({
            "name": name,
            "description": description,
            "inputSchema": {"type": "object", "properties": properties, "required": required},
        })
    };
    json!([
        tool(
            "start_session",
            "Create an agent session with its own persistent Linux workspace. Optionally send the first message, which starts the agent working; then call wait_for_reply.",
            json!({"title": {"type": "string"}, "message": {"type": "string", "description": "First message for the agent."}}),
            &[]
        ),
        tool(
            "send_message",
            "Send a message to the agent. If it is idle this starts a turn; if it is working, the message is delivered as an interjection before its next step.",
            json!({"session_id": session_id, "text": {"type": "string"}}),
            &["session_id", "text"]
        ),
        tool(
            "wait_for_reply",
            "Wait until the agent's turn ends (or the timeout passes) and return its status, its last reply and a summary of the events since after_seq (default: the latest user message).",
            json!({"session_id": session_id, "timeout_seconds": {"type": "integer", "minimum": 1, "maximum": MAX_WAIT_SECONDS, "default": DEFAULT_WAIT_SECONDS}, "after_seq": {"type": "integer", "minimum": 0}}),
            &["session_id"]
        ),
        tool(
            "get_events",
            "Read the session's event log (user, assistant, tool_call, tool_result, interjection, status, config_change, error) after a sequence number.",
            json!({"session_id": session_id, "after": {"type": "integer", "minimum": 0, "default": 0}, "limit": {"type": "integer", "minimum": 1, "maximum": MAX_EVENT_PAGE, "default": 100}}),
            &["session_id"]
        ),
        tool(
            "run_command",
            "Run a bash command directly in the session's workspace (as the agent's user, in /workspace) and return exit code, stdout and stderr.",
            json!({"session_id": session_id, "command": {"type": "string"}, "cwd": {"type": "string"}, "timeout_seconds": {"type": "integer", "minimum": 1, "maximum": 600}}),
            &["session_id", "command"]
        ),
        tool(
            "read_file",
            "Read a file from the workspace. Text comes back as text; binary files as base64 (images also as an image block). Directories are listed.",
            json!({"session_id": session_id, "path": {"type": "string", "description": "Relative to /workspace, or absolute under /workspace or /tmp."}}),
            &["session_id", "path"]
        ),
        tool(
            "write_file",
            "Create or replace a file in the workspace.",
            json!({"session_id": session_id, "path": {"type": "string"}, "content": {"type": "string"}, "encoding": {"type": "string", "enum": ["utf8", "base64"], "default": "utf8"}}),
            &["session_id", "path", "content"]
        ),
        tool(
            "list_files",
            "List a directory in the workspace.",
            json!({"session_id": session_id, "path": {"type": "string", "default": "."}}),
            &["session_id"]
        ),
        tool(
            "cancel",
            "Stop the agent's current turn after its current step and drop queued interjections.",
            json!({"session_id": session_id}),
            &["session_id"]
        ),
        tool(
            "close_session",
            "Close the session and delete its workspace. With delete: true, also remove the session and its history for good.",
            json!({"session_id": session_id, "delete": {"type": "boolean", "default": false, "description": "Also delete the session and its event history permanently."}}),
            &["session_id"]
        ),
        tool(
            "list_sessions",
            "List your agent sessions, newest first.",
            json!({}),
            &[]
        ),
        tool(
            "manage_skills",
            "List, read, save or delete your skills (SKILL.md style instructions the agent loads on demand).",
            json!({"action": {"type": "string", "enum": ["list", "get", "save", "delete"]}, "name": {"type": "string"}, "description": {"type": "string"}, "body": {"type": "string"}, "markdown": {"type": "string", "description": "A whole SKILL.md, instead of name, description and body."}}),
            &["action"]
        ),
    ])
}

const TOOL_NAMES: &[&str] = &[
    "start_session",
    "send_message",
    "wait_for_reply",
    "get_events",
    "run_command",
    "read_file",
    "write_file",
    "list_files",
    "cancel",
    "close_session",
    "list_sessions",
    "manage_skills",
];

async fn call_tool(s: &AppState, owner: &str, name: &str, args: Args<'_>) -> ToolResult {
    if !TOOL_NAMES.contains(&name) {
        return Err(ToolError::Params(format!("Unknown tool: {name}")));
    }
    if name == "manage_skills" {
        return manage_skills(s, owner, &args);
    }
    // Validate arguments before checking that sessions are configured.
    let session_id = match name {
        "start_session" | "list_sessions" => None,
        _ => Some(args.str("session_id")?.to_string()),
    };
    let sessions = engine(s)?;
    let id = session_id.as_deref().unwrap_or("");
    // Waiting for a reply and direct workspace calls hold one of the owner's long-request
    // slots while they run.
    let _slot = match name {
        "wait_for_reply" | "run_command" | "read_file" | "write_file" | "list_files" => {
            Some(sessions.long_request_slot(owner).map_err(lookup_error)?)
        }
        _ => None,
    };
    match name {
        "start_session" => start_session(sessions, owner, &args),
        "send_message" => {
            let text = args.str("text")?;
            let outcome = sessions.send(owner, id, text).map_err(input_error)?;
            Ok(json_text(&json!({
                "session_id": id,
                "seq": outcome.seq,
                "interjection": outcome.interjection,
                "next": "Call wait_for_reply to get the agent's answer.",
            })))
        }
        "wait_for_reply" => wait_for_reply(s, sessions, owner, id, &args).await,
        "get_events" => {
            let after = args.opt_u64("after")?.unwrap_or(0) as i64;
            let limit = args
                .opt_u64("limit")?
                .unwrap_or(100)
                .clamp(1, MAX_EVENT_PAGE as u64) as usize;
            let events = sessions
                .events(owner, id, after, limit)
                .map_err(lookup_error)?;
            let next_after = events.last().map_or(after, |e| e.seq);
            Ok(json_text(
                &json!({"events": events, "next_after": next_after}),
            ))
        }
        "run_command" => {
            let request = ExecRequest {
                command: args.str("command")?.to_string(),
                cwd: args.opt_str("cwd")?.map(String::from),
                timeout_seconds: args
                    .opt_u64("timeout_seconds")?
                    .map(|t| u32::try_from(t).unwrap_or(u32::MAX)),
                stdin: None,
            };
            check_exec(&request).map_err(|e| ToolError::Params(e.1))?;
            let result = sessions
                .exec(owner, id, request)
                .await
                .map_err(workspace_error)?;
            Ok(json_text(&json!(result)))
        }
        "read_file" => {
            let path = args.str("path")?;
            check_path(path).map_err(|e| ToolError::Params(e.1))?;
            let read = sessions
                .read_file(owner, id, path)
                .await
                .map_err(workspace_error)?;
            Ok(file_content(path, read)?)
        }
        "write_file" => {
            let path = args.str("path")?;
            check_path(path).map_err(|e| ToolError::Params(e.1))?;
            let content = args.str("content")?;
            let bytes = match args.opt_str("encoding")?.unwrap_or("utf8") {
                "utf8" => Bytes::copy_from_slice(content.as_bytes()),
                "base64" => Bytes::from(
                    base64::engine::general_purpose::STANDARD
                        .decode(content.trim())
                        .map_err(|_| ToolError::Params("content is not valid base64".into()))?,
                ),
                _ => return Err(ToolError::Params("encoding must be utf8 or base64".into())),
            };
            if bytes.len() > MCP_MAX_FILE_BYTES {
                return Err(ToolError::Failed(
                    "File exceeds 8 MiB; use PUT /v1/sessions/{id}/files for files up to 25 MiB"
                        .into(),
                ));
            }
            let written = sessions
                .write_file(owner, id, path, bytes)
                .await
                .map_err(workspace_error)?;
            Ok(json_text(
                &json!({"path": path, "bytes": written.bytes, "sha256": written.sha256}),
            ))
        }
        "list_files" => {
            let path = args.opt_str("path")?.unwrap_or(".");
            check_path(path).map_err(|e| ToolError::Params(e.1))?;
            match sessions
                .read_file(owner, id, path)
                .await
                .map_err(workspace_error)?
            {
                FileRead::Dir(entries) => Ok(json_text(&json!({"path": path, "entries": entries}))),
                FileRead::File(_) => Err(ToolError::Failed(format!(
                    "{path} is a file, not a directory"
                ))),
            }
        }
        "cancel" => {
            sessions.cancel(owner, id).map_err(lookup_error)?;
            Ok(text(
                "Cancel requested; the turn stops after its current step.",
            ))
        }
        "close_session" => {
            if args.opt_bool("delete")?.unwrap_or(false) {
                sessions.delete(owner, id).await.map_err(lookup_error)?;
                return Ok(text(
                    "Session deleted: its workspace and history are removed for good.",
                ));
            }
            sessions.close(owner, id).await.map_err(lookup_error)?;
            Ok(text("Session closed and its workspace deleted."))
        }
        "list_sessions" => {
            let list = sessions.list(owner).map_err(ApiError::internal)?;
            let list: Vec<Value> = list
                .into_iter()
                .map(|v| json!({"session_id": v.id, "title": v.title, "status": v.status, "updated_at": v.updated_at, "spend_usd": v.spend_usd, "last_seq": v.last_seq}))
                .collect();
            Ok(json_text(&json!({"sessions": list})))
        }
        _ => Err(ToolError::Params(format!("Unknown tool: {name}"))),
    }
}

fn start_session(sessions: &Sessions, owner: &str, args: &Args<'_>) -> ToolResult {
    let title = args.opt_str("title")?;
    let message = args.opt_str("message")?;
    if message.is_some_and(|m| m.trim().is_empty()) {
        return Err(ToolError::Params("message must not be empty".into()));
    }
    let session = sessions.create(owner, title).map_err(input_error)?;
    let sent = match message {
        Some(text) => Some(
            sessions
                .send(owner, &session.id, text)
                .map_err(input_error)?,
        ),
        None => None,
    };
    Ok(json_text(&json!({
        "session_id": session.id,
        "title": session.title,
        "status": if sent.is_some() { "running" } else { "idle" },
        "sent_seq": sent.map(|o| o.seq),
        "next": if sent.is_some() { "Call wait_for_reply with this session_id." } else { "Call send_message to give the agent a task." },
    })))
}

/// A one-line description of an event for summaries.
fn summarize(event: &SessionEvent) -> String {
    let p = &event.payload;
    let clip = |v: &str| crate::agent::clip(v, 200).replace('\n', " ");
    let detail = match event.kind.as_str() {
        "user" | "assistant" | "interjection" => clip(p["text"].as_str().unwrap_or("")),
        "tool_call" => {
            if p["invalid"] == true {
                "invalid tool call".into()
            } else {
                let tool = p["call"]["tool"].as_str().unwrap_or("?");
                format!("{tool} {}", clip(&p["call"].to_string()))
            }
        }
        "tool_result" => format!(
            "{} {}",
            if p["ok"] == true { "ok" } else { "failed" },
            clip(p["output"].as_str().unwrap_or(""))
        ),
        "status" => format!(
            "{}{}",
            p["status"].as_str().unwrap_or(""),
            p["reason"]
                .as_str()
                .map(|r| format!(" ({r})"))
                .unwrap_or_default()
        ),
        "error" => clip(p["message"].as_str().unwrap_or("")),
        _ => clip(&p.to_string()),
    };
    format!("#{} {}: {}", event.seq, event.kind, detail)
}

async fn wait_for_reply(
    s: &AppState,
    sessions: &Sessions,
    owner: &str,
    id: &str,
    args: &Args<'_>,
) -> ToolResult {
    let timeout = args
        .opt_u64("timeout_seconds")?
        .unwrap_or(DEFAULT_WAIT_SECONDS)
        .clamp(1, MAX_WAIT_SECONDS);
    let after_arg = args.opt_u64("after_seq")?;
    let view = sessions
        .wait_idle(owner, id, Duration::from_secs(timeout))
        .await
        .map_err(lookup_error)?;
    // Ownership is established by wait_idle; read the recent log directly.
    let recent = s
        .ledger
        .recent_session_events(id, MAX_EVENT_PAGE)
        .map_err(ApiError::internal)?;
    let after = match after_arg {
        Some(a) => a as i64,
        None => recent
            .iter()
            .rev()
            .find(|e| e.kind == "user")
            .map_or(0, |e| e.seq - 1),
    };
    let new: Vec<&SessionEvent> = recent.iter().filter(|e| e.seq > after).collect();
    let reply = new.iter().rev().find(|e| e.kind == "assistant");
    let running = view.status == SessionStatus::Running;
    let skipped = new.len().saturating_sub(SUMMARY_EVENTS);
    let summary: Vec<String> = new.iter().skip(skipped).map(|e| summarize(e)).collect();
    Ok(json_text(&json!({
        "session_id": id,
        "status": view.status,
        "still_running": running,
        "reply": reply.map(|e| e.payload["text"].clone()),
        "attachments": reply.map(|e| e.payload["attachments"].clone()),
        "events": summary,
        "events_omitted": skipped,
        "next_after": view.last_seq,
        "hint": if running { "The turn is still running; call wait_for_reply again." } else { "" },
    })))
}

fn file_content(path: &str, read: FileRead) -> ToolResult {
    let bytes = match read {
        FileRead::Dir(entries) => {
            return Ok(json_text(
                &json!({"path": path, "directory": true, "entries": entries}),
            ));
        }
        FileRead::File(bytes) => bytes,
    };
    if bytes.len() > MCP_MAX_FILE_BYTES {
        return Err(ToolError::Failed(format!(
            "File is {} bytes; files over 8 MiB are available through GET /v1/sessions/{{id}}/files",
            bytes.len()
        )));
    }
    if !bytes.contains(&0)
        && let Ok(text_content) = std::str::from_utf8(&bytes)
    {
        return Ok(text(text_content));
    }
    let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let lower = path.to_ascii_lowercase();
    let image = [
        (".png", "image/png"),
        (".jpg", "image/jpeg"),
        (".jpeg", "image/jpeg"),
        (".gif", "image/gif"),
        (".webp", "image/webp"),
    ]
    .into_iter()
    .find(|(ext, _)| lower.ends_with(ext))
    .map(|(_, mime)| mime);
    let mut content = json_text(&json!({
        "path": path,
        "encoding": "base64",
        "bytes": bytes.len(),
        "data": data,
    }));
    if let Some(mime) = image {
        content.push(json!({"type": "image", "data": data, "mimeType": mime}));
    }
    Ok(content)
}

fn manage_skills(s: &AppState, owner: &str, args: &Args<'_>) -> ToolResult {
    let failed = |e: anyhow::Error| ToolError::Failed(e.to_string());
    match args.str("action")? {
        "list" => {
            let own: Vec<Value> = s
                .ledger
                .skills(owner)
                .map_err(failed)?
                .into_iter()
                .map(|k| json!({"name": k.name, "description": k.description}))
                .collect();
            let builtin: Vec<Value> = builtin_skills()
                .into_iter()
                .map(|k| json!({"name": k.name, "description": k.description}))
                .collect();
            Ok(json_text(&json!({"skills": own, "builtin": builtin})))
        }
        "get" => {
            let name = args.str("name")?;
            let skill = s
                .ledger
                .skill(owner, name)
                .map_err(failed)?
                .or_else(|| builtin_skills().into_iter().find(|k| k.name == name))
                .ok_or_else(|| ToolError::Failed(format!("Skill {name} not found")))?;
            Ok(text(skill.to_markdown()))
        }
        "save" => {
            let (name, description, body) = match args.opt_str("markdown")? {
                Some(markdown) => parse_skill_md(markdown).map_err(failed)?,
                None => (
                    args.str("name")?.to_string(),
                    args.str("description")?.to_string(),
                    args.str("body")?.to_string(),
                ),
            };
            let skill = s
                .ledger
                .save_skill(owner, &name, &description, &body)
                .map_err(failed)?;
            Ok(text(format!("Saved skill {}.", skill.name)))
        }
        "delete" => {
            let name = args.str("name")?;
            if s.ledger.delete_skill(owner, name).map_err(failed)? {
                Ok(text(format!("Deleted skill {name}.")))
            } else {
                Err(ToolError::Failed(format!("Skill {name} not found")))
            }
        }
        other => Err(ToolError::Params(format!(
            "action must be list, get, save or delete (got {other})"
        ))),
    }
}
