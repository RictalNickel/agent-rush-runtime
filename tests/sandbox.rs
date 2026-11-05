use agent_rush::{
    domain::Limits,
    worker::{DockerRunner, IMAGE},
};
use std::sync::{Arc, atomic::AtomicBool};

#[tokio::test]
#[ignore = "requires local Docker socket access, runsc, pinned image and host cgroups"]
async fn real_rust_execution_artifact_containment_and_cleanup() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .try_init();
    let runner = DockerRunner::new(IMAGE).unwrap();
    let code = r##"fn main(){
      assert!(!std::path::Path::new("/var/run/docker.sock").exists());
      assert!(!std::path::Path::new("/workspace").exists());
      for key in ["VULTR_API_KEY","VULTR_INFERENCE_API_KEY","GH_TOKEN","AR_WORKER_TOKEN"] {assert!(std::env::var(key).is_err());}
      assert!(std::fs::write("/etc/agent-rush-probe", "no").is_err());
      assert!(std::net::TcpStream::connect_timeout(&"1.1.1.1:443".parse().unwrap(),std::time::Duration::from_millis(100)).is_err());
      std::fs::write("/work/artifacts/result.json",r#"{"total":42,"contained":true}"#).unwrap();
      println!("Containment checks passed; total=42");
    }"##;
    let id = format!("proof-{}", uuid::Uuid::new_v4());
    let result = runner
        .run(
            &id,
            code,
            Limits {
                timeout_seconds: 60,
                ..Limits::default()
            },
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .unwrap();
    println!("{}", serde_json::to_string_pretty(&result).unwrap());
    std::fs::create_dir_all("runtime/evidence").unwrap();
    std::fs::write(
        "runtime/evidence/containment.json",
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    assert_eq!(result["cleanup_state"], "verified");
    assert_eq!(result["exit_code"], 0);
    assert_eq!(result["result"]["total"], 42);
    assert_eq!(result["checks_passed"], true);
}

#[tokio::test]
#[ignore = "requires qualified local Docker"]
async fn real_timeout_cleanup() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .try_init();
    let runner = DockerRunner::new(IMAGE).unwrap();
    let id = format!("timeout-{}", uuid::Uuid::new_v4());
    let result = runner
        .run(
            &id,
            "fn main(){loop {std::hint::spin_loop();}}",
            Limits {
                timeout_seconds: 5,
                ..Limits::default()
            },
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .unwrap();
    println!("{}", serde_json::to_string_pretty(&result).unwrap());
    assert_eq!(result["reason"], "timeout");
    assert_eq!(result["cleanup_state"], "verified");
    assert!(result["execution_ms"].as_u64().unwrap() < 8000);
}

async fn case(name: &str, code: &str, limits: Limits) -> serde_json::Value {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .try_init();
    let runner = DockerRunner::new(IMAGE).unwrap();
    let id = format!("{name}-{}", uuid::Uuid::new_v4());
    let result = runner
        .run(&id, code, limits, Arc::new(AtomicBool::new(false)))
        .await
        .unwrap();
    std::fs::create_dir_all("runtime/evidence").unwrap();
    std::fs::write(
        format!("runtime/evidence/{name}.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    assert_eq!(result["cleanup_state"], "verified", "{result}");
    result
}

#[tokio::test]
#[ignore = "requires qualified local Docker"]
async fn real_output_cap() {
    let result = case(
        "output-cap",
        r#"fn main(){loop{println!("{}","x".repeat(4096));}}"#,
        Limits {
            timeout_seconds: 30,
            ..Limits::default()
        },
    )
    .await;
    assert_eq!(result["reason"], "output_limit");
    assert!(
        result["stdout"].as_str().unwrap().len() + result["stderr"].as_str().unwrap().len()
            <= 65536
    );
}

#[tokio::test]
#[ignore = "requires host-enforced memory cgroups"]
async fn real_memory_cap() {
    let result=case("memory-cap",r#"use std::io::Write; fn main(){println!("allocating");std::io::stdout().flush().unwrap();let mut bytes=vec![7u8;384*1024*1024];for b in &mut bytes{*b=std::hint::black_box(*b+1);}std::hint::black_box(&bytes);println!("allocation escaped");}"#,Limits{memory_mb:256,timeout_seconds:30,cpu_millis:1000,..Limits::default()}).await;
    assert!(
        result["stdout"].as_str().unwrap().contains("allocating"),
        "allocator workload did not start: {result}"
    );
    assert!(
        !result["stdout"]
            .as_str()
            .unwrap()
            .contains("allocation escaped"),
        "memory cap was not enforced: {result}"
    );
    assert_ne!(result["exit_code"], 0);
}

#[tokio::test]
#[ignore = "requires local Docker scratch enforcement"]
async fn real_scratch_cap() {
    let result=case("scratch-cap",r##"fn main(){let bytes=vec![0u8;12*1024*1024];let mut denied=false;for i in 0..8{if std::fs::write(format!("/work/fill-{i}"),&bytes).is_err(){denied=true;break;}}for i in 0..8{let _=std::fs::remove_file(format!("/work/fill-{i}"));}assert!(denied,"scratch cap escaped");std::fs::write("/work/artifacts/result.json",r#"{"scratch_limited":true}"#).unwrap();}"##,Limits{timeout_seconds:30,..Limits::default()}).await;
    assert_eq!(result["checks_passed"], true, "{result}");
    assert_eq!(result["result"]["scratch_limited"], true);
}

#[tokio::test]
#[ignore = "requires local Docker"]
async fn real_cancel_cleanup() {
    let runner = DockerRunner::new(IMAGE).unwrap();
    let flag = Arc::new(AtomicBool::new(false));
    let trigger = flag.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        trigger.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    let id = format!("cancel-{}", uuid::Uuid::new_v4());
    let result = runner
        .run(
            &id,
            "fn main(){loop{std::hint::spin_loop();}}",
            Limits::default(),
            flag,
        )
        .await
        .unwrap();
    assert_eq!(result["reason"], "cancelled");
    assert_eq!(result["cleanup_state"], "verified");
}

#[tokio::test]
#[ignore = "requires a genuinely qualified Docker/runsc host; runs all nine native acceptance probes"]
async fn full_native_host_resource_qualification() {
    let runner = DockerRunner::new(IMAGE).unwrap();
    let report = agent_rush::qualification::run(&runner, Arc::new(AtomicBool::new(false))).await;
    std::fs::create_dir_all("runtime/evidence").unwrap();
    std::fs::write(
        "runtime/evidence/qualification.json",
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    assert_eq!(
        report["qualified"], true,
        "Review runtime/evidence/qualification.json; no weaker fallback allowed"
    );
}
