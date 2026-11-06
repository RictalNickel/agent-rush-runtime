use crate::{
    auth, config::Settings, domain::JobRequest, ledger::Ledger, mcp_client::McpRegistry,
    session::Sessions, webhooks::UrlGuard,
};
use axum::{
    Json, Router,
    body::Body,
    extract::{ConnectInfo, DefaultBodyLimit, FromRequestParts, Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header, request::Parts},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{delete, get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    convert::Infallible,
    net::{IpAddr, Ipv6Addr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct AppState {
    pub settings: Arc<Settings>,
    pub ledger: Arc<Ledger>,
    login_attempts: Arc<Mutex<LoginLimiter>>,
    pub http: reqwest::Client,
    worker_seen: Arc<tokio::sync::Mutex<Option<(Instant, bool)>>>,
    /// The agent session engine; None when no worker is configured (`/v1/sessions` → 503).
    pub sessions: Option<Sessions>,
    /// Operator-configured remote MCP servers (the agent config's MCP allowlist).
    pub mcp: Arc<McpRegistry>,
    /// Which webhook destinations are allowed (public addresses only in production).
    pub webhook_guard: UrlGuard,
    /// Client for the optional local voice service: no worker identity, no redirects.
    pub(crate) voice_http: reqwest::Client,
}
/// How long the public status endpoint reuses one worker health check.
const WORKER_STATUS_TTL: Duration = Duration::from_secs(5);
impl AppState {
    pub fn new(settings: Settings, ledger: Ledger) -> anyhow::Result<Self> {
        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .redirect(reqwest::redirect::Policy::none());
        if let Some(path) = &settings.worker_ca_path {
            builder = builder
                .add_root_certificate(reqwest::Certificate::from_pem(&std::fs::read(path)?)?);
        }
        if let Some(path) = &settings.worker_identity_path {
            builder = builder.identity(reqwest::Identity::from_pem(&std::fs::read(path)?)?);
        }
        let http = builder.build()?;
        Ok(Self {
            settings: Arc::new(settings),
            ledger: Arc::new(ledger),
            login_attempts: Arc::default(),
            http,
            worker_seen: Arc::new(tokio::sync::Mutex::new(None)),
            sessions: None,
            mcp: Arc::new(McpRegistry::empty()),
            webhook_guard: UrlGuard::public(),
            voice_http: reqwest::Client::builder()
                .timeout(Duration::from_secs(300))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
        })
    }
    /// The owner of a bearer API key (`Authorization: Bearer ar_...`). `Ok(None)` when no
    /// bearer credential was sent; an unknown or revoked key is 401.
    pub fn bearer_owner(&self, headers: &HeaderMap) -> Result<Option<String>, ApiError> {
        let Some(value) = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
        else {
            return Ok(None);
        };
        let Some(key) = value
            .strip_prefix("Bearer ")
            .or_else(|| value.strip_prefix("bearer "))
        else {
            return Ok(None);
        };
        match self
            .ledger
            .api_key_owner(key.trim())
            .map_err(ApiError::internal)?
        {
            Some(owner) => Ok(Some(owner)),
            None => Err(ApiError(
                StatusCode::UNAUTHORIZED,
                "Invalid or revoked API key".into(),
            )),
        }
    }
    /// A bearer API key, or the operator cookie (with the CSRF header for mutations).
    pub fn owner(&self, headers: &HeaderMap, mutation: bool) -> Result<String, ApiError> {
        if let Some(owner) = self.bearer_owner(headers)? {
            return Ok(owner);
        }
        let token = auth::session_token(headers).ok_or_else(ApiError::unauthorized)?;
        let session = self
            .ledger
            .session(&auth::digest(&token))
            .map_err(ApiError::internal)?
            .ok_or_else(ApiError::unauthorized)?;
        if mutation
            && !headers
                .get("x-csrf-token")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| auth::same(v, &session.1))
        {
            return Err(ApiError(StatusCode::FORBIDDEN, "Invalid CSRF token".into()));
        }
        Ok(session.0)
    }
}
/// Failed logins one client address may make in a minute.
pub const LOGIN_FAILURES_PER_ADDRESS: usize = 10;
/// Failed logins from all addresses together in a minute, so that many addresses still
/// cannot brute force the password. Well above the per-address limit, so one client
/// cannot lock the operator out.
pub const LOGIN_FAILURES_GLOBAL: usize = 100;
const LOGIN_WINDOW: Duration = Duration::from_secs(60);

/// Counts failed logins per client address and in total over the last minute.
#[derive(Default)]
struct LoginLimiter {
    by_address: HashMap<IpAddr, Vec<Instant>>,
    all: Vec<Instant>,
}
impl LoginLimiter {
    /// Reserves an attempt for `address`, or refuses when that address or all addresses
    /// together are at their limit. Reserving before the password check means parallel
    /// requests cannot overshoot; `release` gives a successful login's slot back, so
    /// only failures count.
    fn reserve(&mut self, address: IpAddr) -> Option<Instant> {
        self.all.retain(|at| at.elapsed() < LOGIN_WINDOW);
        self.by_address.retain(|_, attempts| {
            attempts.retain(|at| at.elapsed() < LOGIN_WINDOW);
            !attempts.is_empty()
        });
        let mine = self.by_address.get(&address).map_or(0, Vec::len);
        if mine >= LOGIN_FAILURES_PER_ADDRESS || self.all.len() >= LOGIN_FAILURES_GLOBAL {
            return None;
        }
        let now = Instant::now();
        self.by_address.entry(address).or_default().push(now);
        self.all.push(now);
        Some(now)
    }
    fn release(&mut self, address: IpAddr, at: Instant) {
        if let Some(attempts) = self.by_address.get_mut(&address) {
            if let Some(i) = attempts.iter().rposition(|a| *a == at) {
                attempts.remove(i);
            }
            if attempts.is_empty() {
                self.by_address.remove(&address);
            }
        }
        if let Some(i) = self.all.iter().rposition(|a| *a == at) {
            self.all.remove(i);
        }
    }
}

/// The socket peer of a request, when the server records it (`main` serves with
/// `into_make_service_with_connect_info`). In-process tests may have none.
pub struct Peer(pub Option<SocketAddr>);
impl<S: Send + Sync> FromRequestParts<S> for Peer {
    type Rejection = Infallible;
    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Infallible> {
        Ok(Self(
            parts
                .extensions
                .get::<ConnectInfo<SocketAddr>>()
                .map(|info| info.0),
        ))
    }
}

/// The client address a login is counted against. Caddy on the same host proxies every
/// public request, so a loopback peer means "behind Caddy": then the address is the last
/// `X-Forwarded-For` entry, the one Caddy wrote from the connection it accepted itself
/// (earlier entries come from the client and are ignored). Any other peer is the client,
/// and its `X-Forwarded-For` is ignored, since it could send anything there. Without a
/// peer every request counts as one unspecified address.
pub fn client_address(peer: Option<SocketAddr>, headers: &HeaderMap) -> IpAddr {
    let Some(peer) = peer else {
        return IpAddr::from([0u8; 4]);
    };
    let peer_ip = peer.ip().to_canonical();
    if !peer_ip.is_loopback() {
        return peer_ip;
    }
    headers
        .get_all("x-forwarded-for")
        .iter()
        .next_back()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.rsplit(',').next())
        .and_then(|last| last.trim().parse::<IpAddr>().ok())
        .map_or(peer_ip, |ip| ip.to_canonical())
}

/// The limiter's key: the IPv4 address, or the /64 of an IPv6 one, since a single IPv6
/// host usually holds a whole /64 and could otherwise rotate addresses.
fn login_bucket(address: IpAddr) -> IpAddr {
    match address.to_canonical() {
        IpAddr::V6(v6) => {
            let s = v6.segments();
            IpAddr::V6(Ipv6Addr::new(s[0], s[1], s[2], s[3], 0, 0, 0, 0))
        }
        v4 => v4,
    }
}

pub struct ApiError(pub StatusCode, pub String);
impl ApiError {
    pub(crate) fn unauthorized() -> Self {
        Self(StatusCode::UNAUTHORIZED, "Operator login required".into())
    }
    pub(crate) fn internal(e: impl std::fmt::Display) -> Self {
        tracing::warn!(error=%e,"request failed");
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Operation failed; see operator logs".into(),
        )
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error":self.1}))).into_response()
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/assets/app.css", get(css))
        .route("/assets/app.js", get(js))
        .route("/assets/archivo.woff2", get(font))
        .route(
            "/healthz",
            get(|| async {
                Json(json!({"status":"ok","service":"control","version":env!("CARGO_PKG_VERSION")}))
            }),
        )
        .route("/api/status", get(status))
        .route("/api/login", post(login))
        .route("/api/session", get(session))
        .route("/api/logout", post(logout))
        .route("/api/jobs", get(jobs).post(create_job))
        .route("/api/jobs/{id}", get(job))
        .route("/api/jobs/{id}/cancel", post(cancel))
        .route("/api/jobs/{id}/evidence", get(evidence))
        .route("/api/jobs/{id}/contexts", get(contexts))
        .route("/api/memories", get(memories).post(save_memory))
        .route("/api/memories/{id}", delete(delete_memory))
        .route("/api/jobs/{id}/artifacts/{name}", get(artifact))
        .layer(DefaultBodyLimit::max(32768))
        // Added after the 32 KiB default so their own per-route body limits apply.
        .merge(crate::api_v1::routes())
        .merge(crate::mcp_server::routes())
        .layer(middleware::from_fn(security_headers))
        .with_state(state)
}
async fn security_headers(req: axum::extract::Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    for (name, value) in [
        (
            "content-security-policy",
            "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data: blob:; media-src 'self' blob:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'",
        ),
        ("x-content-type-options", "nosniff"),
        ("referrer-policy", "no-referrer"),
        ("cache-control", "no-store"),
        (
            "permissions-policy",
            "camera=(), microphone=(self), geolocation=()",
        ),
        ("x-frame-options", "DENY"),
    ] {
        // A handler may opt into caching (the font); every other response stays no-store.
        if name == "cache-control" && response.headers().contains_key(header::CACHE_CONTROL) {
            continue;
        }
        response
            .headers_mut()
            .insert(name, HeaderValue::from_static(value));
    }
    response
}
async fn index() -> Html<&'static str> {
    Html(include_str!("../web/index.html"))
}
async fn css() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../web/app.css"),
    )
}
async fn js() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../web/app.js"),
    )
}
/// Archivo (SIL OFL 1.1, web/fonts/OFL.txt), self-hosted because the CSP allows no
/// third-party origins. It is the only cacheable response.
async fn font() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "font/woff2"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        include_bytes!("../web/fonts/archivo.woff2").as_slice(),
    )
}
impl AppState {
    /// Worker reachability for the public status page. Cached so anonymous polling cannot
    /// drive private-worker traffic; concurrent requests share one in-flight check.
    pub(crate) async fn worker_connected(&self) -> bool {
        let Some(url) = &self.settings.worker_url else {
            return false;
        };
        let mut seen = self.worker_seen.lock().await;
        if let Some((at, connected)) = *seen
            && at.elapsed() < WORKER_STATUS_TTL
        {
            return connected;
        }
        let connected = self
            .http
            .get(format!("{url}/v1/health"))
            .bearer_auth(&self.settings.worker_token)
            .send()
            .await
            .ok()
            .is_some_and(|r| r.status().is_success());
        *seen = Some((Instant::now(), connected));
        connected
    }
}
async fn status(State(s): State<AppState>) -> Json<Value> {
    let connected = s.worker_connected().await;
    Json(
        json!({"version":env!("CARGO_PKG_VERSION"),"inference_configured":s.settings.inference_configured(),"inference_provider":s.settings.inference_provider.name(),"agent_roles":agent_roles(&s.settings),"worker_connected":connected,"runtime":"runsc","profiles":[{"id":"cli","available":connected && s.settings.inference_configured()},{"id":"nested-dev","available":false,"reason":"Not qualified"},{"id":"browser","available":false,"reason":"Not qualified"}],"reserved_memory_mb":s.ledger.reserved_memory().unwrap_or(0),"inference_accounted_usd":s.ledger.spend_total().unwrap_or(0.0),"cloud_status":"unverified","resources":{"controller_admission_capacity_mb":s.ledger.capacity_mb(),"max_concurrent_jobs":crate::resources::MAX_CONCURRENT,"cpu_scheduling":"equal weights, shared ceilings","memory_growth":"live within each run, from 256 MiB up to the orchestrator's ceiling","hard_maximum":{"memory_mb":crate::domain::MAX_MEMORY_MB,"cpu_millis":crate::engine::CPU_CEILING_MILLIS,"time_seconds":crate::domain::MAX_TIMEOUT_SECONDS},"utilization":"not measured by this endpoint"}}),
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Login {
    password: String,
}
async fn login(
    State(s): State<AppState>,
    Peer(peer): Peer,
    headers: HeaderMap,
    Json(input): Json<Login>,
) -> Result<Response, ApiError> {
    let bucket = login_bucket(client_address(peer, &headers));
    let Some(reserved) = s.login_attempts.lock().unwrap().reserve(bucket) else {
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many attempts; try again in one minute".into(),
        ));
    };
    if s.settings.operator_password.len() < 16
        || !auth::same(&input.password, &s.settings.operator_password)
    {
        return Err(ApiError::unauthorized());
    }
    // Only failures count.
    s.login_attempts.lock().unwrap().release(bucket, reserved);
    let token = auth::token();
    let csrf = auth::token();
    s.ledger
        .add_session(&auth::digest(&token), &csrf)
        .map_err(ApiError::internal)?;
    let cookie = format!(
        "ar_session={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age=3600{}",
        if s.settings.secure_cookies {
            "; Secure"
        } else {
            ""
        }
    );
    Ok((
        [(header::SET_COOKIE, cookie)],
        Json(json!({"owner":"operator","csrf":csrf})),
    )
        .into_response())
}
async fn session(State(s): State<AppState>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    let owner = s.owner(&headers, false)?;
    let token = auth::session_token(&headers).ok_or_else(ApiError::unauthorized)?;
    let (_, csrf) = s
        .ledger
        .session(&auth::digest(&token))
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::unauthorized)?;
    Ok(Json(json!({"owner":owner,"csrf":csrf})))
}
async fn logout(State(s): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    s.owner(&headers, true)?;
    if let Some(token) = auth::session_token(&headers) {
        s.ledger
            .remove_session(&auth::digest(&token))
            .map_err(ApiError::internal)?;
    }
    Ok((
        [(
            header::SET_COOKIE,
            "ar_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0",
        )],
        Json(json!({"ok":true})),
    )
        .into_response())
}
async fn jobs(State(s): State<AppState>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    let owner = s.owner(&headers, false)?;
    Ok(Json(json!(
        s.ledger.list_jobs(&owner).map_err(ApiError::internal)?
    )))
}
async fn create_job(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<JobRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let owner = s.owner(&headers, true)?;
    input
        .validate()
        .map_err(|e| ApiError(StatusCode::UNPROCESSABLE_ENTITY, e))?;
    // A lease larger than the deployment can ever admit would wait in the queue forever.
    if input.memory_mb > s.ledger.capacity_mb() {
        return Err(ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!(
                "This deployment admits at most {} MiB per task",
                s.ledger.capacity_mb()
            ),
        ));
    }
    let key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let job = s
        .ledger
        .create_job(&owner, input, key)
        .map_err(|e| ApiError(StatusCode::CONFLICT, e.to_string()))?;
    Ok((StatusCode::CREATED, Json(public_job(job))))
}
async fn job(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let owner = s.owner(&headers, false)?;
    Ok(Json(json!(public_job(
        s.ledger
            .get_job(&owner, &id)
            .map_err(ApiError::internal)?
            .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "Run not found".into()))?
    ))))
}
async fn cancel(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let owner = s.owner(&headers, true)?;
    let j = s
        .ledger
        .cancel(&owner, &id)
        .map_err(|_| ApiError(StatusCode::NOT_FOUND, "Run not found".into()))?;
    Ok(Json(json!(public_job(j))))
}
async fn evidence(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let owner = s.owner(&headers, false)?;
    let job = public_job(
        s.ledger
            .get_job(&owner, &id)
            .map_err(ApiError::internal)?
            .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "Run not found".into()))?,
    );
    Ok((
        [(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"evidence-{}.json\"", job.id),
        )],
        Json(
            json!({"schema":"agent-rush.evidence.v1","job":job.id,"request":job.request,"state":job.state,"events":job.events,"dropped_events":job.dropped_events,"receipt":job.receipt,"disclaimer":"Worker-collected execution evidence; not independent remote attestation"}),
        ),
    ))
}
async fn contexts(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let owner = s.owner(&headers, false)?;
    s.ledger
        .get_job(&owner, &id)
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "Run not found".into()))?;
    let contexts = s.ledger.contexts(&owner, &id).map_err(ApiError::internal)?;
    Ok((
        [(
            header::CONTENT_DISPOSITION,
            "attachment; filename=\"agent-contexts.json\"",
        )],
        Json(
            json!({"schema":"agent-rush.contexts.v1","contexts":contexts,"note":"Logical agent contexts; worker receipts govern verification. Stored content is untrusted task data."}),
        ),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MemoryQuery {
    query: String,
}
async fn memories(
    State(s): State<AppState>,
    headers: HeaderMap,
    uri: axum::http::Uri,
) -> Result<Json<Value>, ApiError> {
    // Authenticate before parsing the query, so anonymous callers learn nothing about it.
    let owner = s.owner(&headers, false)?;
    let Query(query) = Query::<MemoryQuery>::try_from_uri(&uri)
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.body_text()))?;
    Ok(Json(json!(
        s.ledger
            .memory_search(&owner, &query.query)
            .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "Invalid memory query".into()))?
    )))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MemoryInput {
    title: String,
    text: String,
}
async fn save_memory(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<MemoryInput>,
) -> Result<impl IntoResponse, ApiError> {
    let owner = s.owner(&headers, true)?;
    let memory = s
        .ledger
        .memory_save(&owner, &input.title, &input.text)
        .map_err(|_| {
            ApiError(
                StatusCode::BAD_REQUEST,
                "Memory limit or input policy rejected this note".into(),
            )
        })?;
    Ok((StatusCode::CREATED, Json(json!(memory))))
}
async fn delete_memory(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let owner = s.owner(&headers, true)?;
    let deleted = s
        .ledger
        .memory_delete(&owner, &id)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "Invalid memory identity".into()))?;
    Ok(Json(json!({"deleted":deleted})))
}
fn public_job(mut job: crate::domain::Job) -> crate::domain::Job {
    if let Some(artifacts) = job
        .receipt
        .as_mut()
        .and_then(|r| r["artifacts"].as_array_mut())
    {
        for artifact in artifacts {
            if let Some(object) = artifact.as_object_mut() {
                object.remove("data_base64");
            }
        }
    }
    job
}
async fn artifact(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((id, name)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let owner = s.owner(&headers, false)?;
    let j = s
        .ledger
        .get_job(&owner, &id)
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "Run not found".into()))?;
    if name.is_empty()
        || name.len() > 100
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        || name == "."
        || name == ".."
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Invalid artifact name".into(),
        ));
    }
    let found = j
        .receipt
        .as_ref()
        .and_then(|r| r["artifacts"].as_array())
        .and_then(|a| a.iter().find(|a| a["name"] == name))
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "Artifact not found".into()))?;
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(found["data_base64"].as_str().unwrap_or(""))
        .map_err(ApiError::internal)?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}\""),
            ),
        ],
        Body::from(bytes),
    )
        .into_response())
}

/// The role table shown to operators, derived from the same mapping the gateway uses.
fn agent_roles(settings: &crate::config::Settings) -> Value {
    use crate::agent::Role;
    let anthropic = settings.inference_provider == crate::config::Provider::Anthropic;
    let describe = |role: Role| {
        let spec = crate::inference::anthropic_role(role);
        if anthropic {
            json!({"model": spec.model, "effort": spec.effort, "cache_ttl": spec.cache_ttl.unwrap_or("default"), "compaction": spec.compaction})
        } else {
            // Vultr serves one model; each role keeps its own reasoning effort.
            json!({"model": settings.inference_model, "reasoning_effort": spec.effort})
        }
    };
    json!({
        "orchestrator": describe(Role::Orchestrator),
        "coder": describe(Role::Coder),
        "worker": describe(Role::Worker),
        "reviewer": describe(Role::Reviewer),
    })
}
