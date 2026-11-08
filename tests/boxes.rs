use agent_rush::{
    api_v1::workspace_error,
    boxes::{
        BoxConfig, BoxError, docker_create_args, error_status, exec_invocation, killed_shell_exit,
        resolve_path, truncate_utf8,
    },
    node::{NodeState, node_router},
    sandbox_api::BoxSpec,
    worker::{DockerRunner, IMAGE},
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;

/// A box that died at its limits, or one already running its maximum calls, is
/// worth retrying: the API says 503 and 429, not 502 as for a broken worker.
#[test]
fn a_crashed_or_busy_box_answers_with_a_retryable_status() {
    let status = |e: BoxError| workspace_error(anyhow::Error::new(e)).0;
    assert_eq!(
        status(BoxError::Unavailable("the box stopped unexpectedly".into())),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        status(BoxError::Busy("too many calls".into())),
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        status(BoxError::NoSuchPath("x".into())),
        StatusCode::NOT_FOUND
    );
    assert_eq!(status(BoxError::NotFound), StatusCode::BAD_GATEWAY);
}

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
        // Executable (compiled packages under $HOME load), but never setuid or devices.
        "--tmpfs=/tmp:rw,exec,nosuid,nodev,size=1024m,",
        "--tmpfs=/home/agent:rw,exec,nosuid,nodev,size=1024m,",
        "--log-driver=none",
    ] {
        assert!(joined.contains(required), "missing {required}");
    }
    for forbidden in [
        "noexec",
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
fn a_shell_killed_by_a_signal_is_an_exit_code_not_an_error() {
    // `docker exec` exits 128+N when the wrapper shell died of signal N.
    assert_eq!(
        killed_shell_exit(Some(143)),
        Some((
            143,
            "[the shell was terminated by signal 15 (SIGTERM)]".into()
        ))
    );
    assert_eq!(
        killed_shell_exit(Some(137)),
        Some((
            137,
            "[the shell was terminated by signal 9 (SIGKILL)]".into()
        ))
    );
    assert_eq!(
        killed_shell_exit(Some(129)),
        Some((
            129,
            "[the shell was terminated by signal 1 (SIGHUP)]".into()
        ))
    );
    assert_eq!(
        killed_shell_exit(Some(162)),
        Some((162, "[the shell was terminated by signal 34]".into()))
    );
    // Docker's own failures (125 daemon, 126 cannot run, 127 not found), a clean exit, no
    // status at all, and anything past the signal range are real exec failures.
    for code in [
        None,
        Some(0),
        Some(1),
        Some(125),
        Some(126),
        Some(127),
        Some(128),
    ] {
        assert_eq!(killed_shell_exit(code), None, "{code:?}");
    }
    for code in [Some(193), Some(255), Some(-1)] {
        assert_eq!(killed_shell_exit(code), None, "{code:?}");
    }
}

/// The exec wrapper, run on this host instead of in a box: the command reaches it on stdin,
/// ahead of the caller's stdin, and never shows in a process's arguments, so `pkill -f`
/// on text from the command cannot kill the shells running it.
#[test]
fn exec_wrapper_keeps_the_command_out_of_process_arguments() {
    use std::io::Write;
    let run_wrapper = |command: &str, stdin: Option<&str>| {
        let run = uuid::Uuid::new_v4().simple().to_string();
        let (args, input) = exec_invocation(command, stdin, 30, &run);
        let mut child = std::process::Command::new(&args[0])
            .args(&args[1..])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&input).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(
            !std::path::Path::new(&format!("/tmp/.arexec.{run}")).exists(),
            "the wrapper cleans up after itself"
        );
        (out.status.code(), String::from_utf8(out.stdout).unwrap())
    };
    // `$0`, arguments and error lines read as under `bash -lc`.
    let (code, out) = run_wrapper("echo $0 $#; no-such-command-xyz; exit 3", None);
    assert_eq!(code, Some(0));
    assert!(out.starts_with("AREXEC 3 0 7 "), "{out}");
    assert!(
        out.contains("bash 0\nbash: line 1: no-such-command-xyz"),
        "{out}"
    );
    // The caller's stdin follows the command.
    let (_, out) = run_wrapper("wc -c; echo done", Some("12345"));
    assert_eq!(out, "AREXEC 0 0 7 0\n5\ndone\n");
    // A pattern taken from the command matches nothing: before, it killed the wrapper.
    if std::process::Command::new("pkill")
        .arg("--version")
        .output()
        .is_ok()
    {
        let marker = format!("ar-marker-{}", uuid::Uuid::new_v4().simple());
        let (code, out) = run_wrapper(
            &format!("echo before; pkill -f '{marker}'; echo after=$?"),
            None,
        );
        assert_eq!(code, Some(0), "{out}");
        assert_eq!(out, "AREXEC 0 0 15 0\nbefore\nafter=1\n");
    }
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
        BoxError::Busy("b".into()),
        BoxError::Unavailable("u".into()),
    ] {
        let back = BoxError::from_code(e.code(), e.to_string()).unwrap();
        assert_eq!(back.code(), e.code());
    }
    let internal = error_status(&anyhow::anyhow!("docker said /secret/path"));
    assert_eq!(internal.0, 500);
    assert!(!internal.2.contains("secret"));
}

