use agent_rush::qualification::{assess, validate_report};
use serde_json::json;
fn receipt() -> serde_json::Value {
    json!({"reason":"completed","exit_code":0,"checks_passed":true,"cleanup_state":"verified","creation_uncertain":false,"result":{"contained":true}})
}
#[test]
fn qualification_requires_observed_enforcement_and_cleanup_not_narration() {
    assert!(assess("containment", &receipt()).unwrap());
    let mut r = receipt();
    r["cleanup_state"] = json!("failed");
    assert!(!assess("containment", &r).unwrap());
    let mut r = receipt();
    r["stdout"] = json!("allocating\nallocation escaped");
    assert!(!assess("memory", &r).unwrap());
    let mut r = receipt();
    r["reason"] = json!("execution_failed");
    r["exit_code"] = json!(137);
    r["stdout"] = json!("allocating");
    assert!(assess("memory", &r).unwrap());
    assert!(!assess("cpu", &receipt()).unwrap());
    let mut r = receipt();
    r["resource_samples"] =
        json!([{"docker_stats":{"CPUPerc":"22.0%"}},{"docker_stats":{"CPUPerc":"24.5%"}}]);
    assert!(assess("cpu", &r).unwrap());
    r["resource_samples"] =
        json!([{"docker_stats":{"CPUPerc":"99.0%"}},{"docker_stats":{"CPUPerc":"100.0%"}}]);
    assert!(!assess("cpu", &r).unwrap());
    assert!(assess("invented", &receipt()).is_err());
}
#[test]
fn worker_admission_requires_fresh_complete_matching_host_and_image_qualification() {
    let health = json!({"host_fingerprint":"host-A","image":"digest-A","local_image_id":"image-A"});
    let now = chrono::Utc::now();
    let mut report = json!({"schema":"agent-rush.qualification.v1","qualified":true,"health":health,"finished_at":now,"checks":[{"name":"containment","passed":true},{"name":"timeout","passed":true},{"name":"output","passed":true},{"name":"memory","passed":true},{"name":"scaling","passed":true},{"name":"scratch","passed":true},{"name":"pids","passed":true},{"name":"cpu","passed":true},{"name":"cancel","passed":true},{"name":"concurrency","passed":true}]});
    for check in report["checks"].as_array_mut().unwrap() {
        let mut raw = receipt();
        match check["name"].as_str().unwrap() {
            "timeout" => {
                raw["reason"] = json!("timeout");
                raw["execution_ms"] = json!(5000);
            }
            "output" => {
                raw["reason"] = json!("output_limit");
            }
            "memory" => {
                raw["reason"] = json!("execution_failed");
                raw["exit_code"] = json!(137);
                raw["stdout"] = json!("allocating");
            }
            "scratch" => raw["result"]["scratch_limited"] = json!(true),
            "pids" => {
                raw["result"]["pids_limited"] = json!(true);
                raw["result"]["children"] = json!(60);
            }
            "cpu" => {
                raw["resource_samples"] = json!([{"docker_stats":{"CPUPerc":"22.0%"}},{"docker_stats":{"CPUPerc":"24.5%"}}])
            }
            "cancel" => raw["reason"] = json!("cancelled"),
            "scaling" => raw = scaled(),
            "concurrency" => {
                raw["result"]["neighbor_contained"] = json!(true);
                raw["execution_started_at"] = json!(now - chrono::Duration::seconds(10));
                raw["execution_finished_at"] = json!(now);
                raw = json!({"receipts":[raw.clone(),raw]});
            }
            _ => {}
        }
        check["receipt"] = raw;
    }
    assert!(validate_report(&report, &health, now).is_ok());
    // A report from before the scaling check existed cannot admit work.
    let mut stale = report.clone();
    stale["checks"]
        .as_array_mut()
        .unwrap()
        .retain(|c| c["name"] != "scaling");
    assert!(validate_report(&stale, &health, now).is_err());
    let mut contradictory = report.clone();
    contradictory["checks"][0]["receipt"]["cleanup_state"] = json!("failed");
    assert!(validate_report(&contradictory, &health, now).is_err());
    report["health"]["host_fingerprint"] = json!("host-B");
    assert!(validate_report(&report, &health, now).is_err());
    report["health"] = health.clone();
    assert!(validate_report(&report, &health, now + chrono::Duration::hours(3)).is_err());
    report["checks"].as_array_mut().unwrap().pop();
    assert!(validate_report(&report, &health, now).is_err());
}

