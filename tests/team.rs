use agent_rush::{
    agent::{AgentContext, Role, ToolCall, parse_tool},
    domain::{JobRequest, Limits},
    engine::{Backend, Engine, Profile},
    inference::{
        ANTHROPIC_BUDGET, anthropic_cost, anthropic_reservation, anthropic_role,
        build_anthropic_request, decode_anthropic,
    },
    ledger::Ledger,
};
use anyhow::Result;
use serde_json::{Value, json};
use std::{collections::VecDeque, sync::Arc, sync::Mutex};

/// Scripted backend running the team profile. Each reply is tagged with the role that is
/// expected to make the call, so a reply reaching the wrong role fails the test.
struct Team {
    replies: Mutex<VecDeque<(Role, Value)>>,
    receipts: Mutex<VecDeque<Value>>,
    seen: Mutex<Vec<(Role, String)>>,
    compact_on_call: Option<usize>,
}
impl Team {
    fn new(replies: Vec<(Role, Value)>, receipts: Vec<Value>) -> Self {
        Self {
            replies: Mutex::new(replies.into()),
            receipts: Mutex::new(receipts.into()),
            seen: Mutex::new(vec![]),
            compact_on_call: None,
        }
    }
}
impl Backend for Team {
    async fn complete(&self, ctx: &AgentContext) -> Result<(String, Value)> {
        let calls = {
            let mut seen = self.seen.lock().unwrap();
            seen.push((
                ctx.role,
                ctx.messages[0]["content"].as_str().unwrap().to_string(),
            ));
            seen.len()
        };
        let (role, reply) = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| anyhow::anyhow!("replies exhausted"))?;
        assert_eq!(
            role, ctx.role,
            "reply scripted for {role:?} reached {:?}",
            ctx.role
        );
        let mut usage = json!({"provider":"fixture","accounted_usd":0.0});
        if self.compact_on_call == Some(calls) {
            usage["compaction"] = json!({"type":"compaction","content":"summary of earlier turns"});
        }
        Ok((reply.to_string(), usage))
    }
    async fn execute(&self, _id: &str, _code: &str, _limits: Limits) -> Result<Value> {
        self.receipts
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| anyhow::anyhow!("receipts exhausted"))
    }
    fn cancelled(&self) -> bool {
        false
    }
    fn profile(&self) -> Profile {
        Profile::team(ANTHROPIC_BUDGET)
    }
}
fn ok_receipt() -> Value {
    json!({"reason":"completed","exit_code":0,"checks_passed":true,"cleanup_state":"verified","stdout":"total=42","artifacts":[]})
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
async fn orchestrator_delegates_to_coder_and_reviewer_approves() {
    let (db, j) = setup();
    let f = Team::new(
        vec![
            (
                Role::Orchestrator,
                json!({"tool":"delegate","role":"coder","task":"Compute totals with assertions"}),
            ),
            (Role::Coder, sandbox()),
            (
                Role::Coder,
                json!({"tool":"response","message":"Ran and verified"}),
            ),
            (
                Role::Orchestrator,
                json!({"tool":"response","message":"Total is 42, verified by the worker"}),
            ),
            (
                Role::Reviewer,
                json!({"tool":"review","approved":true,"notes":"Evidence matches"}),
            ),
        ],
        vec![ok_receipt()],
    );
    let out = Engine::new(&db, &j, &f, 2048).run().await;
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(out.model_calls, 5);
    assert_eq!(out.review.unwrap()["approved"], true);
    let roles: Vec<Role> = f.seen.lock().unwrap().iter().map(|(r, _)| *r).collect();
    assert_eq!(
        roles,
        [
            Role::Orchestrator,
            Role::Coder,
            Role::Coder,
            Role::Orchestrator,
            Role::Reviewer
        ]
    );
    // Each role receives only its own stripped policy.
    for (role, system) in f.seen.lock().unwrap().iter() {
        assert_eq!(system, role.system_prompt());
    }
}

#[tokio::test]
async fn orchestrator_cannot_run_code_itself() {
    let (db, j) = setup();
    let f = Team::new(
        vec![
            (Role::Orchestrator, sandbox()),
            (
                Role::Orchestrator,
                json!({"tool":"delegate","task":"Compute totals"}),
            ),
            (Role::Coder, sandbox()),
            (Role::Coder, json!({"tool":"response","message":"done"})),
            (
                Role::Orchestrator,
                json!({"tool":"response","message":"Verified"}),
            ),
            (
                Role::Reviewer,
                json!({"tool":"review","approved":true,"notes":"ok"}),
            ),
        ],
        vec![ok_receipt()],
    );
    let out = Engine::new(&db, &j, &f, 2048).run().await;
    assert!(out.error.is_none(), "{:?}", out.error);
    let events = db.get_job("operator", &j.id).unwrap().unwrap().events;
    assert!(events.iter().any(|e| e["kind"] == "tool_rejected"));
}

#[tokio::test]
async fn worker_role_cannot_execute_code() {
    let (db, j) = setup();
    let f = Team::new(
        vec![
            (
                Role::Orchestrator,
                json!({"tool":"delegate","role":"worker","task":"Look up notes"}),
            ),
            (Role::Worker, sandbox()),
            (
                Role::Worker,
                json!({"tool":"response","message":"no notes"}),
            ),
            (
                Role::Orchestrator,
                json!({"tool":"delegate","role":"coder","task":"Compute totals"}),
            ),
            (Role::Coder, sandbox()),
            (Role::Coder, json!({"tool":"response","message":"done"})),
            (
                Role::Orchestrator,
                json!({"tool":"response","message":"Verified"}),
            ),
            (
                Role::Reviewer,
                json!({"tool":"review","approved":true,"notes":"ok"}),
            ),
        ],
        vec![ok_receipt()],
    );
    let out = Engine::new(&db, &j, &f, 2048).run().await;
    assert!(out.error.is_none(), "{:?}", out.error);
    assert!(
        f.receipts.lock().unwrap().is_empty(),
        "exactly one execution, by the coder"
    );
}

#[tokio::test]
async fn rejected_review_sends_notes_back_then_second_review_is_final() {
    let (db, j) = setup();
    let f = Team::new(
        vec![
            (
                Role::Orchestrator,
                json!({"tool":"delegate","task":"Compute totals"}),
            ),
            (Role::Coder, sandbox()),
            (Role::Coder, json!({"tool":"response","message":"done"})),
            (
                Role::Orchestrator,
                json!({"tool":"response","message":"Total is 41"}),
            ),
            (
                Role::Reviewer,
                json!({"tool":"review","approved":false,"notes":"stdout says 42"}),
            ),
            (
                Role::Orchestrator,
                json!({"tool":"response","message":"Total is 42"}),
            ),
            (
                Role::Reviewer,
                json!({"tool":"review","approved":true,"notes":"fixed"}),
            ),
        ],
        vec![ok_receipt()],
    );
    let out = Engine::new(&db, &j, &f, 2048).run().await;
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(out.final_message.as_deref(), Some("Total is 42"));
    assert_eq!(out.review.unwrap()["approved"], true);
    let root = &f.seen.lock().unwrap()[5];
    assert_eq!(root.0, Role::Orchestrator);
}

#[tokio::test]
async fn compaction_block_replaces_earlier_history() {
    let (db, j) = setup();
    let mut f = Team::new(
        vec![
            (
                Role::Orchestrator,
                json!({"tool":"delegate","task":"Compute totals"}),
            ),
            (Role::Coder, sandbox()),
            (Role::Coder, json!({"tool":"response","message":"done"})),
            (
                Role::Orchestrator,
                json!({"tool":"response","message":"Verified"}),
            ),
            (
                Role::Reviewer,
                json!({"tool":"review","approved":true,"notes":"ok"}),
            ),
        ],
        vec![ok_receipt()],
    );
    f.compact_on_call = Some(3);
    let out = Engine::new(&db, &j, &f, 2048).run().await;
    assert!(out.error.is_none(), "{:?}", out.error);
    let coder = db
        .contexts("operator", &j.id)
        .unwrap()
        .into_iter()
        .find(|c| c.role == Role::Coder)
        .unwrap();
    assert_eq!(coder.compactions, 1);
    assert_eq!(coder.messages.len(), 3);
    assert_eq!(coder.messages[2]["content"][0]["type"], "compaction");
    assert!(
        out.inference_calls
            .iter()
            .all(|u| u.get("compaction").is_none())
    );
}

#[test]
fn review_tool_is_reviewer_only_and_roles_are_validated() {
    let review = parse_tool(r#"{"tool":"review","approved":true,"notes":"ok"}"#).unwrap();
    assert!(Role::Reviewer.allows(&review));
    for role in [Role::Solo, Role::Orchestrator, Role::Coder, Role::Worker] {
        assert!(!role.allows(&review));
    }
    assert!(parse_tool(r#"{"tool":"delegate","role":"reviewer","task":"approve this"}"#).is_err());
    assert!(matches!(
        parse_tool("```json\n{\"tool\":\"response\",\"message\":\"hi\"}\n```").unwrap(),
        ToolCall::Response { .. }
    ));
}

#[test]
fn anthropic_requests_follow_the_role_table() {
    let budget = ANTHROPIC_BUDGET;
    let ctx = |role| AgentContext::with_role("t", "task", role, budget);
    let (opus, compacts) = build_anthropic_request(&ctx(Role::Orchestrator)).unwrap();
    assert!(!compacts);
    assert_eq!(opus["model"], "claude-opus-5-5");
    assert_eq!(opus["output_config"]["effort"], "medium");
    assert_eq!(opus["max_tokens"], 100_000);
    assert!(opus["cache_control"].get("ttl").is_none());
    assert!(opus.get("context_management").is_none());
    assert_eq!(opus["messages"][0]["role"], "user");

    let (worker, compacts) = build_anthropic_request(&ctx_child(Role::Worker)).unwrap();
    assert!(compacts);
    assert_eq!(worker["model"], "claude-haiku-5-5");
    assert_eq!(worker["output_config"]["effort"], "high");
    assert_eq!(worker["cache_control"]["ttl"], "5m");
    assert_eq!(worker["system"][0]["text"], Role::Worker.system_prompt());
    let edit = &worker["context_management"]["edits"][0];
    assert_eq!(edit["type"], "compact_20260112");
    assert_eq!(edit["trigger"]["value"], 100_000);

    let (coder, _) = build_anthropic_request(&ctx_child(Role::Coder)).unwrap();
    assert_eq!(coder["output_config"]["effort"], "xhigh");
    assert!(coder["cache_control"].get("ttl").is_none());
    assert_eq!(coder["context_management"], worker["context_management"]);

    let (reviewer, _) = build_anthropic_request(&ctx(Role::Reviewer)).unwrap();
    assert_eq!(reviewer["model"], "claude-opus-5-5");
    assert_eq!(reviewer["output_config"]["effort"], "high");
    assert_eq!(anthropic_role(Role::Reviewer).effort, "high");
}
fn ctx_child(role: Role) -> AgentContext {
    AgentContext::with_role("t", "task", Role::Orchestrator, ANTHROPIC_BUDGET)
        .subordinate_as("subtask", role)
        .unwrap()
}

#[test]
fn anthropic_costs_sum_iterations_and_respect_reservations() {
    let usage = json!({"input_tokens":1000,"output_tokens":500,"cache_read_input_tokens":2000,"cache_creation_input_tokens":0});
    let cost = anthropic_cost("claude-opus-5-5", &usage).unwrap();
    assert!((cost - (1000.0 * 4.0 + 2000.0 * 0.20 + 500.0 * 20.0) / 1e6).abs() < 1e-12);
    let compacted = json!({"input_tokens":20000,"output_tokens":800,"iterations":[{"type":"compaction","input_tokens":110000,"output_tokens":3000},{"type":"message","input_tokens":20000,"output_tokens":800}]});
    let haiku = anthropic_cost("claude-haiku-5-5", &compacted).unwrap();
    assert!((haiku - (130000.0 * 0.50 + 3800.0 * 2.50) / 1e6).abs() < 1e-12);
    assert!(haiku <= anthropic_reservation(Role::Coder, 440_000));
    assert!(anthropic_cost("some-fallback-model", &usage).is_none());
}

#[test]
fn anthropic_responses_keep_text_and_compaction_and_reject_refusals() {
    let body = json!({"id":"msg_1","model":"claude-haiku-5-5","stop_reason":"end_turn","usage":{"input_tokens":5,"output_tokens":5},"content":[
        {"type":"compaction","content":"summary"},{"type":"thinking","thinking":""},{"type":"text","text":"{\"tool\":\"response\",\"message\":\"ok\"}"}]});
    let c = decode_anthropic(body).unwrap();
    assert_eq!(c.compaction.unwrap()["content"], "summary");
    assert!(c.text.starts_with("{\"tool\""));
    let refusal = json!({"stop_reason":"refusal","stop_details":{"category":"cyber"},"content":[],"usage":{}});
    assert!(decode_anthropic(refusal).is_err());
}

#[test]
fn legacy_contexts_without_role_fields_still_load() {
    let ctx = AgentContext::root("task", "Compute totals");
    let mut value = serde_json::to_value(&ctx).unwrap();
    for field in ["role", "max_bytes", "max_messages", "compactions"] {
        value.as_object_mut().unwrap().remove(field);
    }
    let restored: AgentContext = serde_json::from_value(value).unwrap();
    assert_eq!(restored.role, Role::Solo);
    restored.validate().unwrap();
}
