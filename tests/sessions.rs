use agent_rush::{
    agent_config::{AgentConfig, CustomTool},
    ledger::Ledger,
    mcp_client::McpRegistry,
    sandbox_api::{
        BoxClient, BoxInfo, BoxSpec, DirEntry, ExecRequest, ExecResult, FileRead, WriteResult,
    },
    session::{ChargedError, ModelReply, SessionError, SessionLimits, SessionModel, Sessions},
    session_store::{SessionEvent, SessionStatus},
    skills::parse_skill_md,
    tools::{ToolCall, apply_edit, expand_template, parse_call, shell_quote, truncate_bytes},
};
use anyhow::{Result, anyhow, bail};
use axum::body::Bytes;
use futures_util::future::BoxFuture;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Notify, Semaphore};

// A scripted model: each call pops the next step. A step can wait for the test first.

enum Step {
    Say(String),
    Gated(Arc<Notify>, Arc<Semaphore>, String),
    /// The gateway failed after billing this many dollars.
    FailCharged(f64),
}

#[derive(Default)]
struct FakeModel {
    steps: Mutex<VecDeque<Step>>,
    calls: Mutex<Vec<(String, Vec<Value>)>>,
    fallback: Mutex<Option<String>>,
}
impl FakeModel {
    fn new(steps: Vec<Value>) -> Arc<Self> {
        let model = Self::default();
        for step in steps {
            model.push(step);
        }
        Arc::new(model)
    }
    fn push(&self, step: Value) {
        self.steps
            .lock()
            .unwrap()
            .push_back(Step::Say(step.to_string()));
    }
    fn push_raw(&self, text: &str) {
        self.steps
            .lock()
            .unwrap()
            .push_back(Step::Say(text.to_string()));
    }
    /// The step signals `started` when the call begins and returns once `release` has a permit.
    fn push_gated(&self, step: Value) -> (Arc<Notify>, Arc<Semaphore>) {
        let started = Arc::new(Notify::new());
        let release = Arc::new(Semaphore::new(0));
        self.steps.lock().unwrap().push_back(Step::Gated(
            started.clone(),
            release.clone(),
            step.to_string(),
        ));
        (started, release)
    }
    fn calls(&self) -> Vec<(String, Vec<Value>)> {
        self.calls.lock().unwrap().clone()
    }
    fn all_text(messages: &[Value]) -> String {
        messages
            .iter()
            .map(|m| m["content"].as_str().unwrap_or("").to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }
}
impl SessionModel for FakeModel {
    fn complete<'a>(
        &'a self,
        system: &'a str,
        messages: &'a [Value],
    ) -> BoxFuture<'a, Result<ModelReply>> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push((system.to_string(), messages.to_vec()));
            let step = self.steps.lock().unwrap().pop_front();
            let text = match step {
                Some(Step::Say(text)) => text,
                Some(Step::Gated(started, release, text)) => {
                    started.notify_one();
                    release.acquire().await.unwrap().forget();
                    text
                }
                Some(Step::FailCharged(usd)) => {
                    return Err(ChargedError::wrap(
                        anyhow!("gateway broke after billing"),
                        usd,
                    ));
                }
                None => match self.fallback.lock().unwrap().clone() {
                    Some(text) => text,
                    None => bail!("script exhausted"),
                },
            };
            Ok(ModelReply {
                text,
                cost_usd: 0.001,
                ..Default::default()
            })
        })
    }
}

// A fake worker: an in-memory filesystem per box and a few scripted commands.

#[derive(Default)]
struct FakeBoxes {
    next: AtomicUsize,
    boxes: Mutex<HashMap<String, HashMap<String, Vec<u8>>>>,
    commands: Mutex<Vec<(String, String)>>,
    exec_requests: Mutex<Vec<ExecRequest>>,
    lose_once: AtomicBool,
    exec_started: Notify,
    /// Releases a running `block` command.
    exec_release: Notify,
    creates: AtomicUsize,
    /// Exec calls running now, and the most ever seen at once.
    in_flight: AtomicUsize,
    max_in_flight: AtomicUsize,
    /// Address of the last buffer `write_file_bytes` received.
    last_write_ptr: AtomicUsize,
    /// Runs of the `poll` command, whose output changes every time.
    polls: AtomicUsize,
}
fn normalize(path: &str) -> String {
    path.trim_start_matches("/workspace/")
        .trim_start_matches("./")
        .to_string()
}
impl BoxClient for FakeBoxes {
    fn create<'a>(&'a self, spec: &'a BoxSpec) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            assert_eq!(spec.memory_mb, 1024);
            self.creates.fetch_add(1, Ordering::SeqCst);
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
            self.commands
                .lock()
                .unwrap()
                .push((box_id.to_string(), request.command.clone()));
            self.exec_requests.lock().unwrap().push(request.clone());
            let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_in_flight.fetch_max(now, Ordering::SeqCst);
            struct Leave<'a>(&'a AtomicUsize);
            impl Drop for Leave<'_> {
                fn drop(&mut self) {
                    self.0.fetch_sub(1, Ordering::SeqCst);
                }
            }
            let _leave = Leave(&self.in_flight);
            let command = request.command.as_str();
            if command == "lose-the-box" && self.lose_once.swap(false, Ordering::SeqCst) {
                self.boxes.lock().unwrap().remove(box_id);
                bail!("box {box_id} not found");
            }
            if command == "block" {
                self.exec_started.notify_one();
                self.exec_release.notified().await;
            }
            if command == "panic" {
                panic!("the fake worker panicked");
            }
            let stdout = match command {
                "big" => "log line\n".repeat(10 * 1024 * 1024 / 9 + 1),
                "poll" => format!("progress {}\n", self.polls.fetch_add(1, Ordering::SeqCst)),
                // About 4 KB of output that names its command.
                c if c.starts_with("noise-") => format!("ran: {c}\n{}\n", "x".repeat(4000)),
                _ => format!("ran: {command}\n"),
            };
            Ok(ExecResult {
                exit_code: 0,
                stdout,
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
    fn write_file_bytes<'a>(
        &'a self,
        box_id: &'a str,
        path: &'a str,
        bytes: Bytes,
    ) -> BoxFuture<'a, Result<WriteResult>> {
        self.last_write_ptr
            .store(bytes.as_ptr() as usize, Ordering::SeqCst);
        Box::pin(async move { self.write_file(box_id, path, &bytes).await })
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

struct Harness {
    ledger: Arc<Ledger>,
    model: Arc<FakeModel>,
    boxes: Arc<FakeBoxes>,
    sessions: Sessions,
}
fn harness(steps: Vec<Value>, limits: SessionLimits) -> Harness {
    let ledger = Arc::new(Ledger::memory(1024).unwrap());
    let model = FakeModel::new(steps);
    let boxes = Arc::new(FakeBoxes::default());
    let sessions = Sessions::new(
        ledger.clone(),
        model.clone(),
        boxes.clone(),
        Arc::new(McpRegistry::empty()),
        limits,
    );
    Harness {
        ledger,
        model,
        boxes,
        sessions,
    }
}
const WAIT: Duration = Duration::from_secs(10);
fn reply(text: &str) -> Value {
    json!({"tool":"reply","text":text})
}
fn bash(command: &str) -> Value {
    json!({"tool":"bash","command":command})
}
impl Harness {
    fn events(&self, id: &str) -> Vec<SessionEvent> {
        self.sessions.events("alice", id, 0, 1000).unwrap()
    }
    fn results(&self, id: &str) -> Vec<Value> {
        self.events(id)
            .into_iter()
            .filter(|e| e.kind == "tool_result")
            .map(|e| e.payload)
            .collect()
    }
    async fn run(&self, text: &str) -> String {
        let session = self.sessions.create("alice", None).unwrap();
        self.sessions.send("alice", &session.id, text).unwrap();
        let view = self
            .sessions
            .wait_idle("alice", &session.id, WAIT)
            .await
            .unwrap();
        assert_ne!(view.status, SessionStatus::Running, "turn did not finish");
        session.id
    }
}

#[tokio::test]
async fn one_turn_records_user_tool_call_result_assistant_then_idle() {
    let h = harness(
        vec![bash("echo hi"), reply("Done: hi")],
        SessionLimits::default(),
    );
    let id = h.run("Say hi from the shell").await;
    let events = h.events(&id);
    let kinds: Vec<&str> = events
        .iter()
        .filter(|e| !(e.kind == "status" && e.payload["status"] == "running"))
        .map(|e| e.kind.as_str())
        .collect();
    assert_eq!(
        kinds,
        vec!["user", "tool_call", "tool_result", "assistant", "status"]
    );
    assert_eq!(events[1].payload["status"], "running");
    assert_eq!(events.last().unwrap().payload["status"], "idle");
    let result = &h.results(&id)[0];
    assert_eq!(result["ok"], true);
    assert!(result["output"].as_str().unwrap().contains("ran: echo hi"));
    let assistant = events.iter().find(|e| e.kind == "assistant").unwrap();
    assert_eq!(assistant.payload["text"], "Done: hi");
    let view = h.sessions.get("alice", &id).unwrap().unwrap();
    assert_eq!(view.status, SessionStatus::Idle);
    assert_eq!(view.box_id.as_deref(), Some("box-1"));
    assert_eq!(view.title, "Say hi from the shell");
    assert!((view.spend_usd - 0.002).abs() < 1e-9);
    // The first call sees the base prompt and the request; the second sees the result.
    let calls = h.model.calls();
    assert!(calls[0].0.contains("Use reply once to finish."));
    assert!(
        calls[0].0.contains("web-research"),
        "built-in skills are indexed"
    );
    assert_eq!(
        calls[0].1,
        vec![json!({"role":"user","content":"Say hi from the shell"})]
    );
    assert!(FakeModel::all_text(&calls[1].1).contains("ran: echo hi"));

    // A second message starts a new turn that still sees the earlier conversation.
    h.model.push(reply("Second answer"));
    h.sessions.send("alice", &id, "And again?").unwrap();
    h.sessions.wait_idle("alice", &id, WAIT).await.unwrap();
    let last = h.model.calls().pop().unwrap();
    let text = FakeModel::all_text(&last.1);
    assert!(text.contains("Say hi from the shell") && text.contains("And again?"));
    assert_eq!(last.1[0]["role"], "user");
}

#[tokio::test]
async fn interjection_mid_turn_reaches_the_next_model_call() {
    let h = harness(vec![], SessionLimits::default());
    h.model.push(bash("sleep 1"));
    let (started, release) = h.model.push_gated(bash("echo next"));
    h.model.push(reply("Handled both"));
    let session = h.sessions.create("alice", None).unwrap();
    h.sessions.send("alice", &session.id, "Build it").unwrap();
    started.notified().await; // the second model call is in flight
    // Too late for this call; it must reach the next one in the same turn.
    let outcome = h
        .sessions
        .send("alice", &session.id, "Also add tests")
        .unwrap();
    assert!(outcome.interjection);
    release.add_permits(1);
    h.sessions
        .wait_idle("alice", &session.id, WAIT)
        .await
        .unwrap();
    let calls = h.model.calls();
    assert_eq!(calls.len(), 3);
    assert!(!FakeModel::all_text(&calls[1].1).contains("Also add tests"));
    assert!(FakeModel::all_text(&calls[2].1).contains("Also add tests"));
    let events = h.events(&session.id);
    let interjection = events.iter().find(|e| e.kind == "interjection").unwrap();
    assert_eq!(interjection.seq, outcome.seq);
    assert_eq!(events.iter().filter(|e| e.kind == "assistant").count(), 1);
    assert!(h.ledger.take_pending(&session.id).unwrap().is_empty());
}

#[tokio::test]
async fn interjection_after_the_final_model_call_starts_a_new_turn() {
    let h = harness(vec![], SessionLimits::default());
    let (started, release) = h.model.push_gated(reply("First answer"));
    h.model.push(reply("Answer to the follow-up"));
    let session = h.sessions.create("alice", None).unwrap();
    h.sessions.send("alice", &session.id, "Question").unwrap();
    started.notified().await; // the final call of the turn is in flight
    let outcome = h.sessions.send("alice", &session.id, "Follow-up").unwrap();
    assert!(outcome.interjection);
    release.add_permits(1);
    let view = h
        .sessions
        .wait_idle("alice", &session.id, WAIT)
        .await
        .unwrap();
    assert_eq!(view.status, SessionStatus::Idle);
    let calls = h.model.calls();
    assert_eq!(calls.len(), 2, "the queued message started another turn");
    assert!(FakeModel::all_text(&calls[1].1).contains("Follow-up"));
    // The follow-up turn's request is the interjection, after the first reply.
    let last = calls[1].1.last().unwrap();
    assert_eq!(last["role"], "user", "{:?}", calls[1].1);
    assert!(last["content"].as_str().unwrap().contains("Follow-up"));
    assert!(FakeModel::all_text(&calls[1].1).contains("First answer"));
    let replies: Vec<_> = h
        .events(&session.id)
        .into_iter()
        .filter(|e| e.kind == "assistant")
        .map(|e| e.payload["text"].clone())
        .collect();
    assert_eq!(
        replies,
        vec![json!("First answer"), json!("Answer to the follow-up")]
    );
}

#[tokio::test]
async fn cancel_lets_the_running_tool_finish_then_stops_before_the_next_model_call() {
    let h = harness(
        vec![bash("block"), reply("never")],
        SessionLimits::default(),
    );
    let session = h.sessions.create("alice", None).unwrap();
    h.sessions.send("alice", &session.id, "Run it").unwrap();
    h.boxes.exec_started.notified().await;
    h.sessions.cancel("alice", &session.id).unwrap();
    // The box exec is not dropped mid-flight: the turn waits for it.
    let view = h
        .sessions
        .wait_idle("alice", &session.id, Duration::from_millis(300))
        .await
        .unwrap();
    assert_eq!(view.status, SessionStatus::Running);
    assert!(h.results(&session.id).is_empty());
    h.boxes.exec_release.notify_one();
    let view = h
        .sessions
        .wait_idle("alice", &session.id, WAIT)
        .await
        .unwrap();
    assert_eq!(view.status, SessionStatus::Idle);
    let events = h.events(&session.id);
    assert!(events.iter().all(|e| e.kind != "assistant"));
    assert_eq!(events.last().unwrap().payload["reason"], "cancelled");
    let results = h.results(&session.id);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["ok"], true, "the command's real result is kept");
    assert!(
        results[0]["output"]
            .as_str()
            .unwrap()
            .contains("ran: block")
    );
    assert_eq!(h.model.calls().len(), 1, "no model call after cancel");
}

