use agent_rush::{
    boxes::{BoxConfig, BoxError, docker_create_args, error_status, resolve_path, truncate_utf8},
    node::{NodeState, node_router},
    sandbox_api::BoxSpec,
    worker::{DockerRunner, IMAGE},
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;

#[test]
fn paths_stay_under_workspace_or_tmp() {
    assert_eq!(resolve_path("src/a.rs").unwrap(), "/workspace/src/a.rs");
    assert_eq!(resolve_path("").unwrap(), "/workspace");
    assert_eq!(resolve_path("./src/../a.rs").unwrap(), "/workspace/a.rs");
    assert_eq!(resolve_path("/workspace//x/").unwrap(), "/workspace/x");
    assert_eq!(resolve_path("/tmp/x").unwrap(), "/tmp/x");
    for bad in [
        "../etc",
        "../../etc/passwd",
        "/etc/passwd",
        "/workspace/../root",
        "/workspacex/a",
        "/tmpfoo",
        "/",
        "a\0b",
        "/../../..",
    ] {
        let error = resolve_path(bad).unwrap_err();
        assert!(
            matches!(error.downcast_ref::<BoxError>(), Some(BoxError::Invalid(_))),
            "{bad:?} was not refused as invalid"
        );
    }
}

#[test]
fn output_is_lossy_utf8_within_the_cap() {
    assert_eq!(truncate_utf8(b"hello", 10), ("hello".into(), false));
    assert_eq!(truncate_utf8(b"hello", 5), ("hello".into(), false));
    assert_eq!(truncate_utf8(b"hello", 3), ("hel".into(), true));
    // A multi-byte character is never split.
    let (text, cut) = truncate_utf8("aé".as_bytes(), 2);
    assert_eq!((text.as_str(), cut), ("a", true));
    // Binary (a PNG header and random bytes) becomes replacement characters, within cap.
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend((0..200_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8));
    for max in [0, 1, 2, 3, 4, 1000, 65536] {
        let (text, cut) = truncate_utf8(&png, max);
        assert!(text.len() <= max, "{} > {max}", text.len());
        assert!(cut);
    }
    let (text, cut) = truncate_utf8(&png, 10 * 1024 * 1024);
    assert!(!cut && text.contains('\u{FFFD}') && text.starts_with('\u{FFFD}'));
    // 10 MB of logs is cut to the cap.
    let logs = "log line\n".repeat(1_200_000);
    let (text, cut) = truncate_utf8(logs.as_bytes(), 256 * 1024);
    assert!(cut && text.len() == 256 * 1024);
}

#[test]
fn box_container_is_locked_down() {
    let spec = BoxSpec {
        session_id: "s1".into(),
        owner: "owner".into(),
        memory_mb: 1024,
        cpu_millis: 1000,
    };
    let id = "0123456789abcdef0123456789abcdef";
    let args = docker_create_args(&spec, "img@sha256:abc", "ar-box", id);
    let joined = args.join(" ");
    for required in [
        "--runtime=runsc",
        "--read-only",
        "--cap-drop=ALL",
        "--security-opt=no-new-privileges",
        "--pids-limit=512",
        "--user=1000:1000",
        "--network=ar-box",
        "--memory=1024m",
        "--memory-swap=1024m",
        "--cpus=1.000",
        "-v arbox-0123456789abcdef0123456789abcdef:/workspace",
        "--tmpfs=/tmp:",
        "--tmpfs=/home/agent:",
        "--log-driver=none",
    ] {
        assert!(joined.contains(required), "missing {required}");
    }
    for forbidden in [
        "--privileged",
        "docker.sock",
        "--env-file",
        "--network=host",
        "--cap-add",
        "API_KEY",
        "TOKEN",
    ] {
        assert!(!joined.contains(forbidden), "unexpected {forbidden}");
    }
    assert_eq!(
        &args[args.len() - 3..],
        ["img@sha256:abc", "sleep", "infinity"]
    );
}

#[test]
fn a_missing_path_is_not_a_missing_box() {
    let gone: anyhow::Error = BoxError::NotFound.into();
    let path: anyhow::Error = BoxError::NoSuchPath("no such file /workspace/x".into()).into();
    assert_eq!(error_status(&gone).0, 404);
    assert_eq!(error_status(&gone).1, "box_not_found");
    assert_eq!(error_status(&path).1, "no_such_path");
    for e in [
        BoxError::NotFound,
        BoxError::NoSuchPath("p".into()),
        BoxError::Invalid("i".into()),
        BoxError::TooLarge("t".into()),
        BoxError::Capacity,
        BoxError::Unavailable("u".into()),
    ] {
        let back = BoxError::from_code(e.code(), e.to_string()).unwrap();
        assert_eq!(back.code(), e.code());
    }
    let internal = error_status(&anyhow::anyhow!("docker said /secret/path"));
    assert_eq!(internal.0, 500);
    assert!(!internal.2.contains("secret"));
}

fn node(dir: &std::path::Path, boxes: bool) -> NodeState {
    let state = NodeState::new(
        DockerRunner::new(IMAGE).unwrap(),
        dir.join("node"),
        "a-long-worker-token-with-32-bytes-minimum".into(),
        4096,
    )
    .unwrap();
    if !boxes {
        return state;
    }
    state
        .with_boxes(BoxConfig {
            image: format!("ws@sha256:{}", "a".repeat(64)),
            network: "ar-box".into(),
            report: dir.join("missing-report.json"),
            state_dir: dir.join("boxes"),
            idle_seconds: 1800,
            ceiling_mb: 4096,
        })
        .unwrap()
}

#[tokio::test]
async fn box_routes_require_the_worker_bearer_before_reading_a_body() {
    let dir = tempfile::tempdir().unwrap();
    for (method, uri) in [
        ("POST", "/v1/boxes"),
        ("GET", "/v1/boxes"),
        ("POST", "/v1/boxes/0123456789abcdef0123456789abcdef/exec"),
        (
            "GET",
            "/v1/boxes/0123456789abcdef0123456789abcdef/files?path=a",
        ),
        (
            "PUT",
            "/v1/boxes/0123456789abcdef0123456789abcdef/files?path=a",
        ),
        ("DELETE", "/v1/boxes/0123456789abcdef0123456789abcdef"),
    ] {
        for token in ["", "Bearer wrong"] {
            let response = node_router(node(dir.path(), true))
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .header("authorization", token)
                        .body(Body::from(vec![b'x'; 1024 * 1024]))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::UNAUTHORIZED,
                "{method} {uri}"
            );
        }
    }
}