fn scaled() -> serde_json::Value {
    let mut r = receipt();
    r["reason"] = json!("memory_limit");
    r["exit_code"] = json!(137);
    r["stdout"] = json!("holding 256 MiB\nholding 288 MiB\nholding 320 MiB\n");
    r["scaling"] = json!({"available":true,"start":{"memory_mb":256},"ceiling":{"memory_mb":512},"final":{"memory_mb":512},"events":[{"memory_mb":512}],"peak_memory_mb":511,"oom_kills":1});
    r
}

#[test]
fn scaling_qualifies_only_live_growth_that_stops_at_the_ceiling() {
    assert!(assess("scaling", &scaled()).unwrap());
    let cases: [(&str, serde_json::Value); 6] = [
        ("/stdout", json!("holding 256 MiB\nallocation escaped")),
        ("/stdout", json!("holding 256 MiB")),
        ("/scaling/final/memory_mb", json!(256)),
        ("/scaling/events", json!([])),
        ("/scaling/peak_memory_mb", json!(700)),
        ("/reason", json!("completed")),
    ];
    for (pointer, value) in cases {
        let mut r = scaled();
        *r.pointer_mut(pointer).unwrap() = value;
        assert!(!assess("scaling", &r).unwrap(), "{pointer}");
    }
    let mut r = scaled();
    r["cleanup_state"] = json!("failed");
    assert!(!assess("scaling", &r).unwrap());
}

fn box_report(at: i64) -> agent_rush::qualification::BoxReport {
    use agent_rush::qualification::{BOX_CHECKS, BOX_REPORT_SCHEMA, BoxCheck, BoxReport};
    BoxReport {
        schema: BOX_REPORT_SCHEMA.into(),
        qualified: true,
        host: "host-A".into(),
        image: "workspace@sha256:aaaa".into(),
        network: "ar-box".into(),
        created_at: at,
        expires_at: at + 7200,
        checks: BOX_CHECKS
            .iter()
            .map(|name| BoxCheck {
                name: (*name).into(),
                passed: true,
                detail: json!({}),
            })
            .collect(),
        notes: Vec::new(),
    }
}

#[test]
fn boxes_require_a_fresh_complete_passing_report_for_this_host_and_image() {
    let at = chrono::Utc::now().timestamp();
    let (host, image) = ("host-A", "workspace@sha256:aaaa");
    assert!(box_report(at).assess(host, image, "ar-box", at).is_ok());
    assert!(
        box_report(at)
            .assess(host, image, "ar-box", at + 7199)
            .is_ok()
    );

    let mut failing = box_report(at);
    failing.checks[0].passed = false;
    assert!(failing.assess(host, image, "ar-box", at).is_err());
    // A failed check cannot be outvoted by a passing duplicate of the same name.
    failing.checks.push(failing.checks[1].clone());
    failing.checks[9].name = "metadata_blocked".into();
    failing.checks[9].passed = true;
    failing.checks.remove(1);
    assert!(failing.assess(host, image, "ar-box", at).is_err());

    let stale = box_report(at - 7300);
    assert!(stale.assess(host, image, "ar-box", at).is_err());
    let mut forged_expiry = box_report(at);
    forged_expiry.expires_at = at + 86400;
    assert!(forged_expiry.assess(host, image, "ar-box", at).is_err());
    let future = box_report(at + 3600);
    assert!(future.assess(host, image, "ar-box", at).is_err());

    assert!(
        box_report(at)
            .assess(host, "workspace@sha256:bbbb", "ar-box", at)
            .is_err()
    );
    assert!(
        box_report(at)
            .assess("host-B", image, "ar-box", at)
            .is_err()
    );

    let mut unqualified = box_report(at);
    unqualified.qualified = false;
    assert!(unqualified.assess(host, image, "ar-box", at).is_err());
    let mut missing = box_report(at);
    missing.checks.retain(|c| c.name != "volume_destroyed");
    assert!(missing.assess(host, image, "ar-box", at).is_err());
    let mut renamed = box_report(at);
    renamed.schema = "agent-rush.qualification.v1".into();
    assert!(renamed.assess(host, image, "ar-box", at).is_err());
}

