use agent_rush::mcp_client::{McpRegistry, McpServerConfig};
use axum::{
    Router,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

const SECRET: &str = "Bearer sk-live-very-secret-token-123";

#[derive(Default)]
struct Calls {
    initialize: AtomicUsize,
    initialized: AtomicUsize,
}

fn rpc_result(id: &Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}

async fn handle(calls: Arc<Calls>, sse: bool, headers: HeaderMap, body: String) -> Response {
    let message: Value = serde_json::from_str(&body).unwrap();
    let id = message["id"].clone();
    let auth = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    if auth != SECRET {
        return (StatusCode::UNAUTHORIZED, "missing credentials").into_response();
    }
    let method = message["method"].as_str().unwrap_or("");
    if method == "initialize" {
        calls.initialize.fetch_add(1, Ordering::SeqCst);
        assert_eq!(message["params"]["protocolVersion"], "2025-06-18");
        let body = rpc_result(
            &id,
            json!({"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"fake","version":"1"}}),
        );
        return ([("mcp-session-id", "sess-1")], axum::Json(body)).into_response();
    }
    if method == "notifications/initialized" {
        calls.initialized.fetch_add(1, Ordering::SeqCst);
        return StatusCode::ACCEPTED.into_response();
    }
    if headers.get("mcp-session-id").and_then(|v| v.to_str().ok()) != Some("sess-1") {
        return (StatusCode::BAD_REQUEST, "no session").into_response();
    }
    let result = match method {
        "tools/list" if message["params"]["cursor"].is_null() => json!({
            "tools":[
                {"name":"echo","description":"Echo text","inputSchema":{"type":"object","properties":{"text":{"type":"string"}}}},
                {"name":"secret_tool","description":"Not allowed","inputSchema":{"type":"object"}}
            ],
            "nextCursor":"page2"
        }),
        "tools/list" => json!({"tools":[{"name":"fail","description":"Always fails"}]}),
        "tools/call" => match message["params"]["name"].as_str() {
            Some("echo") => {
                json!({"content":[{"type":"text","text":message["params"]["arguments"]["text"]}]})
            }
            Some("fail") => {
                // A careless server that echoes the credential back in its error.
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("upstream rejected authorization {auth}"),
                )
                    .into_response();
            }
            _ => {
                return axum::Json(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":format!("unknown tool; you sent {auth}")}}))
                    .into_response();
            }
        },
        _ => json!({}),
    };
    let body = rpc_result(&id, result);
    if sse {
        let stream = format!(
            ": keepalive\n\nevent: message\ndata: {}\n\nevent: message\ndata: {}\n\n",
            json!({"jsonrpc":"2.0","method":"notifications/progress","params":{"progress":1}}),
            body
        );
        return ([(header::CONTENT_TYPE, "text/event-stream")], stream).into_response();
    }
    axum::Json(body).into_response()
}

async fn fake_server() -> (String, Arc<Calls>) {
    let calls = Arc::new(Calls::default());
    let (json_calls, sse_calls) = (calls.clone(), calls.clone());
    let app = Router::new()
        .route(
            "/mcp",
            post(move |headers: HeaderMap, body: String| {
                handle(json_calls.clone(), false, headers, body)
            }),
        )
        .route(
            "/sse",
            post(move |headers: HeaderMap, body: String| {
                handle(sse_calls.clone(), true, headers, body)
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, calls)
}

fn config(name: &str, url: String, allowed: Option<Vec<&str>>) -> McpServerConfig {
    McpServerConfig {
        name: name.into(),
        url,
        headers: BTreeMap::from([("Authorization".to_string(), SECRET.to_string())]),
        allowed_tools: allowed.map(|v| v.into_iter().map(String::from).collect()),
    }
}

#[tokio::test]
async fn initializes_once_lists_and_calls_tools() {
    let (base, calls) = fake_server().await;
    let registry = McpRegistry::new(vec![config("docs", format!("{base}/mcp"), None)]).unwrap();
    assert_eq!(registry.names(), vec!["docs"]);
    let tools = registry.list_tools("docs").await.unwrap();
    assert_eq!(
        tools.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        vec!["echo", "secret_tool", "fail"],
        "pagination is followed"
    );
    assert_eq!(
        tools[0].input_schema["properties"]["text"]["type"],
        "string"
    );
    assert_eq!(tools[2].input_schema, json!({"type":"object"}));
    let result = registry
        .call("docs", "echo", json!({"text":"hello"}))
        .await
        .unwrap();
    assert_eq!(result["content"][0]["text"], "hello");
    assert_eq!(calls.initialize.load(Ordering::SeqCst), 1);
    assert_eq!(calls.initialized.load(Ordering::SeqCst), 1);
    assert!(registry.list_tools("missing").await.is_err());
}

#[tokio::test]
async fn reads_the_matching_message_from_an_sse_response() {
    let (base, _) = fake_server().await;
    let registry = McpRegistry::new(vec![config("live", format!("{base}/sse"), None)]).unwrap();
    let result = registry
        .call("live", "echo", json!({"text":"streamed"}))
        .await
        .unwrap();
    assert_eq!(result["content"][0]["text"], "streamed");
    assert_eq!(registry.list_tools("live").await.unwrap().len(), 3);
}

#[tokio::test]
async fn allowed_tools_filter_listing_and_refuse_calls() {
    let (base, _) = fake_server().await;
    let registry = McpRegistry::new(vec![config(
        "docs",
        format!("{base}/mcp"),
        Some(vec!["echo"]),
    )])
    .unwrap();
    let tools = registry.list_tools("docs").await.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "echo");
    let error = registry
        .call("docs", "secret_tool", json!({}))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("not allowed"), "{error}");
}

#[tokio::test]
async fn errors_never_reveal_header_values() {
    let (base, _) = fake_server().await;
    let registry = McpRegistry::new(vec![config("docs", format!("{base}/mcp"), None)]).unwrap();
    for tool in ["fail", "nonexistent"] {
        let error = format!(
            "{:#}",
            registry.call("docs", tool, json!({})).await.unwrap_err()
        );
        assert!(error.contains("docs"), "{error}");
        assert!(error.contains("[redacted]"), "{error}");
        assert!(!error.contains("very-secret"), "{error}");
    }
    let mut wrong = config("docs", format!("{base}/mcp"), None);
    wrong
        .headers
        .insert("Authorization".into(), "Bearer other-credential-xyz".into());
    let registry = McpRegistry::new(vec![wrong.clone()]).unwrap();
    let error = format!("{:#}", registry.list_tools("docs").await.unwrap_err());
    assert!(error.contains("401"), "{error}");
    assert!(!error.contains("other-credential"), "{error}");
    assert!(!format!("{wrong:?}").contains("other-credential"));
}

#[test]
fn config_file_is_validated() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mcp.json");
    std::fs::write(
        &path,
        r#"{"servers":[{"name":"docs","url":"https://mcp.example.com/mcp","headers":{"Authorization":"Bearer x"},"allowed_tools":["search"]}]}"#,
    )
    .unwrap();
    assert_eq!(McpRegistry::from_file(&path).unwrap().names(), vec!["docs"]);
    std::fs::write(&path, r#"[{"name":"a","url":"https://a.example.com"}]"#).unwrap();
    assert_eq!(McpRegistry::from_file(&path).unwrap().names(), vec!["a"]);
    for bad in [
        r#"[{"name":"Bad Name","url":"https://a.example.com"}]"#,
        r#"[{"name":"a","url":"http://a.example.com"}]"#,
        r#"[{"name":"a","url":"https://a.example.com"},{"name":"a","url":"https://b.example.com"}]"#,
        r#"[{"name":"a","url":"https://a.example.com","extra":1}]"#,
    ] {
        std::fs::write(&path, bad).unwrap();
        assert!(McpRegistry::from_file(&path).is_err(), "{bad}");
    }
    assert!(McpRegistry::empty().names().is_empty());
}