#[test]
fn idle_boxes_are_parked_and_old_workspaces_purged_but_busy_ones_kept() {
    use agent_rush::boxes::{Reap, reap_decision};
    let (idle, retention) = (1800, 7 * 86_400);
    // Running: parked once idle, never while a call is in flight.
    assert_eq!(reap_decision(true, 0, 0, 1799, idle, retention), Reap::Keep);
    assert_eq!(reap_decision(true, 0, 0, 1800, idle, retention), Reap::Park);
    assert_eq!(
        reap_decision(true, 1, 0, 99_999_999, idle, retention),
        Reap::Keep
    );
    // A running box is never purged directly, however old: it is parked first.
    assert_eq!(
        reap_decision(true, 0, 0, retention as i64 * 2, idle, retention),
        Reap::Park
    );
    // Parked: the workspace is kept until retention runs out, then purged.
    assert_eq!(
        reap_decision(false, 0, 0, 1800, idle, retention),
        Reap::Keep
    );
    assert_eq!(
        reap_decision(false, 0, 0, retention as i64 - 1, idle, retention),
        Reap::Keep
    );
    assert_eq!(
        reap_decision(false, 0, 0, retention as i64, idle, retention),
        Reap::Purge
    );
    assert_eq!(
        reap_decision(false, 2, 0, retention as i64, idle, retention),
        Reap::Keep
    );
    // A clock behind last use never reaps.
    assert_eq!(reap_decision(true, 0, 100, 0, 0, 0), Reap::Keep);
    assert_eq!(reap_decision(false, 0, 100, 0, 0, 0), Reap::Keep);
}

#[test]
fn box_creation_needs_twice_the_quota_plus_2_gib_after_existing_workspaces_grow() {
    use agent_rush::boxes::disk_space_ok;
    const GIB: u64 = 1 << 30;
    // 4 GiB quota: 10 GiB free is the threshold.
    assert!(disk_space_ok(10 * GIB, 0, 4096));
    assert!(!disk_space_ok(10 * GIB - 1, 0, 4096));
    // Retained workspaces' unallocated headroom counts against free space.
    assert!(!disk_space_ok(20 * GIB, 11 * GIB, 4096));
    assert!(disk_space_ok(20 * GIB, 10 * GIB, 4096));
    assert!(!disk_space_ok(GIB, 5 * GIB, 256));
}

#[test]
fn host_tool_output_is_parsed_strictly() {
    use agent_rush::boxes::{parse_df_avail, parse_losetup};
    assert_eq!(
        parse_df_avail("    Avail\n99631116288\n"),
        Some(99_631_116_288)
    );
    assert_eq!(parse_df_avail("Avail\n"), None);
    assert_eq!(parse_df_avail("Avail\n-\n"), None);
    assert_eq!(
        parse_losetup(
            "/dev/loop3: []: (/var/lib/agent-rush/worker/boxes/disks/a.img)\n/dev/loop12: [64770]:131 (/x/a.img)\n"
        ),
        ["/dev/loop3", "/dev/loop12"]
    );
    assert!(parse_losetup("").is_empty());
    for hostile in ["/dev/sda: x", "/dev/loop: x", "/dev/loop1x: x", "loop1: x"] {
        assert!(parse_losetup(hostile).is_empty(), "{hostile}");
    }
}

