fn main() {
    assert!(!std::path::Path::new("/var/run/docker.sock").exists());
    assert!(!std::path::Path::new("/workspace").exists());
    for key in ["VULTR_API_KEY","VULTR_INFERENCE_API_KEY","GH_TOKEN","AR_WORKER_TOKEN"] { assert!(std::env::var(key).is_err()); }
    assert!(std::fs::write("/etc/agent-rush-canary", "denied").is_err());
    assert!(std::net::TcpStream::connect_timeout(&"1.1.1.1:443".parse().unwrap(),std::time::Duration::from_millis(150)).is_err());
    std::fs::write("/work/artifacts/result.json",r#"{"contained":true,"total":42}"#).unwrap();
    println!("Containment probes passed; total=42");
}
