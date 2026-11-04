use crate::{
    auth,
    domain::{Limits, now},
    worker::{DockerRunner, SandboxSpec},
};
use anyhow::{Result, ensure};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
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
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone)]
pub struct NodeState {
    pub runner: DockerRunner,
    dir: Arc<PathBuf>,
    token: Arc<String>,
    active: Arc<Mutex<HashMap<String, ActiveLease>>>,
    capacity_mb: u32,
}
#[derive(Clone)]
struct ActiveLease {
    memory_mb: u32,
    cancel: Arc<AtomicBool>,
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
        Ok(Self {
            runner,
            dir: Arc::new(dir),
            token: Arc::new(token),
            active: Arc::new(Mutex::new(HashMap::new())),
            capacity_mb,
        })
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
                let cleaned = self.runner.cleanup(&id).await?;
                record.state = "finished".into();
                record.updated_at = now();
                record.receipt = Some(
                    json!({"job_id":id,"reason":"worker_restarted","exit_code":null,"checks_passed":false,"cleanup_state":if cleaned{"verified"}else{"failed"},"artifacts":[]}),
                );
                self.write(&id, &record)?;
            }
        }
        Ok(())
    }
    pub async fn shutdown(&self) -> Result<()> {
        for lease in self.active.lock().unwrap().values() {
            lease.cancel.store(true, Ordering::SeqCst);
        }
        for _ in 0..100 {
            if self.active.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        self.runner.recover().await?;
        Ok(())
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
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/runs", post(submit))
        .route("/v1/runs/{id}", get(run).delete(cancel))
        .layer(DefaultBodyLimit::max(65536))
        .with_state(state)
}
async fn health(State(s): State<NodeState>, headers: HeaderMap) -> Result<Json<Value>, NodeError> {
    s.auth(&headers)?;
    Ok(Json(s.runner.health().await.map_err(|_| {
        NodeError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Sandbox runtime unavailable".into(),
        )
    })?))
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
        if active.len() >= 2 || used.saturating_add(input.limits.memory_mb) > s.capacity_mb {
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
                memory_mb: input.limits.memory_mb,
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
                    json!({"reason":"node_error","checks_passed":false,"cleanup_state":"failed","artifacts":[]})
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
    let cleaned = s.runner.cleanup(&id).await.map_err(err)?;
    if cleaned {
        s.active.lock().unwrap().remove(&id);
    }
    let mut record = s.read(&id).map_err(err)?;
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