#[test]
fn workspace_disk_is_ext4_owned_by_the_box_user_and_mounted_by_docker() {
    use agent_rush::boxes::{mkfs_args, volume_create_args};
    let args = mkfs_args(std::path::Path::new("/d/abc.img")).join(" ");
    assert!(args.contains("root_owner=1000:1000") && args.contains("-m 0"));
    assert!(args.ends_with("/d/abc.img"));
    let id = "0123456789abcdef0123456789abcdef";
    let args = volume_create_args(id, "/dev/loop7", "ar-box")
        .unwrap()
        .join(" ");
    for required in [
        "--opt=type=ext4",
        "--opt=device=/dev/loop7",
        "nodev,nosuid",
        "--label=agent-rush.box=true",
    ] {
        assert!(args.contains(required), "missing {required}");
    }
    assert!(args.ends_with(&format!("arbox-{id}")));
    for bad in ["/dev/sda", "/dev/loop", "/dev/loop1 --opt=o=bind", "/etc"] {
        assert!(volume_create_args(id, bad, "ar-box").is_err(), "{bad}");
    }
}

#[test]
fn boxes_start_only_with_the_network_guard_installed_this_boot() {
    use agent_rush::boxes::guard_marker_valid;
    let good =
        "boot_id=b-1\nnetwork=ar-box\nsubnet=172.30.0.0/24\nguard_host_input=1\negress_rules=21\n";
    assert!(guard_marker_valid(good, "b-1\n", "ar-box").is_ok());
    // A reboot voids it: the rules are gone until the unit reinstalls them.
    assert!(guard_marker_valid(good, "b-2", "ar-box").is_err());
    assert!(guard_marker_valid(good, "b-1", "ar-box-test").is_err());
    assert!(
        guard_marker_valid(
            &good.replace("guard_host_input=1", "guard_host_input=0"),
            "b-1",
            "ar-box"
        )
        .is_err()
    );
    assert!(guard_marker_valid("", "b-1", "ar-box").is_err());
    assert!(
        guard_marker_valid("boot_id=\nnetwork=ar-box\nguard_host_input=1", "", "ar-box").is_err()
    );
}

#[test]
fn a_new_box_must_find_host_and_metadata_refused() {
    use agent_rush::boxes::guard_probe_ok;
    let targets = vec![
        "172.30.0.1:22".to_string(),
        "169.254.169.254:80".to_string(),
    ];
    assert!(
        guard_probe_ok(
            "169.254.169.254:80 rejected\n172.30.0.1:22 rejected\n",
            &targets
        )
        .is_ok()
    );
    // SSH answering (no guard), a closed port answering RST (no guard), a silent drop, or a
    // missing line all refuse the box.
    for out in [
        "172.30.0.1:22 open\n169.254.169.254:80 rejected\n",
        "172.30.0.1:22 refused\n169.254.169.254:80 rejected\n",
        "172.30.0.1:22 rejected\n169.254.169.254:80 timeout\n",
        "172.30.0.1:22 rejected\n",
        "",
    ] {
        assert!(guard_probe_ok(out, &targets).is_err(), "{out:?}");
    }
}

#[test]
fn a_manager_recovers_only_boxes_on_its_own_network() {
    use agent_rush::boxes::{container_owned, volume_stray};
    // Labelled boxes: the label decides, whatever the network mode says.
    assert!(container_owned(Some("ar-box"), "ar-box", "ar-box"));
    assert!(!container_owned(Some("ar-box-test"), "ar-box", "ar-box"));
    assert!(!container_owned(Some("ar-box"), "ar-box", "ar-box-test"));
    // Boxes from before the label: the network they are attached to decides.
    assert!(container_owned(None, "ar-box", "ar-box"));
    assert!(!container_owned(None, "ar-box", "ar-box-test"));
    // Volumes: another network's are never stray; unlabelled ones only after the grace
    // period, so a volume whose box is being created right now is left alone.
    assert!(volume_stray(Some("ar-box"), "ar-box", 0));
    assert!(!volume_stray(Some("ar-box"), "ar-box-test", 99_999));
    assert!(!volume_stray(None, "ar-box", 60));
    assert!(volume_stray(None, "ar-box", 900));
}

