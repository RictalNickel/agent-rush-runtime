use agent_rush::{
    api::{AppState, router},
    config::Settings,
    ledger::Ledger,
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

async fn call(
    app: axum::Router,
    method: &str,
    path: &str,
    body: Value,
    cookie: &str,
    csrf: &str,
) -> (StatusCode, Value, String) {
    let response = app
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .header("cookie", cookie)
                .header("x-csrf-token", csrf)
                .header("idempotency-key", "one")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let cookie = response
        .headers()
        .get("set-cookie")
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string())
        .unwrap_or_default();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        cookie,
    )
}

fn app() -> axum::Router {
    let mut settings = Settings::local();
    settings.operator_password = "test-password-long-enough".into();
    router(AppState::new(settings, Ledger::memory(1024).unwrap()).unwrap())
}

#[tokio::test]
async fn auth_csrf_job_lifecycle_and_logout() {
    let app = app();
    assert_eq!(
        call(app.clone(), "GET", "/api/jobs", Value::Null, "", "")
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/login",
            json!({"password":"wrong"}),
            "",
            ""
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, session, cookie) = call(
        app.clone(),
        "POST",
        "/api/login",
        json!({"password":"test-password-long-enough"}),
        "",
        "",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let csrf = session["csrf"].as_str().unwrap();
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/jobs",
            json!({"prompt":"Calculate revenue totals"}),
            &cookie,
            ""
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (status, job, _) = call(
        app.clone(),
        "POST",
        "/api/jobs",
        json!({"prompt":"Calculate revenue totals"}),
        &cookie,
        csrf,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        call(app.clone(), "GET", "/api/jobs", Value::Null, &cookie, "")
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        call(
            app.clone(),
            "GET",
            &format!("/api/jobs/{}/evidence", job["id"].as_str().unwrap()),
            Value::Null,
            &cookie,
            ""
        )
        .await
        .1["receipt"],
        Value::Null
    );
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/logout",
            Value::Null,
            &cookie,
            csrf
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(app.clone(), "GET", "/api/jobs", Value::Null, &cookie, "")
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn login_rate_limit_and_unconfigured_status() {
    let app = app();
    let (status, v, _) = call(app.clone(), "GET", "/api/status", Value::Null, "", "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(v["inference_configured"], false);
    assert_eq!(v["worker_connected"], false);
    // On Vultr every role shares one model but keeps its own reasoning effort.
    assert_eq!(v["inference_provider"], "vultr");
    assert_eq!(v["agent_roles"]["coder"]["reasoning_effort"], "xhigh");
    assert_eq!(
        v["agent_roles"]["orchestrator"]["reasoning_effort"],
        "medium"
    );
    assert_eq!(
        v["agent_roles"]["reviewer"]["model"],
        v["agent_roles"]["worker"]["model"]
    );
    for _ in 0..10 {
        call(
            app.clone(),
            "POST",
            "/api/login",
            json!({"password":"wrong"}),
            "",
            "",
        )
        .await;
    }
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/login",
            json!({"password":"wrong"}),
            "",
            ""
        )
        .await
        .0,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test]
async fn web_has_security_headers_and_server_rendered_workspace() {
    let response = app()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let csp = response.headers()["content-security-policy"]
        .to_str()
        .unwrap();
    assert!(csp.contains("default-src 'self'"));
    assert!(csp.contains("script-src 'self'") && csp.contains("style-src 'self'"));
    // Screenshots and spoken replies are fetched with the cookie and shown as blob: URLs.
    assert!(csp.contains("img-src 'self' data: blob:"));
    assert!(csp.contains("media-src 'self' blob:"));
    // The web chat's mic button records on this origin only.
    assert_eq!(
        response.headers()["permissions-policy"],
        "camera=(), microphone=(self), geolocation=()"
    );
    assert_eq!(response.headers()["cache-control"], "no-store");
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(
        String::from_utf8(body.to_vec())
            .unwrap()
            .contains("Give an agent its own sealed Linux workspace.")
    );
}

#[tokio::test]
async fn self_hosted_font_is_the_only_cacheable_asset() {
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/assets/archivo.woff2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "font/woff2");
    assert_eq!(response.headers()["cache-control"], "public, max-age=86400");
    assert!(response.headers().contains_key("content-security-policy"));
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&body[..4], b"wOF2");
}

