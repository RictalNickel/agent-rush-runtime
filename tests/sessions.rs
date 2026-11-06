use agent_rush::{
    agent_config::{AgentConfig, CustomTool},
    ledger::Ledger,
    mcp_client::McpRegistry,
    sandbox_api::{
        BoxClient, BoxInfo, BoxSpec, DirEntry, ExecRequest, ExecResult, FileRead, WriteResult,
    },
    session::{ModelReply, SessionError, SessionLimits, SessionModel, Sessions},
    session_store::{SessionEvent, SessionStatus},
    tools::{apply_edit, expand_template, parse_call, shell_quote, truncate_bytes},
};
use anyhow::{Result, anyhow, bail};
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
                None => match self.fallback.lock().unwrap().clone() {
                    Some(text) => text,
                    None => bail!("script exhausted"),
                },
            };
            Ok(ModelReply {
                text,
                cost_usd: 0.001,
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
    lose_once: AtomicBool,
    exec_started: Notify,
    creates: AtomicUsize,
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
            let command = request.command.as_str();
            if command == "lose-the-box" && self.lose_once.swap(false, Ordering::SeqCst) {
                self.boxes.lock().unwrap().remove(box_id);
                bail!("box {box_id} not found");
            }
            if command == "block" {
                self.exec_started.notify_one();
                std::future::pending::<()>().await;
            }
            let stdout = match command {
                "big" => "log line\n".repeat(10 * 1024 * 1024 / 9 + 1),
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
async fn cancel_stops_the_turn() {
    let h = harness(
        vec![bash("block"), reply("never")],
        SessionLimits::default(),
    );
    let session = h.sessions.create("alice", None).unwrap();
    h.sessions
        .send("alice", &session.id, "Run forever")
        .unwrap();
    h.boxes.exec_started.notified().await;
    h.sessions.cancel("alice", &session.id).unwrap();
    let view = h
        .sessions
        .wait_idle("alice", &session.id, WAIT)
        .await
        .unwrap();
    assert_eq!(view.status, SessionStatus::Idle);
    let events = h.events(&session.id);
    assert!(events.iter().all(|e| e.kind != "assistant"));
    assert_eq!(events.last().unwrap().payload["reason"], "cancelled");
    assert_eq!(h.model.calls().len(), 1);
}

#[tokio::test]
async fn step_cap_ends_the_turn_failed() {
    let h = harness(
        vec![],
        SessionLimits {
            max_turn_steps: 3,
            ..Default::default()
        },
    );
    *h.model.fallback.lock().unwrap() = Some(bash("true").to_string());
    let id = h.run("Loop").await;
    let view = h.sessions.get("alice", &id).unwrap().unwrap();
    assert_eq!(view.status, SessionStatus::Failed);
    assert_eq!(h.model.calls().len(), 3);
    let events = h.events(&id);
    let error = events.iter().find(|e| e.kind == "error").unwrap();
    assert!(
        error.payload["message"]
            .as_str()
            .unwrap()
            .contains("step limit")
    );
    assert_eq!(events.last().unwrap().payload["reason"], "step_limit");
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
        .write_file("alice", &session.id, "blob.bin", &blob)
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
        s.write_file("bob", &id, "a", b"x").await.unwrap_err()
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
