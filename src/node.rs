use crate::{
    auth,
    boxes::{BoxConfig, BoxManager, LEASE_PREFIX},
    domain::{Limits, now},
    resources::{MAX_CONCURRENT, ResourcePolicy},
    worker::{DockerRunner, SandboxSpec},
};
use anyhow::{Result, ensure};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
};

#[derive(Clone)]
pub struct NodeState {
    pub runner: DockerRunner,
    dir: Arc<PathBuf>,
    token: Arc<String>,
    active: Arc<Mutex<HashMap<String, ActiveLease>>>,
    capacity_mb: u32,
    resources: ResourcePolicy,
    qualification_expires_at: Arc<AtomicI64>,
    draining: Arc<AtomicBool>,
    io_lock: Arc<Mutex<()>>,
    health_cache: Arc<tokio::sync::Mutex<Option<(std::time::Instant, Value)>>>,
    boxes: Option<BoxManager>,
}
/// Runtime health (a `docker info` call) is reused for this long; reservations stay live.
const HEALTH_TTL: std::time::Duration = std::time::Duration::from_secs(5);
/// One memory reservation: a one-shot run (keyed by run id) or a box (`box:<id>`).
#[derive(Clone)]
pub(crate) struct ActiveLease {
    pub(crate) memory_mb: u32,
    cancel: Arc<AtomicBool>,
}
impl ActiveLease {
    pub(crate) fn new(memory_mb: u32) -> Self {
        Self {
            memory_mb,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
}
fn is_run(key: &str) -> bool {
    !key.starts_with(LEASE_PREFIX)
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunRequest {
    pub id: String,
    pub code: String,
    pub limits: Limits,
}
#[derive(Clone, Serialize, Deserialize)]
struct Record {
    hash: String,
    state: String,
    receipt: Option<Value>,
    updated_at: String,
}
impl NodeState {
    pub fn new(
        runner: DockerRunner,
        dir: PathBuf,
        token: String,
        capacity_mb: u32,
    ) -> Result<Self> {
        ensure!(
            token.len() >= 32,
            "worker token must be at least 32 characters"
        );
        std::fs::create_dir_all(&dir)?;
        let resources = ResourcePolicy::detect(capacity_mb)?;
        Ok(Self {
            runner,
            dir: Arc::new(dir),
            token: Arc::new(token),
            active: Arc::new(Mutex::new(HashMap::new())),
            capacity_mb: resources.usable_memory_mb,
            resources,
            qualification_expires_at: Arc::new(AtomicI64::new(0)),
            draining: Arc::new(AtomicBool::new(false)),
            io_lock: Arc::new(Mutex::new(())),
            health_cache: Arc::new(tokio::sync::Mutex::new(None)),
            boxes: None,
        })
    }
    /// Enable session boxes. They share this worker's memory reservations with runs.
    pub fn with_boxes(mut self, config: BoxConfig) -> Result<Self> {
        self.boxes = Some(BoxManager::with_leases(
            self.runner.clone(),
            config,
            self.active.clone(),
            self.capacity_mb,
        )?);
        Ok(self)
    }
    pub fn boxes(&self) -> Option<&BoxManager> {
        self.boxes.as_ref()
    }
    fn auth(&self, headers: &HeaderMap) -> Result<(), NodeError> {
        let value = headers
            .get("authorization")
            .and_then(|h| h.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("");
        if !auth::same(value, &self.token) {
            return Err(NodeError(
                StatusCode::UNAUTHORIZED,
                "Private worker authorization required".into(),
            ));
        }
        Ok(())
    }
    pub fn with_qualification_expiry(self, expires_at: i64) -> Self {
        self.qualification_expires_at
            .store(expires_at, Ordering::SeqCst);
        self
    }
    /// Adopt a newer passing qualification without restarting, so in-flight runs survive.
    /// Never shortens the current expiry.
    pub fn extend_qualification(&self, expires_at: i64) {
        self.qualification_expires_at
            .fetch_max(expires_at, Ordering::SeqCst);
    }
    pub fn qualification_expiry(&self) -> i64 {
        self.qualification_expires_at.load(Ordering::SeqCst)
    }
    /// Docker-derived health, cached for `HEALTH_TTL`; concurrent callers share one refresh.
    async fn runtime_health(&self) -> Result<Value> {
        let mut cache = self.health_cache.lock().await;
        if let Some((at, value)) = cache.as_ref()
            && at.elapsed() < HEALTH_TTL
        {
            return Ok(value.clone());
        }
        let value = self.runner.health().await?;
        *cache = Some((std::time::Instant::now(), value.clone()));
        Ok(value)
    }
    fn qualification_expired(&self) -> bool {
        let expires_at = self.qualification_expiry();
        expires_at > 0 && chrono::Utc::now().timestamp() >= expires_at
    }
    fn path(&self, id: &str) -> Result<PathBuf> {
        SandboxSpec::new(id, crate::worker::IMAGE, Limits::default())?;
        Ok(self.dir.join(format!("{id}.json")))
    }
    fn read(&self, id: &str) -> Result<Record> {
        let path = self.path(id)?;
        ensure!(
            std::fs::metadata(&path)?.len() < 8 * 1024 * 1024,
            "record too large"
        );
        Ok(serde_json::from_slice(&std::fs::read(path)?)?)
    }
    fn write(&self, id: &str, record: &Record) -> Result<()> {
        let _guard = self.io_lock.lock().unwrap();
        let path = self.path(id)?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec(record)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }
    pub async fn recover(&self) -> Result<()> {
        self.runner.recover().await?;
        for entry in std::fs::read_dir(&*self.dir)? {
            let entry = entry?;
            if entry.path().extension().is_none_or(|s| s != "json") {
                continue;
            }
            let id = entry
                .path()
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let mut record = self.read(&id)?;
            if record.state == "running" {
                record.state = "finished".into();
                record.updated_at = now();
                record.receipt = Some(
                    json!({"job_id":id,"reason":"worker_restarted","exit_code":null,"checks_passed":false,"cleanup_state":"failed","creation_uncertain":true,"artifacts":[]}),
                );
                self.write(&id, &record)?;
            }
            ensure!(
                !record
                    .receipt
                    .as_ref()
                    .is_some_and(|r| r["creation_uncertain"] != false),
                "Uncertain container creation for {id}; operator fencing/reconciliation required before worker restart"
            );
            if record
                .receipt
                .as_ref()
                .is_some_and(|r| r["cleanup_state"] != "verified")
            {
                let cleaned = self.runner.cleanup(&id).await?;
                ensure!(cleaned, "Cleanup unresolved for {id}");
                if let Some(receipt) = record.receipt.as_mut() {
                    receipt["cleanup_state"] = json!("verified");
                }
                self.write(&id, &record)?;
            }
        }
        Ok(())
    }
    pub async fn shutdown(&self) -> Result<()> {
        self.begin_drain();
        for lease in self.active.lock().unwrap().values() {
            lease.cancel.store(true, Ordering::SeqCst);
        }
        // Boxes outlive the worker process; the next one adopts them (`BoxManager::recover`).
        for _ in 0..100 {
            if !self.active.lock().unwrap().keys().any(|k| is_run(k)) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        for id in self.active.lock().unwrap().keys().filter(|k| is_run(k)) {
            let mut record = self.read(id)?;
            record.state = "finished".into();
            record.receipt = Some(
                json!({"reason":"shutdown_unresolved","creation_uncertain":true,"checks_passed":false,"cleanup_state":"failed","artifacts":[]}),
            );
            self.write(id, &record)?;
        }
        self.runner.recover().await?;
        Ok(())
    }
    pub fn begin_drain(&self) {
        self.draining.store(true, Ordering::SeqCst);
    }
}
struct NodeError(StatusCode, String);
impl IntoResponse for NodeError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error":self.1}))).into_response()
    }
}
fn err(e: impl std::fmt::Display) -> NodeError {
    tracing::warn!(error=%e,"node request rejected");
    NodeError(StatusCode::BAD_REQUEST, "Worker request rejected".into())
}
pub fn node_router(state: NodeState) -> Router {
    // Box routes check the bearer before any body is read, since file bodies are large.
    let boxes = Router::new()
        .route("/v1/boxes", post(box_create).get(box_list))
        .route("/v1/boxes/{id}", axum::routing::delete(box_destroy))
        .route("/v1/boxes/{id}/exec", post(box_exec))
        .route("/v1/boxes/{id}/files", get(box_read).put(box_write))
        .layer(DefaultBodyLimit::max(
            crate::sandbox_api::MAX_FILE_BYTES + 64 * 1024,
        ))
        .route_layer(middleware::from_fn_with_state(state.clone(), box_auth));
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/runs", post(submit))
        .route("/v1/runs/{id}", get(run).delete(cancel))
        .layer(DefaultBodyLimit::max(65536))
        .merge(boxes)
        .with_state(state)
}
async fn health(State(s): State<NodeState>, headers: HeaderMap) -> Result<Json<Value>, NodeError> {
    s.auth(&headers)?;
    if s.draining.load(Ordering::SeqCst) || s.qualification_expired() {
        return Err(NodeError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Worker is draining".into(),
        ));
    }
    let mut health = s.runtime_health().await.map_err(|_| {
        NodeError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Sandbox runtime unavailable".into(),
        )
    })?;
    health["resource_policy"] = json!(s.resources);
    health["boxes"] = match &s.boxes {
        Some(boxes) => {
            json!({"enabled":true,"image":boxes.image(),"count":boxes.count(),"running":boxes.running_count(),"disk_mb":boxes.disk_mb()})
        }
        None => json!({"enabled":false}),
    };
    health["reserved_memory_mb"] = json!(
        s.active
            .lock()
            .unwrap()
            .values()
            .map(|v| v.memory_mb)
            .sum::<u32>()
    );
    Ok(Json(health))
}
async fn submit(
    State(s): State<NodeState>,
    headers: HeaderMap,
    Json(input): Json<RunRequest>,
) -> Result<impl IntoResponse, NodeError> {
    s.auth(&headers)?;
    input.limits.validate().map_err(err)?;
    s.path(&input.id).map_err(err)?;
    if input.code.is_empty() || input.code.len() > 32768 {
        return Err(err("invalid code length"));
    }
    let hash = auth::digest(&serde_json::to_string(&input).map_err(err)?);
    let record;
    {
        let mut active = s.active.lock().unwrap();
        if s.draining.load(Ordering::SeqCst) || s.qualification_expired() {
            return Err(NodeError(
                StatusCode::SERVICE_UNAVAILABLE,
                "Worker is draining".into(),
            ));
        }
        if let Ok(existing) = s.read(&input.id) {
            if existing.hash != hash {
                return Err(NodeError(
                    StatusCode::CONFLICT,
                    "Run identity conflicts with previous payload".into(),
                ));
            }
            return Ok((StatusCode::ACCEPTED, Json(json!(existing))));
        }
        let used: u32 = active.values().map(|v| v.memory_mb).sum();
        if active.keys().filter(|k| is_run(k)).count() >= MAX_CONCURRENT
            // Runs grow live up to their ceiling, so admission reserves the ceiling.
            || used.saturating_add(input.limits.memory_ceiling()) > s.capacity_mb
        {
            return Err(NodeError(
                StatusCode::TOO_MANY_REQUESTS,
                "Worker capacity exhausted".into(),
            ));
        }
        if std::fs::read_dir(&*s.dir).map_err(err)?.count() >= 1000 {
            return Err(NodeError(
                StatusCode::INSUFFICIENT_STORAGE,
                "Worker evidence retention limit reached".into(),
            ));
        }
        record = Record {
            hash,
            state: "running".into(),
            receipt: None,
            updated_at: now(),
        };
        s.write(&input.id, &record).map_err(err)?;
        let cancel = Arc::new(AtomicBool::new(false));
        active.insert(
            input.id.clone(),
            ActiveLease {
                memory_mb: input.limits.memory_ceiling(),
                cancel: cancel.clone(),
            },
        );
        let state = s.clone();
        let mut finished = record.clone();
        tokio::spawn(async move {
            let result = state
                .runner
                .run(&input.id, &input.code, input.limits, cancel)
                .await;
            finished.state = "finished".into();
            finished.updated_at = now();
            finished.receipt = Some(match result {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(error=%e,"node execution failed");
                    json!({"reason":"preflight_rejected","creation_uncertain":false,"checks_passed":false,"cleanup_state":"verified","artifacts":[]})
                }
            });
            if let Err(e) = state.write(&input.id, &finished) {
                tracing::error!(error=%e,"failed to persist execution receipt");
            }
            // Cleanup failures keep their resource reservation until restart/reconciliation.
            if finished
                .receipt
                .as_ref()
                .is_some_and(|r| r["cleanup_state"] == "verified")
            {
                state.active.lock().unwrap().remove(&input.id);
            }
        });
    }
    Ok((StatusCode::ACCEPTED, Json(json!(record))))
}
async fn run(
    State(s): State<NodeState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, NodeError> {
    s.auth(&headers)?;
    Ok(Json(json!(s.read(&id).map_err(|_| NodeError(
        StatusCode::NOT_FOUND,
        "Run not found".into()
    ))?)))
}
async fn cancel(
    State(s): State<NodeState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, NodeError> {
    s.auth(&headers)?;
    s.path(&id).map_err(err)?;
    if let Some(lease) = s.active.lock().unwrap().get(&id)
        && s.read(&id).is_ok_and(|record| record.state == "running")
    {
        lease.cancel.store(true, Ordering::SeqCst);
        return Ok(Json(json!({"cancel_requested":true})));
    }
    let mut record = s.read(&id).map_err(err)?;
    if record
        .receipt
        .as_ref()
        .is_none_or(|r| r["creation_uncertain"] != false)
    {
        return Ok(Json(
            json!({"cancel_requested":true,"cleanup_state":"failed","reason":"creation_uncertain_requires_operator_reconciliation"}),
        ));
    }
    let cleaned = s.runner.cleanup(&id).await.map_err(err)?;
    if cleaned {
        s.active.lock().unwrap().remove(&id);
    }
    if let Some(receipt) = record.receipt.as_mut() {
        receipt["cleanup_state"] = json!(if cleaned { "verified" } else { "failed" });
    } else {
        record.receipt = Some(
            json!({"reason":"cancelled","checks_passed":false,"cleanup_state":if cleaned{"verified"}else{"failed"}}),
        );
    }
    record.state = "finished".into();
    s.write(&id, &record).map_err(err)?;
    Ok(Json(
        json!({"cancel_requested":true,"cleanup_state":if cleaned{"verified"}else{"failed"}}),
    ))
}

async fn box_auth(
    State(s): State<NodeState>,
    request: Request,
    next: Next,
) -> Result<Response, NodeError> {
    s.auth(request.headers())?;
    Ok(next.run(request).await)
}
fn box_manager(s: &NodeState) -> Result<&BoxManager, NodeError> {
    s.boxes.as_ref().ok_or_else(|| {
        NodeError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Session boxes are not enabled on this worker".into(),
        )
    })
}
fn box_error(e: anyhow::Error) -> Response {
    let (status, code, message) = crate::boxes::error_status(&e);
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        Json(json!({"error":message,"code":code})),
    )
        .into_response()
}
async fn box_create(
    State(s): State<NodeState>,
    Json(spec): Json<crate::sandbox_api::BoxSpec>,
) -> Result<Response, NodeError> {
    let boxes = box_manager(&s)?;
    if s.draining.load(Ordering::SeqCst) {
        return Err(NodeError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Worker is draining".into(),
        ));
    }
    Ok(match boxes.create(&spec).await {
        Ok(box_id) => (StatusCode::CREATED, Json(json!({"box_id":box_id}))).into_response(),
        Err(e) => box_error(e),
    })
}
async fn box_list(State(s): State<NodeState>) -> Result<Response, NodeError> {
    let boxes = box_manager(&s)?;
    Ok(match boxes.reconcile().await {
        Ok(()) => Json(json!(boxes.list_boxes())).into_response(),
        Err(e) => box_error(e),
    })
}
async fn box_destroy(
    State(s): State<NodeState>,
    Path(id): Path<String>,
) -> Result<Response, NodeError> {
    Ok(match box_manager(&s)?.destroy_box(&id).await {
        Ok((removed, volume_removed)) => {
            Json(json!({"removed":removed,"volume_removed":volume_removed})).into_response()
        }
        Err(e) => box_error(e),
    })
}
async fn box_exec(
    State(s): State<NodeState>,
    Path(id): Path<String>,
    Json(request): Json<crate::sandbox_api::ExecRequest>,
) -> Result<Response, NodeError> {
    Ok(match box_manager(&s)?.exec_box(&id, &request).await {
        Ok(result) => Json(result).into_response(),
        Err(e) => box_error(e),
    })
}
#[derive(Deserialize)]
struct FileQuery {
    path: String,
}
/// A file comes back as raw bytes, a directory as a JSON listing; `x-box-entry` says which.
async fn box_read(
    State(s): State<NodeState>,
    Path(id): Path<String>,
    Query(query): Query<FileQuery>,
) -> Result<Response, NodeError> {
    use crate::sandbox_api::FileRead;
    Ok(
        match box_manager(&s)?.read_box_file(&id, &query.path).await {
            Ok(FileRead::File(bytes)) => (
                [
                    (header::CONTENT_TYPE, "application/octet-stream"),
                    (header::HeaderName::from_static("x-box-entry"), "file"),
                ],
                bytes,
            )
                .into_response(),
            Ok(FileRead::Dir(entries)) => (
                [(header::HeaderName::from_static("x-box-entry"), "dir")],
                Json(entries),
            )
                .into_response(),
            Err(e) => box_error(e),
        },
    )
}
async fn box_write(
    State(s): State<NodeState>,
    Path(id): Path<String>,
    Query(query): Query<FileQuery>,
    body: Bytes,
) -> Result<Response, NodeError> {
    Ok(
        match box_manager(&s)?
            .write_box_file(&id, &query.path, body)
            .await
        {
            Ok(result) => Json(result).into_response(),
            Err(e) => box_error(e),
        },
    )
}