#[test]
fn box_disk_and_retention_settings_are_validated() {
    let dir = tempfile::tempdir().unwrap();
    let config = |disk_mb: u32, idle_seconds: u64, retention_seconds: u64| BoxConfig {
        image: format!("ws@sha256:{}", "a".repeat(64)),
        network: "ar-box".into(),
        report: dir.path().join("r.json"),
        state_dir: dir.path().join("boxes"),
        idle_seconds,
        ceiling_mb: 4096,
        disk_mb,
        retention_seconds,
        guard_marker: dir.path().join("m.env"),
        max_boxes: agent_rush::boxes::MAX_BOXES,
    };
    let manager =
        |c| agent_rush::boxes::BoxManager::new(DockerRunner::new(IMAGE).unwrap(), c, 4096);
    assert!(manager(config(4096, 1800, 604_800)).is_ok());
    assert!(manager(config(255, 1800, 604_800)).is_err());
    assert!(manager(config(262_145, 1800, 604_800)).is_err());
    // A workspace is kept at least as long as its box may sit idle before parking.
    assert!(manager(config(4096, 1800, 60)).is_err());
    // Workspaces live in a private directory under the state dir.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.path().join("boxes/disks"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
    }
}

#[test]
fn box_count_limit_is_validated() {
    let dir = tempfile::tempdir().unwrap();
    let config = |max_boxes: usize| BoxConfig {
        image: format!("ws@sha256:{}", "a".repeat(64)),
        network: "ar-box".into(),
        report: dir.path().join("r.json"),
        state_dir: dir.path().join("boxes"),
        idle_seconds: 1800,
        ceiling_mb: 4096,
        disk_mb: 4096,
        retention_seconds: 604_800,
        guard_marker: dir.path().join("m.env"),
        max_boxes,
    };
    let manager =
        |c| agent_rush::boxes::BoxManager::new(DockerRunner::new(IMAGE).unwrap(), c, 4096);
    for ok in [1, 8, 64, agent_rush::boxes::MAX_BOXES_LIMIT] {
        assert!(manager(config(ok)).is_ok(), "{ok}");
    }
    for bad in [0, agent_rush::boxes::MAX_BOXES_LIMIT + 1] {
        assert!(manager(config(bad)).is_err(), "{bad}");
    }
}

