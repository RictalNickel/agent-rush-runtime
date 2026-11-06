use agent_rush::{
    agent_config::{AgentConfig, BUILTIN_TOOLS, CustomTool},
    ledger::Ledger,
    session_store::SessionStatus,
    skills::{builtin_skills, parse_skill_md, valid_skill_name},
};
use serde_json::json;

fn tool(name: &str) -> CustomTool {
    CustomTool {
        name: name.into(),
        description: "Count lines".into(),
        command_template: "wc -l {{path}}".into(),
    }
}

#[test]
fn event_seq_is_monotonic_per_session() {
    let db = Ledger::memory(1024).unwrap();
    let a = db.create_agent_session("alice", "First").unwrap();
    let b = db.create_agent_session("alice", "").unwrap();
    assert_eq!(b.title, "New session");
    assert_eq!(a.status, SessionStatus::Idle);
    for i in 0..5 {
        let event = db.append_event(&a.id, "user", json!({"text": i})).unwrap();
        assert_eq!(event.seq, i + 1);
    }
    assert_eq!(
        db.append_event(&b.id, "status", json!({"status":"running"}))
            .unwrap()
            .seq,
        1
    );
    assert!(db.append_event(&a.id, "bogus", json!({})).is_err());
    assert!(db.append_event("missing", "user", json!({})).is_err());
    let events = db.session_events("alice", &a.id, 2, 100).unwrap();
    assert_eq!(
        events.iter().map(|e| e.seq).collect::<Vec<_>>(),
        vec![3, 4, 5]
    );
    assert_eq!(events[0].payload, json!({"text": 2}));
    assert_eq!(db.session_events("alice", &a.id, 0, 2).unwrap().len(), 2);
    let view = db.agent_session("alice", &a.id).unwrap().unwrap();
    assert_eq!(view.last_seq, 5);
    let recent = db.recent_session_events(&a.id, 2).unwrap();
    assert_eq!(recent.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![4, 5]);
}

#[test]
fn sessions_are_owner_isolated_and_track_state() {
    let db = Ledger::memory(1024).unwrap();
    let a = db.create_agent_session("alice", "Mine").unwrap();
    db.append_event(&a.id, "user", json!({"text":"secret"}))
        .unwrap();
    assert!(db.agent_session("bob", &a.id).unwrap().is_none());
    assert!(db.agent_sessions("bob").unwrap().is_empty());
    assert!(db.session_events("bob", &a.id, 0, 100).unwrap().is_empty());
    assert_eq!(db.agent_sessions("alice").unwrap().len(), 1);

    db.set_session_status(&a.id, SessionStatus::Running)
        .unwrap();
    db.set_session_box(&a.id, Some("box-1")).unwrap();
    db.add_session_spend(&a.id, 0.25).unwrap();
    db.add_session_spend(&a.id, 0.5).unwrap();
    assert!(db.add_session_spend(&a.id, f64::NAN).is_err());
    let view = db.agent_session("alice", &a.id).unwrap().unwrap();
    assert_eq!(view.status, SessionStatus::Running);
    assert_eq!(view.box_id.as_deref(), Some("box-1"));
    assert!((view.spend_usd - 0.75).abs() < 1e-9);
    assert_eq!(serde_json::to_value(view.status).unwrap(), json!("running"));
    assert_eq!(db.running_agent_sessions().unwrap().len(), 1);
    db.set_session_box(&a.id, None).unwrap();
    assert!(
        db.agent_session("alice", &a.id)
            .unwrap()
            .unwrap()
            .box_id
            .is_none()
    );
}

#[test]
fn pending_messages_are_taken_exactly_once() {
    let db = Ledger::memory(1024).unwrap();
    let a = db.create_agent_session("alice", "x").unwrap();
    let b = db.create_agent_session("alice", "y").unwrap();
    db.push_pending(&a.id, "one").unwrap();
    db.push_pending(&a.id, "two").unwrap();
    db.push_pending(&b.id, "other").unwrap();
    assert!(db.push_pending(&a.id, "  ").is_err());
    assert_eq!(db.take_pending(&a.id).unwrap(), vec!["one", "two"]);
    assert!(db.take_pending(&a.id).unwrap().is_empty());
    assert_eq!(db.take_pending(&b.id).unwrap(), vec!["other"]);
}

