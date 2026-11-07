//! The public `/v1` API: a thin layer over `Sessions`, the skill and agent-config stores,
//! API keys, webhooks and the optional voice service. Every route needs a bearer API key
//! (`Authorization: Bearer ar_...`) or the operator cookie (plus `X-CSRF-Token` for
//! changes). Creating or revoking keys and registering or deleting webhooks needs the
//! operator cookie: a bearer key gets 403 there. A session that is missing or belongs to
//! another owner is always 404.
use crate::{
    agent_config::{AgentConfig, BUILTIN_TOOLS},
    api::{ApiError, AppState},
    config::ModelCatalog,
    ledger::Ledger,
    sandbox_api::{ExecRequest, FileRead, MAX_EXEC_TIMEOUT_SECONDS, MAX_FILE_BYTES},
    session::{OwnerSlot, SessionError, Sessions},
    session_store::{MAX_EVENT_PAGE, MAX_MESSAGE_BYTES, ModelUsage, SessionEvent, SessionStatus},
    skills::{builtin_skills, parse_skill_md},
};
use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, FromRequest, FromRequestParts, Path, Query, Request, State},
    http::{HeaderMap, Method, StatusCode, header, request::Parts},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{delete, get, post},
};
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{collections::VecDeque, convert::Infallible, time::Duration};
use tokio::sync::broadcast::{self, error::RecvError};

/// Largest JSON body on `/v1` routes other than file uploads and audio.
const JSON_BODY_LIMIT: usize = 512 * 1024;
/// Largest exec request (the command plus optional stdin).
const EXEC_BODY_LIMIT: usize = 2 * 1024 * 1024;
const MAX_STDIN_BYTES: usize = 1024 * 1024;
const MAX_PATH_BYTES: usize = 4096;
pub const MAX_AUDIO_BYTES: usize = 25 * 1024 * 1024;
const MAX_SPEAK_REQUEST_BYTES: usize = 64 * 1024;
const MAX_TRANSCRIPT_BYTES: usize = 1024 * 1024;
const SSE_KEEPALIVE: Duration = Duration::from_secs(15);

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/status", get(status))
        .route("/v1/models", get(list_models))
        .route("/v1/usage", get(usage))
        .route("/v1/keys", get(list_keys).post(create_key))
        .route("/v1/keys/{id}", delete(revoke_key))
        .route("/v1/sessions", get(list_sessions).post(create_session))
        .route("/v1/sessions/purge-closed", post(purge_closed_sessions))
        .route("/v1/sessions/{id}", get(get_session).delete(close_session))
        .route("/v1/sessions/{id}/messages", post(send_message))
        .route("/v1/sessions/{id}/cancel", post(cancel_session))
        .route("/v1/sessions/{id}/events", get(list_events))
        .route("/v1/sessions/{id}/stream", get(stream_events))
        .route("/v1/sessions/{id}/archive", get(archive))
        .route("/v1/sessions/{id}/fork", post(fork_session))
        .route(
            "/v1/sessions/{id}/files",
            get(read_file)
                .put(write_file)
                .layer(DefaultBodyLimit::max(MAX_FILE_BYTES)),
        )
        .route(
            "/v1/sessions/{id}/exec",
            post(exec).layer(DefaultBodyLimit::max(EXEC_BODY_LIMIT)),
        )
        .route("/v1/skills", get(list_skills).post(create_skill))
        .route(
            "/v1/skills/{name}",
            get(get_skill).put(put_skill).delete(delete_skill),
        )
        .route("/v1/agent-config", get(get_config).put(put_config))
        .route("/v1/agent-config/history", get(config_history))
        .route("/v1/agent-config/revert", post(revert_config))
        .route("/v1/webhooks", get(list_webhooks).post(create_webhook))
        .route("/v1/webhooks/{id}", delete(delete_webhook))
        .route("/v1/voice/transcribe", post(transcribe))
        .route("/v1/voice/speak", post(speak))
        .layer(DefaultBodyLimit::max(JSON_BODY_LIMIT))
}

// Extractors and error mapping shared with the MCP server.

/// The authenticated owner. Runs before any body extractor, so a request without
/// credentials is 401 whatever its body.
pub(crate) struct Owner(pub String);
impl FromRequestParts<AppState> for Owner {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let mutation = !matches!(parts.method, Method::GET | Method::HEAD);
        state
            .owner(&parts.headers, mutation)
            .map(Owner)
            .map_err(|e| {
                if e.0 == StatusCode::UNAUTHORIZED && e.1 == "Operator login required" {
                    ApiError(
                        StatusCode::UNAUTHORIZED,
                        "API key required: send Authorization: Bearer ar_...".into(),
                    )
                } else {
                    e
                }
            })
    }
}

