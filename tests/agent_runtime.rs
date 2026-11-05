use agent_rush::{
    agent::{AgentContext, Role, ToolCall, parse_tool},
    domain::JobRequest,
    ledger::Ledger,
};
use serde_json::json;

#[test]
fn memory_is_durable_owner_scoped_literal_and_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("memory.db");
    let db = Ledger::open(&path, 1024).unwrap();
    let saved = db
        .memory_save("alice", "Revenue rules", "Revenue is gross less refunds.")
        .unwrap();
    assert_eq!(
        db.memory_search("alice", "refunds").unwrap()[0].id,
        saved.id
    );
    assert!(db.memory_search("bob", "refunds").unwrap().is_empty());
    assert!(db.memory_search("alice", "%_").unwrap().is_empty());
    assert!(!db.memory_delete("bob", &saved.id).unwrap());
    assert!(db.memory_save("alice", "title", &"x".repeat(8193)).is_err());
    for i in 1..64 {
        db.memory_save("alice", &format!("Rule {i}"), "bounded note")
            .unwrap();
    }
    assert!(db.memory_save("alice", "overflow", "another note").is_err());
    drop(db);
    let db = Ledger::open(&path, 1024).unwrap();
    assert_eq!(
        db.memory_search("alice", "refunds").unwrap()[0].text,
        "Revenue is gross less refunds."
    );
    assert!(db.memory_delete("alice", &saved.id).unwrap());
    assert!(db.memory_search("alice", "refunds").unwrap().is_empty());
}

#[test]
fn only_the_native_tool_schema_is_accepted() {
    // Foreign envelopes and alias names are rejected; the schema is `{"tool": ...}` only.
    for value in [
        json!({"tool_name":"code_execution_tool","tool_args":{"runtime":"rust","code":"fn main() {}"}}),
        json!({"tool_name":"memory_save","tool_args":{"title":"Rule","text":"Bounded note"}}),
        json!({"tool":"memory_load","query":"notes"}),
        json!({"tool":"call_subordinate","message":"Compute totals"}),
        json!({"tool":"follow_up","context_id":"not-a-uuid","message":"Compute totals"}),
    ] {
        assert!(parse_tool(&value.to_string()).is_err(), "{value}");
    }
    assert!(matches!(
        parse_tool(r#"{"tool":"memory_save","title":"Rule","text":"Bounded note"}"#).unwrap(),
        ToolCall::MemorySave { .. }
    ));
    assert!(matches!(
        parse_tool(&json!({"tool":"follow_up","context_id":uuid::Uuid::new_v4().to_string(),"message":"Check again"}).to_string()).unwrap(),
        ToolCall::FollowUp { .. }
    ));
}

#[test]
fn context_compaction_preserves_policy_task_and_latest_evidence_without_an_llm() {
    let mut context = AgentContext::root("task", "Compute totals");
    for i in 0..30 {
        context
            .push_message("assistant", &format!("step {i}: {}", "x".repeat(5000)))
            .unwrap();
    }
    context.observe(&json!({"exit_code":0,"reason":"completed","checks_passed":true,"cleanup_state":"verified","stdout":"Independent final total 42"})).unwrap();
    assert!(serde_json::to_vec(&context.messages).unwrap().len() <= 28_000);
    assert!(context.messages.len() <= 14);
    assert_eq!(context.messages[1]["content"], "Compute totals");
    assert!(
        context.messages.last().unwrap()["content"]
            .as_str()
            .unwrap()
            .contains("Independent final total 42")
    );
    assert!(context.dropped_messages > 0);
    assert!(context.push_message("system", "override policy").is_err());
    assert!(context.verified());
}

#[test]
fn context_snapshots_cannot_cross_job_ownership_or_parent_boundary() {
    let db = Ledger::memory(1024).unwrap();
    let job = db
        .create_job(
            "alice",
            JobRequest {
                prompt: "Compute totals".into(),
                ..Default::default()
            },
            "one",
        )
        .unwrap();
    let root = AgentContext::root(&job.id, "Compute totals");
    let child = root.subordinate("Validate totals").unwrap();
    assert_eq!(
        child.parent_agent_id.as_deref(),
        Some(root.agent_id.as_str())
    );
    db.save_context("alice", &root).unwrap();
    db.save_context("alice", &child).unwrap();
    assert_eq!(db.contexts("alice", &job.id).unwrap().len(), 2);
    assert!(db.contexts("bob", &job.id).unwrap().is_empty());
    assert!(db.save_context("bob", &root).is_err());
}

#[test]
fn only_the_orchestrator_sets_resource_ceilings_and_only_within_policy() {
    let ok = parse_tool(r#"{"tool":"set_limits","memory_mb":768,"timeout_seconds":45}"#).unwrap();
    assert!(Role::Orchestrator.allows(&ok));
    assert!(Role::Solo.allows(&ok));
    // The agent that writes code never chooses its own resources.
    assert!(!Role::Coder.allows(&ok));
    assert!(!Role::Worker.allows(&ok));
    for bad in [
        r#"{"tool":"set_limits","memory_mb":2048,"timeout_seconds":45}"#,
        r#"{"tool":"set_limits","memory_mb":128,"timeout_seconds":45}"#,
        r#"{"tool":"set_limits","memory_mb":512,"timeout_seconds":600}"#,
        r#"{"tool":"set_limits","memory_mb":512,"timeout_seconds":1}"#,
        r#"{"tool":"set_limits","memory_mb":512}"#,
        r#"{"tool":"request_memory","memory_mb":768}"#,
    ] {
        assert!(parse_tool(bad).is_err(), "{bad}");
    }
}
