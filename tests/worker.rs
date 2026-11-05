use agent_rush::{
    domain::Limits,
    worker::{SandboxSpec, read_artifacts},
};

#[test]
fn fixed_sandbox_policy_does_not_inherit_host_authority() {
    let spec = SandboxSpec::new("test-job", "rust:1.90.0-bookworm@sha256:3914072ca0c3b8aad871db9169a651ccfce30cf58303e5d6f2db16d1d8a7e58f", Limits::default()).unwrap();
    let args = spec.create_args();
    let joined = args.join(" ");
    assert!(joined.contains("/work:rw,exec,nosuid,nodev,size=64m"));
    for required in [
        "--runtime runsc",
        "--network none",
        "--read-only",
        "--cap-drop ALL",
        "--security-opt no-new-privileges",
        "--pids-limit 64",
        "--user 65532:65532",
        "--memory 512m",
        "--memory-swap 512m",
        "--log-driver none",
    ] {
        assert!(joined.contains(required), "missing {required}");
    }
    for forbidden in [
        "--privileged",
        "--volume",
        "--mount",
        "docker.sock",
        "API_KEY",
        "--env-file",
    ] {
        assert!(!joined.contains(forbidden), "unexpected {forbidden}");
    }
    assert!(SandboxSpec::new("../evil", &spec.image, Limits::default()).is_err());
    assert!(SandboxSpec::new("job", "rust:latest", Limits::default()).is_err());
}

fn archive(path: &str, content: &[u8], kind: tar::EntryType) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut tar = tar::Builder::new(&mut bytes);
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(kind);
        if kind.is_symlink() {
            header.set_link_name("/etc/passwd").unwrap();
        }
        header.set_cksum();
        tar.append_data(&mut header, path, content).unwrap();
        tar.finish().unwrap();
    }
    bytes
}

#[test]
fn artifacts_accept_only_bounded_regular_files() {
    let files = read_artifacts(&archive(
        "./result.json",
        b"{\"total\":42}",
        tar::EntryType::Regular,
    ))
    .unwrap();
    assert_eq!(files[0].name, "result.json");
    assert_eq!(
        files[0].sha256,
        "bb1453c7fec8dfc7c9d2c40136205f7cef6a1d9bb63e1ee85c038f04e730baa8"
    );
    assert!(read_artifacts(&archive("secret", b"", tar::EntryType::Symlink)).is_err());
    assert!(
        read_artifacts(&archive(
            "result.json",
            &vec![b'x'; 1_048_577],
            tar::EntryType::Regular
        ))
        .is_err()
    );
}

#[test]
fn host_without_real_resource_controllers_is_rejected() {
    let invalid = serde_json::json!({"MemoryLimit":false,"CpuCfsQuota":false,"PidsLimit":false,"CgroupDriver":"none","Runtimes":{"runsc":{"runtimeArgs":["--ignore-cgroups"]}}});
    assert!(agent_rush::worker::validate_host(&invalid).is_err());
    let mut valid = serde_json::json!({"MemoryLimit":true,"CpuCfsQuota":true,"PidsLimit":true,"CgroupDriver":"systemd","Runtimes":{"runsc":{"runtimeArgs":["--platform=systrap"]}}});
    assert!(agent_rush::worker::validate_host(&valid).is_ok());
    valid["Runtimes"]["runsc"]["runtimeArgs"] = serde_json::json!(["--ignore-cgroups"]);
    assert!(agent_rush::worker::validate_host(&valid).is_err());
}

#[test]
fn fast_exit_cannot_override_final_output_overflow_or_incomplete_capture() {
    let (reason, _, passed) =
        agent_rush::worker::reconcile_output(Some(0), "completed", true, true, true);
    assert_eq!(reason, "output_limit");
    assert!(!passed);
    let (_, _, passed) =
        agent_rush::worker::reconcile_output(Some(0), "completed", false, true, false);
    assert!(!passed);
    let (reason, _, passed) =
        agent_rush::worker::reconcile_output(Some(0), "completed", false, true, true);
    assert_eq!(reason, "completed");
    assert!(passed);
}

#[test]
fn memory_grows_under_pressure_and_never_past_the_ceiling() {
    use agent_rush::worker::next_memory_mb;
    const MIB: u64 = 1_048_576;
    // Below 70% of the current limit nothing changes.
    assert_eq!(next_memory_mb(256, 170 * MIB, 512), None);
    // At 70% it doubles.
    assert_eq!(next_memory_mb(256, 180 * MIB, 1024), Some(512));
    // A big jump gets twice the current use, rounded up to 64 MiB.
    assert_eq!(next_memory_mb(256, 250 * MIB, 1024), Some(512));
    assert_eq!(next_memory_mb(512, 400 * MIB, 1024), Some(1024));
    // Capped at the ceiling, and nothing once there.
    assert_eq!(next_memory_mb(256, 250 * MIB, 384), Some(384));
    assert_eq!(next_memory_mb(512, 510 * MIB, 512), None);
    // Usage beyond the limit (reported during a kill) still cannot pass the ceiling.
    assert_eq!(next_memory_mb(256, 4096 * MIB, 768), Some(768));
}

#[test]
fn cpu_jumps_to_the_ceiling_only_when_saturated() {
    use agent_rush::worker::next_cpu_millis;
    // 0.5 vCPU quota, 0.45 vCPU used over the window: 90% saturated.
    assert_eq!(next_cpu_millis(500, 225_000, 500_000, 1000), Some(1000));
    // 0.3 vCPU used: not saturated.
    assert_eq!(next_cpu_millis(500, 150_000, 500_000, 1000), None);
    assert_eq!(next_cpu_millis(1000, 500_000, 500_000, 1000), None);
    assert_eq!(next_cpu_millis(500, 1, 0, 1000), None);
}

#[test]
fn ceilings_are_validated_against_the_start_and_the_hard_maximum() {
    let ok = Limits {
        memory_mb: 256,
        memory_ceiling_mb: 1024,
        cpu_millis: 500,
        cpu_ceiling_millis: 1000,
        ..Limits::default()
    };
    assert!(ok.validate().is_ok());
    assert_eq!((ok.memory_ceiling(), ok.cpu_ceiling()), (1024, 1000));
    // No ceiling means no growth.
    assert_eq!(
        Limits::default().memory_ceiling(),
        Limits::default().memory_mb
    );
    for bad in [
        Limits {
            memory_mb: 512,
            memory_ceiling_mb: 256,
            ..Limits::default()
        },
        Limits {
            memory_ceiling_mb: 2048,
            ..Limits::default()
        },
        Limits {
            cpu_millis: 1000,
            cpu_ceiling_millis: 500,
            ..Limits::default()
        },
        Limits {
            cpu_ceiling_millis: 4000,
            ..Limits::default()
        },
    ] {
        assert!(bad.validate().is_err(), "{bad:?}");
    }
}