/// The operator's browser login: the `ar_session` cookie, plus `X-CSRF-Token` for changes.
/// Minting and revoking keys and managing webhooks need it, so a leaked API key cannot
/// mint more keys, revoke the operator's other keys or point webhooks somewhere new.
pub(crate) struct Operator(pub String);
impl FromRequestParts<AppState> for Operator {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        if state.bearer_owner(&parts.headers)?.is_some() {
            return Err(ApiError(
                StatusCode::FORBIDDEN,
                "API keys cannot create or revoke keys or manage webhooks: sign in as the operator and use the cookie with X-CSRF-Token".into(),
            ));
        }
        let mutation = !matches!(parts.method, Method::GET | Method::HEAD);
        state
            .owner(&parts.headers, mutation)
            .map(Operator)
            .map_err(|e| {
                if e.0 == StatusCode::UNAUTHORIZED {
                    ApiError(
                        StatusCode::UNAUTHORIZED,
                        "Operator login required (API keys cannot manage keys or webhooks)".into(),
                    )
                } else {
                    e
                }
            })
    }
}

/// The owner, holding one of their long-request slots (`max_long_requests_per_owner`)
/// for as long as the request runs. It is taken before the body is read, so an owner
/// at the cap is refused (429) before an upload is buffered. Without sessions there is
/// no slot; the handler then answers 503 as before.
pub(crate) struct BusyOwner {
    pub owner: String,
    _slot: Option<OwnerSlot>,
}
impl FromRequestParts<AppState> for BusyOwner {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let Owner(owner) = Owner::from_request_parts(parts, state).await?;
        let slot = match &state.sessions {
            Some(sessions) => Some(sessions.long_request_slot(&owner).map_err(lookup_error)?),
            None => None,
        };
        Ok(Self { owner, _slot: slot })
    }
}

/// `Json` whose rejections are `{"error": ...}` like every other API error.
pub(crate) struct JsonBody<T>(pub T);
impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for JsonBody<T> {
    type Rejection = ApiError;
    async fn from_request(req: Request, state: &S) -> Result<Self, ApiError> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(Self(value)),
            Err(rejection) => Err(ApiError(rejection.status(), rejection.body_text())),
        }
    }
}

/// A JSON body that may be empty.
fn optional_json<T: DeserializeOwned + Default>(bytes: &Bytes) -> Result<T, ApiError> {
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(T::default());
    }
    serde_json::from_slice(bytes).map_err(|e| {
        ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("Invalid JSON: {e}"),
        )
    })
}

pub(crate) fn engine(s: &AppState) -> Result<&Sessions, ApiError> {
    s.sessions.as_ref().ok_or_else(|| {
        ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Agent sessions are not configured on this deployment: connect a worker (AR_WORKER_URL)"
                .into(),
        )
    })
}

fn find_session_error(e: &anyhow::Error) -> Option<&SessionError> {
    e.chain().find_map(|c| c.downcast_ref::<SessionError>())
}
fn session_status(e: &SessionError) -> ApiError {
    match e {
        SessionError::NotFound => ApiError(StatusCode::NOT_FOUND, "Session not found".into()),
        SessionError::Closed => ApiError(StatusCode::CONFLICT, "Session is closed".into()),
        SessionError::TooManySessions
        | SessionError::TooManyStreams
        | SessionError::TooManyRequests => ApiError(StatusCode::TOO_MANY_REQUESTS, e.to_string()),
        SessionError::InvalidModel(message) => ApiError(StatusCode::BAD_REQUEST, message.clone()),
    }
}
/// For calls whose other failures are caused by the request (validation): 400.
pub(crate) fn input_error(e: anyhow::Error) -> ApiError {
    match find_session_error(&e) {
        Some(se) => session_status(se),
        None => ApiError(StatusCode::BAD_REQUEST, format!("{e:#}")),
    }
}
/// For calls into the workspace box: 404 for a missing path, 503 for a box that stopped (it
/// restarts on the next call), 429 for a busy one, 502 otherwise.
pub fn workspace_error(e: anyhow::Error) -> ApiError {
    if let Some(se) = find_session_error(&e) {
        return session_status(se);
    }
    let message = format!("{e:#}");
    if let Some(be) = e
        .chain()
        .find_map(|c| c.downcast_ref::<crate::boxes::BoxError>())
    {
        let status = match be {
            crate::boxes::BoxError::NoSuchPath(_) => StatusCode::NOT_FOUND,
            crate::boxes::BoxError::Invalid(_) => StatusCode::BAD_REQUEST,
            crate::boxes::BoxError::TooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
            crate::boxes::BoxError::Capacity | crate::boxes::BoxError::Unavailable(_) => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            crate::boxes::BoxError::Busy(_) => StatusCode::TOO_MANY_REQUESTS,
            _ => StatusCode::BAD_GATEWAY,
        };
        return ApiError(status, format!("Workspace: {message}"));
    }
    let lower = message.to_ascii_lowercase();
    if lower.contains("no such file") || lower.contains("not found") {
        return ApiError(StatusCode::NOT_FOUND, format!("Workspace: {message}"));
    }
    ApiError(
        StatusCode::BAD_GATEWAY,
        format!("Workspace error: {message}"),
    )
}
/// For engine reads that only fail on a missing session or a storage fault.
pub(crate) fn lookup_error(e: anyhow::Error) -> ApiError {
    match find_session_error(&e) {
        Some(se) => session_status(se),
        None => ApiError::internal(e),
    }
}

