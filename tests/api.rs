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
    assert!(
        response
            .headers()
            .get("content-security-policy")
            .unwrap()
            .to_str()
            .unwrap()
            .contains("default-src 'self'")
    );
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(
        String::from_utf8(body.to_vec())
            .unwrap()
            .contains("Give every agent a world.")
    );
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