#[test]
fn busy_boxes_answer_429() {
    let busy: anyhow::Error =
        BoxError::Busy("this box is running its maximum of calls".into()).into();
    let (status, code, message) = error_status(&busy);
    assert_eq!((status, code), (429, "busy"));
    assert!(message.contains("maximum"));
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
            disk_mb: agent_rush::boxes::DEFAULT_DISK_MB,
            retention_seconds: agent_rush::boxes::DEFAULT_RETENTION_SECONDS,
            guard_marker: dir.join("box-network.env"),
            max_boxes: agent_rush::boxes::MAX_BOXES,
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
        // A scratch network and marker can stand in for the live ones.
        network: std::env::var("AR_TEST_BOX_NETWORK").unwrap_or_else(|_| "ar-box".into()),
        report: report.into(),
        state_dir: dir.path().join("boxes"),
        idle_seconds: 1800,
        ceiling_mb: 4096,
        disk_mb: 1024,
        retention_seconds: agent_rush::boxes::DEFAULT_RETENTION_SECONDS,
        guard_marker: std::env::var("AR_TEST_BOX_GUARD_MARKER")
            .unwrap_or_else(|_| agent_rush::boxes::DEFAULT_GUARD_MARKER.into())
            .into(),
        max_boxes: agent_rush::boxes::MAX_BOXES,
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

    // Escapes by `..` and by symlinks are refused, never served.
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

    // Large and binary output is truncated, lossy, never a failure.
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

    // Restarting a server with `pkill -f` on its own command line: the pattern used to match
    // the shells running the command too, and the call failed with HTTP 500.
    let r = client
        .exec(&id, &exec("pkill -f 'python3 -m http.server 8000'; sleep 1; (nohup python3 -m http.server 8000 >/dev/null 2>&1 &); sleep 1; curl -s -o /dev/null -w '%{http_code}' localhost:8000"))
        .await
        .unwrap();
    log("pkill -f restart of the server", format!("{r:?}"));
    assert_eq!((r.exit_code, r.stdout.as_str()), (0, "200"));
    // A command that does kill its own shell gets 128+signal and the output it made.
    for (signal, code) in [("TERM", 143), ("KILL", 137)] {
        let r = client
            .exec(
                &id,
                &exec(&format!(
                    "echo partial; echo oops >&2; pkill -{signal} -f arexec; sleep 5; echo never"
                )),
            )
            .await
            .unwrap();
        log(
            &format!("a command that kills its shell with SIG{signal}"),
            format!("{r:?}"),
        );
        assert_eq!(r.exit_code, code);
        assert_eq!(r.stdout, "partial\n");
        assert!(
            r.stderr
                .starts_with("oops\n[the shell was terminated by signal")
        );
    }
    let r = client
        .exec(&id, &exec("ls -d /tmp/.arexec.* | wc -l"))
        .await
        .unwrap();
    assert_eq!(
        r.stdout.trim(),
        "1",
        "only this call's own directory is left in /tmp"
    );

    // /tmp and $HOME allow executables (but not setuid or devices); the root stays read-only.
    let r = client
        .exec(&id, &exec("grep -E ' /(tmp|home/agent) ' /proc/mounts; cp /bin/true /tmp/t && /tmp/t && cp /bin/true ~/t && ~/t && echo exec-ok; touch /usr/x /etc/x /opt/x 2>&1 | wc -l"))
        .await
        .unwrap();
    log("tmpfs mounts and the read-only root", format!("{r:?}"));
    assert!(r.stdout.contains("exec-ok\n3\n"), "{}", r.stdout);
    // gVisor's /proc/mounts lists nosuid but not nodev; `nodev` is in the run arguments
    // (box_container_is_locked_down), and the box has no CAP_MKNOD to make a device anyway.
    let mounts: Vec<&str> = r.stdout.lines().filter(|l| l.contains("tmpfs")).collect();
    assert_eq!(mounts.len(), 2, "{mounts:?}");
    for line in mounts {
        assert!(
            line.contains("nosuid") && !line.contains("noexec"),
            "{line}"
        );
    }
    // The data stack is preinstalled, a compiled package installs per user (onto
    // /workspace, and into $HOME when asked), and SVG converts.
    let r = client
        .exec(&id, &exec("python3 -c \"import numpy as np, pandas as pd, matplotlib.pyplot as plt\ndf = pd.DataFrame({'region': ['North', 'East', 'South'], 'revenue': np.array([2100750, 1938000, 1828320])})\ndf.plot.bar(x='region', y='revenue')\nplt.savefig('chart.png')\nprint(np.__version__, pd.__version__)\" && file chart.png"))
        .await
        .unwrap();
    log("numpy, pandas and matplotlib chart", format!("{r:?}"));
    assert!(
        r.exit_code == 0 && r.stdout.contains("PNG image data"),
        "{r:?}"
    );
    let r = client
        .exec(&id, &ExecRequest {
            command: "pip install --user --ignore-installed -q ujson && python3 -c 'import ujson; print(ujson.__file__)' && PYTHONUSERBASE=$HOME/.local pip install --user --ignore-installed -q numpy && PYTHONUSERBASE=$HOME/.local python3 -c 'import numpy; print(numpy.__file__, numpy.ones(3).sum())'".into(),
            timeout_seconds: Some(300),
            ..Default::default()
        })
        .await
        .unwrap();
    log("pip install --user of compiled packages", format!("{r:?}"));
    assert_eq!(r.exit_code, 0, "{r:?}");
    assert!(
        r.stdout.contains("/workspace/.local/lib/") && r.stdout.contains("/home/agent/.local/lib/")
    );
    client
        .write_file(&id, "a.svg", b"<svg xmlns='http://www.w3.org/2000/svg' width='120' height='80'><rect width='120' height='80' fill='#2a6'/><text x='10' y='45' font-size='20'>chart</text></svg>")
        .await
        .unwrap();
    let r = client
        .exec(&id, &exec("convert a.svg a.png && /usr/bin/convert a.svg b.jpg && convert a.svg c.pdf && file a.png b.jpg c.pdf"))
        .await
        .unwrap();
    log("convert of an SVG", format!("{r:?}"));
    assert!(
        r.exit_code == 0
            && r.stdout.contains("120 x 80")
            && r.stdout.contains("JPEG")
            && r.stdout.contains("PDF"),
        "{r:?}"
    );

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

    // A box whose container is lost outside the manager (an operator `docker rm -f`) keeps
    // its workspace: it is parked, and the next call starts a new container on it.
    let lost = client.create(&spec).await.unwrap();
    client
        .write_file(&lost, "survivor.txt", b"workspace kept")
        .await
        .unwrap();
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
    assert!(list.iter().any(|b| b.box_id == lost));
    let r = client.exec(&lost, &exec("cat survivor.txt")).await.unwrap();
    log("exec on the lost box", format!("{r:?}"));
    assert_eq!(r.stdout, "workspace kept");
    client.destroy(&lost).await.unwrap();
    let volume = std::process::Command::new("docker")
        .args(["volume", "inspect", &format!("arbox-{lost}")])
        .output()
        .unwrap();
    assert!(!volume.status.success(), "destroy removes the volume");

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

/// Workspace lifecycle on a real worker host: the hard disk quota, parking an idle box
/// (container gone, workspace kept), the transparent restart, adoption by a new worker
/// process, retention, and full removal. Run as the worker user with the `disk` group:
///   AR_TEST_BOX_IMAGE=<pinned image> AR_TEST_BOX_REPORT=<passing report> \
///   AR_TEST_BOX_NETWORK=<network> AR_TEST_BOX_GUARD_MARKER=<marker> \
///   AR_TEST_BOX_STATE_DIR=<dir on the data disk> \
///   cargo test --test boxes live_workspace -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn live_workspace_quota_parking_and_retention() {
    use agent_rush::{
        boxes::BoxManager,
        sandbox_api::{ExecRequest, FileRead},
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter("agent_rush=info")
        .with_ansi(false)
        .try_init();
    let var = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name}"));
    let state_dir = std::path::PathBuf::from(var("AR_TEST_BOX_STATE_DIR"));
    let config = || BoxConfig {
        image: var("AR_TEST_BOX_IMAGE"),
        network: var("AR_TEST_BOX_NETWORK"),
        report: var("AR_TEST_BOX_REPORT").into(),
        state_dir: state_dir.clone(),
        idle_seconds: 2,
        ceiling_mb: 4096,
        disk_mb: 300,
        retention_seconds: 5,
        guard_marker: var("AR_TEST_BOX_GUARD_MARKER").into(),
        max_boxes: agent_rush::boxes::MAX_BOXES,
    };
    let log = |label: &str, value: String| println!("## {label}\n{value}\n");
    let disks = state_dir.join("disks");
    let manager = BoxManager::new(DockerRunner::new(IMAGE).unwrap(), config(), 6144).unwrap();
    let spec = BoxSpec {
        session_id: "live-workspace".into(),
        owner: "smoke".into(),
        memory_mb: 512,
        cpu_millis: 500,
    };
    let exec = |command: &str| ExecRequest {
        command: command.into(),
        ..Default::default()
    };
    let started = std::time::Instant::now();
    let id = manager.create(&spec).await.unwrap();
    log(
        "create (with the in-box guard probe)",
        format!("box_id={id} in {:?}", started.elapsed()),
    );
    let image = disks.join(format!("{id}.img"));
    assert!(image.is_file());

    // The quota: /workspace is the 300 MiB image; writing past it fails with ENOSPC.
    let r = manager
        .exec_box(
            &id,
            &exec("ls -la /workspace; df -B1 --output=size,avail /workspace; dd if=/dev/zero of=/workspace/fill bs=1M count=400 2>/tmp/dd.err; echo dd_exit=$?; cat /tmp/dd.err; stat -c %s /workspace/fill"),
        )
        .await
        .unwrap();
    log("dd 400 MiB into a 300 MiB workspace", format!("{r:#?}"));
    assert!(r.stdout.contains("No space left on device"));
    assert!(!r.stdout.contains("lost+found"));
    let written: u64 = r.stdout.lines().last().unwrap().trim().parse().unwrap();
    assert!(written <= 300 * 1024 * 1024);
    let e = manager
        .write_box_file(&id, "more.bin", vec![b'x'; 8 * 1024 * 1024].into())
        .await
        .unwrap_err();
    log("8 MiB file write into the full workspace", format!("{e:#}"));
    assert!(matches!(
        e.downcast_ref::<BoxError>(),
        Some(BoxError::TooLarge(_))
    ));
    manager
        .exec_box(&id, &exec("rm /workspace/fill"))
        .await
        .unwrap();
    manager
        .write_box_file(
            &id,
            "notes/keep.txt",
            b"kept across parking".to_vec().into(),
        )
        .await
        .unwrap();

    // Parking: idle past AR_BOX_IDLE_SECONDS, the container goes, the workspace stays.
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    manager.reap(chrono::Utc::now().timestamp()).await;
    let inspect = std::process::Command::new("docker")
        .args(["inspect", "--type=container", &format!("arbox-{id}")])
        .output()
        .unwrap();
    log(
        "after the idle reap",
        format!(
            "boxes={} running={} container_present={} image_present={}",
            manager.count(),
            manager.running_count(),
            inspect.status.success(),
            image.is_file()
        ),
    );
    assert_eq!((manager.count(), manager.running_count()), (1, 0));
    assert!(!inspect.status.success() && image.is_file());

    // The next call starts a new container on the same workspace.
    let started = std::time::Instant::now();
    let FileRead::File(bytes) = manager.read_box_file(&id, "notes/keep.txt").await.unwrap() else {
        panic!("expected a file")
    };
    log(
        "read after parking",
        format!(
            "{:?} in {:?} (includes the restart)",
            String::from_utf8_lossy(&bytes),
            started.elapsed()
        ),
    );
    assert_eq!(bytes, b"kept across parking");
    assert_eq!(manager.running_count(), 1);

    // A new worker process adopts the running box; after another park, a third adopts the
    // parked workspace from its metadata.
    // A box of another manager (another network, as the live worker's boxes are to this
    // test's) must survive this manager's recovery untouched.
    let decoy = "decafbaddecafbaddecafbaddecafbad";
    let created = std::process::Command::new("docker")
        .args([
            "create",
            "--name",
            &format!("arbox-{decoy}"),
            "--network=bridge",
            "--label=agent-rush.box=true",
            &format!("--label=agent-rush.box.id={decoy}"),
            "--label=agent-rush.box.owner=someone-else",
            "debian:bookworm-slim",
            "true",
        ])
        .output()
        .unwrap();
    assert!(created.status.success(), "{created:?}");
    let second = BoxManager::new(DockerRunner::new(IMAGE).unwrap(), config(), 6144).unwrap();
    let recovered = second.recover().await.unwrap();
    log("second worker recover", recovered.to_string());
    let decoy_kept = std::process::Command::new("docker")
        .args(["inspect", "--type=container", &format!("arbox-{decoy}")])
        .output()
        .unwrap()
        .status
        .success();
    let _ = std::process::Command::new("docker")
        .args(["rm", "-f", &format!("arbox-{decoy}")])
        .output();
    assert!(decoy_kept, "another network's box was removed");
    assert!(!recovered.to_string().contains(decoy));
    assert_eq!(second.running_count(), 1);
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    second.reap(chrono::Utc::now().timestamp()).await;
    assert_eq!(second.running_count(), 0);
    let third = BoxManager::new(DockerRunner::new(IMAGE).unwrap(), config(), 6144).unwrap();
    let recovered = third.recover().await.unwrap();
    log("third worker recover", recovered.to_string());
    assert!(
        recovered["parked"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p == id.as_str())
    );
    let r = third
        .exec_box(&id, &exec("cat notes/keep.txt"))
        .await
        .unwrap();
    assert_eq!(r.stdout, "kept across parking");

    // Retention: parked longer than AR_BOX_RETENTION_SECONDS, the workspace is deleted.
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    third.reap(chrono::Utc::now().timestamp()).await; // parks
    assert_eq!((third.count(), third.running_count()), (1, 0));
    tokio::time::sleep(std::time::Duration::from_secs(6)).await;
    third.reap(chrono::Utc::now().timestamp()).await; // purges
    log(
        "after retention",
        format!(
            "boxes={} image_present={} meta_present={}",
            third.count(),
            image.exists(),
            disks.join(format!("{id}.json")).exists()
        ),
    );
    assert_eq!(third.count(), 0);
    assert!(!image.exists() && !disks.join(format!("{id}.json")).exists());
    let e = third.exec_box(&id, &exec("true")).await.unwrap_err();
    assert!(matches!(
        e.downcast_ref::<BoxError>(),
        Some(BoxError::NotFound)
    ));

    // Explicit destroy removes container, volume, loop device and image at once.
    let id = third.create(&spec).await.unwrap();
    let (container, workspace) = third.destroy_box(&id).await.unwrap();
    let losetup = std::process::Command::new("losetup")
        .args(["-a"])
        .output()
        .unwrap();
    log(
        "destroy",
        format!(
            "container_removed={container} workspace_removed={workspace} image_present={}",
            disks.join(format!("{id}.img")).exists()
        ),
    );
    assert!(container && workspace);
    assert!(!disks.join(format!("{id}.img")).exists());
    assert!(!String::from_utf8_lossy(&losetup.stdout).contains(&id));
}