pub(crate) fn check_path(path: &str) -> Result<(), ApiError> {
    if path.is_empty() || path.len() > MAX_PATH_BYTES || path.contains('\0') {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "path must be 1-4096 bytes without NUL".into(),
        ));
    }
    Ok(())
}
pub(crate) fn check_exec(req: &ExecRequest) -> Result<(), ApiError> {
    let bad = |m: &str| Err(ApiError(StatusCode::BAD_REQUEST, m.into()));
    if req.command.trim().is_empty() || req.command.len() > 64 * 1024 {
        return bad("command must be 1-65536 bytes");
    }
    if req
        .timeout_seconds
        .is_some_and(|t| !(1..=MAX_EXEC_TIMEOUT_SECONDS).contains(&t))
    {
        return bad("timeout_seconds must be 1-600");
    }
    if req.cwd.as_deref().is_some_and(|c| check_path(c).is_err()) {
        return bad("cwd must be 1-4096 bytes without NUL");
    }
    if req
        .stdin
        .as_ref()
        .is_some_and(|s| s.len() > MAX_STDIN_BYTES)
    {
        return bad("stdin must be at most 1 MiB");
    }
    Ok(())
}

// Status, models and usage.

/// The models sessions run on: the engine's list, or the configured one without a worker.
pub(crate) fn catalog(s: &AppState) -> ModelCatalog {
    match &s.sessions {
        Some(sessions) => sessions.models().clone(),
        None => s.settings.model_catalog(),
    }
}
/// `GET /v1/models` and the MCP `list_models` tool.
pub(crate) fn models_report(catalog: &ModelCatalog) -> Value {
    let models: Vec<Value> = catalog
        .models
        .iter()
        .map(|m| {
            json!({
                "id": m.id,
                "input_usd_per_million": m.input_usd_per_million,
                "output_usd_per_million": m.output_usd_per_million,
                "default": m.id == catalog.default,
            })
        })
        .collect();
    json!({
        "provider": catalog.provider,
        "default": catalog.default,
        "per_session": catalog.per_session,
        "models": models,
    })
}
/// `GET /v1/usage` and the MCP `get_usage` tool: the owner's spend and tokens in total,
/// per model and per session (the 200 most recently active), from what each session
/// recorded. Deleted sessions no longer count; closed ones do.
pub(crate) fn usage_report(ledger: &Ledger, owner: &str) -> anyhow::Result<Value> {
    let by_model = ledger.usage_by_model(owner)?;
    let mut total = ModelUsage::default();
    for m in &by_model {
        total.sessions += m.sessions;
        total.spend_usd += m.spend_usd;
        total.input_tokens += m.input_tokens;
        total.output_tokens += m.output_tokens;
        total.model_calls += m.model_calls;
    }
    let total = json!({
        "sessions": total.sessions,
        "spend_usd": total.spend_usd,
        "input_tokens": total.input_tokens,
        "output_tokens": total.output_tokens,
        "model_calls": total.model_calls,
    });
    let sessions: Vec<Value> = ledger
        .agent_sessions(owner)?
        .into_iter()
        .map(|v| {
            json!({
                "id": v.id,
                "title": v.title,
                "status": v.status,
                "model": v.model,
                "forked_from": v.forked_from,
                "spend_usd": v.spend_usd,
                "input_tokens": v.input_tokens,
                "output_tokens": v.output_tokens,
                "model_calls": v.model_calls,
                "created_at": v.created_at,
            })
        })
        .collect();
    Ok(json!({"total": total, "by_model": by_model, "sessions": sessions}))
}
async fn list_models(State(s): State<AppState>, Owner(_): Owner) -> Json<Value> {
    Json(models_report(&catalog(&s)))
}
async fn usage(State(s): State<AppState>, Owner(owner): Owner) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        usage_report(&s.ledger, &owner).map_err(ApiError::internal)?,
    ))
}

