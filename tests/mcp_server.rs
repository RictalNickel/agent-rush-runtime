mod common;

use agent_rush::session::SessionLimits;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{App, app, app_with_limits, bash, reply, send};
use http_body_util::BodyExt;
use serde_json::{Value, json};

async fn rpc(a: &App, key: Option<&str>, message: Value) -> (StatusCode, Value) {
    let mut builder = Request::post("/mcp")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream");
    if let Some(key) = key {
        builder = builder.header("authorization", format!("Bearer {key}"));
    }
    let response = send(
        &a.router,
        builder.body(Body::from(message.to_string())).unwrap(),
    )
    .await;
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// Calls a tool; returns (isError, the first text block parsed as JSON or as a string).
async fn tool(a: &App, key: &str, name: &str, arguments: Value) -> (bool, Value) {
    let (status, reply) = rpc(
        a,
        Some(key),
        json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":name,"arguments":arguments}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply["id"], 7);
    let result = &reply["result"];
    assert!(result.is_object(), "{name}: {reply}");
    let text = result["content"][0]["text"].as_str().unwrap_or("");
    let value = serde_json::from_str(text).unwrap_or_else(|_| json!(text));
    (result["isError"] == true, value)
}

#[tokio::test]
async fn no_key_is_401_with_a_bearer_challenge() {
    let a = app(vec![]);
    let response = send(
        &a.router,
        Request::post("/mcp")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers()["www-authenticate"], "Bearer");
    // A revoked key is refused too.
    let created = a.ledger().create_api_key("alice", "x").unwrap();
    a.ledger()
        .revoke_api_key("alice", &created.view.id)
        .unwrap();
    let (status, _) = rpc(
        &a,
        Some(&created.key),
        json!({"jsonrpc":"2.0","id":1,"method":"ping"}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // There is no server-sent stream: GET is not allowed.
    let response = send(&a.router, Request::get("/mcp").body(Body::empty()).unwrap()).await;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn initialize_list_start_send_and_wait_for_reply() {
    let a = app(vec![
        bash("echo hello > hello.txt"),
        reply("Wrote hello.txt"),
        reply("Second answer"),
    ]);
    let key = a.key("alice");

    let (status, init) = rpc(
        &a,
        Some(&key),
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["capabilities"], json!({"tools":{}}));
    assert_eq!(init["result"]["serverInfo"]["name"], "agent-rush");

    let (status, _) = rpc(
        &a,
        Some(&key),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let (_, list) = rpc(
        &a,
        Some(&key),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
    )
    .await;
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "start_session",
            "send_message",
            "wait_for_reply",
            "get_events",
            "run_command",
            "read_file",
            "write_file",
            "list_files",
            "cancel",
            "close_session",
            "list_sessions",
            "manage_skills"
        ]
    );
    for t in list["result"]["tools"].as_array().unwrap() {
        assert_eq!(t["inputSchema"]["type"], "object", "{t}");
    }

    let (error, started) = tool(
        &a,
        &key,
        "start_session",
        json!({"title":"demo","message":"write a file"}),
    )
    .await;
    assert!(!error, "{started}");
    let id = started["session_id"].as_str().unwrap().to_string();

    let (error, waited) = tool(
        &a,
        &key,
        "wait_for_reply",
        json!({"session_id": id, "timeout_seconds": 10}),
    )
    .await;
    assert!(!error, "{waited}");
    assert_eq!(waited["status"], "idle");
    assert_eq!(waited["still_running"], false);
    assert_eq!(waited["reply"], "Wrote hello.txt");
    let summary = waited["events"].as_array().unwrap();
    assert!(
        summary[0]
            .as_str()
            .unwrap()
            .starts_with("#1 user: write a file")
    );
    assert!(
        summary
            .iter()
            .any(|e| e.as_str().unwrap().contains("tool_call: bash"))
    );

    let (error, sent) = tool(
        &a,
        &key,
        "send_message",
        json!({"session_id": id, "text": "and again"}),
    )
    .await;
    assert!(!error, "{sent}");
    assert_eq!(sent["interjection"], false);
    let (_, waited) = tool(&a, &key, "wait_for_reply", json!({"session_id": id})).await;
    assert_eq!(waited["reply"], "Second answer");
    assert!(
        waited["events"][0]
            .as_str()
            .unwrap()
            .contains("user: and again")
    );

    // Direct workspace tools.
    let (error, _) = tool(
        &a,
        &key,
        "write_file",
        json!({"session_id": id, "path": "notes.md", "content": "# notes"}),
    )
    .await;
    assert!(!error);
    let (error, read) = tool(
        &a,
        &key,
        "read_file",
        json!({"session_id": id, "path": "notes.md"}),
    )
    .await;
    assert!(!error);
    assert_eq!(read, json!("# notes"));
    let (_, written) = tool(
        &a,
        &key,
        "write_file",
        json!({"session_id": id, "path": "bin.dat", "content": "AAEC/w==", "encoding": "base64"}),
    )
    .await;
    assert_eq!(written["bytes"], 4);
    let (_, read) = tool(
        &a,
        &key,
        "read_file",
        json!({"session_id": id, "path": "bin.dat"}),
    )
    .await;
    assert_eq!(read["encoding"], "base64");
    assert_eq!(read["data"], "AAEC/w==");
    let (_, listing) = tool(&a, &key, "list_files", json!({"session_id": id})).await;
    assert_eq!(listing["entries"].as_array().unwrap().len(), 2);
    let (_, ran) = tool(
        &a,
        &key,
        "run_command",
        json!({"session_id": id, "command": "pwd"}),
    )
    .await;
    assert_eq!(ran["stdout"], "ran: pwd\n");
    let (_, sessions) = tool(&a, &key, "list_sessions", json!({})).await;
    assert_eq!(sessions["sessions"][0]["session_id"], id);
    let (_, events) = tool(
        &a,
        &key,
        "get_events",
        json!({"session_id": id, "after": 0, "limit": 2}),
    )
    .await;
    assert_eq!(events["events"].as_array().unwrap().len(), 2);

    let (error, saved) = tool(
        &a,
        &key,
        "manage_skills",
        json!({"action":"save","name":"notes","description":"Keep notes","body":"Write notes.md"}),
    )
    .await;
    assert!(!error, "{saved}");
    let (_, got) = tool(
        &a,
        &key,
        "manage_skills",
        json!({"action":"get","name":"notes"}),
    )
    .await;
    assert!(got.as_str().unwrap().contains("name: notes"));

    let (error, _) = tool(&a, &key, "close_session", json!({"session_id": id})).await;
    assert!(!error);
    let (error, closed) = tool(
        &a,
        &key,
        "send_message",
        json!({"session_id": id, "text": "hi"}),
    )
    .await;
    assert!(error);
    assert_eq!(closed, json!("Session is closed"));
}

#[tokio::test]
async fn unknown_tools_methods_and_bad_params_are_protocol_errors() {
    let a = app(vec![]);
    let key = a.key("alice");
    let (_, reply) = rpc(
        &a,
        Some(&key),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"rm_rf","arguments":{}}}),
    )
    .await;
    assert_eq!(reply["error"]["code"], -32602);
    assert_eq!(reply["id"], 3);
    let (_, reply) = rpc(
        &a,
        Some(&key),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"send_message","arguments":{"session_id":"x"}}}),
    )
    .await;
    assert_eq!(reply["error"]["code"], -32602, "missing text: {reply}");
    let (_, reply) = rpc(
        &a,
        Some(&key),
        json!({"jsonrpc":"2.0","id":5,"method":"resources/list"}),
    )
    .await;
    assert_eq!(reply["error"]["code"], -32601);
    let (status, reply) = rpc(&a, Some(&key), json!("not an object")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply["error"]["code"], -32600);

    // Batches answer every request and skip notifications.
    let (status, reply) = rpc(
        &a,
        Some(&key),
        json!([
            {"jsonrpc":"2.0","id":1,"method":"ping"},
            {"jsonrpc":"2.0","method":"notifications/initialized"},
            {"jsonrpc":"2.0","id":2,"method":"nope"}
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply[0]["result"], json!({}));
    assert_eq!(reply[1]["error"]["code"], -32601);
    assert_eq!(reply.as_array().unwrap().len(), 2);
    let (status, _) = rpc(
        &a,
        Some(&key),
        json!([{"jsonrpc":"2.0","method":"notifications/initialized"}]),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
}

#[tokio::test]
async fn another_owners_session_is_not_found_on_every_session_tool() {
    let a = app(vec![]);
    let alice = a.key("alice");
    let bob = a.key("bob");
    let (_, started) = tool(&a, &alice, "start_session", json!({})).await;
    let id = started["session_id"].as_str().unwrap().to_string();
    let missing = "00000000-0000-0000-0000-000000000000";
    for target in [id.as_str(), missing] {
        for (name, extra) in [
            ("send_message", json!({"text":"hi"})),
            ("wait_for_reply", json!({"timeout_seconds": 1})),
            ("get_events", json!({})),
            ("run_command", json!({"command":"id"})),
            ("read_file", json!({"path":"a.txt"})),
            ("write_file", json!({"path":"a.txt","content":"x"})),
            ("list_files", json!({})),
            ("cancel", json!({})),
            ("close_session", json!({})),
        ] {
            let mut args = extra.as_object().unwrap().clone();
            args.insert("session_id".into(), json!(target));
            let (error, text) = tool(&a, &bob, name, Value::Object(args)).await;
            assert!(error, "{name} on {target}");
            assert_eq!(text, json!("Session not found"), "{name} on {target}");
        }
    }
    let (_, list) = tool(&a, &bob, "list_sessions", json!({})).await;
    assert_eq!(list["sessions"], json!([]));
    // Alice's session is untouched.
    let view = a.sessions.get("alice", &id).unwrap().unwrap();
    assert_eq!(view.last_seq, 0);
    assert!(view.box_id.is_none());
}

/// Regression: a batch could hold any number of calls (each one up to a 300 s wait).
#[tokio::test]
async fn batches_beyond_the_cap_are_refused() {
    let a = app(vec![]);
    let key = a.key("alice");
    let ping = |id: usize| json!({"jsonrpc":"2.0","id":id,"method":"ping"});
    let max = SessionLimits::default().max_mcp_batch;
    assert_eq!(max, 16);
    let (status, reply) = rpc(
        &a,
        Some(&key),
        json!((0..max).map(ping).collect::<Vec<_>>()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply.as_array().unwrap().len(), max);
    let (status, reply) = rpc(
        &a,
        Some(&key),
        json!((0..=max).map(ping).collect::<Vec<_>>()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(reply["error"]["code"], -32600, "{reply}");
    assert!(
        reply["error"]["message"]
            .as_str()
            .unwrap()
            .contains("at most 16"),
        "{reply}"
    );
}

/// Regression: start_session had no cap on open sessions per owner.
#[tokio::test]
async fn start_session_beyond_the_open_session_cap_is_a_tool_error() {
    let a = app_with_limits(
        vec![],
        SessionLimits {
            max_open_sessions_per_owner: 1,
            ..SessionLimits::default()
        },
    );
    let key = a.key("alice");
    let (is_error, first) = tool(&a, &key, "start_session", json!({})).await;
    assert!(!is_error, "{first}");
    let (is_error, second) = tool(&a, &key, "start_session", json!({})).await;
    assert!(is_error);
    assert_eq!(second, json!("too many open sessions; close one"));
    let id = first["session_id"].as_str().unwrap();
    let (is_error, _) = tool(&a, &key, "close_session", json!({"session_id": id})).await;
    assert!(!is_error);
    let (is_error, _) = tool(&a, &key, "start_session", json!({})).await;
    assert!(!is_error);
}

/// Regression: wait_for_reply and workspace calls had no per-owner concurrency cap.
#[tokio::test]
async fn long_tool_calls_beyond_the_per_owner_cap_are_tool_errors() {
    let a = app_with_limits(
        vec![],
        SessionLimits {
            max_long_requests_per_owner: 1,
            ..SessionLimits::default()
        },
    );
    let key = a.key("alice");
    let session = a.sessions.create("alice", None).unwrap();
    let held = a.sessions.long_request_slot("alice").unwrap();
    let id = session.id.as_str();
    for (name, arguments) in [
        (
            "wait_for_reply",
            json!({"session_id": id, "timeout_seconds": 1}),
        ),
        ("run_command", json!({"session_id": id, "command": "id"})),
        ("read_file", json!({"session_id": id, "path": "a.txt"})),
        (
            "write_file",
            json!({"session_id": id, "path": "a.txt", "content": "x"}),
        ),
        ("list_files", json!({"session_id": id})),
    ] {
        let (is_error, message) = tool(&a, &key, name, arguments).await;
        assert!(is_error, "{name}: {message}");
        assert_eq!(
            message,
            json!("too many requests in flight; wait for one to finish"),
            "{name}"
        );
    }
    // Quick calls are not capped.
    let (is_error, _) = tool(&a, &key, "list_sessions", json!({})).await;
    assert!(!is_error);
    drop(held);
    let (is_error, reply) = tool(
        &a,
        &key,
        "wait_for_reply",
        json!({"session_id": id, "timeout_seconds": 1}),
    )
    .await;
    assert!(!is_error, "{reply}");
}
