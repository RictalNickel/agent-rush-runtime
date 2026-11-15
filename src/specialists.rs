//! Built-in specialist identities share the image runner's reviewed catalog.
//! A selected identity supplies guidance; it does not grant permissions or change
//! the worker's image, MCP allowlist, or owner isolation.
use serde_json::{Value, json};
use std::sync::OnceLock;

static CATALOG: OnceLock<Value> = OnceLock::new();

pub fn catalog() -> &'static Value {
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../deploy/agents/catalog.json"))
            .expect("the checked-in specialist catalog must be valid JSON")
    })
}

pub fn known(id: &str) -> bool {
    catalog()["agents"].get(id).is_some()
}

pub fn index() -> Value {
    let agents: Vec<Value> = catalog()["agents"].as_object().expect("catalog agents").iter()
        .map(|(id, agent)| json!({"id":id,"name":agent["name"],"description":agent["description"],
            "family":agent["family"],"skill":format!("specialist-{id}"),"resources":agent["resources"]}))
        .collect();
    json!({"schema_version":1,"agents":agents,"readiness":"Run specialist doctor inside the selected worker; this index is capability metadata."})
}

pub fn describe(id: &str) -> Option<Value> {
    let profile = catalog()["agents"].get(id)?;
    let operations: serde_json::Map<String, Value> = catalog()["operations"]
        .as_object()?
        .iter()
        .filter(|(operation, _)| operation_allowed(id, operation))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let tools: serde_json::Map<String, Value> = profile["tools"]
        .as_array()?
        .iter()
        .filter_map(|key| {
            key.as_str().and_then(|key| {
                catalog()["tools"]
                    .get(key)
                    .map(|tool| (key.to_string(), tool.clone()))
            })
        })
        .collect();
    let mut result = profile.clone();
    result["id"] = json!(id);
    result["operations"] = json!(operations);
    result["tool_details"] = json!(tools);
    Some(result)
}

pub fn operation_allowed(agent: &str, operation: &str) -> bool {
    let Some(tool) = catalog()["operations"]
        .get(operation)
        .and_then(|op| op["tool"].as_str())
    else {
        return false;
    };
    catalog()["agents"][agent]["tools"]
        .as_array()
        .is_some_and(|tools| tools.iter().any(|value| value.as_str() == Some(tool)))
}

pub fn workflow_known(agent: &str, workflow: &str) -> bool {
    catalog()["agents"][agent]["workflows"]
        .get(workflow)
        .is_some()
}

pub fn skills() -> Vec<crate::skills::Skill> {
    catalog()["agents"]
        .as_object()
        .expect("catalog agents must be an object")
        .iter()
        .map(|(id, profile)| crate::skills::Skill {
            name: format!("specialist-{id}"),
            description: profile["description"].as_str().unwrap_or("").into(),
            body: profile["skill"].as_str().unwrap_or("").into(),
            updated_at: 0,
        })
        .collect()
}

pub fn prompt(id: &str) -> String {
    let profile = &catalog()["agents"][id];
    format!(
        "\nSelected specialist: {id} — {}. {}\nLoad specialist-{id}; choose the intended task, then scope specialist describe and doctor with its operation or workflow selector. Use unscoped discovery for a full profile audit. The worker image is fixed by the operator; profile selection does not install tools, grant MCP access or change permissions. Report missing coverage explicitly.\n",
        profile["name"].as_str().unwrap_or(id),
        profile["description"].as_str().unwrap_or("")
    )
}