async fn status(State(s): State<AppState>, Owner(_): Owner) -> Json<Value> {
    let worker = s.worker_connected().await;
    let limits = s.sessions.as_ref().map(|e| e.limits()).unwrap_or_default();
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "provider": s.settings.inference_provider.name(),
        "inference_configured": s.settings.inference_configured(),
        "worker_connected": worker,
        "sessions_available": s.sessions.is_some(),
        "voice_available": s.settings.voice_url.is_some(),
        "mcp_servers": s.mcp.names(),
        "models": models_report(&catalog(&s)),
        "limits": {
            "max_turn_seconds": limits.max_turn_seconds,
            "box_memory_mb": limits.box_memory_mb,
            "box_cpu_millis": limits.box_cpu_millis,
            "max_message_bytes": MAX_MESSAGE_BYTES,
            "max_file_bytes": MAX_FILE_BYTES,
            "max_exec_timeout_seconds": MAX_EXEC_TIMEOUT_SECONDS,
            "max_event_page": MAX_EVENT_PAGE,
            "max_audio_bytes": MAX_AUDIO_BYTES,
            "max_open_sessions_per_owner": limits.max_open_sessions_per_owner,
            "max_streams_per_owner": limits.max_streams_per_owner,
            "max_long_requests_per_owner": limits.max_long_requests_per_owner,
            "max_box_calls_per_session": limits.max_box_calls_per_session,
            "max_mcp_batch": limits.max_mcp_batch,
        },
    }))
}

// API keys.

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct KeyInput {
    #[serde(default)]
    name: String,
}
async fn create_key(
    State(s): State<AppState>,
    Operator(owner): Operator,
    body: Bytes,
) -> Result<impl IntoResponse, ApiError> {
    let input: KeyInput = optional_json(&body)?;
    let created = s
        .ledger
        .create_api_key(&owner, &input.name)
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok((StatusCode::CREATED, Json(json!(created))))
}
async fn list_keys(
    State(s): State<AppState>,
    Owner(owner): Owner,
) -> Result<Json<Value>, ApiError> {
    let keys = s.ledger.api_keys(&owner).map_err(ApiError::internal)?;
    Ok(Json(json!({"keys": keys})))
}
async fn revoke_key(
    State(s): State<AppState>,
    Operator(owner): Operator,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    if !s
        .ledger
        .revoke_api_key(&owner, &id)
        .map_err(ApiError::internal)?
    {
        return Err(ApiError(StatusCode::NOT_FOUND, "Key not found".into()));
    }
    Ok(Json(json!({"revoked": true})))
}

