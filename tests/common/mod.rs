//! Fakes and an app harness shared by the `/v1` and `/mcp` tests.
#![allow(dead_code)]
use agent_rush::{
    api::{AppState, router},
    config::{ModelCatalog, ModelPrice, Settings},
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

/// A scripted model: each call pops the next reply, then repeats the fallback. Every
/// call costs the price of the model it ran on (default $0.001) for 1000 prompt and 100
/// completion tokens, and is recorded with that model and its messages.
pub struct FakeModel {
    steps: Mutex<VecDeque<String>>,
    fallback: String,
    prices: HashMap<String, f64>,
    calls: Mutex<Vec<(String, Vec<Value>)>>,
}
impl FakeModel {
    pub fn new(steps: Vec<Value>) -> Arc<Self> {
        Self::priced(steps, &[])
    }
    /// A model whose calls on each listed model cost that many dollars.
    pub fn priced(steps: Vec<Value>, prices: &[(&str, f64)]) -> Arc<Self> {
        Arc::new(Self {
            steps: Mutex::new(steps.into_iter().map(|s| s.to_string()).collect()),
            fallback: reply("fallback reply").to_string(),
            prices: prices.iter().map(|(m, p)| (m.to_string(), *p)).collect(),
            calls: Mutex::new(Vec::new()),
        })
    }
    /// Each call so far: the model it ran on and the messages it was sent.
    pub fn calls(&self) -> Vec<(String, Vec<Value>)> {
        self.calls.lock().unwrap().clone()
    }
}
impl SessionModel for FakeModel {
    fn complete<'a>(
        &'a self,
        system: &'a str,
        messages: &'a [Value],
    ) -> BoxFuture<'a, Result<ModelReply>> {
        self.complete_with_model("", system, messages, None)
    }
    fn complete_with_model<'a>(
        &'a self,
        model: &'a str,
        _system: &'a str,
        messages: &'a [Value],
        _effort: Option<&'a str>,
    ) -> BoxFuture<'a, Result<ModelReply>> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push((model.to_string(), messages.to_vec()));
            let text = self
                .steps
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| self.fallback.clone());
            Ok(ModelReply {
                text,
                cost_usd: self.prices.get(model).copied().unwrap_or(0.001),
                input_tokens: 1000,
                output_tokens: 100,
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

/// A fake worker: an in-memory filesystem per box; exec echoes the command, except the
/// workspace archive commands, which it runs on its files (as a plain tar), and `rm -f`.
#[derive(Default)]
pub struct FakeBoxes {
    next: AtomicUsize,
    boxes: Mutex<HashMap<String, HashMap<String, Vec<u8>>>>,
}
impl FakeBoxes {
    /// Boxes that exist now (created and not destroyed).
    #[allow(dead_code)]
    pub fn live(&self) -> usize {
        self.boxes.lock().unwrap().len()
    }
    /// Puts a file straight into a box, past the API's size limit.
    pub fn put_file(&self, box_id: &str, path: &str, data: Vec<u8>) {
        self.boxes
            .lock()
            .unwrap()
            .get_mut(box_id)
            .unwrap()
            .insert(normalize(path), data);
    }
    /// The paths in a box, sorted.
    pub fn files(&self, box_id: &str) -> Vec<String> {
        let mut paths: Vec<String> = self.boxes.lock().unwrap()[box_id].keys().cloned().collect();
        paths.sort();
        paths
    }
    /// Runs the archive, extract and `rm -f` commands on a box's files.
    fn run_file_command(
        files: &mut HashMap<String, Vec<u8>>,
        command: &str,
    ) -> Option<Result<ExecResult>> {
        let first = command.lines().next().unwrap_or("");
        let done = |stdout: String| {
            Some(Ok(ExecResult {
                stdout,
                ..Default::default()
            }))
        };
        if let Some(path) = first.strip_prefix("# agent-rush: archive ") {
            let mut builder = tar::Builder::new(Vec::new());
            let mut names: Vec<&String> =
                files.keys().filter(|n| !n.starts_with("/tmp/")).collect();
            names.sort();
            for name in names {
                let data = &files[name];
                let mut header = tar::Header::new_gnu();
                header.set_size(data.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                builder
                    .append_data(&mut header, format!("./{name}"), data.as_slice())
                    .unwrap();
            }
            let mut archive = builder.into_inner().unwrap();
            archive.truncate(agent_rush::sandbox_api::MAX_FILE_BYTES + 1);
            let size = archive.len();
            files.insert(path.to_string(), archive);
            return done(format!("0 {size}\n"));
        }
        if let Some(path) = first.strip_prefix("# agent-rush: extract ") {
            let Some(data) = files.remove(path) else {
                return Some(Ok(ExecResult {
                    exit_code: 2,
                    stderr: "no archive".into(),
                    ..Default::default()
                }));
            };
            let mut archive = tar::Archive::new(data.as_slice());
            for entry in archive.entries().unwrap() {
                let mut entry = entry.unwrap();
                let name = entry.path().unwrap().to_string_lossy().into_owned();
                let mut bytes = Vec::new();
                std::io::Read::read_to_end(&mut entry, &mut bytes).unwrap();
                files.insert(normalize(&name), bytes);
            }
            return done(String::new());
        }
        if let Some(path) = command.strip_prefix("rm -f ") {
            files.remove(path.trim_matches('\''));
            return done(String::new());
        }
        None
    }
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
            let mut boxes = self.boxes.lock().unwrap();
            let Some(files) = boxes.get_mut(box_id) else {
                bail!("box {box_id} not found");
            };
            if let Some(result) = Self::run_file_command(files, &request.command) {
                return result;
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
    pub model: Arc<FakeModel>,
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
    app_with_limits(steps, SessionLimits::default())
}

/// `app` with explicit session limits.
pub fn app_with_limits(steps: Vec<Value>, limits: SessionLimits) -> App {
    build(
        FakeModel::new(steps),
        limits,
        Settings::local().model_catalog(),
    )
}

/// The two GLM models at their Vultr tariffs (USD per million tokens), Flash the default.
pub fn glm_catalog() -> ModelCatalog {
    let price = |id: &str, input: f64, output: f64| ModelPrice {
        id: id.into(),
        input_usd_per_million: input,
        output_usd_per_million: output,
    };
    ModelCatalog {
        provider: "vultr".into(),
        default: "glm-5.3-flash".into(),
        models: vec![
            price("glm-5.3-flash", 0.10, 0.35),
            price("glm-5.3", 0.75, 3.00),
        ],
        per_session: true,
    }
}

/// `app` whose sessions may choose a model from `catalog`; a call costs what `prices`
/// says for its model.
pub fn app_with_models(
    steps: Vec<Value>,
    limits: SessionLimits,
    catalog: ModelCatalog,
    prices: &[(&str, f64)],
) -> App {
    build(FakeModel::priced(steps, prices), limits, catalog)
}

fn build(model: Arc<FakeModel>, limits: SessionLimits, catalog: ModelCatalog) -> App {
    let mut settings = Settings::local();
    settings.operator_password = PASSWORD.into();
    let mut state = AppState::new(settings, Ledger::memory(1024).unwrap()).unwrap();
    let boxes = Arc::new(FakeBoxes::default());
    let sessions = Sessions::with_models(
        state.ledger.clone(),
        model.clone(),
        boxes.clone(),
        Arc::new(McpRegistry::empty()),
        limits,
        catalog,
    );
    state.sessions = Some(sessions.clone());
    state.webhook_guard = UrlGuard::allow_loopback_for_tests();
    App {
        router: router(state.clone()),
        state,
        sessions,
        boxes,
        model,
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

/// Signs in as the operator; returns the `ar_session=...` cookie pair and the CSRF token.
pub async fn operator_login(router: &Router) -> (String, String) {
    let response = send(
        router,
        Request::post("/api/login")
            .header("content-type", "application/json")
            .body(Body::from(json!({"password": PASSWORD}).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    (cookie, body["csrf"].as_str().unwrap().to_string())
}

/// A JSON request with the operator cookie and, when given, the CSRF header.
pub async fn call_operator(
    router: &Router,
    method: &str,
    path: &str,
    cookie: &str,
    csrf: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("cookie", cookie);
    if let Some(csrf) = csrf {
        builder = builder.header("x-csrf-token", csrf);
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
