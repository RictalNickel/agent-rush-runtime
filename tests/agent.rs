use agent_rush::agent::{AgentContext, ToolCall, parse_tool};
use serde_json::json;

#[test]
fn unknown_tools_and_authority_fields_are_denied() {
    for value in [
        json!({"tool":"shell", "code":"rm -rf /"}),
        json!({"tool":"sandbox","code":"fn main() {}","summary":"test","privileged":true}),
        json!({"tool":"sandbox","code":"","summary":"empty"}),
    ] {
        assert!(parse_tool(&value.to_string()).is_err());
    }
    assert!(matches!(
        parse_tool(r#"{"tool":"sandbox","code":"fn main() {}","summary":"Compile a task"}"#)
            .unwrap(),
        ToolCall::Sandbox { .. }
    ));
}

#[test]
fn subordinate_context_is_bounded_and_not_a_security_boundary() {
    let context = AgentContext::root("task-id", "Compute revenue totals");
    let child = context.subordinate("Design an independent check").unwrap();
    assert_eq!(child.task_id, context.task_id);
    assert_ne!(child.agent_id, context.agent_id);
    assert_eq!(child.depth, 1);
    assert!(child.subordinate("another").is_err());
}

#[test]
fn model_output_is_not_authoritative_verification() {
    let mut context = AgentContext::root("task", "Compute totals");
    context
        .observe(&json!({"exit_code":1,"checks_passed":false,"cleanup_state":"verified"}))
        .unwrap();
    assert!(!context.verified());
    context
        .observe(&json!({"exit_code":0,"checks_passed":true,"cleanup_state":"failed"}))
        .unwrap();
    assert!(!context.verified());
    context
        .observe(&json!({"exit_code":0,"checks_passed":true,"cleanup_state":"verified"}))
        .unwrap();
    assert!(context.verified());
}