// Sessions.

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct NewSession {
    #[serde(default)]
    title: Option<String>,
    /// Optional first message; starts the first turn.
    #[serde(default)]
    message: Option<String>,
    /// Optional model from `GET /v1/models`; the default model otherwise.
    #[serde(default)]
    model: Option<String>,
}
fn check_message(message: Option<&str>) -> Result<(), ApiError> {
    if let Some(text) = message
        && (text.trim().is_empty() || text.len() > MAX_MESSAGE_BYTES)
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "message must be 1-65536 bytes".into(),
        ));
    }
    Ok(())
}
async fn create_session(
    State(s): State<AppState>,
    Owner(owner): Owner,
    body: Bytes,
) -> Result<impl IntoResponse, ApiError> {
    let engine = engine(&s)?;
    let input: NewSession = optional_json(&body)?;
    check_message(input.message.as_deref())?;
    let session = engine
        .create_with_model(&owner, input.title.as_deref(), input.model.as_deref())
        .map_err(input_error)?;
    let sent = match &input.message {
        Some(text) => Some(
            engine
                .send(&owner, &session.id, text)
                .map_err(input_error)?,
        ),
        None => None,
    };
    let session = engine
        .get(&owner, &session.id)
        .map_err(ApiError::internal)?
        .unwrap_or(session);
    Ok((
        StatusCode::CREATED,
        Json(json!({"session": session, "sent": sent})),
    ))
}
async fn list_sessions(
    State(s): State<AppState>,
    Owner(owner): Owner,
) -> Result<Json<Value>, ApiError> {
    let sessions = engine(&s)?.list(&owner).map_err(ApiError::internal)?;
    Ok(Json(json!({"sessions": sessions})))
}
async fn get_session(
    State(s): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let session = engine(&s)?
        .get(&owner, &id)
        .map_err(ApiError::internal)?
        .ok_or_else(|| session_status(&SessionError::NotFound))?;
    Ok(Json(json!(session)))
}
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ForkInput {
    #[serde(default)]
    title: Option<String>,
    /// The fork's model; the source's model otherwise.
    #[serde(default)]
    model: Option<String>,
    /// Optional first message for the fork; starts its first turn.
    #[serde(default)]
    message: Option<String>,
}
/// Forks a session: a new session with a copy of its workspace and its conversation.
async fn fork_session(
    State(s): State<AppState>,
    BusyOwner { owner, _slot }: BusyOwner,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<impl IntoResponse, ApiError> {
    let engine = engine(&s)?;
    let input: ForkInput = optional_json(&body)?;
    check_message(input.message.as_deref())?;
    let fork = engine
        .fork(&owner, &id, input.title.as_deref(), input.model.as_deref())
        .await
        .map_err(workspace_error)?;
    let sent = match &input.message {
        Some(text) => Some(engine.send(&owner, &fork.id, text).map_err(input_error)?),
        None => None,
    };
    let session = engine
        .get(&owner, &fork.id)
        .map_err(ApiError::internal)?
        .unwrap_or(fork);
    Ok((
        StatusCode::CREATED,
        Json(json!({"session": session, "sent": sent})),
    ))
}
/// Unknown parameters are ignored, as they were before `purge` existed.
#[derive(Deserialize)]
struct CloseQuery {
    #[serde(default)]
    purge: Option<bool>,
}
/// Closes the session (box destroyed, history kept); with `?purge=true` deletes it for
/// good: the session, its events and queued messages.
async fn close_session(
    State(s): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
    Query(q): Query<CloseQuery>,
) -> Result<Json<Value>, ApiError> {
    let sessions = engine(&s)?;
    if q.purge == Some(true) {
        sessions.delete(&owner, &id).await.map_err(lookup_error)?;
        return Ok(Json(json!({"closed": true, "deleted": true})));
    }
    sessions.close(&owner, &id).await.map_err(lookup_error)?;
    Ok(Json(json!({"closed": true})))
}
/// Deletes every closed session of the caller for good.
async fn purge_closed_sessions(
    State(s): State<AppState>,
    Owner(owner): Owner,
) -> Result<Json<Value>, ApiError> {
    let deleted = engine(&s)?
        .delete_closed(&owner)
        .map_err(ApiError::internal)?;
    Ok(Json(json!({"deleted": deleted})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MessageInput {
    text: String,
}
async fn send_message(
    State(s): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
    JsonBody(input): JsonBody<MessageInput>,
) -> Result<impl IntoResponse, ApiError> {
    let outcome = engine(&s)?
        .send(&owner, &id, &input.text)
        .map_err(input_error)?;
    Ok((StatusCode::ACCEPTED, Json(json!(outcome))))
}
async fn cancel_session(
    State(s): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    engine(&s)?.cancel(&owner, &id).map_err(lookup_error)?;
    Ok(Json(json!({"cancelled": true})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EventsQuery {
    #[serde(default)]
    after: Option<i64>,
    #[serde(default)]
    limit: Option<usize>,
}
async fn list_events(
    State(s): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
    Query(q): Query<EventsQuery>,
) -> Result<Json<Value>, ApiError> {
    let after = q.after.unwrap_or(0).max(0);
    let limit = q.limit.unwrap_or(200).clamp(1, MAX_EVENT_PAGE);
    let events = engine(&s)?
        .events(&owner, &id, after, limit)
        .map_err(lookup_error)?;
    let next_after = events.last().map_or(after, |e| e.seq);
    Ok(Json(
        json!({"events": events, "next_after": next_after, "more": events.len() == limit}),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StreamQuery {
    #[serde(default)]
    after: Option<i64>,
}
struct StreamState {
    sessions: Sessions,
    owner: String,
    id: String,
    last: i64,
    rx: broadcast::Receiver<SessionEvent>,
    backlog: VecDeque<SessionEvent>,
    need_replay: bool,
    /// The session was closed when the client connected: end once the replay is done.
    closed_at_start: bool,
    done: bool,
}
fn sse_event(event: &SessionEvent) -> Event {
    Event::default()
        .id(event.seq.to_string())
        .event(event.kind.clone())
        .data(serde_json::to_string(event).unwrap_or_else(|_| "{}".into()))
}
/// Replays stored events after the cursor, then streams live ones. The SSE `id` is the
/// event's `seq`, so a reconnect with `Last-Event-ID` resumes without gaps or repeats.
pub(crate) fn event_stream(
    sessions: Sessions,
    owner: String,
    id: String,
    after: i64,
    closed_at_start: bool,
) -> impl Stream<Item = Result<Event, Infallible>> {
    // Subscribed by the caller's construction order: before the first replay page.
    let state = StreamState {
        rx: sessions.subscribe(),
        sessions,
        owner,
        id,
        last: after,
        backlog: VecDeque::new(),
        need_replay: true,
        closed_at_start,
        done: false,
    };
    futures_util::stream::unfold(state, |mut st| async move {
        loop {
            if let Some(event) = st.backlog.pop_front() {
                if event.seq <= st.last {
                    continue;
                }
                st.last = event.seq;
                if event.kind == "status" && event.payload["status"] == "closed" {
                    st.done = true;
                }
                return Some((Ok(sse_event(&event)), st));
            }
            if st.done {
                return None;
            }
            if st.need_replay {
                match st
                    .sessions
                    .events(&st.owner, &st.id, st.last, MAX_EVENT_PAGE)
                {
                    Ok(page) => {
                        st.need_replay = page.len() == MAX_EVENT_PAGE;
                        if page.is_empty() && st.closed_at_start {
                            return None;
                        }
                        st.backlog.extend(page);
                        continue;
                    }
                    Err(_) => return None,
                }
            }
            match st.rx.recv().await {
                Ok(event) if event.session_id == st.id => {
                    if event.seq == st.last + 1 {
                        st.backlog.push_back(event);
                    } else if event.seq > st.last + 1 {
                        // A gap: fill it (and this event) from the stored log.
                        st.need_replay = true;
                    }
                }
                Ok(_) => {}
                Err(RecvError::Lagged(_)) => st.need_replay = true,
                Err(RecvError::Closed) => return None,
            }
        }
    })
}
async fn stream_events(
    State(s): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
    Query(q): Query<StreamQuery>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let sessions = engine(&s)?.clone();
    let session = sessions
        .get(&owner, &id)
        .map_err(ApiError::internal)?
        .ok_or_else(|| session_status(&SessionError::NotFound))?;
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<i64>().ok());
    let after = last_event_id.or(q.after).unwrap_or(0).max(0);
    let closed = session.status == SessionStatus::Closed;
    // Held by the stream: given back when it ends or the client disconnects.
    let slot = sessions.stream_slot(&owner).map_err(lookup_error)?;
    let stream = event_stream(sessions, owner, id, after, closed).map(move |event| {
        let _held = &slot;
        event
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(SSE_KEEPALIVE)))
}

// Files and the direct shell.

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathQuery {
    #[serde(default)]
    path: Option<String>,
}
/// Content types for downloads. Anything a browser could run (HTML, SVG, scripts) is
/// served as plain text, and every file is an attachment.
fn content_type(path: &str) -> &'static str {
    let name = path.rsplit('/').next().unwrap_or(path);
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "json" => "application/json",
        "zip" => "application/zip",
        "gz" | "tgz" => "application/gzip",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "txt" | "md" | "log" | "csv" | "tsv" | "rs" | "py" | "js" | "mjs" | "ts" | "tsx"
        | "jsx" | "html" | "htm" | "css" | "svg" | "xml" | "toml" | "yaml" | "yml" | "sh"
        | "go" | "java" | "c" | "h" | "cpp" | "rb" | "sql" | "ini" | "cfg" => {
            "text/plain; charset=utf-8"
        }
        _ => "application/octet-stream",
    }
}
fn download_name(path: &str) -> String {
    let name: String = path
        .rsplit('/')
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || "._-".contains(*c))
        .take(100)
        .collect();
    if name.is_empty() || name.chars().all(|c| c == '.') {
        "file".into()
    } else {
        name
    }
}
async fn read_file(
    State(s): State<AppState>,
    BusyOwner { owner, _slot }: BusyOwner,
    Path(id): Path<String>,
    Query(q): Query<PathQuery>,
) -> Result<Response, ApiError> {
    let path = q.path.unwrap_or_else(|| ".".into());
    check_path(&path)?;
    match engine(&s)?
        .read_file(&owner, &id, &path)
        .await
        .map_err(workspace_error)?
    {
        FileRead::Dir(entries) => {
            Ok(Json(json!({"path": path, "entries": entries})).into_response())
        }
        FileRead::File(bytes) => {
            if bytes.len() > MAX_FILE_BYTES {
                return Err(ApiError(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "File exceeds 25 MiB".into(),
                ));
            }
            Ok((
                [
                    (header::CONTENT_TYPE, content_type(&path).to_string()),
                    (
                        header::CONTENT_DISPOSITION,
                        format!("attachment; filename=\"{}\"", download_name(&path)),
                    ),
                ],
                Body::from(bytes),
            )
                .into_response())
        }
    }
}
/// The whole workspace as a gzip tarball (up to 25 MiB; 413 beyond).
async fn archive(
    State(s): State<AppState>,
    BusyOwner { owner, _slot }: BusyOwner,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let bytes = engine(&s)?
        .archive(&owner, &id)
        .await
        .map_err(workspace_error)?;
    let short: String = id.chars().take(8).collect();
    Ok((
        [
            (header::CONTENT_TYPE, "application/gzip".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!(
                    "attachment; filename=\"{}\"",
                    download_name(&format!("workspace-{short}.tar.gz"))
                ),
            ),
        ],
        Body::from(bytes),
    )
        .into_response())
}
async fn write_file(
    State(s): State<AppState>,
    BusyOwner { owner, _slot }: BusyOwner,
    Path(id): Path<String>,
    Query(q): Query<PathQuery>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let path = q
        .path
        .ok_or_else(|| ApiError(StatusCode::BAD_REQUEST, "path is required".into()))?;
    check_path(&path)?;
    let written = engine(&s)?
        .write_file(&owner, &id, &path, body)
        .await
        .map_err(workspace_error)?;
    Ok(Json(json!(written)))
}
async fn exec(
    State(s): State<AppState>,
    BusyOwner { owner, _slot }: BusyOwner,
    Path(id): Path<String>,
    JsonBody(req): JsonBody<ExecRequest>,
) -> Result<Json<Value>, ApiError> {
    let engine = engine(&s)?;
    check_exec(&req)?;
    let result = engine
        .exec(&owner, &id, req)
        .await
        .map_err(workspace_error)?;
    Ok(Json(json!(result)))
}

// Skills.

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillInput {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    body: Option<String>,
    /// A whole `SKILL.md` (front matter with name and description, then the body).
    #[serde(default)]
    markdown: Option<String>,
}
fn skill_parts(
    input: SkillInput,
    path_name: Option<&str>,
) -> Result<(String, String, String), ApiError> {
    let bad = |m: String| ApiError(StatusCode::BAD_REQUEST, m);
    let (name, description, body) = match input.markdown {
        Some(markdown) => parse_skill_md(&markdown).map_err(|e| bad(e.to_string()))?,
        None => (
            input
                .name
                .or(path_name.map(String::from))
                .unwrap_or_default(),
            input.description.unwrap_or_default(),
            input.body.unwrap_or_default(),
        ),
    };
    if let Some(path_name) = path_name
        && name != path_name
    {
        return Err(bad("the skill name must match the URL".into()));
    }
    Ok((name, description, body))
}
async fn list_skills(
    State(s): State<AppState>,
    Owner(owner): Owner,
) -> Result<Json<Value>, ApiError> {
    let skills: Vec<Value> = s
        .ledger
        .skills(&owner)
        .map_err(ApiError::internal)?
        .into_iter()
        .map(|k| json!({"name": k.name, "description": k.description, "updated_at": k.updated_at}))
        .collect();
    let builtin: Vec<Value> = builtin_skills()
        .into_iter()
        .map(|k| json!({"name": k.name, "description": k.description}))
        .collect();
    Ok(Json(json!({"skills": skills, "builtin": builtin})))
}
async fn create_skill(
    State(s): State<AppState>,
    Owner(owner): Owner,
    JsonBody(input): JsonBody<SkillInput>,
) -> Result<impl IntoResponse, ApiError> {
    let (name, description, body) = skill_parts(input, None)?;
    let skill = s
        .ledger
        .save_skill(&owner, &name, &description, &body)
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok((StatusCode::CREATED, Json(json!(skill))))
}
async fn get_skill(
    State(s): State<AppState>,
    Owner(owner): Owner,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    if let Some(skill) = s.ledger.skill(&owner, &name).map_err(ApiError::internal)? {
        return Ok(Json(
            json!({"name": skill.name, "description": skill.description, "body": skill.body, "updated_at": skill.updated_at, "builtin": false, "markdown": skill.to_markdown()}),
        ));
    }
    let skill = builtin_skills()
        .into_iter()
        .find(|k| k.name == name)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "Skill not found".into()))?;
    Ok(Json(
        json!({"name": skill.name, "description": skill.description, "body": skill.body, "builtin": true, "markdown": skill.to_markdown()}),
    ))
}
async fn put_skill(
    State(s): State<AppState>,
    Owner(owner): Owner,
    Path(name): Path<String>,
    JsonBody(input): JsonBody<SkillInput>,
) -> Result<Json<Value>, ApiError> {
    let (name, description, body) = skill_parts(input, Some(&name))?;
    let skill = s
        .ledger
        .save_skill(&owner, &name, &description, &body)
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(json!(skill)))
}
async fn delete_skill(
    State(s): State<AppState>,
    Owner(owner): Owner,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    if !s
        .ledger
        .delete_skill(&owner, &name)
        .map_err(ApiError::internal)?
    {
        return Err(ApiError(StatusCode::NOT_FOUND, "Skill not found".into()));
    }
    Ok(Json(json!({"deleted": true})))
}

// Agent config.

async fn get_config(
    State(s): State<AppState>,
    Owner(owner): Owner,
) -> Result<Json<Value>, ApiError> {
    let (version, config) = s.ledger.agent_config(&owner).map_err(ApiError::internal)?;
    Ok(Json(
        json!({"version": version, "config": config, "builtin_tools": BUILTIN_TOOLS, "mcp_servers_available": s.mcp.names()}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigInput {
    config: AgentConfig,
    #[serde(default)]
    reason: Option<String>,
}
async fn put_config(
    State(s): State<AppState>,
    Owner(owner): Owner,
    JsonBody(input): JsonBody<ConfigInput>,
) -> Result<Json<Value>, ApiError> {
    // `update_agent_config` does not know the operator's MCP allowlist; check it here.
    input
        .config
        .validate(BUILTIN_TOOLS, &s.mcp.names())
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
    let reason = input
        .reason
        .filter(|r| !r.trim().is_empty())
        .unwrap_or_else(|| "updated through the API".into());
    let version = s
        .ledger
        .update_agent_config(&owner, &input.config, &reason, "user")
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(json!({"version": version, "config": input.config})))
}
async fn config_history(
    State(s): State<AppState>,
    Owner(owner): Owner,
) -> Result<Json<Value>, ApiError> {
    let history: Vec<Value> = s
        .ledger
        .agent_config_history(&owner)
        .map_err(ApiError::internal)?
        .into_iter()
        .map(|(version, reason, author, created_at)| {
            json!({"version": version, "reason": reason, "author": author, "created_at": created_at})
        })
        .collect();
    Ok(Json(json!({"history": history})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RevertInput {
    version: i64,
}
async fn revert_config(
    State(s): State<AppState>,
    Owner(owner): Owner,
    JsonBody(input): JsonBody<RevertInput>,
) -> Result<Json<Value>, ApiError> {
    let config = s
        .ledger
        .agent_config_version(&owner, input.version)
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "Config version not found".into()))?;
    // The operator's MCP list may have changed since that version was saved.
    config
        .validate(BUILTIN_TOOLS, &s.mcp.names())
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
    let version = s
        .ledger
        .revert_agent_config(&owner, input.version)
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(json!({"version": version, "config": config})))
}

// Webhooks.

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WebhookInput {
    url: String,
}
async fn create_webhook(
    State(s): State<AppState>,
    Operator(owner): Operator,
    JsonBody(input): JsonBody<WebhookInput>,
) -> Result<impl IntoResponse, ApiError> {
    let (url, _) = s
        .webhook_guard
        .check(&input.url)
        .await
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
    let hook = s
        .ledger
        .create_webhook(&owner, url.as_str())
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok((StatusCode::CREATED, Json(json!(hook))))
}
async fn list_webhooks(
    State(s): State<AppState>,
    Owner(owner): Owner,
) -> Result<Json<Value>, ApiError> {
    let hooks = s.ledger.webhooks(&owner).map_err(ApiError::internal)?;
    Ok(Json(json!({"webhooks": hooks})))
}
async fn delete_webhook(
    State(s): State<AppState>,
    Operator(owner): Operator,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    if !s
        .ledger
        .delete_webhook(&owner, &id)
        .map_err(ApiError::internal)?
    {
        return Err(ApiError(StatusCode::NOT_FOUND, "Webhook not found".into()));
    }
    Ok(Json(json!({"deleted": true})))
}

// Voice proxy (optional local service at AR_VOICE_URL).

async fn transcribe(
    State(s): State<AppState>,
    Owner(_): Owner,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    voice_proxy(
        &s,
        "transcribe",
        &headers,
        body,
        MAX_AUDIO_BYTES,
        MAX_TRANSCRIPT_BYTES,
    )
    .await
}
async fn speak(
    State(s): State<AppState>,
    Owner(_): Owner,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    voice_proxy(
        &s,
        "speak",
        &headers,
        body,
        MAX_SPEAK_REQUEST_BYTES,
        MAX_AUDIO_BYTES,
    )
    .await
}
/// Streams the request to the voice service and its answer back, both capped.
async fn voice_proxy(
    s: &AppState,
    path: &str,
    headers: &HeaderMap,
    body: Body,
    max_request: usize,
    max_response: usize,
) -> Result<Response, ApiError> {
    let Some(base) = &s.settings.voice_url else {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            "Voice is not configured on this deployment".into(),
        ));
    };
    let too_large = || {
        ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "Request body too large".into(),
        )
    };
    if headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok())
        .is_some_and(|n| n > max_request)
    {
        return Err(too_large());
    }
    let limited = Body::new(http_body_util::Limited::new(body, max_request));
    let mut request = s
        .voice_http
        .post(format!("{base}/{path}"))
        .body(reqwest::Body::wrap_stream(limited.into_data_stream()));
    if let Some(ct) = headers.get(header::CONTENT_TYPE) {
        request = request.header(header::CONTENT_TYPE, ct.clone());
    }
    let response = request.send().await.map_err(|e| {
        tracing::warn!("voice service request failed: {}", e.without_url());
        ApiError(
            StatusCode::BAD_GATEWAY,
            "The voice service did not answer, or the request was too large".into(),
        )
    })?;
    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        let message = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| {
                v["error"]
                    .as_str()
                    .or(v["detail"].as_str())
                    .map(String::from)
            })
            .unwrap_or_else(|| format!("voice service returned {}", status.as_u16()));
        let code = if status.is_client_error() {
            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_REQUEST)
        } else {
            StatusCode::BAD_GATEWAY
        };
        return Err(ApiError(
            code,
            crate::agent::clip(&message, 500).to_string(),
        ));
    }
    if response
        .content_length()
        .is_some_and(|n| n > max_response as u64)
    {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            "The voice service answer is too large".into(),
        ));
    }
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .filter(|v| v.starts_with("audio/") || v.starts_with("application/json"))
        .unwrap_or("application/octet-stream")
        .to_string();
    let mut seen = 0usize;
    let stream = response.bytes_stream().map(move |chunk| {
        let chunk = chunk.map_err(std::io::Error::other)?;
        seen += chunk.len();
        if seen > max_response {
            return Err(std::io::Error::other("voice answer too large"));
        }
        Ok(chunk)
    });
    Ok((
        [(header::CONTENT_TYPE, content_type)],
        Body::from_stream(stream),
    )
        .into_response())
}
