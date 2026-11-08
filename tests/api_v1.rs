mod common;

use agent_rush::{
    api_keys::looks_like_key,
    session::SessionLimits,
    session_store::SessionStatus,
    webhooks::{self, UrlGuard, hmac_sha256, is_forbidden_ip, signature},
};
use axum::{
    Router,
    body::{Body, Bytes},
    http::{HeaderMap, Request, StatusCode},
    routing::post,
};
use common::{
    App, app, app_with_limits, app_without_sessions, bash, call, call_operator, operator_login,
    reply, send,
};
use futures_util::StreamExt;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(10);

/// Every key-scoped route, with a placeholder session id.
fn key_scoped_routes(id: &str) -> Vec<(&'static str, String)> {
    vec![
        ("GET", "/v1/status".into()),
        ("GET", "/v1/keys".into()),
        ("POST", "/v1/keys".into()),
        ("DELETE", "/v1/keys/some-key".into()),
        ("GET", "/v1/sessions".into()),
        ("POST", "/v1/sessions".into()),
        ("GET", format!("/v1/sessions/{id}")),
        ("DELETE", format!("/v1/sessions/{id}")),
        ("DELETE", format!("/v1/sessions/{id}?purge=true")),
        ("POST", "/v1/sessions/purge-closed".into()),
        ("POST", format!("/v1/sessions/{id}/messages")),
        ("POST", format!("/v1/sessions/{id}/cancel")),
        ("GET", format!("/v1/sessions/{id}/events")),
        ("GET", format!("/v1/sessions/{id}/stream")),
        ("GET", format!("/v1/sessions/{id}/archive")),
        ("POST", format!("/v1/sessions/{id}/fork")),
        ("GET", "/v1/models".into()),
        ("GET", "/v1/usage".into()),
        ("GET", format!("/v1/sessions/{id}/files?path=a.txt")),
        ("PUT", format!("/v1/sessions/{id}/files?path=a.txt")),
        ("POST", format!("/v1/sessions/{id}/exec")),
        ("GET", "/v1/skills".into()),
        ("POST", "/v1/skills".into()),
        ("GET", "/v1/skills/demo".into()),
        ("PUT", "/v1/skills/demo".into()),
        ("DELETE", "/v1/skills/demo".into()),
        ("GET", "/v1/agent-config".into()),
        ("PUT", "/v1/agent-config".into()),
        ("GET", "/v1/agent-config/history".into()),
        ("POST", "/v1/agent-config/revert".into()),
        ("GET", "/v1/webhooks".into()),
        ("POST", "/v1/webhooks".into()),
        ("DELETE", "/v1/webhooks/some-hook".into()),
        ("POST", "/v1/voice/transcribe".into()),
        ("POST", "/v1/voice/speak".into()),
    ]
}

