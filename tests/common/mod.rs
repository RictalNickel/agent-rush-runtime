//! Fakes and an app harness shared by the `/v1` and `/mcp` tests.
#![allow(dead_code)]
use agent_rush::{
    api::{AppState, router},
    config::Settings,
    ledger::Ledger,
    mcp_client::McpRegistry,
    sandbox_api::{
        BoxClient, BoxInfo, BoxSpec, DirEntry, ExecRequest, ExecResult, FileRead, WriteResult,
    },
    session::{ModelReply, SessionLimits, SessionModel, Sessions},
    webhooks::UrlGuard,
};
use anyhow::{Result, anyhow, bail};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use futures_util::future::BoxFuture;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use tower::ServiceExt;

/// A scripted model: each call pops the next reply, then repeats the fallback.
pub struct FakeModel {
    steps: Mutex<VecDeque<String>>,
    fallback: String,
}
impl FakeModel {
    pub fn new(steps: Vec<Value>) -> Arc<Self> {
        Arc::new(Self {
            steps: Mutex::new(steps.into_iter().map(|s| s.to_string()).collect()),
            fallback: reply("fallback reply").to_string(),
        })
    }
}
impl SessionModel for FakeModel {
    fn complete<'a>(
        &'a self,
        _system: &'a str,
        _messages: &'a [Value],
    ) -> BoxFuture<'a, Result<ModelReply>> {
        Box::pin(async move {
            let text = self
                .steps
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| self.fallback.clone());
            Ok(ModelReply {
                text,
                cost_usd: 0.001,
            })
        })
    }
}
pub fn reply(text: &str) -> Value {
    json!({"tool":"reply","text":text})
}
pub fn bash(command: &str) -> Value {
    json!({"tool":"bash","command":command})
}

/// A fake worker: an in-memory filesystem per box; exec echoes the command.
#[derive(Default)]
pub struct FakeBoxes {
    next: AtomicUsize,
    boxes: Mutex<HashMap<String, HashMap<String, Vec<u8>>>>,
}
fn normalize(path: &str) -> String {
    path.trim_start_matches("/workspace/")
        .trim_start_matches("./")
        .to_string()
}
impl BoxClient for FakeBoxes {
    fn create<'a>(&'a self, _spec: &'a BoxSpec) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let id = format!("box-{}", self.next.fetch_add(1, Ordering::SeqCst) + 1);
            self.boxes
                .lock()
                .unwrap()
                .insert(id.clone(), HashMap::new());
            Ok(id)
        })
    }
    fn exec<'a>(
        &'a self,
        box_id: &'a str,
        request: &'a ExecRequest,
    ) -> BoxFuture<'a, Result<ExecResult>> {
        Box::pin(async move {
            if !self.boxes.lock().unwrap().contains_key(box_id) {
                bail!("box {box_id} not found");
            }
            Ok(ExecResult {
                exit_code: 0,
                stdout: format!("ran: {}\n", request.command),
                ..Default::default()
            })
        })
    }
    fn read_file<'a>(&'a self, box_id: &'a str, path: &'a str) -> BoxFuture<'a, Result<FileRead>> {
        Box::pin(async move {
            let boxes = self.boxes.lock().unwrap();
            let files = boxes
                .get(box_id)
                .ok_or_else(|| anyhow!("box {box_id} not found"))?;
            let path = normalize(path);
            if path == "." || path.is_empty() {
                return Ok(FileRead::Dir(
                    files
                        .iter()
                        .map(|(name, data)| DirEntry {
                            name: name.clone(),
                            is_dir: false,
                            size: data.len() as u64,
                        })
                        .collect(),
                ));
            }
            files
                .get(&path)
                .map(|data| FileRead::File(data.clone()))
                .ok_or_else(|| anyhow!("no such file: {path}"))
        })
    }
    fn write_file<'a>(
        &'a self,
        box_id: &'a str,
        path: &'a str,
        bytes: &'a [u8],
    ) -> BoxFuture<'a, Result<WriteResult>> {
        Box::pin(async move {
            let mut boxes = self.boxes.lock().unwrap();
            let files = boxes
                .get_mut(box_id)
                .ok_or_else(|| anyhow!("box {box_id} not found"))?;
            files.insert(normalize(path), bytes.to_vec());
            Ok(WriteResult {
                bytes: bytes.len() as u64,
                sha256: "fake".into(),
            })
        })
    }
    fn destroy<'a>(&'a self, box_id: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.boxes.lock().unwrap().remove(box_id);
            Ok(())
        })
    }
    fn list<'a>(&'a self) -> BoxFuture<'a, Result<Vec<BoxInfo>>> {
        Box::pin(async move {
            Ok(self
                .boxes
                .lock()
                .unwrap()
                .keys()
                .map(|id| BoxInfo {
                    box_id: id.clone(),
                    session_id: String::new(),
                    owner: String::new(),
                    created_at: 0,
                    last_used_at: 0,
                })
                .collect())
        })
    }
}

pub const PASSWORD: &str = "test-password-long-enough";

pub struct App {
    pub router: Router,
    pub state: AppState,
    pub sessions: Sessions,
    pub boxes: Arc<FakeBoxes>,
}
impl App {
    pub fn ledger(&self) -> &Ledger {
        &self.state.ledger
    }
    /// A fresh API key for `owner`.
    pub fn key(&self, owner: &str) -> String {
        self.state.ledger.create_api_key(owner, "test").unwrap().key
    }
}

/// The control router with fake sessions; webhooks may target loopback (test-only).
pub fn app(steps: Vec<Value>) -> App {
    let mut settings = Settings::local();
    settings.operator_password = PASSWORD.into();
    let mut state = AppState::new(settings, Ledger::memory(1024).unwrap()).unwrap();
    let boxes = Arc::new(FakeBoxes::default());
    let sessions = Sessions::new(
        state.ledger.clone(),
        FakeModel::new(steps),
        boxes.clone(),
        Arc::new(McpRegistry::empty()),
        SessionLimits::default(),
    );
    state.sessions = Some(sessions.clone());
    state.webhook_guard = UrlGuard::allow_loopback_for_tests();
    App {
        router: router(state.clone()),
        state,
        sessions,
        boxes,
    }
}

/// The control router without a worker: no sessions.
pub fn app_without_sessions() -> App {
    let mut a = app(vec![]);
    a.state.sessions = None;
    a.router = router(a.state.clone());
    a
}

pub async fn send(router: &Router, request: Request<Body>) -> axum::response::Response {
    router.clone().oneshot(request).await.unwrap()
}

/// A JSON request with an optional bearer key. Returns the status and the JSON body
/// (or `{"raw": text}` when the body is not JSON).
pub async fn call(
    router: &Router,
    method: &str,
    path: &str,
    key: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(key) = key {
        builder = builder.header("authorization", format!("Bearer {key}"));
    }
    let body = match body {
        Some(v) => {
            builder = builder.header("content-type", "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    let response = send(router, builder.body(body).unwrap()).await;
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| json!({"raw": String::from_utf8_lossy(&bytes)}));
    (status, value)
}
