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
    let mut report = json!({"schema":"agent-rush.qualification.v1","qualified":true,"health":health,"finished_at":now,"checks":[{"name":"containment","passed":true},{"name":"timeout","passed":true},{"name":"output","passed":true},{"name":"memory","passed":true},{"name":"scratch","passed":true},{"name":"pids","passed":true},{"name":"cpu","passed":true},{"name":"cancel","passed":true},{"name":"concurrency","passed":true}]});
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