#[tokio::test]
async fn every_key_scoped_route_rejects_a_missing_key() {
    let a = app(vec![]);
    let session = a.sessions.create("alice", None).unwrap();
    for (method, path) in key_scoped_routes(&session.id) {
        let (status, body) = call(&a.router, method, &path, None, Some(json!({}))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {path}");
        assert!(
            body["error"].as_str().unwrap().contains("API key"),
            "{body}"
        );
        // A malformed or unknown key is rejected the same way.
        let (status, _) = call(&a.router, method, &path, Some("ar_nope"), None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {path}");
    }
}

#[tokio::test]
async fn keys_are_shown_once_listed_without_secret_and_revocable() {
    let a = app(vec![]);
    let (cookie, csrf) = operator_login(&a.router).await;
    let mut created = Vec::new();
    for name in ["ci", "laptop"] {
        let (status, body) = call_operator(
            &a.router,
            "POST",
            "/v1/keys",
            &cookie,
            Some(&csrf),
            Some(json!({"name": name})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        created.push(body);
    }
    let key = created[0]["key"].as_str().unwrap().to_string();
    let other = created[1]["key"].as_str().unwrap().to_string();
    assert!(looks_like_key(&key) && key.len() == 51, "{key}");
    assert_eq!(created[0]["prefix"], key[..11]);
    // A key can list the owner's keys, but never sees key material.
    let (status, list) = call(&a.router, "GET", "/v1/keys", Some(&key), None).await;
    assert_eq!(status, StatusCode::OK);
    let text = list.to_string();
    for k in [&key, &other] {
        assert!(!text.contains(k.as_str()), "a key must never be listed");
        assert!(!text.contains(&agent_rush::api_keys::key_hash(k)));
    }
    assert_eq!(list["keys"].as_array().unwrap().len(), 2);

    // Another owner cannot revoke it, even through the operator path.
    let id = created[1]["id"].as_str().unwrap();
    assert!(!a.ledger().revoke_api_key("bob", id).unwrap());

    let (status, _) = call_operator(
        &a.router,
        "DELETE",
        &format!("/v1/keys/{id}"),
        &cookie,
        Some(&csrf),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = call(&a.router, "GET", "/v1/sessions", Some(&other), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "Invalid or revoked API key");
    // The first key still works, and the revoked key shows as revoked.
    let (status, list) = call(&a.router, "GET", "/v1/keys", Some(&key), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        list["keys"]
            .as_array()
            .unwrap()
            .iter()
            .any(|k| k["id"] == id && k["revoked"] == true)
    );
    let (status, _) = call_operator(
        &a.router,
        "DELETE",
        "/v1/keys/no-such-key",
        &cookie,
        Some(&csrf),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_operator_cookie_with_csrf_can_create_keys() {
    let a = app(vec![]);
    let (cookie, csrf) = operator_login(&a.router).await;
    let (status, _) = call_operator(
        &a.router,
        "POST",
        "/v1/keys",
        &cookie,
        None,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "CSRF header required");
    let (status, body) = call_operator(
        &a.router,
        "POST",
        "/v1/keys",
        &cookie,
        Some(&csrf),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let key = body["key"].as_str().unwrap();
    assert_eq!(
        a.ledger().api_key_owner(key).unwrap().as_deref(),
        Some("operator")
    );
}

/// Regression: a bearer key could mint more keys, revoke the operator's other keys and
/// register webhooks. Those routes need the operator cookie with CSRF.
#[tokio::test]
async fn a_bearer_key_cannot_mint_or_revoke_keys_or_manage_webhooks() {
    let a = app(vec![]);
    let key = a.key("operator");
    let victim = a.ledger().create_api_key("operator", "victim").unwrap();
    let hook = a
        .ledger()
        .create_webhook("operator", "https://93.184.215.14/hook")
        .unwrap();
    let routes = [
        (
            "POST",
            "/v1/keys".to_string(),
            Some(json!({"name": "minted"})),
        ),
        ("DELETE", format!("/v1/keys/{}", victim.view.id), None),
        (
            "POST",
            "/v1/webhooks".to_string(),
            Some(json!({"url": "https://93.184.215.14/other"})),
        ),
        ("DELETE", format!("/v1/webhooks/{}", hook.webhook.id), None),
    ];
    let (cookie, csrf) = operator_login(&a.router).await;
    for (method, path, body) in routes {
        let (status, reply) = call(&a.router, method, &path, Some(&key), body.clone()).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}: {reply}");
        assert!(
            reply["error"].as_str().unwrap().contains("operator"),
            "{reply}"
        );
        // A bearer key next to the operator cookie does not get through either.
        let mut builder = Request::builder()
            .method(method)
            .uri(&path)
            .header("authorization", format!("Bearer {key}"))
            .header("cookie", &cookie)
            .header("x-csrf-token", &csrf);
        let request_body = match body {
            Some(v) => {
                builder = builder.header("content-type", "application/json");
                Body::from(v.to_string())
            }
            None => Body::empty(),
        };
        let response = send(&a.router, builder.body(request_body).unwrap()).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method} {path}");
        // Without any credentials it is 401.
        let (status, _) = call(&a.router, method, &path, None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {path}");
    }
    // Nothing changed: two keys (neither revoked) and the one webhook.
    let keys = a.ledger().api_keys("operator").unwrap();
    assert_eq!(keys.len(), 2);
    assert!(keys.iter().all(|k| !k.revoked));
    assert_eq!(a.ledger().webhooks("operator").unwrap().len(), 1);
    // Listing stays open to keys and shows no key material or webhook secret.
    let (status, list) = call(&a.router, "GET", "/v1/keys", Some(&key), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!list.to_string().contains(&victim.key));
    let (status, list) = call(&a.router, "GET", "/v1/webhooks", Some(&key), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["webhooks"].as_array().unwrap().len(), 1);
    assert!(!list.to_string().contains(&hook.secret));
    // The operator cookie with CSRF can do all four.
    let (status, _) = call_operator(
        &a.router,
        "DELETE",
        &format!("/v1/webhooks/{}", hook.webhook.id),
        &cookie,
        Some(&csrf),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call_operator(
        &a.router,
        "DELETE",
        &format!("/v1/keys/{}", victim.view.id),
        &cookie,
        Some(&csrf),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

/// Regression: an owner could open sessions without bound.
#[tokio::test]
async fn open_sessions_per_owner_are_capped_and_closing_one_frees_a_place() {
    let a = app_with_limits(
        vec![],
        SessionLimits {
            max_open_sessions_per_owner: 2,
            ..SessionLimits::default()
        },
    );
    let alice = a.key("alice");
    let mut ids = Vec::new();
    for _ in 0..2 {
        let (status, body) = call(&a.router, "POST", "/v1/sessions", Some(&alice), None).await;
        assert_eq!(status, StatusCode::CREATED);
        ids.push(body["session"]["id"].as_str().unwrap().to_string());
    }
    let (status, body) = call(&a.router, "POST", "/v1/sessions", Some(&alice), None).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(body["error"], "too many open sessions; close one");
    // Another owner has its own allowance.
    let bob = a.key("bob");
    let (status, _) = call(&a.router, "POST", "/v1/sessions", Some(&bob), None).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = call(
        &a.router,
        "DELETE",
        &format!("/v1/sessions/{}", ids[0]),
        Some(&alice),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(&a.router, "POST", "/v1/sessions", Some(&alice), None).await;
    assert_eq!(status, StatusCode::CREATED);
}

/// Regression: an owner could hold any number of SSE streams open.
#[tokio::test]
async fn concurrent_event_streams_per_owner_are_capped() {
    let a = app_with_limits(
        vec![],
        SessionLimits {
            max_streams_per_owner: 2,
            ..SessionLimits::default()
        },
    );
    let alice = a.key("alice");
    let session = a.sessions.create("alice", None).unwrap();
    let stream = |key: &str| {
        Request::get(format!("/v1/sessions/{}/stream", session.id))
            .header("authorization", format!("Bearer {key}"))
            .body(Body::empty())
            .unwrap()
    };
    let first = send(&a.router, stream(&alice)).await;
    assert_eq!(first.status(), StatusCode::OK);
    let second = send(&a.router, stream(&alice)).await;
    assert_eq!(second.status(), StatusCode::OK);
    let third = send(&a.router, stream(&alice)).await;
    assert_eq!(third.status(), StatusCode::TOO_MANY_REQUESTS);
    // A disconnect (the stream is dropped) gives its place back.
    drop(first);
    let again = send(&a.router, stream(&alice)).await;
    assert_eq!(again.status(), StatusCode::OK);
    drop((second, again));
}

/// Regression: an owner could keep any number of workspace calls in flight. The slot is
/// taken before an upload body is read.
#[tokio::test]
async fn long_requests_per_owner_are_capped() {
    let a = app_with_limits(
        vec![],
        SessionLimits {
            max_long_requests_per_owner: 2,
            ..SessionLimits::default()
        },
    );
    let alice = a.key("alice");
    let session = a.sessions.create("alice", None).unwrap();
    let held: Vec<_> = (0..2)
        .map(|_| a.sessions.long_request_slot("alice").unwrap())
        .collect();
    let id = &session.id;
    for (method, path, body) in [
        (
            "POST",
            format!("/v1/sessions/{id}/exec"),
            Some(json!({"command":"id"})),
        ),
        ("GET", format!("/v1/sessions/{id}/files?path=."), None),
        (
            "PUT",
            format!("/v1/sessions/{id}/files?path=a.txt"),
            Some(json!("x")),
        ),
    ] {
        let (status, reply) = call(&a.router, method, &path, Some(&alice), body).await;
        assert_eq!(
            status,
            StatusCode::TOO_MANY_REQUESTS,
            "{method} {path}: {reply}"
        );
    }
    // Other owners are not affected, and the slots come back when released.
    let bob = a.key("bob");
    let (status, _) = call(&a.router, "GET", "/v1/sessions", Some(&bob), None).await;
    assert_eq!(status, StatusCode::OK);
    drop(held);
    let (status, reply) = call(
        &a.router,
        "POST",
        &format!("/v1/sessions/{id}/exec"),
        Some(&alice),
        Some(json!({"command":"id"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    // The finished request gave its slot back too.
    let _a = a.sessions.long_request_slot("alice").unwrap();
    let _b = a.sessions.long_request_slot("alice").unwrap();
}

#[tokio::test]
async fn another_owner_gets_404_on_every_session_route() {
    let a = app(vec![]);
    let alice = a.key("alice");
    let bob = a.key("bob");
    let (status, created) = call(&a.router, "POST", "/v1/sessions", Some(&alice), None).await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["session"]["id"].as_str().unwrap().to_string();
    let missing = "00000000-0000-0000-0000-000000000000";
    for target in [id.as_str(), missing] {
        let routes: Vec<(&str, String, Option<Value>)> = vec![
            ("GET", format!("/v1/sessions/{target}"), None),
            (
                "POST",
                format!("/v1/sessions/{target}/messages"),
                Some(json!({"text":"hi"})),
            ),
            ("POST", format!("/v1/sessions/{target}/cancel"), None),
            ("GET", format!("/v1/sessions/{target}/events"), None),
            ("GET", format!("/v1/sessions/{target}/stream"), None),
            ("GET", format!("/v1/sessions/{target}/archive"), None),
            ("POST", format!("/v1/sessions/{target}/fork"), None),
            (
                "GET",
                format!("/v1/sessions/{target}/files?path=a.txt"),
                None,
            ),
            (
                "PUT",
                format!("/v1/sessions/{target}/files?path=a.txt"),
                Some(json!("x")),
            ),
            (
                "POST",
                format!("/v1/sessions/{target}/exec"),
                Some(json!({"command":"id"})),
            ),
            ("DELETE", format!("/v1/sessions/{target}?purge=true"), None),
            ("DELETE", format!("/v1/sessions/{target}"), None),
        ];
        for (method, path, body) in routes {
            let (status, body) = call(&a.router, method, &path, Some(&bob), body).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path}: {body}");
            assert_eq!(body["error"], "Session not found", "{method} {path}");
        }
    }
    // Bob's attempts changed nothing: no box, no events, still idle, not closed.
    let view = a.sessions.get("alice", &id).unwrap().unwrap();
    assert_eq!(view.status, SessionStatus::Idle);
    assert_eq!(view.last_seq, 0);
    assert!(view.box_id.is_none());
    let (status, list) = call(&a.router, "GET", "/v1/sessions", Some(&bob), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["sessions"], json!([]));
}

/// Reads SSE frames until `done` holds for the event ids seen so far.
async fn read_sse(
    body: &mut (impl futures_util::Stream<Item = Result<Bytes, axum::Error>> + Unpin),
    ids: &mut Vec<i64>,
    kinds: &mut Vec<String>,
    done: impl Fn(&[String]) -> bool,
) {
    let mut buffer = String::new();
    tokio::time::timeout(WAIT, async {
        while !done(kinds) {
            let chunk = body.next().await.expect("stream ended").unwrap();
            buffer.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(end) = buffer.find("\n\n") {
                let frame: String = buffer.drain(..end + 2).collect();
                for line in frame.lines() {
                    if let Some(id) = line.strip_prefix("id: ") {
                        ids.push(id.parse().unwrap());
                    }
                    if let Some(kind) = line.strip_prefix("event: ") {
                        kinds.push(kind.to_string());
                    }
                }
            }
        }
    })
    .await
    .expect("timed out reading the event stream");
}

#[tokio::test]
async fn session_turn_then_sse_replays_after_last_event_id_and_streams_live() {
    let a = app(vec![bash("echo hi"), reply("Done: hi")]);
    let alice = a.key("alice");
    let (status, created) = call(
        &a.router,
        "POST",
        "/v1/sessions",
        Some(&alice),
        Some(json!({"title":"demo","message":"say hi"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["session"]["id"].as_str().unwrap().to_string();
    assert_eq!(created["sent"]["interjection"], false);
    let view = a.sessions.wait_idle("alice", &id, WAIT).await.unwrap();
    assert_eq!(view.status, SessionStatus::Idle);

    let (status, events) = call(
        &a.router,
        "GET",
        &format!("/v1/sessions/{id}/events?after=0&limit=1000"),
        Some(&alice),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let kinds: Vec<&str> = events["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        [
            "user",
            "status",
            "tool_call",
            "tool_result",
            "assistant",
            "status"
        ]
    );
    let last = events["next_after"].as_i64().unwrap();
    assert_eq!(last, 6);

    // Reconnect after event 2: the replay starts at 3 and has no repeats.
    let response = send(
        &a.router,
        Request::get(format!("/v1/sessions/{id}/stream"))
            .header("authorization", format!("Bearer {alice}"))
            .header("last-event-id", "2")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream")
    );
    let mut body = response.into_body().into_data_stream();
    let (mut ids, mut seen) = (Vec::new(), Vec::new());
    read_sse(&mut body, &mut ids, &mut seen, |k| k.len() >= 4).await;
    assert_eq!(ids, [3, 4, 5, 6]);
    assert_eq!(seen, ["tool_call", "tool_result", "assistant", "status"]);

    // Then live: a new message streams its events after the replay.
    let (status, sent) = call(
        &a.router,
        "POST",
        &format!("/v1/sessions/{id}/messages"),
        Some(&alice),
        Some(json!({"text":"again"})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(sent["seq"], 7);
    read_sse(&mut body, &mut ids, &mut seen, |k| {
        k.iter().filter(|k| *k == "assistant").count() == 2
    })
    .await;
    assert_eq!(&ids[4..7], [7, 8, 9]);

    // `?after=` works the same way.
    let response = send(
        &a.router,
        Request::get(format!("/v1/sessions/{id}/stream?after=8"))
            .header("authorization", format!("Bearer {alice}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    let mut body = response.into_body().into_data_stream();
    let (mut ids, mut seen) = (Vec::new(), Vec::new());
    read_sse(&mut body, &mut ids, &mut seen, |k| k.len() >= 2).await;
    assert_eq!(ids, [9, 10]);
}

#[tokio::test]
async fn files_exec_and_close() {
    let a = app(vec![]);
    let alice = a.key("alice");
    let (_, created) = call(&a.router, "POST", "/v1/sessions", Some(&alice), None).await;
    let id = created["session"]["id"].as_str().unwrap().to_string();

    let response = send(
        &a.router,
        Request::put(format!("/v1/sessions/{id}/files?path=notes/a.md"))
            .header("authorization", format!("Bearer {alice}"))
            .body(Body::from("# hello\n"))
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = send(
        &a.router,
        Request::get(format!("/v1/sessions/{id}/files?path=notes/a.md"))
            .header("authorization", format!("Bearer {alice}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "text/plain; charset=utf-8"
    );
    assert!(
        response.headers()["content-disposition"]
            .to_str()
            .unwrap()
            .starts_with("attachment")
    );
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&bytes[..], b"# hello\n");

    let (status, listing) = call(
        &a.router,
        "GET",
        &format!("/v1/sessions/{id}/files?path=."),
        Some(&alice),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing["entries"][0]["name"], "notes/a.md");

    let (status, missing) = call(
        &a.router,
        "GET",
        &format!("/v1/sessions/{id}/files?path=nope.txt"),
        Some(&alice),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{missing}");

    let (status, ran) = call(
        &a.router,
        "POST",
        &format!("/v1/sessions/{id}/exec"),
        Some(&alice),
        Some(json!({"command":"ls -la","timeout_seconds":30})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ran["stdout"], "ran: ls -la\n");
    let (status, _) = call(
        &a.router,
        "POST",
        &format!("/v1/sessions/{id}/exec"),
        Some(&alice),
        Some(json!({"command":"ls","timeout_seconds":0})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = call(
        &a.router,
        "DELETE",
        &format!("/v1/sessions/{id}"),
        Some(&alice),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = call(
        &a.router,
        "POST",
        &format!("/v1/sessions/{id}/messages"),
        Some(&alice),
        Some(json!({"text":"hi"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    // A plain DELETE keeps the closed session and its history.
    let view = a.sessions.get("alice", &id).unwrap().unwrap();
    assert_eq!(view.status, SessionStatus::Closed);
    let (status, list) = call(&a.router, "GET", "/v1/sessions", Some(&alice), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["sessions"][0]["id"], id.as_str());
    assert_eq!(list["sessions"][0]["status"], "closed");
}

#[tokio::test]
async fn delete_with_purge_removes_the_session_and_ends_its_stream() {
    let a = app(vec![reply("hi")]);
    let alice = a.key("alice");
    let bob = a.key("bob");
    let (_, created) = call(
        &a.router,
        "POST",
        "/v1/sessions",
        Some(&alice),
        Some(json!({"message":"hello"})),
    )
    .await;
    let id = created["session"]["id"].as_str().unwrap().to_string();
    a.sessions.wait_idle("alice", &id, WAIT).await.unwrap();
    let (status, _) = call(
        &a.router,
        "POST",
        &format!("/v1/sessions/{id}/exec"),
        Some(&alice),
        Some(json!({"command":"true"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(a.boxes.live(), 1);

    // A live stream is open while the session is deleted.
    let response = send(
        &a.router,
        Request::get(format!("/v1/sessions/{id}/stream"))
            .header("authorization", format!("Bearer {alice}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body().into_data_stream();
    let (mut ids, mut kinds) = (Vec::new(), Vec::new());
    read_sse(&mut body, &mut ids, &mut kinds, |k| {
        k.iter().any(|k| k == "assistant")
    })
    .await;

    let (status, body_bob) = call(
        &a.router,
        "DELETE",
        &format!("/v1/sessions/{id}?purge=true"),
        Some(&bob),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body_bob}");
    let (status, deleted) = call(
        &a.router,
        "DELETE",
        &format!("/v1/sessions/{id}?purge=true"),
        Some(&alice),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{deleted}");
    assert_eq!(deleted, json!({"closed": true, "deleted": true}));
    assert_eq!(a.boxes.live(), 0);
    assert!(
        a.ledger()
            .recent_session_events(&id, 1000)
            .unwrap()
            .is_empty()
    );

    // The stream delivers the closed status, then ends.
    tokio::time::timeout(WAIT, async {
        while let Some(chunk) = body.next().await {
            let text = String::from_utf8_lossy(&chunk.unwrap()).to_string();
            for line in text.lines() {
                if let Some(kind) = line.strip_prefix("event: ") {
                    kinds.push(kind.to_string());
                }
            }
        }
    })
    .await
    .expect("the stream did not end");
    assert_eq!(kinds.last().map(String::as_str), Some("status"));

    let (status, list) = call(&a.router, "GET", "/v1/sessions", Some(&alice), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["sessions"], json!([]));
    let (status, _) = call(
        &a.router,
        "GET",
        &format!("/v1/sessions/{id}"),
        Some(&alice),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(
        &a.router,
        "DELETE",
        &format!("/v1/sessions/{id}?purge=true"),
        Some(&alice),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn purge_closed_removes_only_the_callers_closed_sessions() {
    let a = app(vec![]);
    let alice = a.key("alice");
    let closed = a.sessions.create("alice", None).unwrap();
    let open = a.sessions.create("alice", None).unwrap();
    a.sessions.close("alice", &closed.id).await.unwrap();
    let bobs = a.sessions.create("bob", None).unwrap();
    a.sessions.close("bob", &bobs.id).await.unwrap();

    let (status, body) = call(
        &a.router,
        "POST",
        "/v1/sessions/purge-closed",
        Some(&alice),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({"deleted": 1}));
    assert!(a.sessions.get("alice", &closed.id).unwrap().is_none());
    assert!(a.sessions.get("alice", &open.id).unwrap().is_some());
    assert!(a.sessions.get("bob", &bobs.id).unwrap().is_some());

    // The operator cookie works too, with the CSRF header.
    let mine = a.sessions.create("operator", None).unwrap();
    a.sessions.close("operator", &mine.id).await.unwrap();
    let other = a.sessions.create("operator", None).unwrap();
    let (cookie, csrf) = operator_login(&a.router).await;
    let (status, _) = call_operator(
        &a.router,
        "POST",
        "/v1/sessions/purge-closed",
        &cookie,
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "CSRF header required");
    let (status, body) = call_operator(
        &a.router,
        "POST",
        "/v1/sessions/purge-closed",
        &cookie,
        Some(&csrf),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"deleted": 1}));
    let (status, _) = call_operator(
        &a.router,
        "DELETE",
        &format!("/v1/sessions/{}?purge=true", other.id),
        &cookie,
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "CSRF header required");
    let (status, body) = call_operator(
        &a.router,
        "DELETE",
        &format!("/v1/sessions/{}?purge=true", other.id),
        &cookie,
        Some(&csrf),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(a.sessions.get("operator", &other.id).unwrap().is_none());
}

#[tokio::test]
async fn sessions_without_a_worker_answer_503_but_skills_still_work() {
    let a = app_without_sessions();
    let key = a.key("alice");
    let (status, body) = call(&a.router, "GET", "/v1/sessions", Some(&key), None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(body["error"].as_str().unwrap().contains("not configured"));
    let (status, _) = call(&a.router, "GET", "/v1/skills", Some(&key), None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = call(&a.router, "GET", "/v1/status", Some(&key), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["sessions_available"], false);
    assert_eq!(body["voice_available"], false);
    assert_eq!(body["mcp_servers"], json!([]));
    assert_eq!(body["limits"]["max_file_bytes"], 25 * 1024 * 1024);
}

#[tokio::test]
async fn voice_routes_are_404_when_unconfigured() {
    let a = app(vec![]);
    let key = a.key("alice");
    for path in ["/v1/voice/transcribe", "/v1/voice/speak"] {
        let (status, body) = call(
            &a.router,
            "POST",
            path,
            Some(&key),
            Some(json!({"text":"hi"})),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        assert_eq!(body["error"], "Voice is not configured on this deployment");
    }
}

#[tokio::test]
async fn skills_and_agent_config_round_trip_with_the_mcp_allowlist_enforced() {
    let a = app(vec![]);
    let key = a.key("alice");
    let k = Some(key.as_str());
    let (status, _) = call(
        &a.router,
        "POST",
        "/v1/skills",
        k,
        Some(json!({"name":"deploy","description":"Ship it","body":"Run make deploy."})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, skill) = call(&a.router, "GET", "/v1/skills/deploy", k, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(skill["markdown"].as_str().unwrap().contains("name: deploy"));
    let (status, _) = call(
        &a.router,
        "PUT",
        "/v1/skills/deploy",
        k,
        Some(json!({"name":"other","description":"x","body":"y"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (_, list) = call(&a.router, "GET", "/v1/skills", k, None).await;
    assert_eq!(list["skills"][0]["name"], "deploy");
    let bob = a.key("bob");
    let (status, _) = call(&a.router, "DELETE", "/v1/skills/deploy", Some(&bob), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(&a.router, "DELETE", "/v1/skills/deploy", k, None).await;
    assert_eq!(status, StatusCode::OK);

    // An MCP server the operator did not configure is refused.
    let (status, body) = call(
        &a.router,
        "PUT",
        "/v1/agent-config",
        k,
        Some(json!({"config":{"mcp_servers":["not-configured"]}})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, body) = call(
        &a.router,
        "PUT",
        "/v1/agent-config",
        k,
        Some(json!({"config":{"system_addendum":"Be brief.","disabled_tools":["browser"]},"reason":"tune"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["version"], 1);
    let (_, current) = call(&a.router, "GET", "/v1/agent-config", k, None).await;
    assert_eq!(current["config"]["system_addendum"], "Be brief.");
    let (status, body) = call(
        &a.router,
        "POST",
        "/v1/agent-config/revert",
        k,
        Some(json!({"version":0})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["version"], 2);
    let (_, history) = call(&a.router, "GET", "/v1/agent-config/history", k, None).await;
    assert_eq!(history["history"].as_array().unwrap().len(), 2);
    assert_eq!(history["history"][0]["author"], "user");
    let (status, _) = call(
        &a.router,
        "POST",
        "/v1/agent-config/revert",
        k,
        Some(json!({"version":99})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[test]
fn hmac_matches_rfc_4231() {
    // RFC 4231 test case 2.
    assert_eq!(
        hex::encode(hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
    // Test case 6: a key longer than the block size.
    assert_eq!(
        hex::encode(hmac_sha256(
            &[0xaa; 131],
            b"Test Using Larger Than Block-Size Key - Hash Key First"
        )),
        "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
    );
}

#[tokio::test]
async fn the_production_guard_refuses_private_and_reserved_webhook_targets() {
    let public = UrlGuard::public();
    for url in [
        "https://127.0.0.1/hook",
        "https://localhost/hook",
        "https://10.1.2.3/hook",
        "https://172.16.0.1/hook",
        "https://192.168.1.1/hook",
        "https://169.254.169.254/latest/meta-data",
        "https://100.64.0.1/hook",
        "https://0.0.0.0/hook",
        "https://[::1]/hook",
        "https://[fd00::1]/hook",
        "https://[fe80::1]/hook",
        "https://[::ffff:10.0.0.1]/hook",
        "http://93.184.215.14/hook",
        "https://user:pw@93.184.215.14/hook",
        "not a url",
    ] {
        assert!(public.check(url).await.is_err(), "{url} must be refused");
    }
    assert!(public.check("https://93.184.215.14/hook").await.is_ok());
    assert!(!is_forbidden_ip("8.8.8.8".parse().unwrap()));
    // The test-only guard allows loopback and nothing else private.
    let test = UrlGuard::allow_loopback_for_tests();
    assert!(test.check("http://127.0.0.1:9/hook").await.is_ok());
    for url in [
        "http://10.0.0.1/hook",
        "http://169.254.169.254/",
        "http://[fd00::1]/",
    ] {
        assert!(test.check(url).await.is_err(), "{url} must be refused");
    }
    // The API applies the guard at create time.
    let a = app(vec![]);
    let (cookie, csrf) = operator_login(&a.router).await;
    let (status, body) = call_operator(
        &a.router,
        "POST",
        "/v1/webhooks",
        &cookie,
        Some(&csrf),
        Some(json!({"url":"http://169.254.169.254/latest"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn turn_end_webhook_is_signed_with_the_webhook_secret() {
    // An in-process receiver on loopback (allowed only by the test-only guard).
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(HeaderMap, Bytes)>();
    let receiver = Router::new().route(
        "/hook",
        post(move |headers: HeaderMap, body: Bytes| {
            let tx = tx.clone();
            async move {
                tx.send((headers, body)).unwrap();
                "ok"
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, receiver).await.unwrap() });

    let a: App = app(vec![reply("Bob done"), reply("All done")]);
    // Webhooks are registered through the operator login; they belong to "operator".
    let (cookie, csrf) = operator_login(&a.router).await;
    let key = a.key("operator");
    let (status, hook) = call_operator(
        &a.router,
        "POST",
        "/v1/webhooks",
        &cookie,
        Some(&csrf),
        Some(json!({"url": format!("http://127.0.0.1:{port}/hook")})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{hook}");
    let secret = hook["secret"].as_str().unwrap().to_string();
    let (_, list) = call(&a.router, "GET", "/v1/webhooks", Some(&key), None).await;
    assert!(
        !list.to_string().contains(&secret),
        "secrets are shown once"
    );
    // Another owner's turns never reach the operator's webhook.
    let bob_session = a.sessions.create("bob", None).unwrap();

    webhooks::spawn(
        a.sessions.clone(),
        a.state.ledger.clone(),
        UrlGuard::allow_loopback_for_tests(),
    );
    a.sessions.send("bob", &bob_session.id, "hi").unwrap();
    a.sessions
        .wait_idle("bob", &bob_session.id, WAIT)
        .await
        .unwrap();
    let session = a.sessions.create("operator", None).unwrap();
    a.sessions
        .send("operator", &session.id, "finish up")
        .unwrap();

    let (headers, body) = tokio::time::timeout(WAIT, rx.recv())
        .await
        .expect("no webhook delivery")
        .unwrap();
    let sig = headers["x-agent-signature"].to_str().unwrap();
    assert_eq!(sig, signature(&secret, &body));
    assert_eq!(
        sig,
        format!(
            "sha256={}",
            hex::encode(hmac_sha256(secret.as_bytes(), &body))
        )
    );
    assert_ne!(sig, signature("whsec_wrong", &body));
    let payload: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload["session_id"], session.id);
    assert_eq!(payload["status"], "idle");
    assert_eq!(payload["last_reply"], "All done");
    assert!(
        rx.try_recv().is_err(),
        "only the operator's turn end is delivered"
    );
}

// Models, usage, workspace archives and forks.

use agent_rush::config::{ModelCatalog, ModelPrice};
use common::{app_with_models, glm_catalog};

/// A call on full GLM 5.3 costs ten times one on Flash in these tests.
const PRICES: &[(&str, f64)] = &[("glm-5.3-flash", 0.001), ("glm-5.3", 0.01)];

fn glm_app(steps: Vec<Value>) -> App {
    app_with_models(steps, SessionLimits::default(), glm_catalog(), PRICES)
}

/// Creates a session through the API and waits for its first turn to end.
async fn run_session(a: &App, key: &str, owner: &str, body: Value) -> Value {
    let (status, created) = call(&a.router, "POST", "/v1/sessions", Some(key), Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["session"]["id"].as_str().unwrap();
    a.sessions.wait_idle(owner, id, WAIT).await.unwrap();
    created["session"].clone()
}

async fn events_of(a: &App, key: &str, id: &str) -> Vec<Value> {
    let (status, body) = call(
        &a.router,
        "GET",
        &format!("/v1/sessions/{id}/events?limit=1000"),
        Some(key),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    body["events"].as_array().unwrap().clone()
}

#[tokio::test]
async fn models_are_listed_and_every_call_of_a_session_runs_on_its_model() {
    let a = glm_app(vec![bash("make"), reply("Built on full GLM")]);
    let key = a.key("alice");
    let (status, models) = call(&a.router, "GET", "/v1/models", Some(&key), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(models["provider"], "vultr");
    assert_eq!(models["default"], "glm-5.3-flash");
    assert_eq!(models["per_session"], true);
    assert_eq!(
        models["models"],
        json!([
            {"id":"glm-5.3-flash","input_usd_per_million":0.10,"output_usd_per_million":0.35,"default":true},
            {"id":"glm-5.3","input_usd_per_million":0.75,"output_usd_per_million":3.00,"default":false},
        ])
    );
    let (_, status_body) = call(&a.router, "GET", "/v1/status", Some(&key), None).await;
    assert_eq!(status_body["models"], models);

    let full = run_session(
        &a,
        &key,
        "alice",
        json!({"message":"Build it","model":"glm-5.3"}),
    )
    .await;
    assert_eq!(full["model"], "glm-5.3");
    let flash = run_session(&a, &key, "alice", json!({"message":"Hello"})).await;
    assert_eq!(flash["model"], "glm-5.3-flash");
    // The full model ran both calls of its turn, Flash the one of the other session.
    let models_called: Vec<String> = a.model.calls().into_iter().map(|(m, _)| m).collect();
    assert_eq!(models_called, ["glm-5.3", "glm-5.3", "glm-5.3-flash"]);

    let full_id = full["id"].as_str().unwrap();
    let (_, view) = call(
        &a.router,
        "GET",
        &format!("/v1/sessions/{full_id}"),
        Some(&key),
        None,
    )
    .await;
    assert_eq!(view["model"], "glm-5.3");
    // Each model's price is what the session was charged.
    assert!(
        (view["spend_usd"].as_f64().unwrap() - 0.02).abs() < 1e-9,
        "{view}"
    );
    assert_eq!(
        (view["input_tokens"].as_i64(), view["model_calls"].as_i64()),
        (Some(2000), Some(2))
    );
    let (_, list) = call(&a.router, "GET", "/v1/sessions", Some(&key), None).await;
    let listed: Vec<&str> = list["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["model"].as_str().unwrap())
        .collect();
    assert_eq!(listed, ["glm-5.3-flash", "glm-5.3"]);
    // The reply records the model that produced it.
    let events = events_of(&a, &key, full_id).await;
    let answer = events.iter().find(|e| e["kind"] == "assistant").unwrap();
    assert_eq!(answer["payload"]["model"], "glm-5.3");
}

#[tokio::test]
async fn the_turn_spend_cap_counts_each_call_at_its_models_price() {
    let limits = SessionLimits {
        max_turn_usd: 0.015,
        ..SessionLimits::default()
    };
    let steps = vec![bash("one"), bash("two"), bash("three"), reply("Done")];
    let a = app_with_models(steps, limits, glm_catalog(), PRICES);
    let key = a.key("alice");
    // Two full GLM 5.3 calls ($0.02) pass the $0.015 turn cap: the third is refused.
    let full = run_session(&a, &key, "alice", json!({"message":"go","model":"glm-5.3"})).await;
    let view = a
        .sessions
        .get("alice", full["id"].as_str().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(view.status, SessionStatus::Failed);
    assert_eq!(view.model_calls, 2);
    // Two Flash calls ($0.002) stay well inside it.
    let flash = run_session(&a, &key, "alice", json!({"message":"go"})).await;
    let view = a
        .sessions
        .get("alice", flash["id"].as_str().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(view.status, SessionStatus::Idle);
    assert!((view.spend_usd - 0.002).abs() < 1e-9);
}

#[tokio::test]
async fn an_unknown_or_unsupported_model_is_400_and_creates_nothing() {
    let a = glm_app(vec![]);
    let key = a.key("alice");
    let (status, body) = call(
        &a.router,
        "POST",
        "/v1/sessions",
        Some(&key),
        Some(json!({"message":"hi","model":"gpt-9"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let error = body["error"].as_str().unwrap();
    assert!(
        error.contains("unknown model \"gpt-9\"") && error.contains("glm-5.3-flash, glm-5.3"),
        "{error}"
    );
    assert!(a.sessions.list("alice").unwrap().is_empty());
    assert!(a.model.calls().is_empty());

    // Without a model list only the default exists; nothing else is accepted.
    let plain = app(vec![]);
    let key = plain.key("alice");
    let (status, body) = call(
        &plain.router,
        "POST",
        "/v1/sessions",
        Some(&key),
        Some(json!({"model":"glm-5.3"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    // A provider that picks models per role cannot honour a chosen one.
    let anthropic = ModelCatalog {
        provider: "anthropic".into(),
        default: "claude-opus-5-5".into(),
        models: vec![ModelPrice {
            id: "claude-opus-5-5".into(),
            input_usd_per_million: 4.0,
            output_usd_per_million: 20.0,
        }],
        per_session: false,
    };
    let claude = app_with_models(vec![], SessionLimits::default(), anthropic, &[]);
    let key = claude.key("alice");
    let (status, body) = call(
        &claude.router,
        "POST",
        "/v1/sessions",
        Some(&key),
        Some(json!({"model":"claude-opus-5-5"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap().contains("anthropic"),
        "{body}"
    );
    let (status, created) = call(&claude.router, "POST", "/v1/sessions", Some(&key), None).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["session"]["model"], "claude-opus-5-5");
}

#[tokio::test]
async fn usage_totals_add_up_per_model_and_per_session() {
    let a = glm_app(vec![bash("make"), reply("Built")]);
    let key = a.key("alice");
    let full = run_session(
        &a,
        &key,
        "alice",
        json!({"message":"Build","model":"glm-5.3"}),
    )
    .await;
    run_session(&a, &key, "alice", json!({"message":"Hello"})).await;
    run_session(&a, &key, "alice", json!({"message":"Hello again"})).await;
    let (status, usage) = call(&a.router, "GET", "/v1/usage", Some(&key), None).await;
    assert_eq!(status, StatusCode::OK);
    let total = &usage["total"];
    assert_eq!(total["sessions"], 3);
    assert_eq!(total["model_calls"], 4);
    assert_eq!(total["input_tokens"], 4000);
    assert_eq!(total["output_tokens"], 400);
    assert!(
        (total["spend_usd"].as_f64().unwrap() - 0.022).abs() < 1e-9,
        "{usage}"
    );
    let by_model = usage["by_model"].as_array().unwrap();
    assert_eq!(by_model.len(), 2);
    assert_eq!(by_model[0]["model"], "glm-5.3");
    assert_eq!(
        (
            by_model[0]["sessions"].as_i64(),
            by_model[0]["model_calls"].as_i64()
        ),
        (Some(1), Some(2))
    );
    assert!((by_model[0]["spend_usd"].as_f64().unwrap() - 0.02).abs() < 1e-9);
    assert_eq!(by_model[1]["model"], "glm-5.3-flash");
    assert_eq!(
        (
            by_model[1]["sessions"].as_i64(),
            by_model[1]["input_tokens"].as_i64()
        ),
        (Some(2), Some(2000))
    );
    let sessions = usage["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 3);
    let row = sessions.iter().find(|s| s["id"] == full["id"]).unwrap();
    assert_eq!(row["model"], "glm-5.3");
    assert_eq!(row["output_tokens"], 200);
    // Another owner sees only their own (here, nothing).
    let bob = a.key("bob");
    let (_, theirs) = call(&a.router, "GET", "/v1/usage", Some(&bob), None).await;
    assert_eq!(theirs["total"]["sessions"], 0);
    assert_eq!(theirs["by_model"], json!([]));
}

/// The paths in a tarball, as the fake worker writes them (uncompressed).
fn tar_paths(bytes: &[u8]) -> Vec<String> {
    let mut archive = tar::Archive::new(bytes);
    archive
        .entries()
        .unwrap()
        .map(|e| e.unwrap().path().unwrap().to_string_lossy().into_owned())
        .collect()
}

#[tokio::test]
async fn the_workspace_downloads_as_a_tarball_and_over_25_mib_is_413() {
    let a = app(vec![]);
    let key = a.key("alice");
    let session = a.sessions.create("alice", None).unwrap();
    let id = session.id.clone();
    for (path, text) in [("app/main.py", "print('hi')"), ("notes.md", "# notes")] {
        a.sessions
            .write_file("alice", &id, path, Bytes::from(text))
            .await
            .unwrap();
    }
    let response = send(
        &a.router,
        Request::get(format!("/v1/sessions/{id}/archive"))
            .header("authorization", format!("Bearer {key}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "application/gzip");
    let disposition = response.headers()["content-disposition"].to_str().unwrap();
    assert!(
        disposition.starts_with("attachment; filename=\"workspace-")
            && disposition.ends_with(".tar.gz\""),
        "{disposition}"
    );
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(tar_paths(&bytes), ["app/main.py", "notes.md"]);
    let box_id = a
        .sessions
        .get("alice", &id)
        .unwrap()
        .unwrap()
        .box_id
        .unwrap();
    // The archive was built in the box's /tmp and removed after it was read.
    assert_eq!(a.boxes.files(&box_id), ["app/main.py", "notes.md"]);

    a.boxes.put_file(
        &box_id,
        "big.bin",
        vec![7; agent_rush::sandbox_api::MAX_FILE_BYTES + 1024],
    );
    let (status, body) = call(
        &a.router,
        "GET",
        &format!("/v1/sessions/{id}/archive"),
        Some(&key),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    assert!(body["error"].as_str().unwrap().contains("25 MiB"), "{body}");
    assert_eq!(
        a.boxes.files(&box_id),
        ["app/main.py", "big.bin", "notes.md"]
    );

    a.sessions.close("alice", &id).await.unwrap();
    let (status, _) = call(
        &a.router,
        "GET",
        &format!("/v1/sessions/{id}/archive"),
        Some(&key),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn a_fork_copies_files_and_the_conversation_and_runs_on_its_own_model() {
    let a = glm_app(vec![bash("make"), reply("Built it"), reply("Added tests")]);
    let key = a.key("alice");
    let source = run_session(
        &a,
        &key,
        "alice",
        json!({"title":"Benchmark","message":"Build the app"}),
    )
    .await;
    let id = source["id"].as_str().unwrap().to_string();
    a.sessions.send("alice", &id, "Add tests").unwrap();
    a.sessions.wait_idle("alice", &id, WAIT).await.unwrap();
    a.sessions
        .write_file("alice", &id, "app/main.py", Bytes::from("print('hi')"))
        .await
        .unwrap();
    let source_events = events_of(&a, &key, &id).await.len();

    let (status, forked) = call(
        &a.router,
        "POST",
        &format!("/v1/sessions/{id}/fork"),
        Some(&key),
        Some(json!({"model":"glm-5.3","message":"Run it once more"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{forked}");
    let fork = &forked["session"];
    let fork_id = fork["id"].as_str().unwrap().to_string();
    assert_eq!(fork["model"], "glm-5.3");
    assert_eq!(fork["forked_from"], id.as_str());
    assert_eq!(fork["title"], "Benchmark (fork)");
    assert_eq!(forked["sent"]["interjection"], false);
    a.sessions.wait_idle("alice", &fork_id, WAIT).await.unwrap();

    // The fork's workspace is a copy of the source's files.
    let fork_box = a
        .sessions
        .get("alice", &fork_id)
        .unwrap()
        .unwrap()
        .box_id
        .unwrap();
    assert_eq!(a.boxes.files(&fork_box), ["app/main.py"]);
    // Its log starts with the source's requests and replies, without the tool steps.
    let events = events_of(&a, &key, &fork_id).await;
    let copied: Vec<(String, String)> = events
        .iter()
        .take_while(|e| e["payload"]["forked_from"] == id.as_str())
        .map(|e| {
            (
                e["kind"].as_str().unwrap().to_string(),
                e["payload"]["text"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let pair = |k: &str, t: &str| (k.to_string(), t.to_string());
    assert_eq!(
        copied,
        [
            pair("user", "Build the app"),
            pair("assistant", "Built it"),
            pair("user", "Add tests"),
            pair("assistant", "Added tests"),
        ]
    );
    // The fork's first call ran on its model and saw the earlier conversation, but not
    // the source's tool output.
    let (model, messages) = a.model.calls().pop().unwrap();
    assert_eq!(model, "glm-5.3");
    let text = messages
        .iter()
        .map(|m| m["content"].as_str().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    for expected in [
        "Build the app",
        "Built it",
        "Add tests",
        "Added tests",
        "Run it once more",
    ] {
        assert!(text.contains(expected), "{expected} missing from {text}");
    }
    assert!(!text.contains("ran: make"), "{text}");
    // The source is unchanged.
    assert_eq!(events_of(&a, &key, &id).await.len(), source_events);

    // Without a model, a fork keeps the source's.
    let (status, again) = call(
        &a.router,
        "POST",
        &format!("/v1/sessions/{id}/fork"),
        Some(&key),
        Some(json!({"title":"Second"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{again}");
    assert_eq!(again["session"]["model"], "glm-5.3-flash");
    assert_eq!(again["session"]["title"], "Second");
    assert!(again["sent"].is_null());
}

#[tokio::test]
async fn a_fork_is_refused_cleanly_by_the_cap_an_unknown_model_or_a_large_workspace() {
    let limits = SessionLimits {
        max_open_sessions_per_owner: 2,
        ..SessionLimits::default()
    };
    let a = app_with_models(vec![], limits, glm_catalog(), PRICES);
    let key = a.key("alice");
    let source = a.sessions.create("alice", Some("source")).unwrap();
    let id = source.id.clone();
    a.sessions
        .write_file("alice", &id, "a.txt", Bytes::from("a"))
        .await
        .unwrap();
    let other = a.sessions.create("alice", None).unwrap();
    let fork_path = format!("/v1/sessions/{id}/fork");

    let (status, body) = call(&a.router, "POST", &fork_path, Some(&key), None).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(body["error"], "too many open sessions; close one");
    a.sessions.close("alice", &other.id).await.unwrap();

    let (status, body) = call(
        &a.router,
        "POST",
        &fork_path,
        Some(&key),
        Some(json!({"model":"gpt-9"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let box_id = a
        .sessions
        .get("alice", &id)
        .unwrap()
        .unwrap()
        .box_id
        .unwrap();
    a.boxes.put_file(
        &box_id,
        "big.bin",
        vec![1; agent_rush::sandbox_api::MAX_FILE_BYTES + 1],
    );
    let (status, body) = call(&a.router, "POST", &fork_path, Some(&key), None).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    // Neither refusal left a session behind.
    assert_eq!(a.sessions.list("alice").unwrap().len(), 2);

    a.sessions.close("alice", &id).await.unwrap();
    let (status, _) = call(&a.router, "POST", &fork_path, Some(&key), None).await;
    assert_eq!(status, StatusCode::CONFLICT);
}