#[tokio::test]
async fn evidence_and_job_lists_do_not_embed_artifact_bytes() {
    let mut settings = Settings::local();
    settings.operator_password = "test-password-long-enough".into();
    let state = AppState::new(settings, Ledger::memory(1024).unwrap()).unwrap();
    let job = state
        .ledger
        .create_job(
            "operator",
            serde_json::from_value(json!({"prompt":"Return a greeting"})).unwrap(),
            "fixture",
        )
        .unwrap();
    state.ledger.claim(&job.id).unwrap();
    state.ledger.finish(&job.id,json!({"reason":"completed","exit_code":0,"checks_passed":true,"cleanup_state":"verified","artifacts":[{"name":"hello.txt","bytes":5,"sha256":"2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824","data_base64":"aGVsbG8="}]})).unwrap();
    let app = router(state);
    let (_, session, cookie) = call(
        app.clone(),
        "POST",
        "/api/login",
        json!({"password":"test-password-long-enough"}),
        "",
        "",
    )
    .await;
    for path in [
        "/api/jobs".to_string(),
        format!("/api/jobs/{}", job.id),
        format!("/api/jobs/{}/evidence", job.id),
    ] {
        let (status, value, _) = call(
            app.clone(),
            "GET",
            &path,
            Value::Null,
            &cookie,
            session["csrf"].as_str().unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(!value.to_string().contains("data_base64"));
    }
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/jobs/{}/artifacts/hello.txt", job.id))
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        "hello"
    );
}

#[test]
fn configured_worker_trust_cannot_be_silently_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let mut settings = Settings::local();
    settings.worker_ca_path = Some(dir.path().join("missing-ca.pem"));
    assert!(AppState::new(settings, Ledger::memory(1024).unwrap()).is_err());
}

