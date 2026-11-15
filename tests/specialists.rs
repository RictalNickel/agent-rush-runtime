use agent_rush::{
    agent_config::{AgentConfig, BUILTIN_TOOLS},
    ledger::Ledger,
    skills::builtin_skills,
    tools::{parse_call, system_prompt},
};
use serde_json::json;

#[test]
fn specialist_tool_accepts_catalog_queries_and_rejects_arbitrary_execution() {
    let config = AgentConfig::default();
    for value in [
        json!({"tool":"specialist","action":"list"}),
        json!({"tool":"specialist","action":"describe","agent":"testforge"}),
        json!({"tool":"specialist","action":"doctor","agent":"finance-lab"}),
    ] {
        let call = parse_call(&value.to_string(), &config).unwrap();
        assert_eq!(call.name(), "specialist");
    }
    for value in [
        json!({"tool":"specialist","action":"list","command":"id"}),
        json!({"tool":"specialist","action":"list","path":"."}),
        json!({"tool":"specialist","action":"describe","agent":"unknown"}),
        json!({"tool":"specialist","action":"describe"}),
        json!({"tool":"specialist","action":"run","agent":"testforge","operation":"/bin/bash"}),
        json!({"tool":"specialist","action":"run","agent":"testforge","operation":"pytest-test","timeout_seconds":541}),
        json!({"tool":"specialist","action":"run","agent":"testforge","operation":"pytest-test","parameters":{"value":"a\0b"}}),
        json!({"tool":"specialist","action":"run","agent":"testforge","operation":"pytest-test","parameters":[]}),
        json!({"tool":"specialist","action":"run","agent":"testforge","operation":"pytest-test","allow_network":"true"}),
        json!({"tool":"specialist","action":"doctor","agent":"testforge","operation":"pytest-test"}),
    ] {
        assert!(
            parse_call(&value.to_string(), &config).is_err(),
            "accepted {value}"
        );
    }
}

#[test]
fn specialist_rejects_duplicate_parameter_fields_before_reencoding() {
    let config = AgentConfig::default();
    for raw in [
        r#"{"tool":"specialist","action":"run","agent":"testforge","operation":"coverage-report","parameters":{"input":"safe.db","input":"other.db"}}"#,
        r#"{"tool":"specialist","action":"run","agent":"testforge","operation":"coverage-report","parameters":{"input":"safe.db","\u0069nput":"other.db"}}"#,
    ] {
        assert!(
            parse_call(raw, &config).is_err(),
            "accepted duplicate parameter: {raw}"
        );
    }
    for raw in [
        r#"{"tool":"specialist","action":"run","agent":"testforge","operation":"coverage-report","parameters":{"input":"safe.db","minimum":80.5}}"#,
        r#"{"tool":"specialist","action":"run","agent":"testforge","operation":"pytest-test","parameters":null}"#,
        r#"{"tool":"specialist","action":"run","agent":"testforge","operation":"pytest-test"}"#,
    ] {
        assert!(
            parse_call(raw, &config).is_ok(),
            "rejected valid parameters: {raw}"
        );
    }
}

#[test]
fn specialist_rejects_nonfinite_parameter_numbers() {
    let config = AgentConfig::default();
    for raw in [
        r#"{"tool":"specialist","action":"run","agent":"testforge","operation":"coverage-report","parameters":{"minimum":1e999}}"#,
        r#"{"tool":"specialist","action":"run","agent":"testforge","operation":"coverage-report","parameters":{"minimum":NaN}}"#,
    ] {
        assert!(
            parse_call(raw, &config).is_err(),
            "accepted nonfinite parameter: {raw}"
        );
    }
}

#[test]
fn all_specialists_have_skills_and_valid_membership() {
    let catalog = agent_rush::specialists::catalog();
    assert_eq!(catalog["schema_version"], 1);
    assert_eq!(catalog["agents"].as_object().unwrap().len(), 21);
    for (id, agent) in catalog["agents"].as_object().unwrap() {
        assert!(agent_rush::specialists::known(id));
        let skill = builtin_skills()
            .into_iter()
            .find(|s| s.name == format!("specialist-{id}"))
            .unwrap();
        assert!(!skill.body.is_empty());
        for tool in agent["tools"].as_array().unwrap() {
            assert!(catalog["tools"].get(tool.as_str().unwrap()).is_some());
        }
    }
}

#[test]
fn specialist_preference_is_validated_versioned_and_owner_scoped() {
    let db = Ledger::memory(1024).unwrap();
    let config = AgentConfig {
        specialist: Some("finance-lab".into()),
        ..Default::default()
    };
    config.validate(BUILTIN_TOOLS, &[]).unwrap();
    db.update_agent_config("alice", &config, "Finance research", "user")
        .unwrap();
    assert_eq!(
        db.agent_config("alice").unwrap().1.specialist.as_deref(),
        Some("finance-lab")
    );
    assert_eq!(db.agent_config("bob").unwrap().1.specialist, None);
    let old: AgentConfig = serde_json::from_value(json!({"system_addendum":"existing"})).unwrap();
    assert_eq!(old.specialist, None);
    let bad = AgentConfig {
        specialist: Some("../../custom".into()),
        ..Default::default()
    };
    assert!(bad.validate(BUILTIN_TOOLS, &[]).is_err());
    let prompt = system_prompt(&config, 1, &[], &[], &[]);
    assert!(prompt.contains("finance-lab"));
    assert!(prompt.contains("specialist-finance-lab"));
}

#[test]
fn specialist_tool_can_be_disabled_and_preference_cleared() {
    let disabled = AgentConfig {
        disabled_tools: vec!["specialist".into()],
        ..Default::default()
    };
    assert!(!system_prompt(&disabled, 1, &[], &[], &[]).contains(r#"{"tool":"specialist""#));
    let selected = AgentConfig {
        specialist: Some("testforge".into()),
        ..Default::default()
    };
    let agent_rush::tools::ToolCall::AgentConfig {
        config: Some(patch),
        ..
    } = parse_call(
        r#"{"tool":"agent_config","action":"update","config":{"specialist":""},"reason":"clear"}"#,
        &selected,
    )
    .unwrap()
    else {
        panic!("wrong call")
    };
    assert_eq!(patch.apply(selected).specialist, None);
}
