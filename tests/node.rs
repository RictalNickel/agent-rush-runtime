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