#[test]
fn box_report_file_must_be_a_regular_file() {
    use agent_rush::qualification::BoxReport;
    let dir = tempfile::tempdir().unwrap();
    assert!(BoxReport::load(&dir.path().join("absent.json")).is_err());
    let real = dir.path().join("report.json");
    std::fs::write(&real, serde_json::to_vec(&box_report(0)).unwrap()).unwrap();
    assert!(BoxReport::load(&real).is_ok());
    #[cfg(unix)]
    {
        let link = dir.path().join("link.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(BoxReport::load(&link).is_err());
    }
}

#[test]
fn box_report_is_bound_to_the_network_and_to_this_boot() {
    use agent_rush::qualification::{BOX_CHECKS, box_host_digest};
    let at = chrono::Utc::now().timestamp();
    let (host, image) = ("host-A", "workspace@sha256:aaaa");
    assert!(
        box_report(at)
            .assess(host, image, "ar-box-test", at)
            .is_err()
    );
    // The same machine after a reboot is another host: its firewall rules were reset.
    let before = box_host_digest("machine-1\n", "boot-1\n");
    assert_eq!(before, box_host_digest("machine-1", "boot-1"));
    assert_ne!(before, box_host_digest("machine-1", "boot-2"));
    assert_ne!(before, box_host_digest("machine-2", "boot-1"));
    // Nine checks, including the host guard and the disk quota.
    assert_eq!(BOX_CHECKS.len(), 9);
    for name in [
        "host_services_blocked",
        "disk_quota_enforced",
        "volume_destroyed",
    ] {
        assert!(BOX_CHECKS.contains(&name), "{name}");
        let mut missing = box_report(at);
        missing.checks.retain(|c| c.name != name);
        assert!(missing.assess(host, image, "ar-box", at).is_err(), "{name}");
    }
    // An eight-check report from before the guard and quota checks no longer admits boxes.
    let mut old = box_report(at);
    old.checks.retain(|c| c.name != "disk_quota_enforced");
    old.checks.push(agent_rush::qualification::BoxCheck {
        name: "worker_listener_blocked".into(),
        passed: true,
        detail: json!({}),
    });
    assert!(old.assess(host, image, "ar-box", at).is_err());
}

#[test]
fn host_probes_cover_closed_ports_on_the_gateway_and_ssh_everywhere() {
    use agent_rush::qualification::host_probe_targets;
    let addresses = vec![
        "172.30.0.1".to_string(),
        "10.0.0.4".to_string(),
        "10.0.0.4".to_string(),
    ];
    let targets = host_probe_targets("172.30.0.1", &addresses, &[8090, 8443, 22]);
    for closed in [22, 2375, 5432, 9100, 8090, 8443] {
        assert!(
            targets.contains(&format!("172.30.0.1:{closed}")),
            "{closed}"
        );
    }
    assert_eq!(
        targets
            .iter()
            .filter(|t| t.starts_with("10.0.0.4:"))
            .collect::<Vec<_>>(),
        ["10.0.0.4:22", "10.0.0.4:8090", "10.0.0.4:8443"]
    );
    let mut unique = targets.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), targets.len());
}

#[test]
fn box_report_replaces_an_existing_report_whole() {
    use agent_rush::qualification::write_report_atomically;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("box-qualification.json");
    write_report_atomically(&path, b"first").unwrap();
    write_report_atomically(&path, b"second").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"second");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    // A directory in the way fails cleanly and leaves no temporary file behind.
    let blocked = dir.path().join("dir");
    std::fs::create_dir(&blocked).unwrap();
    assert!(write_report_atomically(&blocked, b"x").is_err());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
}
