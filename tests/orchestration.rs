use agent_rush::{
    agent::AgentContext,
    domain::{JobRequest, Limits},
    engine::{Backend, Engine},
    ledger::Ledger,
};
use anyhow::Result;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
struct Fixture {
    replies: Mutex<VecDeque<String>>,
    receipts: Mutex<VecDeque<Value>>,
    executions: Mutex<Vec<Limits>>,
    cancelled: AtomicBool,
    repeat: bool,
    delay: bool,
}
impl Fixture {
    fn new(replies: Vec<Value>, receipts: Vec<Value>) -> Self {
        Self {
            replies: Mutex::new(replies.into_iter().map(|v| v.to_string()).collect()),
            receipts: Mutex::new(receipts.into()),
            executions: Mutex::new(vec![]),
            cancelled: AtomicBool::new(false),
            repeat: false,
            delay: false,
        }
    }
}
impl Backend for Fixture {
    async fn complete(&self, ctx: &AgentContext) -> Result<(String, Value)> {
        if self.delay {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        }
        let mut reply = if self.repeat {
            if ctx.depth == 0 {
                json!({"tool":"delegate","task":"Check data carefully"}).to_string()
            } else {
                json!({"tool":"memory_search","query":"notes"}).to_string()
            }
        } else {
            self.replies
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("fixture replies exhausted"))?
        };
        if reply.contains("@CHILD_ID@") {
            let id = ctx
                .messages
                .iter()
                .rev()
                .filter_map(|m| m["content"].as_str())
                .filter_map(|s| s.split_once("policy: ").map(|v| v.1))
                .filter_map(|s| serde_json::from_str::<Value>(s).ok())
                .find_map(|v| v["context_id"].as_str().map(String::from))
                .unwrap();
            reply = reply.replace("@CHILD_ID@", &id);
        }
        Ok((reply, json!({"provider":"fixture","accounted_usd":0.0})))
    }
    async fn execute(&self, _id: &str, _code: &str, limits: Limits) -> Result<Value> {
        self.executions.lock().unwrap().push(limits);
        self.receipts
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| anyhow::anyhow!("fixture receipts exhausted"))
    }
    fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}
fn receipt(ok: bool, clean: bool) -> Value {
    json!({"reason":if ok{"completed"}else{"execution_failed"},"exit_code":if ok{0}else{1},"checks_passed":ok,"cleanup_state":if clean{"verified"}else{"failed"},"stdout":"observed fixture output","artifacts":[]})
}
fn setup() -> (Arc<Ledger>, agent_rush::domain::Job) {
    let db = Arc::new(Ledger::memory(2048).unwrap());
    let j = db
        .create_job(
            "operator",
            JobRequest {
                prompt: "Compute and check totals".into(),
                ..Default::default()
            },
            "one",
        )
        .unwrap();
    db.claim(&j.id).unwrap();
    (db, j)
}
fn sandbox() -> Value {
    json!({"tool":"sandbox","code":"fn main() {}","summary":"Compute totals"})
}
#[tokio::test]
async fn actual_engine_plans_uses_memory_grows_then_retries_and_explains() {
    let (db, j) = setup();
    let f = Fixture::new(
        vec![
            json!({"tool":"plan","steps":["Compute","Verify"]}),
            json!({"tool":"memory_save","title":"Totals","text":"Use integer cents"}),
            json!({"tool":"memory_search","query":"cents"}),
            json!({"tool":"request_memory","memory_mb":768}),
            sandbox(),
            sandbox(),
            json!({"tool":"response","message":"Verified total from worker evidence"}),
        ],
        vec![receipt(false, true), receipt(true, true)],
    );
    let out = Engine::new(&db, &j, &f, 2048).run().await;
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(out.model_calls, 7);
    assert_eq!(out.inference_calls.len(), 7);
    assert_eq!(
        f.executions
            .lock()
            .unwrap()
            .iter()
            .map(|l| l.memory_mb)
            .collect::<Vec<_>>(),
        vec![768, 768]
    );
    assert_eq!(db.contexts("operator", &j.id).unwrap().len(), 1);
    assert_eq!(db.memory_search("operator", "cents").unwrap().len(), 1);
    assert_eq!(out.receipt.unwrap()["checks_passed"], true);
}
#[tokio::test]
async fn subordinate_executes_an_iterative_workflow_using_the_same_lease_and_budget() {
    let (db, j) = setup();
    let f = Fixture::new(
        vec![
            json!({"tool":"delegate","task":"Independently compute totals"}),
            sandbox(),
            sandbox(),
            json!({"tool":"response","message":"Subordinate checked the result"}),
            json!({"tool":"response","message":"Result verified by worker"}),
        ],
        vec![receipt(false, true), receipt(true, true)],
    );
    let out = Engine::new(&db, &j, &f, 2048).run().await;
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(out.model_calls, 5);
    assert_eq!(db.contexts("operator", &j.id).unwrap().len(), 2);
    assert_eq!(f.executions.lock().unwrap().len(), 2);
}
#[tokio::test]
async fn unverified_narration_and_unresolved_cleanup_cannot_become_success() {
    for (replies, receipts) in [
        (
            vec![json!({"tool":"response","message":"I succeeded"})],
            vec![],
        ),
        (vec![sandbox(), sandbox()], vec![receipt(true, false)]),
    ] {
        let (db, j) = setup();
        let f = Fixture::new(replies, receipts);
        let out = Engine::new(&db, &j, &f, 2048).run().await;
        assert!(out.error.is_some());
        assert!(f.executions.lock().unwrap().len() <= 1);
    }
}
#[tokio::test]
async fn all_contexts_share_a_finite_call_budget_and_cannot_recursively_spawn() {
    let (db, j) = setup();
    let mut f = Fixture::new(vec![], vec![]);
    f.repeat = true;
    let out = Engine::new(&db, &j, &f, 2048).run().await;
    assert!(out.error.is_some());
    assert!(out.model_calls <= 12);
    assert!(db.contexts("operator", &j.id).unwrap().len() <= 3);
    assert!(f.executions.lock().unwrap().is_empty());
}
#[tokio::test]
async fn denied_memory_increase_is_feedback_and_does_not_overcommit_the_node() {
    let (db, j) = setup();
    let f = Fixture::new(
        vec![
            json!({"tool":"request_memory","memory_mb":1024}),
            sandbox(),
            json!({"tool":"response","message":"Used admitted resources"}),
        ],
        vec![receipt(true, true)],
    );
    let out = Engine::new(&db, &j, &f, 512).run().await;
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(f.executions.lock().unwrap()[0].memory_mb, 512);
    assert_eq!(db.reserved_memory().unwrap(), 512);
}
#[tokio::test]
async fn cancellation_interrupts_a_waiting_model_without_starting_execution() {
    let (db, j) = setup();
    let mut f = Fixture::new(vec![sandbox()], vec![receipt(true, true)]);
    f.delay = true;
    let f = Arc::new(f);
    let trigger = f.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        trigger.cancelled.store(true, Ordering::SeqCst);
    });
    let out = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        Engine::new(&db, &j, &*f, 2048).run(),
    )
    .await
    .unwrap();
    assert!(out.error.unwrap().contains("cancelled"));
    assert!(f.executions.lock().unwrap().is_empty());
}