#[tokio::test]
async fn operator_memory_management_requires_auth_csrf_and_rejects_authority_fields() {
    let app = app();
    assert_eq!(
        call(
            app.clone(),
            "GET",
            "/api/memories?query=rule",
            Value::Null,
            "",
            ""
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (_, session, cookie) = call(
        app.clone(),
        "POST",
        "/api/login",
        json!({"password":"test-password-long-enough"}),
        "",
        "",
    )
    .await;
    let csrf = session["csrf"].as_str().unwrap();
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/memories",
            json!({"title":"Rule","text":"Integer cents"}),
            &cookie,
            ""
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (status, memory, _) = call(
        app.clone(),
        "POST",
        "/api/memories",
        json!({"title":"Rule","text":"Integer cents"}),
        &cookie,
        csrf,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        call(
            app.clone(),
            "GET",
            "/api/memories?query=cents",
            Value::Null,
            &cookie,
            ""
        )
        .await
        .1
        .as_array()
        .unwrap()
        .len(),
        1
    );
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/memories",
            json!({"title":"Rule","text":"note","owner":"admin"}),
            &cookie,
            csrf
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            app.clone(),
            "DELETE",
            &format!("/api/memories/{}", memory["id"].as_str().unwrap()),
            Value::Null,
            &cookie,
            csrf
        )
        .await
        .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn context_export_is_authenticated_and_admission_policy_is_explicit() {
    let app = app();
    let (_, session, cookie) = call(
        app.clone(),
        "POST",
        "/api/login",
        json!({"password":"test-password-long-enough"}),
        "",
        "",
    )
    .await;
    let csrf = session["csrf"].as_str().unwrap();
    let (_, job, _) = call(
        app.clone(),
        "POST",
        "/api/jobs",
        json!({"prompt":"Compute totals"}),
        &cookie,
        csrf,
    )
    .await;
    let path = format!("/api/jobs/{}/contexts", job["id"].as_str().unwrap());
    assert_eq!(
        call(app.clone(), "GET", &path, Value::Null, "", "").await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(app.clone(), "GET", &path, Value::Null, &cookie, "")
            .await
            .1["contexts"],
        json!([])
    );
    let status = call(app.clone(), "GET", "/api/status", Value::Null, "", "")
        .await
        .1;
    assert_eq!(status["resources"]["max_concurrent_jobs"], 2);
    assert_eq!(
        status["resources"]["controller_admission_capacity_mb"],
        1024
    );
}

#[tokio::test]
async fn a_task_larger_than_the_deployment_can_admit_is_rejected_not_queued_forever() {
    let mut settings = Settings::local();
    settings.operator_password = "test-password-long-enough".into();
    let app = router(AppState::new(settings, Ledger::memory(768).unwrap()).unwrap());
    let (_, session, cookie) = call(
        app.clone(),
        "POST",
        "/api/login",
        json!({"password":"test-password-long-enough"}),
        "",
        "",
    )
    .await;
    let csrf = session["csrf"].as_str().unwrap();
    // The UI sends no limits, so the request asks for the 1 GiB default lease.
    let (status, body, _) = call(
        app.clone(),
        "POST",
        "/api/jobs",
        json!({"prompt":"Calculate revenue totals"}),
        &cookie,
        csrf,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        body["error"].as_str().unwrap().contains("768 MiB"),
        "{body}"
    );
    let (status, _, _) = call(
        app,
        "POST",
        "/api/jobs",
        json!({"prompt":"Calculate revenue totals","memory_mb":512}),
        &cookie,
        csrf,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
}

/// A login from `peer`, optionally behind a proxy that sent `forwarded` as X-Forwarded-For.
async fn login_from(
    app: &axum::Router,
    peer: &str,
    forwarded: Option<&str>,
    password: &str,
) -> StatusCode {
    let mut request = Request::builder()
        .method("POST")
        .uri("/api/login")
        .header("content-type", "application/json")
        .extension(axum::extract::ConnectInfo(
            peer.parse::<std::net::SocketAddr>().unwrap(),
        ));
    if let Some(forwarded) = forwarded {
        request = request.header("x-forwarded-for", forwarded);
    }
    app.clone()
        .oneshot(
            request
                .body(Body::from(json!({ "password": password }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}
const RIGHT: &str = "test-password-long-enough";

#[tokio::test]
async fn one_address_failing_logins_does_not_lock_out_another() {
    use agent_rush::api::LOGIN_FAILURES_PER_ADDRESS;
    let app = app();
    for _ in 0..LOGIN_FAILURES_PER_ADDRESS {
        assert_eq!(
            login_from(&app, "203.0.113.5:4000", None, "wrong").await,
            StatusCode::UNAUTHORIZED
        );
    }
    // The per-address limit still holds, even for the right password.
    for password in ["wrong", RIGHT] {
        assert_eq!(
            login_from(&app, "203.0.113.5:4001", None, password).await,
            StatusCode::TOO_MANY_REQUESTS
        );
    }
    // Another address is unaffected.
    assert_eq!(
        login_from(&app, "203.0.113.6:4000", None, "wrong").await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login_from(&app, "203.0.113.6:4000", None, RIGHT).await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn behind_the_proxy_the_last_forwarded_address_is_limited() {
    let app = app();
    for _ in 0..10 {
        login_from(&app, "127.0.0.1:5000", Some("198.51.100.1"), "wrong").await;
    }
    assert_eq!(
        login_from(&app, "127.0.0.1:5000", Some("198.51.100.1"), "wrong").await,
        StatusCode::TOO_MANY_REQUESTS
    );
    // Entries before the last one come from the client; changing them does not help.
    assert_eq!(
        login_from(
            &app,
            "127.0.0.1:5000",
            Some("1.2.3.4, 198.51.100.1"),
            "wrong"
        )
        .await,
        StatusCode::TOO_MANY_REQUESTS
    );
    // A different client behind the same proxy is unaffected.
    assert_eq!(
        login_from(&app, "127.0.0.1:5000", Some("198.51.100.2"), "wrong").await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn a_direct_client_cannot_dodge_the_limit_with_forwarded_for() {
    let app = app();
    for i in 0..10 {
        let spoofed = format!("192.0.2.{i}");
        login_from(&app, "203.0.113.9:4000", Some(&spoofed), "wrong").await;
    }
    assert_eq!(
        login_from(&app, "203.0.113.9:4000", Some("192.0.2.200"), "wrong").await,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test]
async fn an_ipv6_client_is_limited_by_its_slash_64() {
    let app = app();
    for i in 1..=10 {
        login_from(&app, &format!("[2001:db8::{i}]:4000"), None, "wrong").await;
    }
    assert_eq!(
        login_from(&app, "[2001:db8::ffff]:4000", None, "wrong").await,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        login_from(&app, "[2001:db8:0:1::1]:4000", None, "wrong").await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn successful_logins_do_not_count_towards_the_limit() {
    let app = app();
    for _ in 0..3 {
        login_from(&app, "203.0.113.7:4000", None, "wrong").await;
    }
    for _ in 0..20 {
        assert_eq!(
            login_from(&app, "203.0.113.7:4000", None, RIGHT).await,
            StatusCode::OK
        );
    }
    for _ in 0..7 {
        assert_eq!(
            login_from(&app, "203.0.113.7:4000", None, "wrong").await,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        login_from(&app, "203.0.113.7:4000", None, "wrong").await,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test]
async fn many_addresses_together_hit_the_global_ceiling() {
    use agent_rush::api::LOGIN_FAILURES_GLOBAL;
    let app = app();
    for i in 0..LOGIN_FAILURES_GLOBAL {
        let peer = format!("10.0.{}.{}:4000", i / 250, i % 250 + 1);
        assert_eq!(
            login_from(&app, &peer, None, "wrong").await,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        login_from(&app, "10.9.9.9:4000", None, "wrong").await,
        StatusCode::TOO_MANY_REQUESTS
    );
}
