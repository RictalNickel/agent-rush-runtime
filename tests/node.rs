use agent_rush::{
    node::{NodeState, node_router},
    worker::{DockerRunner, IMAGE},
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;

#[tokio::test]
async fn private_worker_denies_missing_or_incorrect_auth_before_docker_access() {
    let dir = tempfile::tempdir().unwrap();
    let state = NodeState::new(
        DockerRunner::new(IMAGE).unwrap(),
        dir.path().into(),
        "a-long-worker-token-with-32-bytes-minimum".into(),
        1024,
    )
    .unwrap();
    for token in ["", "Bearer wrong"] {
        let response = node_router(state.clone())
            .oneshot(
                Request::builder()
                    .uri("/v1/health")
                    .header("authorization", token)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn malformed_worker_source_and_authority_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let token = "a-long-worker-token-with-32-bytes-minimum";
    let state = NodeState::new(
        DockerRunner::new(IMAGE).unwrap(),
        dir.path().into(),
        token.into(),
        1024,
    )
    .unwrap();
    let response = node_router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/runs")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"id":"../escape","code":"fn main(){}","limits":{},"privileged":true}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn uncertain_creation_cannot_be_verified_by_cancel_inventory_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let token = "a-long-worker-token-with-32-bytes-minimum";
    std::fs::write(dir.path().join("uncertain.json"),r#"{"hash":"fixture","state":"finished","receipt":{"creation_uncertain":true,"cleanup_state":"failed"},"updated_at":"2026-11-04T00:00:00Z"}"#).unwrap();
    let state = NodeState::new(
        DockerRunner::new(IMAGE).unwrap(),
        dir.path().into(),
        token.into(),
        1024,
    )
    .unwrap();
    let response = node_router(state)
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/v1/runs/uncertain")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    use http_body_util::BodyExt;
    let value: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(value["cleanup_state"], "failed");
}

#[tokio::test]
async fn draining_worker_rejects_new_admission_before_docker_access() {
    let dir = tempfile::tempdir().unwrap();
    let token = "a-long-worker-token-with-32-bytes-minimum";
    let state = NodeState::new(
        DockerRunner::new(IMAGE).unwrap(),
        dir.path().into(),
        token.into(),
        1024,
    )
    .unwrap();
    state.begin_drain();
    let response = node_router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/runs")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"id":"during-drain","code":"fn main(){}","limits":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn expired_qualification_rejects_new_work_without_destroying_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let token = "a-long-worker-token-with-32-bytes-minimum";
    let state = NodeState::new(
        DockerRunner::new(IMAGE).unwrap(),
        dir.path().into(),
        token.into(),
        1024,
    )
    .unwrap()
    .with_qualification_expiry(chrono::Utc::now().timestamp() - 1);
    let response = node_router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/runs")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"id":"expired-report","code":"fn main(){}","limits":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}