#[test]
fn skills_are_validated_owner_scoped_and_bounded() {
    let db = Ledger::memory(1024).unwrap();
    for name in ["web-research", "a", "x1-y2"] {
        assert!(valid_skill_name(name), "{name}");
    }
    for name in [
        "",
        "-lead",
        "Upper",
        "under_score",
        "has space",
        "a".repeat(65).as_str(),
    ] {
        assert!(!valid_skill_name(name), "{name}");
        assert!(db.save_skill("alice", name, "d", "body").is_err());
    }
    assert!(
        db.save_skill("alice", "big", "d", &"x".repeat(65537))
            .is_err()
    );
    assert!(db.save_skill("alice", "empty", "", "body").is_err());
    let saved = db
        .save_skill("alice", "deploy", "How to deploy", "Run make deploy.")
        .unwrap();
    assert!(saved.to_markdown().starts_with("---\nname: deploy\n"));
    db.save_skill("alice", "deploy", "How to deploy v2", "Run make release.")
        .unwrap();
    assert_eq!(
        db.skill("alice", "deploy").unwrap().unwrap().body,
        "Run make release."
    );
    assert!(db.skill("bob", "deploy").unwrap().is_none());
    assert!(db.skills("bob").unwrap().is_empty());
    assert!(!db.delete_skill("bob", "deploy").unwrap());
    for i in 1..200 {
        db.save_skill("alice", &format!("s{i}"), "d", "b").unwrap();
    }
    assert_eq!(db.skills("alice").unwrap().len(), 200);
    assert!(db.save_skill("alice", "overflow", "d", "b").is_err());
    // Updating an existing skill at the limit still works.
    db.save_skill("alice", "s1", "d2", "b2").unwrap();
    assert!(db.delete_skill("alice", "deploy").unwrap());
    db.save_skill("alice", "overflow", "d", "b").unwrap();

    let (name, description, body) =
        parse_skill_md("---\nname: lint\ndescription: \"Run linters\"\n---\n\nRun ruff.\n")
            .unwrap();
    assert_eq!(
        (name.as_str(), description.as_str(), body.as_str()),
        ("lint", "Run linters", "Run ruff.")
    );
    assert!(parse_skill_md("no front matter").is_err());
    assert_eq!(builtin_skills().len(), 3);
    assert!(builtin_skills().iter().all(|s| valid_skill_name(&s.name)));
}

#[test]
fn agent_config_is_versioned_with_history_and_revert() {
    let db = Ledger::memory(1024).unwrap();
    assert_eq!(
        db.agent_config("alice").unwrap(),
        (0, AgentConfig::default())
    );
    let first = AgentConfig {
        system_addendum: "Prefer Python.".into(),
        ..Default::default()
    };
    assert_eq!(
        db.update_agent_config("alice", &first, "user preference", "user")
            .unwrap(),
        1
    );
    let second = AgentConfig {
        disabled_tools: vec!["browser".into()],
        custom_tools: vec![tool("count_lines")],
        ..first.clone()
    };
    assert_eq!(
        db.update_agent_config("alice", &second, "add a tool", "agent")
            .unwrap(),
        2
    );
    assert!(
        db.update_agent_config("alice", &second, "x", "root")
            .is_err()
    );
    assert_eq!(db.agent_config("alice").unwrap(), (2, second.clone()));
    assert_eq!(db.agent_config("bob").unwrap().0, 0);
    let history = db.agent_config_history("alice").unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(
        (history[0].0, history[0].2.as_str(), history[0].1.as_str()),
        (2, "agent", "add a tool")
    );
    assert_eq!(db.revert_agent_config("alice", 1).unwrap(), 3);
    assert_eq!(db.agent_config("alice").unwrap(), (3, first));
    assert_eq!(
        db.agent_config_history("alice").unwrap()[0].1,
        "revert to version 1"
    );
    assert!(db.revert_agent_config("alice", 99).is_err());
    assert!(db.revert_agent_config("bob", 1).is_err());
    assert_eq!(db.revert_agent_config("alice", 0).unwrap(), 4);
    assert_eq!(db.agent_config("alice").unwrap().1, AgentConfig::default());
}

#[test]
fn validate_rejects_builtin_clash_unknown_mcp_and_oversized_addendum() {
    let allowed = vec!["docs".to_string()];
    let ok = AgentConfig {
        custom_tools: vec![tool("count_lines")],
        mcp_servers: vec!["docs".into()],
        reasoning_effort: Some("high".into()),
        ..Default::default()
    };
    ok.validate(BUILTIN_TOOLS, &allowed).unwrap();
    let clash = AgentConfig {
        custom_tools: vec![tool("bash")],
        ..Default::default()
    };
    assert!(clash.validate(BUILTIN_TOOLS, &allowed).is_err());
    let unknown_mcp = AgentConfig {
        mcp_servers: vec!["evil".into()],
        ..Default::default()
    };
    assert!(unknown_mcp.validate(BUILTIN_TOOLS, &allowed).is_err());
    let big = AgentConfig {
        system_addendum: "x".repeat(8 * 1024 + 1),
        ..Default::default()
    };
    assert!(big.validate(BUILTIN_TOOLS, &allowed).is_err());
    let db = Ledger::memory(1024).unwrap();
    assert!(db.update_agent_config("alice", &big, "x", "user").is_err());
    assert!(
        db.update_agent_config("alice", &clash, "x", "user")
            .is_err()
    );
    for bad in [
        AgentConfig {
            disabled_tools: vec!["reply".into()],
            ..Default::default()
        },
        AgentConfig {
            disabled_tools: vec!["nonexistent".into()],
            ..Default::default()
        },
        AgentConfig {
            custom_tools: vec![tool("Bad-Name")],
            ..Default::default()
        },
        AgentConfig {
            custom_tools: (0..21).map(|i| tool(&format!("t{i}"))).collect(),
            ..Default::default()
        },
        AgentConfig {
            custom_tools: vec![CustomTool {
                command_template: "echo {{Bad Arg}}".into(),
                ..tool("echoer")
            }],
            ..Default::default()
        },
        AgentConfig {
            reasoning_effort: Some("ludicrous".into()),
            ..Default::default()
        },
    ] {
        assert!(bad.validate(BUILTIN_TOOLS, &allowed).is_err(), "{bad:?}");
    }
    assert_eq!(
        tool("x").args().unwrap(),
        vec!["path".to_string()],
        "placeholders are parsed in order"
    );
}