#[tokio::test]
async fn cancel_during_a_model_call_still_records_its_cost() {
    let h = harness(vec![], SessionLimits::default());
    let (started, release) = h.model.push_gated(bash("echo late"));
    let session = h.sessions.create("alice", None).unwrap();
    h.sessions.send("alice", &session.id, "Think").unwrap();
    started.notified().await;
    h.sessions.cancel("alice", &session.id).unwrap();
    let view = h
        .sessions
        .wait_idle("alice", &session.id, WAIT)
        .await
        .unwrap();
    assert_eq!(view.status, SessionStatus::Idle);
    assert!(view.spend_usd < 1e-12);
    // The abandoned call completes in the background and its cost is recorded.
    release.add_permits(1);
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let spend = h
            .sessions
            .get("alice", &session.id)
            .unwrap()
            .unwrap()
            .spend_usd;
        if (spend - 0.001).abs() < 1e-9 {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "spend never recorded"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(h.results(&session.id).is_empty(), "its tool call never ran");
    assert!(h.boxes.commands.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_long_turn_is_not_cut_off_by_a_step_count() {
    let mut script: Vec<Value> = (0..80).map(|i| bash(&format!("step {i}"))).collect();
    script.push(reply("Done after 80 steps"));
    let h = harness(script, SessionLimits::default());
    let id = h.run("Work for a long time").await;
    let view = h.sessions.get("alice", &id).unwrap().unwrap();
    assert_eq!(view.status, SessionStatus::Idle);
    assert_eq!(h.model.calls().len(), 81);
    assert!(h.events(&id).iter().all(|e| e.kind != "error"));
}

#[tokio::test]
async fn a_box_lost_mid_turn_is_recreated_and_the_session_continues() {
    let h = harness(
        vec![bash("touch a"), bash("lose-the-box"), reply("Recovered")],
        SessionLimits::default(),
    );
    h.boxes.lose_once.store(true, Ordering::SeqCst);
    let id = h.run("Work").await;
    let view = h.sessions.get("alice", &id).unwrap().unwrap();
    assert_eq!(view.status, SessionStatus::Idle);
    assert_eq!(view.box_id.as_deref(), Some("box-2"));
    assert_eq!(h.boxes.creates.load(Ordering::SeqCst), 2);
    let events = h.events(&id);
    let error = events.iter().find(|e| e.kind == "error").unwrap();
    assert!(error.payload["message"].as_str().unwrap().contains("lost"));
    assert_eq!(error.payload["box_id"], "box-1");
    let results = h.results(&id);
    assert_eq!(
        results[1]["ok"], true,
        "the command was retried on the new box"
    );
    assert!(events.iter().any(|e| e.kind == "assistant"));
    // Direct exec on a box that vanished between turns is recovered too.
    h.boxes.boxes.lock().unwrap().clear();
    let result = h
        .sessions
        .exec(
            "alice",
            &id,
            ExecRequest {
                command: "pwd".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(
        h.sessions
            .get("alice", &id)
            .unwrap()
            .unwrap()
            .box_id
            .as_deref(),
        Some("box-3")
    );
}

#[tokio::test]
async fn edit_file_requires_an_exact_unique_match() {
    let h = harness(
        vec![
            json!({"tool":"write_file","path":"a.txt","content":"one two two"}),
            json!({"tool":"edit_file","path":"a.txt","old":"one","new":"1"}),
            json!({"tool":"edit_file","path":"a.txt","old":"two","new":"2"}),
            json!({"tool":"edit_file","path":"a.txt","old":"two","new":"2","replace_all":true}),
            json!({"tool":"edit_file","path":"a.txt","old":"zzz","new":"y"}),
            json!({"tool":"read_file","path":"a.txt"}),
            reply("Edited"),
        ],
        SessionLimits::default(),
    );
    let id = h.run("Edit").await;
    let results = h.results(&id);
    let ok: Vec<bool> = results.iter().map(|r| r["ok"] == true).collect();
    assert_eq!(ok, vec![true, true, false, true, false, true]);
    assert!(
        results[2]["output"]
            .as_str()
            .unwrap()
            .contains("matches 2 places")
    );
    assert!(
        results[3]["output"]
            .as_str()
            .unwrap()
            .contains("Replaced 2 occurrences")
    );
    assert!(results[4]["output"].as_str().unwrap().contains("not found"));
    assert_eq!(results[5]["output"], "1 2 2");
    assert!(
        apply_edit("aXa", "a", "b", false)
            .unwrap_err()
            .to_string()
            .contains("2 places")
    );
    assert_eq!(
        apply_edit("aXa", "X", "Y", false).unwrap(),
        ("aYa".to_string(), 1)
    );
}

#[tokio::test]
async fn agent_config_update_is_versioned_as_agent_and_disables_a_tool() {
    let h = harness(
        vec![
            json!({"tool":"agent_config","action":"update","config":{"disabled_tools":["web_search"],"system_addendum":"Be terse."},"reason":"no search needed"}),
            json!({"tool":"web_search","query":"anything"}),
            json!({"tool":"agent_config","action":"update","config":{"mcp_servers":["not-allowed"]}}),
            json!({"tool":"agent_config","action":"update","config":{"custom_tools":[{"name":"bash","description":"x","command_template":"x"}]}}),
            reply("Configured"),
        ],
        SessionLimits::default(),
    );
    let id = h.run("Configure yourself").await;
    let (version, config) = h.ledger.agent_config("alice").unwrap();
    assert_eq!(version, 1, "the refused updates were not stored");
    assert_eq!(config.disabled_tools, vec!["web_search"]);
    let history = h.ledger.agent_config_history("alice").unwrap();
    assert_eq!(
        (history[0].1.as_str(), history[0].2.as_str()),
        ("no search needed", "agent")
    );
    let events = h.events(&id);
    let change = events.iter().find(|e| e.kind == "config_change").unwrap();
    assert_eq!(change.payload["version"], 1);
    assert_eq!(change.payload["author"], "agent");
    let results = h.results(&id);
    assert_eq!(results[1]["ok"], false);
    assert!(results[1]["output"].as_str().unwrap().contains("disabled"));
    assert_eq!(results[2]["ok"], false);
    assert!(
        results[2]["output"]
            .as_str()
            .unwrap()
            .contains("allowed list")
    );
    assert_eq!(results[3]["ok"], false);
    assert!(results[3]["output"].as_str().unwrap().contains("built-in"));
    // The next prompt reflects the new config.
    let system = &h.model.calls()[1].0;
    assert!(!system.contains(r#"{"tool":"web_search""#));
    assert!(system.contains("Be terse."));
    assert!(
        h.boxes
            .commands
            .lock()
            .unwrap()
            .iter()
            .all(|(_, c)| !c.starts_with("websearch"))
    );
}

#[tokio::test]
async fn custom_tool_expands_with_shell_quoted_args() {
    let h = harness(
        vec![
            json!({"tool":"count_lines","path":"it's here; rm -rf /"}),
            json!({"tool":"count_lines","path":"x","extra":"y"}),
            reply("Counted"),
        ],
        SessionLimits::default(),
    );
    let config = AgentConfig {
        custom_tools: vec![CustomTool {
            name: "count_lines".into(),
            description: "Count lines in a file".into(),
            command_template: "wc -l {{path}}".into(),
        }],
        ..Default::default()
    };
    h.ledger
        .update_agent_config("alice", &config, "add tool", "user")
        .unwrap();
    let id = h.run("Count").await;
    let commands = h.boxes.commands.lock().unwrap().clone();
    assert_eq!(
        commands.len(),
        1,
        "the call with an unknown argument never ran"
    );
    assert_eq!(commands[0].1, r"wc -l 'it'\''s here; rm -rf /'");
    assert!(
        h.results(&id)[1]["output"]
            .as_str()
            .unwrap()
            .contains("unknown argument")
    );
    assert!(
        h.model.calls()[0]
            .0
            .contains(r#"{"tool":"count_lines","path":"..."}"#)
    );
    assert_eq!(shell_quote("a'b"), r"'a'\''b'");
    let args = json!({"n": 3, "s": "$(id)"});
    assert_eq!(
        expand_template("head -n {{n}} {{ s }}", args.as_object().unwrap()).unwrap(),
        "head -n '3' '$(id)'"
    );
    assert!(expand_template("x {{missing}}", args.as_object().unwrap()).is_err());
}

#[test]
fn security_scan_decodes_actions_profiles_and_nullable_options() {
    let config = AgentConfig::default();
    let digest = "a".repeat(64);
    for call in [
        json!({"tool":"security_scan","action":"doctor"}),
        json!({"tool":"security_scan","action":"scan"}),
        json!({"tool":"security_scan","action":"image","image":format!("registry.example/team/app@sha256:{digest}")}),
        json!({"tool":"security_scan","action":"web","target":"http://127.0.0.1:8080"}),
        json!({"tool":"security_scan","action":"update_databases","offline":false}),
        json!({"tool":"security_scan","action":"doctor","path":null,"profile":null,"image":null,"target":null,"offline":null,"timeout_seconds":null}),
        json!({"tool":"security_scan","action":"scan","path":null,"profile":null,"offline":null,"timeout_seconds":null}),
    ] {
        let parsed = parse_call(&call.to_string(), &config)
            .unwrap_or_else(|error| panic!("valid call rejected: {call}: {error}"));
        assert_eq!(parsed.name(), "security_scan");
    }
    for profile in [
        "quick",
        "standard",
        "secrets",
        "dependencies",
        "sast",
        "lint",
        "supply_chain",
        "iac",
        "malware",
    ] {
        let call = json!({"tool":"security_scan","action":"scan","profile":profile});
        assert!(parse_call(&call.to_string(), &config).is_ok(), "{profile}");
    }
}

#[test]
fn security_scan_rejects_unknown_and_action_inappropriate_options() {
    let config = AgentConfig::default();
    parse_call(r#"{"tool":"security_scan","action":"scan"}"#, &config).unwrap();
    for call in [
        json!({"tool":"security_scan"}),
        json!({"tool":"security_scan","action":"run"}),
        json!({"tool":"security_scan","action":"SCAN"}),
        json!({"tool":"security_scan","action":"scan","profile":"all"}),
        json!({"tool":"security_scan","action":"scan","extra":null}),
        json!({"tool":"security_scan","action":"scan","command":"id"}),
        json!({"tool":"security_scan","action":"scan","cwd":"/tmp"}),
        json!({"tool":"security_scan","action":"scan","image":"registry/app"}),
        json!({"tool":"security_scan","action":"scan","target":"http://127.0.0.1:80"}),
        json!({"tool":"security_scan","action":"doctor","path":"."}),
        json!({"tool":"security_scan","action":"doctor","profile":"standard"}),
        json!({"tool":"security_scan","action":"doctor","offline":false}),
        json!({"tool":"security_scan","action":"doctor","image":"registry/app"}),
        json!({"tool":"security_scan","action":"doctor","target":"http://127.0.0.1:80"}),
        json!({"tool":"security_scan","action":"image"}),
        json!({"tool":"security_scan","action":"image","image":null}),
        json!({"tool":"security_scan","action":"web"}),
        json!({"tool":"security_scan","action":"web","target":null}),
        json!({"tool":"security_scan","action":"web","target":"http://127.0.0.1:80","offline":true}),
        json!({"tool":"security_scan","action":"web","target":"http://127.0.0.1:80","offline":false}),
        json!({"tool":"security_scan","action":"web","target":"http://127.0.0.1:80","path":"."}),
        json!({"tool":"security_scan","action":"web","target":"http://127.0.0.1:80","profile":"quick"}),
        json!({"tool":"security_scan","action":"update_databases"}),
        json!({"tool":"security_scan","action":"update_databases","offline":true}),
        json!({"tool":"security_scan","action":"update_databases","offline":null}),
        json!({"tool":"security_scan","action":"update_databases","offline":false,"path":"."}),
        json!({"tool":"security_scan","action":"update_databases","offline":false,"profile":"quick"}),
    ] {
        assert!(
            parse_call(&call.to_string(), &config).is_err(),
            "accepted {call}"
        );
    }
    let image = format!("registry.example/app@sha256:{}", "a".repeat(64));
    for (key, value) in [
        ("path", json!(".")),
        ("profile", json!("standard")),
        ("target", json!("http://127.0.0.1:80")),
    ] {
        let mut call = json!({"tool":"security_scan","action":"image","image":image});
        call[key] = value;
        assert!(
            parse_call(&call.to_string(), &config).is_err(),
            "accepted {call}"
        );
    }
}

#[test]
fn security_scan_rejects_wrong_types_and_out_of_range_timeouts() {
    let config = AgentConfig::default();
    for timeout in [10, 540] {
        let call = json!({"tool":"security_scan","action":"scan","timeout_seconds":timeout});
        assert!(parse_call(&call.to_string(), &config).is_ok());
    }
    for timeout in [
        json!(0),
        json!(9),
        json!(541),
        json!(-1),
        json!(10.5),
        json!("540"),
        json!(true),
    ] {
        let call = json!({"tool":"security_scan","action":"scan","timeout_seconds":timeout});
        assert!(
            parse_call(&call.to_string(), &config).is_err(),
            "accepted {call}"
        );
    }
    for (key, value) in [
        ("action", Value::Null),
        ("action", json!(1)),
        ("path", json!(1)),
        ("path", json!("")),
        ("path", json!(" \t")),
        ("path", json!("a\0b")),
        ("path", json!("a".repeat(4097))),
        ("profile", json!([])),
        ("offline", json!("false")),
        ("offline", json!(0)),
    ] {
        let mut call = json!({"tool":"security_scan","action":"scan"});
        call[key] = value;
        assert!(
            parse_call(&call.to_string(), &config).is_err(),
            "accepted {call}"
        );
    }
}

#[test]
fn security_scan_image_requires_an_explicit_registry_and_lowercase_sha256_digest() {
    let config = AgentConfig::default();
    let digest = "a".repeat(64);
    for image in [
        format!("registry.example/team/app@sha256:{digest}"),
        format!("localhost:5000/app-one_v2.3@sha256:{digest}"),
        format!("registry/app@sha256:{digest}"),
    ] {
        let call = json!({"tool":"security_scan","action":"image","image":image});
        assert!(parse_call(&call.to_string(), &config).is_ok(), "{image}");
    }
    for image in [
        "registry.example/app:latest".to_string(),
        format!("app@sha256:{digest}"),
        format!("registry.example/app:tag@sha256:{digest}"),
        format!("https://registry.example/app@sha256:{digest}"),
        format!("user:password@registry.example/app@sha256:{digest}"),
        format!("registry.example/app@sha256:{}", "A".repeat(64)),
        format!("registry.example/app@sha256:{}", "a".repeat(63)),
        format!("registry.example/app@sha256:{}", "a".repeat(65)),
        format!("registry.example/app@sha256:{}g", "a".repeat(63)),
        format!("registry.example/App@sha256:{digest}"),
        format!("registry.example/app__name@sha256:{digest}"),
        format!("registry..example/app@sha256:{digest}"),
        format!("-registry.example/app@sha256:{digest}"),
        format!("registry.example-/app@sha256:{digest}"),
        format!("registry.example:0/app@sha256:{digest}"),
        format!("registry.example:65536/app@sha256:{digest}"),
        format!("registry.example:abc/app@sha256:{digest}"),
        format!("registry.example/../app@sha256:{digest}"),
        format!("registry.example/app@sha256:{digest} "),
        format!("registry.example/app@sha256:{digest};id"),
        format!("registry.example/{}@sha256:{digest}", "a".repeat(512)),
    ] {
        let call = json!({"tool":"security_scan","action":"image","image":image});
        assert!(
            parse_call(&call.to_string(), &config).is_err(),
            "accepted {image}"
        );
    }
}

#[test]
fn security_scan_web_accepts_only_literal_loopback_http_root_origins() {
    let config = AgentConfig::default();
    for target in [
        "http://127.0.0.1:1",
        "http://127.0.0.1:65535/",
        "http://[::1]:8080",
        "http://[::1]:80/",
    ] {
        let call = json!({"tool":"security_scan","action":"web","target":target});
        assert!(parse_call(&call.to_string(), &config).is_ok(), "{target}");
    }
    for target in [
        "http://localhost:8080",
        "http://example.com:8080",
        "https://127.0.0.1:8080",
        "HTTP://127.0.0.1:8080",
        "http://127.1:8080",
        "http://2130706433:8080",
        "http://127.0.0.2:8080",
        "http://[::ffff:127.0.0.1]:8080",
        "http://127.0.0.1",
        "http://[::1]",
        "http://127.0.0.1:0",
        "http://127.0.0.1:65536",
        "http://127.0.0.1:+80",
        "http://127.0.0.1:000080",
        "http://127.0.0.1:80/path",
        "http://127.0.0.1:80//",
        "http://127.0.0.1:80/..",
        "http://127.0.0.1:80?",
        "http://127.0.0.1:80/#",
        "http://127.0.0.1:80/#fragment",
        "http://user@127.0.0.1:80",
        "http://127.0.0.1:80@evil.example",
        "http://127.0.0.1:80\\evil.example",
        "http://127.0.0.1:80\n",
        " http://127.0.0.1:80",
        "http://127.0.0.1:80/%2e%2e",
    ] {
        let call = json!({"tool":"security_scan","action":"web","target":target});
        assert!(
            parse_call(&call.to_string(), &config).is_err(),
            "accepted {target}"
        );
    }
}

#[test]
fn security_scan_rejects_duplicate_json_arguments() {
    let config = AgentConfig::default();
    parse_call(r#"{"tool":"security_scan","action":"scan"}"#, &config).unwrap();
    for call in [
        r#"{"tool":"security_scan","action":"doctor","action":"scan"}"#,
        r#"{"tool":"security_scan","action":"scan","offline":false,"offline":true}"#,
        r#"{"tool":"security_scan","action":"scan","path":"one","path":"two"}"#,
        r#"{"tool":"security_scan","tool":"security_scan","action":"doctor"}"#,
    ] {
        assert!(parse_call(call, &config).is_err(), "accepted {call}");
    }
}

#[tokio::test]
async fn security_scan_dispatches_one_quoted_json_argument_with_bounded_box_timeouts() {
    let h = harness(
        vec![
            json!({"tool":"security_scan","action":"scan"}),
            json!({"tool":"security_scan","action":"scan","path":"it's here; $(id) `whoami`","profile":"secrets","offline":false,"timeout_seconds":10}),
            json!({"tool":"security_scan","action":"doctor","timeout_seconds":30}),
            json!({"tool":"security_scan","action":"web","target":"http://[::1]:8080/"}),
            json!({"tool":"security_scan","action":"image","image":"registry.example/app@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}),
            json!({"tool":"security_scan","action":"update_databases","offline":false}),
            json!({"tool":"security_scan","action":"scan","path":null,"profile":null,"offline":null,"timeout_seconds":null}),
            reply("Scanned"),
        ],
        SessionLimits::default(),
    );
    let id = h.run("Run the security checks").await;
    let requests = h.boxes.exec_requests.lock().unwrap().clone();
    let expected = [
        (
            r#"/usr/bin/python3 -I /opt/box/bin/security-scan '{"action":"scan","offline":true,"path":".","profile":"standard","timeout_seconds":540}'"#,
            570,
        ),
        (
            r#"/usr/bin/python3 -I /opt/box/bin/security-scan '{"action":"scan","offline":false,"path":"it'\''s here; $(id) `whoami`","profile":"secrets","timeout_seconds":10}'"#,
            40,
        ),
        (
            r#"/usr/bin/python3 -I /opt/box/bin/security-scan '{"action":"doctor","timeout_seconds":30}'"#,
            60,
        ),
        (
            r#"/usr/bin/python3 -I /opt/box/bin/security-scan '{"action":"web","target":"http://[::1]:8080/","timeout_seconds":540}'"#,
            570,
        ),
        (
            r#"/usr/bin/python3 -I /opt/box/bin/security-scan '{"action":"image","image":"registry.example/app@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","offline":true,"timeout_seconds":540}'"#,
            570,
        ),
        (
            r#"/usr/bin/python3 -I /opt/box/bin/security-scan '{"action":"update_databases","offline":false,"timeout_seconds":540}'"#,
            570,
        ),
        (
            r#"/usr/bin/python3 -I /opt/box/bin/security-scan '{"action":"scan","offline":true,"path":".","profile":"standard","timeout_seconds":540}'"#,
            570,
        ),
    ];
    assert_eq!(
        requests.len(),
        expected.len(),
        "all checks must dispatch to the box"
    );
    for (request, (command, timeout)) in requests.iter().zip(expected) {
        assert_eq!(request.command, command);
        assert_eq!(request.timeout_seconds, Some(timeout));
        assert_eq!(request.cwd, None);
        assert_eq!(request.stdin, None);
    }
    assert!(h.results(&id).iter().all(|result| result["ok"] == true));
    assert!(h.model.calls()[0].0.contains(r#"{"tool":"security_scan""#));
}

#[tokio::test]
async fn security_scan_can_be_disabled_without_allocating_a_box() {
    let h = harness(
        vec![
            json!({"tool":"agent_config","action":"update","config":{"disabled_tools":["security_scan"]}}),
            json!({"tool":"security_scan","action":"doctor"}),
            reply("Disabled"),
        ],
        SessionLimits::default(),
    );
    let id = h.run("Disable security checks").await;
    let results = h.results(&id);
    assert_eq!(results[0]["ok"], true);
    assert_eq!(results[1]["ok"], false);
    assert!(results[1]["output"].as_str().unwrap().contains("disabled"));
    assert_eq!(h.boxes.creates.load(Ordering::SeqCst), 0);
    assert!(h.boxes.exec_requests.lock().unwrap().is_empty());
    assert!(!h.model.calls()[1].0.contains(r#"{"tool":"security_scan""#));
}

#[tokio::test]
async fn security_scan_audit_skill_is_indexed_and_loads_as_one_skill_document() {
    let h = harness(
        vec![
            json!({"tool":"skill","action":"list"}),
            json!({"tool":"skill","action":"load","name":"security-audit"}),
            reply("Loaded"),
        ],
        SessionLimits::default(),
    );
    let id = h.run("Load security audit guidance").await;
    let results = h.results(&id);
    assert_eq!(results[1]["ok"], true);
    let index: Vec<Value> = serde_json::from_str(results[0]["output"].as_str().unwrap()).unwrap();
    let entries: Vec<_> = index
        .iter()
        .filter(|entry| entry["name"] == "security-audit")
        .collect();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["builtin"], true);
    assert!(h.model.calls()[0].0.contains("- security-audit:"));
    let boxes = h.boxes.boxes.lock().unwrap();
    let copied = boxes["box-1"]
        .get(".skills/security-audit/SKILL.md")
        .unwrap();
    let (name, description, body) = parse_skill_md(std::str::from_utf8(copied).unwrap()).unwrap();
    assert_eq!(name, "security-audit");
    assert_eq!(description, entries[0]["description"].as_str().unwrap());
    assert!(
        !body.starts_with("---"),
        "embedded asset must be a body, not nested front matter"
    );
}

#[tokio::test]
async fn ten_megabytes_of_output_is_truncated_to_the_context_budget() {
    let h = harness(
        vec![
            bash("big"),
            json!({"tool":"read_file","path":"blob.bin"}),
            reply("Done"),
        ],
        SessionLimits::default(),
    );
    let session = h.sessions.create("alice", None).unwrap();
    let mut blob = vec![0xffu8; 10 * 1024 * 1024];
    blob[0] = 0;
    h.sessions
        .write_file("alice", &session.id, "blob.bin", Bytes::from(blob))
        .await
        .unwrap();
    h.sessions.send("alice", &session.id, "Make noise").unwrap();
    h.sessions
        .wait_idle("alice", &session.id, WAIT)
        .await
        .unwrap();
    let results = h.results(&session.id);
    let output = results[0]["output"].as_str().unwrap();
    assert!(output.len() <= 28_000 / 5);
    assert_eq!(results[0]["truncated"], true);
    assert!(output.contains("bytes omitted"));
    assert!(
        results[1]["output"]
            .as_str()
            .unwrap()
            .starts_with("Binary file")
    );
    for (system, messages) in h.model.calls() {
        let size = serde_json::to_vec(&messages).unwrap().len() + system.len();
        assert!(size <= 28_000, "context of {size} bytes exceeds the budget");
    }
    // Invalid UTF-8 never panics and stays within the cap.
    let (text, cut) = truncate_bytes(&[0xf0, 0x9f, 0x98, 0xff, 0xfe].repeat(100_000), 1000);
    assert!(cut && text.len() <= 1000);
}

#[tokio::test]
async fn malformed_calls_go_back_to_the_model_as_errors() {
    let h = harness(vec![], SessionLimits::default());
    h.model.push_raw("I will now run ls.");
    h.model
        .push(json!({"tool":"bash","command":"ls","unexpected":1}));
    h.model.push(json!({"tool":"teleport"}));
    h.model.push(reply("Recovered"));
    let id = h.run("List files").await;
    let results = h.results(&id);
    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|r| r["ok"] == false));
    assert!(
        results[1]["output"]
            .as_str()
            .unwrap()
            .contains("unknown field")
    );
    assert!(
        results[2]["output"]
            .as_str()
            .unwrap()
            .contains("unknown tool")
    );
    assert_eq!(
        h.sessions.get("alice", &id).unwrap().unwrap().status,
        SessionStatus::Idle
    );
    let config = AgentConfig::default();
    assert!(parse_call(r#"{"tool":"browser","action":"fly"}"#, &config).is_err());
    assert!(
        parse_call(
            "```json\n{\"tool\":\"reply\",\"text\":\"hi\"}\n```",
            &config
        )
        .is_ok()
    );
}

#[tokio::test]
async fn skills_are_loaded_into_the_workspace() {
    let h = harness(
        vec![
            json!({"tool":"skill","action":"save","name":"deploy","description":"How to deploy","body":"Run make deploy."}),
            json!({"tool":"skill","action":"load","name":"deploy"}),
            json!({"tool":"skill","action":"load","name":"app-testing"}),
            reply("Ok"),
        ],
        SessionLimits::default(),
    );
    let id = h.run("Learn").await;
    let results = h.results(&id);
    assert!(
        results[1]["output"]
            .as_str()
            .unwrap()
            .contains("Run make deploy.")
    );
    assert!(results[2]["output"].as_str().unwrap().contains("browser"));
    let boxes = h.boxes.boxes.lock().unwrap();
    let files = boxes.get("box-1").unwrap();
    assert!(files.contains_key(".skills/deploy/SKILL.md"));
    assert!(h.model.calls()[3].0.contains("- deploy: How to deploy"));
}

#[tokio::test]
async fn another_owner_cannot_get_send_exec_or_read_events() {
    let h = harness(vec![reply("hi")], SessionLimits::default());
    let id = h.run("Hello").await;
    let s = &h.sessions;
    assert!(s.get("bob", &id).unwrap().is_none());
    assert!(s.list("bob").unwrap().is_empty());
    let not_found = |e: anyhow::Error| {
        matches!(
            e.downcast_ref::<SessionError>(),
            Some(SessionError::NotFound)
        )
    };
    assert!(not_found(s.send("bob", &id, "hi").unwrap_err()));
    assert!(not_found(s.events("bob", &id, 0, 10).unwrap_err()));
    assert!(not_found(s.cancel("bob", &id).unwrap_err()));
    assert!(not_found(
        s.exec(
            "bob",
            &id,
            ExecRequest {
                command: "id".into(),
                ..Default::default()
            }
        )
        .await
        .unwrap_err()
    ));
    assert!(not_found(s.read_file("bob", &id, "a").await.unwrap_err()));
    assert!(not_found(
        s.write_file("bob", &id, "a", Bytes::from_static(b"x"))
            .await
            .unwrap_err()
    ));
    assert!(not_found(s.close("bob", &id).await.unwrap_err()));
    assert!(not_found(s.wait_idle("bob", &id, WAIT).await.unwrap_err()));
    assert!(h.boxes.commands.lock().unwrap().is_empty());
    // The owner can close it; the box is destroyed and further messages are refused.
    s.exec(
        "alice",
        &id,
        ExecRequest {
            command: "true".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    s.close("alice", &id).await.unwrap();
    let view = s.get("alice", &id).unwrap().unwrap();
    assert_eq!(view.status, SessionStatus::Closed);
    assert!(view.box_id.is_none());
    assert!(h.boxes.boxes.lock().unwrap().is_empty());
    assert!(matches!(
        s.send("alice", &id, "more")
            .unwrap_err()
            .downcast_ref::<SessionError>(),
        Some(SessionError::Closed)
    ));
}

#[tokio::test]
async fn replace_all_cannot_grow_a_file_past_the_file_limit() {
    let h = harness(
        vec![
            json!({"tool":"write_file","path":"a.txt","content":"a".repeat(1000)}),
            json!({"tool":"edit_file","path":"a.txt","old":"a","new":"x".repeat(30_000),"replace_all":true}),
            reply("Tried"),
        ],
        SessionLimits::default(),
    );
    let id = h.run("Grow it").await;
    let results = h.results(&id);
    assert_eq!(results[1]["ok"], false);
    assert!(
        results[1]["output"].as_str().unwrap().contains("25 MiB"),
        "{}",
        results[1]["output"]
    );
    {
        let boxes = h.boxes.boxes.lock().unwrap();
        assert_eq!(boxes["box-1"]["a.txt"].len(), 1000, "the file is unchanged");
    }
    // The size is checked before replacing, for unique matches too.
    let big = "y".repeat(25 * 1024 * 1024);
    assert!(apply_edit(&big, "y", "zz", true).is_err());
    assert!(apply_edit("abc", "b", &"q".repeat(25 * 1024 * 1024), false).is_err());
    assert_eq!(
        apply_edit("aXaXa", "X", "--", true).unwrap(),
        ("a--a--a".to_string(), 2)
    );
}

#[tokio::test]
async fn expanded_and_generated_commands_obey_the_command_limit() {
    let h = harness(
        vec![
            json!({"tool":"triple","a":"z".repeat(30_000)}),
            json!({"tool":"browser","action":"eval","js":"j".repeat(70_000)}),
            json!({"tool":"triple","a":"ok"}),
            reply("Done"),
        ],
        SessionLimits::default(),
    );
    let config = AgentConfig {
        custom_tools: vec![CustomTool {
            name: "triple".into(),
            description: "Echo three times".into(),
            command_template: "echo {{a}} {{a}} {{a}}".into(),
        }],
        ..Default::default()
    };
    h.ledger
        .update_agent_config("alice", &config, "add tool", "user")
        .unwrap();
    let id = h.run("Expand").await;
    let results = h.results(&id);
    assert_eq!(results[0]["ok"], false);
    assert!(results[0]["output"].as_str().unwrap().contains("65536"));
    assert_eq!(results[1]["ok"], false);
    assert!(results[1]["output"].as_str().unwrap().contains("65536"));
    let commands = h.boxes.commands.lock().unwrap().clone();
    assert_eq!(
        commands.len(),
        1,
        "only the small command ran: {commands:?}"
    );
    assert_eq!(commands[0].1, "echo 'ok' 'ok' 'ok'");
    let args = json!({"a": "z".repeat(40_000)});
    let error = expand_template("{{a}}{{a}}", args.as_object().unwrap()).unwrap_err();
    assert!(error.to_string().contains("65536"));
}

#[tokio::test]
async fn spend_caps_end_the_turn_before_the_next_model_call() {
    // Per turn: each fake call costs $0.001.
    let h = harness(
        vec![],
        SessionLimits {
            max_turn_usd: 0.0025,
            ..Default::default()
        },
    );
    *h.model.fallback.lock().unwrap() = Some(bash("true").to_string());
    let id = h.run("Spend").await;
    let view = h.sessions.get("alice", &id).unwrap().unwrap();
    assert_eq!(view.status, SessionStatus::Failed);
    assert_eq!(h.model.calls().len(), 3);
    let events = h.events(&id);
    let error = events.iter().find(|e| e.kind == "error").unwrap();
    assert!(
        error.payload["message"]
            .as_str()
            .unwrap()
            .contains("spend limit")
    );
    assert_eq!(events.last().unwrap().payload["reason"], "spend_limit");

    // Per session: the second turn is refused before any model call.
    let h = harness(
        vec![bash("true"), reply("First")],
        SessionLimits {
            max_session_usd: 0.0015,
            ..Default::default()
        },
    );
    let id = h.run("One").await;
    assert_eq!(
        h.sessions.get("alice", &id).unwrap().unwrap().status,
        SessionStatus::Idle
    );
    h.model.push(reply("Second"));
    h.sessions.send("alice", &id, "Two").unwrap();
    let view = h.sessions.wait_idle("alice", &id, WAIT).await.unwrap();
    assert_eq!(view.status, SessionStatus::Failed);
    assert_eq!(h.model.calls().len(), 2);
    assert_eq!(
        h.events(&id).last().unwrap().payload["reason"],
        "spend_limit"
    );
}

#[tokio::test]
async fn a_model_error_that_was_billed_is_still_recorded() {
    let h = harness(vec![bash("true")], SessionLimits::default());
    h.model
        .steps
        .lock()
        .unwrap()
        .push_back(Step::FailCharged(0.25));
    let id = h.run("Fail late").await;
    let view = h.sessions.get("alice", &id).unwrap().unwrap();
    assert_eq!(view.status, SessionStatus::Failed);
    assert!((view.spend_usd - 0.251).abs() < 1e-9, "{}", view.spend_usd);
    let events = h.events(&id);
    let error = events.iter().find(|e| e.kind == "error").unwrap();
    assert!(
        error.payload["message"]
            .as_str()
            .unwrap()
            .contains("gateway broke after billing")
    );
}

#[tokio::test]
async fn a_panic_inside_a_turn_does_not_leave_the_session_running() {
    let h = harness(vec![bash("panic")], SessionLimits::default());
    let session = h.sessions.create("alice", None).unwrap();
    h.sessions.send("alice", &session.id, "Crash").unwrap();
    let view = h
        .sessions
        .wait_idle("alice", &session.id, WAIT)
        .await
        .unwrap();
    assert_eq!(view.status, SessionStatus::Failed);
    let events = h.events(&session.id);
    assert!(events.iter().any(|e| e.kind == "error"));
    assert_eq!(events.last().unwrap().payload["status"], "failed");
    // The turn is forgotten: the next message starts a new turn, not an interjection.
    h.model.push(reply("Back"));
    let outcome = h.sessions.send("alice", &session.id, "Again").unwrap();
    assert!(!outcome.interjection);
    let view = h
        .sessions
        .wait_idle("alice", &session.id, WAIT)
        .await
        .unwrap();
    assert_eq!(view.status, SessionStatus::Idle);
    // Short invalid UTF-8 whose lossy text outgrows the cap never underflows.
    for len in [1, 90, 100, 200, 256] {
        let (text, _) = truncate_bytes(&vec![0xff; len], 0);
        assert!(text.len() <= 256, "{len}: {}", text.len());
    }
}

#[tokio::test]
async fn an_oversized_tool_call_is_clipped_in_the_log_or_refused() {
    let mut wide = serde_json::Map::new();
    wide.insert("tool".into(), json!("browser"));
    wide.insert("action".into(), json!("eval"));
    for i in 0..30 {
        wide.insert(format!("k{i}"), json!("w".repeat(16_000)));
    }
    let mut many = serde_json::Map::new();
    many.insert("tool".into(), json!("browser"));
    many.insert("action".into(), json!("snapshot"));
    for i in 0..3000 {
        many.insert(format!("key{i}"), json!("m".repeat(100)));
    }
    let h = harness(
        vec![Value::Object(wide), Value::Object(many), reply("Survived")],
        SessionLimits::default(),
    );
    let id = h.run("Big calls").await;
    let view = h.sessions.get("alice", &id).unwrap().unwrap();
    assert_eq!(view.status, SessionStatus::Idle);
    let calls: Vec<Value> = h
        .events(&id)
        .into_iter()
        .filter(|e| e.kind == "tool_call")
        .map(|e| e.payload)
        .collect();
    assert_eq!(calls.len(), 2);
    for call in &calls {
        assert!(serde_json::to_vec(call).unwrap().len() <= 64 * 1024);
    }
    assert_eq!(calls[0]["call"]["action"], "eval");
    assert_eq!(calls[1]["oversized"], true);
    let results = h.results(&id);
    assert_eq!(results[0]["ok"], false, "the command limit refused it");
    assert_eq!(results[1]["ok"], false);
    assert!(
        results[1]["output"]
            .as_str()
            .unwrap()
            .contains("too large to record")
    );
    assert!(h.boxes.commands.lock().unwrap().is_empty());
}

#[test]
fn tool_calls_are_found_in_prose_fences_and_after_stray_braces() {
    let config = AgentConfig::default();
    let text = |call: ToolCall| match call {
        ToolCall::Reply { text, .. } => text,
        other => panic!("not a reply: {other:?}"),
    };
    let prose = r#"I'll keep {braces} and {"a":1} aside. {"tool":"reply","text":"hi"} Done }."#;
    assert_eq!(text(parse_call(prose, &config).unwrap()), "hi");
    let two = "{\"tool\":\"reply\",\"text\":\"first\"}\n{\"tool\":\"reply\",\"text\":\"second\"}";
    assert_eq!(text(parse_call(two, &config).unwrap()), "first");
    let fenced = "```json\n{\"tool\":\"reply\",\"text\":\"fenced\"}\n```\nNote: done }";
    assert_eq!(text(parse_call(fenced, &config).unwrap()), "fenced");
    let nested = r#"{"tool":"reply","text":"say {\"tool\":\"bash\"}"}"#;
    assert_eq!(
        text(parse_call(nested, &config).unwrap()),
        r#"say {"tool":"bash"}"#
    );
    // Strict decoding still applies to the object that was found.
    let error = parse_call(r#"Sure: {"tool":"bash","command":"ls","x":1}"#, &config)
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown field"), "{error}");
    assert!(parse_call(r#"{"tool":"bash""#, &config).is_err());
    assert!(parse_call("no json here", &config).is_err());
}

#[tokio::test]
async fn cancelled_interjections_never_reach_a_later_turn() {
    let h = harness(vec![], SessionLimits::default());
    let (started, release) = h.model.push_gated(bash("echo 1"));
    let session = h.sessions.create("alice", None).unwrap();
    h.sessions.send("alice", &session.id, "Start").unwrap();
    started.notified().await;
    let outcome = h.sessions.send("alice", &session.id, "Ignore me").unwrap();
    assert!(outcome.interjection);
    h.sessions.cancel("alice", &session.id).unwrap();
    h.sessions
        .wait_idle("alice", &session.id, WAIT)
        .await
        .unwrap();
    release.add_permits(1);
    h.model.push(reply("Fresh"));
    h.sessions.send("alice", &session.id, "Next").unwrap();
    let view = h
        .sessions
        .wait_idle("alice", &session.id, WAIT)
        .await
        .unwrap();
    assert_eq!(view.status, SessionStatus::Idle);
    let calls = h.model.calls();
    assert_eq!(calls.len(), 2);
    let text = FakeModel::all_text(&calls[1].1);
    assert!(text.contains("Start") && text.contains("Next"), "{text}");
    assert!(!text.contains("Ignore me"), "{text}");
}

#[tokio::test]
async fn closing_a_session_forgets_its_box_lock() {
    let h = harness(vec![], SessionLimits::default());
    let session = h.sessions.create("alice", None).unwrap();
    h.sessions
        .exec(
            "alice",
            &session.id,
            ExecRequest {
                command: "true".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(h.sessions.tracked_box_locks(), 1);
    h.sessions.close("alice", &session.id).await.unwrap();
    assert_eq!(h.sessions.tracked_box_locks(), 0);
    // A late box access on the closed session does not bring it back.
    assert!(
        h.sessions
            .read_file("alice", &session.id, "a")
            .await
            .is_err()
    );
    assert_eq!(h.sessions.tracked_box_locks(), 0);
}

fn blocking_exec(sessions: &Sessions, id: &str) -> tokio::task::JoinHandle<Result<ExecResult>> {
    let sessions = sessions.clone();
    let id = id.to_string();
    tokio::spawn(async move {
        sessions
            .exec(
                "alice",
                &id,
                ExecRequest {
                    command: "block".into(),
                    ..Default::default()
                },
            )
            .await
    })
}

/// Waits until `n` fake execs are running, lets the rest queue, then releases them all.
async fn settle_and_release(
    h: &Harness,
    n: usize,
    tasks: Vec<tokio::task::JoinHandle<Result<ExecResult>>>,
) {
    tokio::time::timeout(WAIT, async {
        while h.boxes.in_flight.load(Ordering::SeqCst) < n {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("box calls did not start");
    // Give queued calls a chance to (wrongly) start.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(h.boxes.max_in_flight.load(Ordering::SeqCst), n);
    let all = futures_util::future::join_all(tasks);
    tokio::pin!(all);
    let results = tokio::time::timeout(WAIT, async {
        loop {
            h.boxes.exec_release.notify_waiters();
            tokio::select! {
                results = &mut all => break results,
                _ = tokio::time::sleep(Duration::from_millis(10)) => {}
            }
        }
    })
    .await
    .expect("box calls did not finish");
    for result in results {
        result.unwrap().unwrap();
    }
    assert_eq!(h.boxes.max_in_flight.load(Ordering::SeqCst), n);
}

/// Regression: box exec and file calls were unbounded per session.
#[tokio::test]
async fn box_calls_per_session_are_limited() {
    let h = harness(
        vec![],
        SessionLimits {
            max_box_calls_per_session: 2,
            ..SessionLimits::default()
        },
    );
    let session = h.sessions.create("alice", None).unwrap();
    let tasks = (0..5)
        .map(|_| blocking_exec(&h.sessions, &session.id))
        .collect();
    settle_and_release(&h, 2, tasks).await;
}

/// Regression: box exec and file calls were unbounded across sessions.
#[tokio::test]
async fn box_calls_across_sessions_are_limited() {
    let h = harness(
        vec![],
        SessionLimits {
            max_box_calls: 3,
            ..SessionLimits::default()
        },
    );
    let mut tasks = Vec::new();
    for _ in 0..3 {
        let session = h.sessions.create("alice", None).unwrap();
        tasks.push(blocking_exec(&h.sessions, &session.id));
        tasks.push(blocking_exec(&h.sessions, &session.id));
    }
    settle_and_release(&h, 3, tasks).await;
}

/// Regression: an upload was copied on its way to the box client.
#[tokio::test]
async fn uploads_reach_the_box_client_without_a_copy() {
    let h = harness(vec![], SessionLimits::default());
    let session = h.sessions.create("alice", None).unwrap();
    let upload = Bytes::from(vec![7u8; 1024 * 1024]);
    let address = upload.as_ptr() as usize;
    let written = h
        .sessions
        .write_file("alice", &session.id, "upload.bin", upload)
        .await
        .unwrap();
    assert_eq!(written.bytes, 1024 * 1024);
    assert_eq!(h.boxes.last_write_ptr.load(Ordering::SeqCst), address);
}

/// Regression: an owner could open sessions without bound.
#[tokio::test]
async fn open_sessions_are_capped_per_owner_and_closed_ones_do_not_count() {
    let h = harness(
        vec![],
        SessionLimits {
            max_open_sessions_per_owner: 3,
            ..SessionLimits::default()
        },
    );
    let ids: Vec<String> = (0..3)
        .map(|_| h.sessions.create("alice", None).unwrap().id)
        .collect();
    let error = h.sessions.create("alice", None).unwrap_err();
    assert!(matches!(
        error.downcast_ref::<SessionError>(),
        Some(SessionError::TooManySessions)
    ));
    assert_eq!(error.to_string(), "too many open sessions; close one");
    h.sessions.create("bob", None).unwrap();
    h.sessions.close("alice", &ids[1]).await.unwrap();
    h.sessions.create("alice", None).unwrap();
    assert!(h.sessions.create("alice", None).is_err());
}

#[tokio::test]
async fn owner_slots_are_capped_and_given_back_on_drop() {
    let h = harness(
        vec![],
        SessionLimits {
            max_streams_per_owner: 2,
            max_long_requests_per_owner: 1,
            ..SessionLimits::default()
        },
    );
    let s = &h.sessions;
    let a = s.stream_slot("alice").unwrap();
    let _b = s.stream_slot("alice").unwrap();
    assert!(matches!(
        s.stream_slot("alice")
            .unwrap_err()
            .downcast_ref::<SessionError>(),
        Some(SessionError::TooManyStreams)
    ));
    s.stream_slot("bob").unwrap();
    drop(a);
    let _c = s.stream_slot("alice").unwrap();
    let long = s.long_request_slot("alice").unwrap();
    assert!(matches!(
        s.long_request_slot("alice")
            .unwrap_err()
            .downcast_ref::<SessionError>(),
        Some(SessionError::TooManyRequests)
    ));
    drop(long);
    s.long_request_slot("alice").unwrap();
}

/// Every model call fits the compact budget: 28,000 bytes with the system prompt and 13
/// messages besides it, starting with a user message and alternating roles.
fn assert_within_budget(model: &FakeModel) {
    for (n, (system, messages)) in model.calls().iter().enumerate() {
        let size = serde_json::to_vec(messages).unwrap().len() + system.len();
        assert!(size <= 28_000, "call {n}: context of {size} bytes");
        assert!(
            messages.len() <= 13,
            "call {n}: {} messages",
            messages.len()
        );
        assert_eq!(messages[0]["role"], "user", "call {n}");
        for pair in messages.windows(2) {
            assert_ne!(
                pair[0]["role"], pair[1]["role"],
                "call {n}: roles alternate"
            );
        }
    }
}

#[tokio::test]
async fn an_identical_call_loop_is_warned_then_stopped() {
    let h = harness(vec![], SessionLimits::default());
    // One call in three spellings: key order and spacing do not make it a new call.
    let spellings = [
        r#"{"tool":"bash","command":"same"}"#,
        r#"{"command":"same","tool":"bash"}"#,
        r#"{ "tool": "bash", "command": " same " }"#,
    ];
    for i in 0..20 {
        h.model.push_raw(spellings[i % 3]);
    }
    let id = h.run("Make the chart").await;
    let view = h.sessions.get("alice", &id).unwrap().unwrap();
    assert_eq!(view.status, SessionStatus::Failed);
    assert_eq!(
        h.model.calls().len(),
        6,
        "the sixth identical call ended the turn"
    );
    assert_eq!(
        h.boxes.commands.lock().unwrap().len(),
        2,
        "only the first two calls ran"
    );
    let results = h.results(&id);
    assert_eq!(results.len(), 6, "every recorded call has a result");
    assert!(results[..2].iter().all(|r| r["ok"] == true));
    for (n, warned) in results[2..5].iter().enumerate() {
        assert_eq!(warned["ok"], false);
        let output = warned["output"].as_str().unwrap();
        assert!(
            output.contains(&format!(
                "You have made this exact call {} times in a row with the same result. Change approach",
                n + 3
            )),
            "{output}"
        );
    }
    assert!(
        results[5]["output"]
            .as_str()
            .unwrap()
            .starts_with("not run")
    );
    // The warning reached the model.
    assert!(FakeModel::all_text(&h.model.calls()[3].1).contains("Change approach"));
    let events = h.events(&id);
    let error = events.iter().find(|e| e.kind == "error").unwrap();
    assert!(
        error.payload["message"]
            .as_str()
            .unwrap()
            .contains("same bash call 6 times in a row"),
        "{:?}",
        error.payload
    );
    let status = &events.last().unwrap().payload;
    assert_eq!(
        (&status["status"], &status["reason"]),
        (&json!("failed"), &json!("repeating"))
    );
}

#[tokio::test]
async fn polling_whose_output_changes_is_not_stopped() {
    let mut script: Vec<Value> = (0..8).map(|_| bash("poll")).collect();
    script.push(reply("The job finished"));
    let h = harness(script, SessionLimits::default());
    let id = h.run("Wait for the job").await;
    let view = h.sessions.get("alice", &id).unwrap().unwrap();
    assert_eq!(view.status, SessionStatus::Idle);
    assert_eq!(h.boxes.commands.lock().unwrap().len(), 8);
    assert!(h.results(&id).iter().all(|r| r["ok"] == true));
    assert!(h.events(&id).iter().all(|e| e.kind != "error"));
}

#[tokio::test]
async fn a_different_call_resets_the_repeat_count() {
    let h = harness(
        vec![
            bash("same"),
            bash("same"),
            bash("same"), // warned, not run
            bash("other"),
            bash("same"),
            bash("same"),
            bash("other"),
            bash("same"),
            bash("same"),
            reply("Done"),
        ],
        SessionLimits::default(),
    );
    let id = h.run("Work").await;
    let view = h.sessions.get("alice", &id).unwrap().unwrap();
    assert_eq!(view.status, SessionStatus::Idle);
    let ok: Vec<bool> = h.results(&id).iter().map(|r| r["ok"] == true).collect();
    assert_eq!(
        ok,
        vec![true, true, false, true, true, true, true, true, true]
    );
    assert_eq!(h.boxes.commands.lock().unwrap().len(), 8);
}

#[tokio::test]
async fn earlier_turns_stay_in_context_after_a_long_noisy_turn() {
    let h = harness(vec![], SessionLimits::default());
    for i in 1..=12 {
        h.model.push(bash(&format!("noise-t1-{i:02}")));
    }
    let long_reply = format!(
        "TURN1-REPLY North 2,100,750 {} END-OF-REPLY",
        "detail ".repeat(1500)
    );
    h.model.push(reply(&long_reply));
    // Enough steps that this turn alone overflows the window.
    for i in 1..=7 {
        h.model.push(bash(&format!("noise-t2-{i:02}")));
    }
    h.model.push(reply("Chart done"));
    let id = h.sessions.create("alice", None).unwrap().id;
    h.sessions
        .send("alice", &id, "TURN1-REQUEST make sales.csv and total it")
        .unwrap();
    h.sessions.wait_idle("alice", &id, WAIT).await.unwrap();
    let turn1_calls = h.model.calls().len();
    assert_eq!(turn1_calls, 13);
    h.sessions
        .send("alice", &id, "TURN2-REQUEST chart the totals")
        .unwrap();
    let view = h.sessions.wait_idle("alice", &id, WAIT).await.unwrap();
    assert_eq!(view.status, SessionStatus::Idle);
    let calls = h.model.calls();
    let turn2 = &calls[turn1_calls..];
    assert_eq!(turn2.len(), 8);

    let first = FakeModel::all_text(&turn2[0].1);
    assert!(first.contains("TURN1-REQUEST"), "{first}");
    assert!(
        first.contains("TURN1-REPLY North 2,100,750") && first.contains("END-OF-REPLY"),
        "the long reply is clipped, not dropped: {first}"
    );
    assert!(
        first.contains("from an earlier turn omitted; files they created are still in /workspace"),
        "{first}"
    );
    assert!(
        !first.contains("noise-t1-01"),
        "the oldest steps of the earlier turn went first"
    );
    for (n, (_, messages)) in turn2.iter().enumerate() {
        let text = FakeModel::all_text(messages);
        assert!(
            text.contains("TURN1-REQUEST") && text.contains("TURN1-REPLY"),
            "call {n} lost turn 1"
        );
        assert!(text.contains("TURN2-REQUEST"), "call {n}");
        // Earlier-turn steps are dropped before any step of this turn.
        let current = (1..=n)
            .filter(|i| text.contains(&format!("noise-t2-{i:02}")))
            .count();
        let earlier = (1..=12)
            .filter(|i| text.contains(&format!("noise-t1-{i:02}")))
            .count();
        if earlier > 0 {
            assert_eq!(
                current, n,
                "call {n} kept an earlier turn's steps over this one's"
            );
            // The ones that survive are the newest.
            assert!(
                (13 - earlier..=12).all(|i| text.contains(&format!("noise-t1-{i:02}"))),
                "call {n}"
            );
        }
        if n > 0 {
            assert!(
                text.contains(&format!("noise-t2-{n:02}")),
                "call {n} lost the newest step"
            );
        }
    }
    // By the last call this turn's oldest steps had to go too, after every earlier one.
    let last = FakeModel::all_text(&turn2[7].1);
    assert!((1..=12).all(|i| !last.contains(&format!("noise-t1-{i:02}"))));
    assert!(!last.contains("noise-t2-01"), "{last}");
    assert!(
        last.contains("from earlier in this turn omitted to fit the context"),
        "{last}"
    );
    assert!(
        h.model.calls()[0]
            .0
            .contains("Files from earlier turns persist in /workspace")
    );
    assert_within_budget(&h.model);
}

#[tokio::test]
async fn a_long_conversation_keeps_its_newest_turns_within_the_budget() {
    let h = harness(vec![], SessionLimits::default());
    let id = h.sessions.create("alice", None).unwrap().id;
    for turn in 1..=10 {
        h.model
            .push(reply(&format!("REPLY-{turn:02} {}", "r".repeat(3000))));
        h.sessions
            .send(
                "alice",
                &id,
                &format!("REQUEST-{turn:02} {}", "q".repeat(3000)),
            )
            .unwrap();
        let view = h.sessions.wait_idle("alice", &id, WAIT).await.unwrap();
        assert_eq!(view.status, SessionStatus::Idle);
    }
    assert_within_budget(&h.model);
    let last = FakeModel::all_text(&h.model.calls().last().unwrap().1);
    for kept in [
        "REQUEST-10",
        "REQUEST-09",
        "REPLY-09",
        "REQUEST-08",
        "REPLY-08",
    ] {
        assert!(last.contains(kept), "{kept} missing");
    }
    assert!(!last.contains("REQUEST-01") && !last.contains("REPLY-01"));
    assert!(last.contains("earlier messages"), "{last}");
}

fn is_not_found(e: anyhow::Error) -> bool {
    matches!(
        e.downcast_ref::<SessionError>(),
        Some(SessionError::NotFound)
    )
}

#[tokio::test]
async fn delete_removes_the_session_its_events_and_its_box() {
    let h = harness(vec![reply("hi")], SessionLimits::default());
    let id = h.run("Hello").await;
    h.sessions
        .exec(
            "alice",
            &id,
            ExecRequest {
                command: "true".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(h.boxes.boxes.lock().unwrap().len(), 1);
    assert!(
        !h.ledger
            .recent_session_events(&id, 1000)
            .unwrap()
            .is_empty()
    );
    // Another owner cannot delete it, and a missing id is not found either.
    assert!(is_not_found(
        h.sessions.delete("bob", &id).await.unwrap_err()
    ));
    assert!(is_not_found(
        h.sessions.delete("alice", "missing").await.unwrap_err()
    ));
    assert!(h.sessions.get("alice", &id).unwrap().is_some());
    assert_eq!(h.boxes.boxes.lock().unwrap().len(), 1);

    let mut live = h.sessions.subscribe();
    h.sessions.delete("alice", &id).await.unwrap();
    assert!(h.sessions.get("alice", &id).unwrap().is_none());
    assert!(h.sessions.list("alice").unwrap().is_empty());
    assert!(
        h.ledger
            .recent_session_events(&id, 1000)
            .unwrap()
            .is_empty()
    );
    assert!(h.ledger.take_pending(&id).unwrap().is_empty());
    assert!(h.boxes.boxes.lock().unwrap().is_empty(), "box destroyed");
    assert_eq!(h.sessions.tracked_box_locks(), 0);
    // Live subscribers got the final `closed` status, which ends event streams.
    let mut closed = false;
    while let Ok(event) = live.try_recv() {
        closed |= event.session_id == id && event.payload["status"] == "closed";
    }
    assert!(closed, "a closed status was broadcast");
    // Every later call is NotFound, including a second delete.
    assert!(is_not_found(
        h.sessions.delete("alice", &id).await.unwrap_err()
    ));
    assert!(is_not_found(
        h.sessions.send("alice", &id, "hi").unwrap_err()
    ));
    assert!(is_not_found(
        h.sessions.read_file("alice", &id, "a").await.unwrap_err()
    ));
    assert_eq!(h.sessions.tracked_box_locks(), 0);
}

#[tokio::test]
async fn deleting_a_running_session_cancels_the_turn_then_deletes_it() {
    let h = harness(
        vec![bash("block"), reply("never")],
        SessionLimits::default(),
    );
    let session = h.sessions.create("alice", None).unwrap();
    h.sessions.send("alice", &session.id, "Run it").unwrap();
    h.boxes.exec_started.notified().await;
    // An interjection is queued while the tool runs.
    h.sessions.send("alice", &session.id, "and then").unwrap();
    h.sessions.delete("alice", &session.id).await.unwrap();
    assert!(h.sessions.get("alice", &session.id).unwrap().is_none());
    assert!(h.boxes.boxes.lock().unwrap().is_empty());
    // A wait in flight ends with NotFound instead of holding its slot.
    assert!(is_not_found(
        h.sessions
            .wait_idle("alice", &session.id, WAIT)
            .await
            .unwrap_err()
    ));
    // The running tool finishes; the turn settles without recreating anything.
    h.boxes.exec_release.notify_one();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(h.sessions.get("alice", &session.id).unwrap().is_none());
    assert!(
        h.ledger
            .recent_session_events(&session.id, 1000)
            .unwrap()
            .is_empty()
    );
    assert!(h.ledger.take_pending(&session.id).unwrap().is_empty());
    assert!(h.boxes.boxes.lock().unwrap().is_empty(), "no box recreated");
    assert_eq!(h.model.calls().len(), 1, "no model call after delete");
    assert_eq!(h.sessions.tracked_box_locks(), 0);
}

#[tokio::test]
async fn delete_closed_removes_only_the_owners_closed_sessions() {
    let h = harness(vec![reply("one"), reply("two")], SessionLimits::default());
    let closed = h.run("First").await;
    let open = h.run("Second").await;
    h.sessions.close("alice", &closed).await.unwrap();
    let bobs = h.sessions.create("bob", None).unwrap();
    h.sessions.close("bob", &bobs.id).await.unwrap();

    assert_eq!(h.sessions.delete_closed("alice").unwrap(), 1);
    assert!(h.sessions.get("alice", &closed).unwrap().is_none());
    assert!(
        h.ledger
            .recent_session_events(&closed, 1000)
            .unwrap()
            .is_empty()
    );
    let kept = h.sessions.get("alice", &open).unwrap().unwrap();
    assert_eq!(kept.status, SessionStatus::Idle);
    assert!(
        !h.events(&open).is_empty(),
        "open session keeps its history"
    );
    let bob = h.sessions.get("bob", &bobs.id).unwrap().unwrap();
    assert_eq!(bob.status, SessionStatus::Closed, "another owner's is kept");
    assert_eq!(h.sessions.delete_closed("alice").unwrap(), 0);
    assert_eq!(h.sessions.delete_closed("bob").unwrap(), 1);
}
