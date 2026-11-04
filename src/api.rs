use crate::{auth, config::Settings, domain::JobRequest, ledger::Ledger};
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
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
        Ok(Self {
            settings: Arc::new(settings),
            ledger: Arc::new(ledger),
            login_attempts: Arc::new(Mutex::new(Vec::new())),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(3))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
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
        json!({"version":env!("CARGO_PKG_VERSION"),"inference_configured":s.settings.inference_configured(),"worker_connected":connected,"runtime":"runsc","profiles":[{"id":"cli","available":connected && s.settings.inference_configured()},{"id":"nested-dev","available":false,"reason":"Not qualified"},{"id":"browser","available":false,"reason":"Not qualified"}],"reserved_memory_mb":s.ledger.reserved_memory().unwrap_or(0),"inference_accounted_usd":s.ledger.spend_total().unwrap_or(0.0),"cloud_status":"unverified"}),
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
            json!({"schema":"agent-rush.evidence.v1","job":job.id,"request":job.request,"state":job.state,"events":job.events,"receipt":job.receipt,"disclaimer":"Worker-collected execution evidence; not independent remote attestation"}),
        ),
    ))
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