#[tokio::test]
async fn subordinate_context_can_continue_and_advice_cannot_inherit_another_execution_receipt() {
    let (db, j) = setup();
    let f = Fixture::new(
        vec![
            sandbox(),
            json!({"tool":"delegate","task":"Check the strategy"}),
            json!({"tool":"response","message":"An unexecuted suggestion"}),
            json!({"tool":"follow_up","context_id":"@CHILD_ID@","message":"Now independently execute the check"}),
            sandbox(),
            json!({"tool":"response","message":"Independently checked"}),
            json!({"tool":"response","message":"Worker verified result"}),
        ],
        vec![receipt(true, true), receipt(true, true)],
    );
    let out = Engine::new(&db, &j, &f, 2048).run().await;
    assert!(out.error.is_none(), "{:?}", out.error);
    let contexts = db.contexts("operator", &j.id).unwrap();
    assert_eq!(contexts.len(), 2);
    let root = contexts.iter().find(|c| c.depth == 0).unwrap();
    assert!(
        root.messages
            .iter()
            .filter_map(|m| m["content"].as_str())
            .any(|v| v.contains("An unexecuted suggestion") && v.contains("\"verified\":false"))
    );
    assert_eq!(f.executions.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn escaped_worker_output_cannot_panic_or_hide_an_unresolved_cleanup_receipt() {
    let db = Ledger::memory(2048).unwrap();
    let job = db
        .create_job(
            "operator",
            JobRequest {
                prompt: "\"".repeat(8000),
                ..Default::default()
            },
            "escaped",
        )
        .unwrap();
    db.claim(&job.id).unwrap();
    for clean in [true, false] {
        let mut observed = receipt(true, clean);
        observed["stdout"] = json!("\\".repeat(1500));
        observed["stderr"] = json!("\\".repeat(1500));
        let f = Fixture::new(
            vec![
                sandbox(),
                json!({"tool":"response","message":"Observed result"}),
            ],
            vec![observed],
        );
        let out = Engine::new(&db, &job, &f, 2048).run().await;
        assert_eq!(
            out.receipt.as_ref().unwrap()["cleanup_state"],
            if clean { "verified" } else { "failed" }
        );
        assert_eq!(out.error.is_none(), clean);
        assert_eq!(f.executions.lock().unwrap().len(), 1);
        for c in db.contexts("operator", &job.id).unwrap() {
            assert!(serde_json::to_vec(&c.messages).unwrap().len() <= 28000);
        }
    }
}