#[tokio::test]
async fn boxes_are_refused_without_configuration_or_a_passing_report() {
    let dir = tempfile::tempdir().unwrap();
    let request = || {
        Request::builder()
            .method("POST")
            .uri("/v1/boxes")
            .header(
                "authorization",
                "Bearer a-long-worker-token-with-32-bytes-minimum",
            )
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"session_id":"s1","owner":"o","memory_mb":1024,"cpu_millis":1000}"#,
            ))
            .unwrap()
    };
    // Not enabled at all.
    let response = node_router(node(dir.path(), false))
        .oneshot(request())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    // Enabled, but no box qualification report: refused before any Docker call.
    let response = node_router(node(dir.path(), true))
        .oneshot(request())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    use http_body_util::BodyExt;
    let body: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["code"], "unavailable");
    assert!(body["error"].as_str().unwrap().contains("qualify-box"));
}

/// End to end on a real worker host, over HTTP with `HttpBoxClient`:
///   AR_TEST_BOX_IMAGE=<pinned image> AR_TEST_BOX_REPORT=<passing report> \
///   cargo test --test boxes live_box -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn live_box_round_trip() {
    use agent_rush::{
        box_client::HttpBoxClient,
        sandbox_api::{BoxClient, ExecRequest, FileRead},
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter("agent_rush=info")
        .with_ansi(false)
        .try_init();
    let image = std::env::var("AR_TEST_BOX_IMAGE").expect("AR_TEST_BOX_IMAGE");
    let report = std::env::var("AR_TEST_BOX_REPORT").expect("AR_TEST_BOX_REPORT");
    let dir = tempfile::tempdir().unwrap();
    let token = "a-long-worker-token-with-32-bytes-minimum";
    let state = NodeState::new(
        DockerRunner::new(IMAGE).unwrap(),
        dir.path().join("node"),
        token.into(),
        6144,
    )
    .unwrap()
    .with_boxes(BoxConfig {
        image,
        network: "ar-box".into(),
        report: report.into(),
        state_dir: dir.path().join("boxes"),
        idle_seconds: 1800,
        ceiling_mb: 4096,
    })
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = node_router(state);
    tokio::spawn(async move { axum::serve(listener, router).await });
    let client = HttpBoxClient::new(reqwest::Client::new(), base.clone(), token.into());
    let log = |label: &str, value: String| println!("## {label}\n{value}\n");
    log("worker", format!("in-process node router at {base}"));

    let spec = BoxSpec {
        session_id: "live-smoke".into(),
        owner: "smoke".into(),
        memory_mb: 1024,
        cpu_millis: 1000,
    };
    let started = std::time::Instant::now();
    let id = client.create(&spec).await.unwrap();
    log("create", format!("box_id={id} in {:?}", started.elapsed()));
    let exec = |command: &str| ExecRequest {
        command: command.into(),
        ..Default::default()
    };
    let r = client
        .exec(&id, &exec("id; pwd; echo $HOME; touch /etc/x 2>&1; cat /proc/self/status | grep -E 'NoNewPrivs|CapEff'"))
        .await
        .unwrap();
    log("exec identity", format!("{r:#?}"));
    assert_eq!(r.exit_code, 0);
    assert!(r.stdout.contains("uid=1000") && r.stdout.contains("/workspace"));

    let w = client
        .write_file(&id, "src/hello.py", b"print('hello from the box')\n")
        .await
        .unwrap();
    log("write src/hello.py", format!("{w:?}"));
    let r = client
        .exec(&id, &exec("python3 src/hello.py"))
        .await
        .unwrap();
    assert_eq!(r.stdout, "hello from the box\n");
    let FileRead::File(bytes) = client
        .read_file(&id, "/workspace/src/hello.py")
        .await
        .unwrap()
    else {
        panic!("expected a file")
    };
    assert_eq!(bytes, b"print('hello from the box')\n");
    let FileRead::Dir(entries) = client.read_file(&id, "src").await.unwrap() else {
        panic!("expected a directory")
    };
    log("list src", format!("{entries:?}"));
    assert!(entries.iter().any(|e| e.name == "hello.py" && !e.is_dir));

    // Review Focus 1: escapes by `..` and by symlinks are refused, never served.
    for path in ["../../etc/passwd", "/etc/passwd", "/workspace/../root"] {
        let e = client.read_file(&id, path).await.unwrap_err();
        log(&format!("read {path}"), format!("refused: {e:#}"));
        assert!(matches!(
            e.downcast_ref::<BoxError>(),
            Some(BoxError::Invalid(_))
        ));
    }
    client
        .exec(
            &id,
            &exec("ln -s / root-link && ln -s /etc/passwd pw && mkdir -p d && ln -s /etc d/up"),
        )
        .await
        .unwrap();
    for path in ["root-link/etc/passwd", "pw", "d/up/hostname", "root-link"] {
        let e = client.read_file(&id, path).await.unwrap_err();
        log(&format!("read symlink {path}"), format!("refused: {e:#}"));
        assert!(matches!(
            e.downcast_ref::<BoxError>(),
            Some(BoxError::Invalid(_))
        ));
    }
    let e = client
        .write_file(&id, "root-link/etc/evil", b"x")
        .await
        .map(|_| ())
        .unwrap_err();
    log("write through symlink to /", format!("refused: {e:#}"));
    let e = client.read_file(&id, "missing.txt").await.unwrap_err();
    assert!(matches!(
        e.downcast_ref::<BoxError>(),
        Some(BoxError::NoSuchPath(_))
    ));

    // Review Focus 4: large and binary output is truncated, lossy, never a failure.
    let r = client
        .exec(
            &id,
            &exec("head -c 10485760 /dev/zero | tr '\\0' 'x'; echo done >&2"),
        )
        .await
        .unwrap();
    log(
        "10 MB of stdout",
        format!(
            "stdout {} bytes, truncated={}, stderr={:?}",
            r.stdout.len(),
            r.truncated,
            r.stderr
        ),
    );
    assert!(r.truncated && r.stdout.len() == 256 * 1024 && r.stderr == "done\n");
    client
        .exec(&id, &exec("browse '{\"action\":\"goto\",\"url\":\"https://example.com\"}' >/dev/null; browse '{\"action\":\"screenshot\",\"path\":\"/workspace/shot.png\"}'"))
        .await
        .unwrap();
    let r = client.exec(&id, &exec("cat shot.png")).await.unwrap();
    log(
        "cat of a PNG",
        format!(
            "exit {} stdout {} bytes (lossy), truncated={}",
            r.exit_code,
            r.stdout.len(),
            r.truncated
        ),
    );
    assert!(r.stdout.contains('\u{FFFD}'));
    let FileRead::File(png) = client.read_file(&id, "shot.png").await.unwrap() else {
        panic!("expected a file")
    };
    assert!(png.starts_with(b"\x89PNG"));
    log(
        "read shot.png",
        format!("{} bytes, PNG signature intact", png.len()),
    );

    // Timeout, stdin, background servers that outlive the call.
    let started = std::time::Instant::now();
    let r = client
        .exec(
            &id,
            &ExecRequest {
                command: "sleep 30".into(),
                timeout_seconds: Some(2),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    log(
        "sleep 30 with a 2 s timeout",
        format!("{r:?} in {:?}", started.elapsed()),
    );
    assert!(r.timed_out && started.elapsed().as_secs() < 15);
    let r = client
        .exec(
            &id,
            &ExecRequest {
                command: "wc -c".into(),
                stdin: Some("12345".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(r.stdout.trim(), "5");
    let started = std::time::Instant::now();
    let r = client
        .exec(&id, &exec("python3 -m http.server 8000 & sleep 1; curl -s -o /dev/null -w '%{http_code}' localhost:8000"))
        .await
        .unwrap();
    log(
        "background server",
        format!("{r:?} in {:?}", started.elapsed()),
    );
    assert_eq!(r.stdout, "200");
    let r = client
        .exec(
            &id,
            &exec("curl -s -o /dev/null -w '%{http_code}' localhost:8000"),
        )
        .await
        .unwrap();
    assert_eq!(r.stdout, "200", "the server keeps running between calls");

    let r = client
        .exec(
            &id,
            &exec("curl -sS -m 5 http://169.254.169.254/; echo exit=$?"),
        )
        .await
        .unwrap();
    log("metadata from the box", format!("{}{}", r.stdout, r.stderr));

    // Live scaling: the box starts at 1024 MiB; holding 1200 MiB, touched gradually,
    // only works if the scaler raises the limit while the command runs.
    let r = client
        .exec(
            &id,
            &exec("python3 -c \"import time\nchunks=[]\nfor i in range(24):\n    chunks.append(b'x' * (50 * 1024 * 1024))\n    time.sleep(0.4)\nprint('held', len(chunks) * 50, 'MiB')\""),
        )
        .await
        .unwrap();
    log(
        "live scaling: hold 1200 MiB in a box started at 1024 MiB",
        format!("{r:?}"),
    );
    assert_eq!(r.stdout.trim(), "held 1200 MiB");
    let list = client.list().await.unwrap();
    log("list", format!("{list:?}"));
    assert!(
        list.iter()
            .any(|b| b.box_id == id && b.session_id == "live-smoke")
    );

    client.destroy(&id).await.unwrap();
    log("destroy", "container and volume removed".into());
    assert!(client.list().await.unwrap().is_empty());
    let e = client.exec(&id, &exec("true")).await.unwrap_err();
    assert!(matches!(
        e.downcast_ref::<BoxError>(),
        Some(BoxError::NotFound)
    ));
    log("exec after destroy", format!("{e:#}"));

    // A box lost outside the manager (an operator `docker rm -f`) leaves `list` at once.
    let lost = client.create(&spec).await.unwrap();
    let removed = std::process::Command::new("docker")
        .args(["rm", "-f", &format!("arbox-{lost}")])
        .output()
        .unwrap();
    assert!(removed.status.success());
    let list = client.list().await.unwrap();
    log(
        "list after the container was removed behind the manager's back",
        format!("{list:?}"),
    );
    assert!(list.is_empty());
    let e = client.exec(&lost, &exec("true")).await.unwrap_err();
    assert!(matches!(
        e.downcast_ref::<BoxError>(),
        Some(BoxError::NotFound)
    ));
    let volume = std::process::Command::new("docker")
        .args(["volume", "inspect", &format!("arbox-{lost}")])
        .output()
        .unwrap();
    assert!(
        !volume.status.success(),
        "the lost box's volume is removed too"
    );
    log("exec on the lost box", format!("{e:#}; its volume is gone"));

    // A stopped box (as after an out-of-memory kill) is restarted with its workspace.
    let stopped = client.create(&spec).await.unwrap();
    client
        .write_file(&stopped, "keep.txt", b"still here")
        .await
        .unwrap();
    let killed = std::process::Command::new("docker")
        .args(["kill", &format!("arbox-{stopped}")])
        .output()
        .unwrap();
    assert!(killed.status.success());
    let r = client.exec(&stopped, &exec("cat keep.txt")).await.unwrap();
    log("exec on a killed box", format!("{r:?}"));
    assert_eq!(r.stdout, "still here");
    client.destroy(&stopped).await.unwrap();
}
