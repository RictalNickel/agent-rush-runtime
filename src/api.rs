use crate::{auth, config::Settings, domain::JobRequest, ledger::Ledger};
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{delete, get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct AppState {
    pub settings: Arc<Settings>,
    pub ledger: Arc<Ledger>,
    login_attempts: Arc<Mutex<Vec<Instant>>>,
    pub http: reqwest::Client,
}
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
            login_attempts: Arc::new(Mutex::new(Vec::new())),
            http,
        })
    }
    pub fn owner(&self, headers: &HeaderMap, mutation: bool) -> Result<String, ApiError> {
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
pub struct ApiError(pub StatusCode, pub String);
impl ApiError {
    fn unauthorized() -> Self {
        Self(StatusCode::UNAUTHORIZED, "Operator login required".into())
    }
    fn internal(e: impl std::fmt::Display) -> Self {
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
        .layer(middleware::from_fn(security_headers))
        .with_state(state)
}
async fn security_headers(req: axum::extract::Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    for (name, value) in [
        (
            "content-security-policy",
            "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'",
        ),
        ("x-content-type-options", "nosniff"),
        ("referrer-policy", "no-referrer"),
        ("cache-control", "no-store"),
        (
            "permissions-policy",
            "camera=(), microphone=(), geolocation=()",
        ),
        ("x-frame-options", "DENY"),
    ] {
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
async fn status(State(s): State<AppState>) -> Json<Value> {
    let connected = if let Some(url) = &s.settings.worker_url {
        s.http
            .get(format!("{url}/v1/health"))
            .bearer_auth(&s.settings.worker_token)
            .send()
            .await
            .ok()
            .is_some_and(|r| r.status().is_success())
    } else {
        false
    };
    Json(
        json!({"version":env!("CARGO_PKG_VERSION"),"inference_configured":s.settings.inference_configured(),"inference_provider":s.settings.inference_provider.name(),"agent_roles":agent_roles(&s.settings),"worker_connected":connected,"runtime":"runsc","profiles":[{"id":"cli","available":connected && s.settings.inference_configured()},{"id":"nested-dev","available":false,"reason":"Not qualified"},{"id":"browser","available":false,"reason":"Not qualified"}],"reserved_memory_mb":s.ledger.reserved_memory().unwrap_or(0),"inference_accounted_usd":s.ledger.spend_total().unwrap_or(0.0),"cloud_status":"unverified","resources":{"controller_admission_capacity_mb":s.ledger.capacity_mb(),"max_concurrent_jobs":crate::resources::MAX_CONCURRENT,"cpu_scheduling":"equal weights, shared ceilings","memory_growth":"between sandbox attempts only","utilization":"not measured by this endpoint"}}),
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Login {
    password: String,
}
async fn login(State(s): State<AppState>, Json(input): Json<Login>) -> Result<Response, ApiError> {
    {
        let mut attempts = s.login_attempts.lock().unwrap();
        attempts.retain(|i| i.elapsed() < Duration::from_secs(60));
        if attempts.len() >= 10 {
            return Err(ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "Too many attempts; try again in one minute".into(),
            ));
        }
        attempts.push(Instant::now());
    }
    if s.settings.operator_password.len() < 16
        || !auth::same(&input.password, &s.settings.operator_password)
    {
        return Err(ApiError::unauthorized());
    }
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
    Query(query): Query<MemoryQuery>,
) -> Result<Json<Value>, ApiError> {
    let owner = s.owner(&headers, false)?;
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
    if settings.inference_provider != crate::config::Provider::Anthropic {
        return json!({"all": settings.inference_model});
    }
    let describe = |role: Role| {
        let spec = crate::inference::anthropic_role(role);
        json!({"model": spec.model, "effort": spec.effort, "cache_ttl": spec.cache_ttl.unwrap_or("default"), "compaction": spec.compaction})
    };
    json!({
        "orchestrator": describe(Role::Orchestrator),
        "coder": describe(Role::Coder),
        "worker": describe(Role::Worker),
        "reviewer": describe(Role::Reviewer),
    })
}
