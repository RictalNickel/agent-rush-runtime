//! Real, fixed containment/resource probes. A green unit suite never substitutes for this report.
use crate::{
    domain::{Limits, now},
    resources::ResourcePolicy,
    worker::DockerRunner,
};
use anyhow::{Result, ensure};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

pub const CHECKS: [&str; 10] = [
    "containment",
    "timeout",
    "output",
    "memory",
    "scaling",
    "scratch",
    "pids",
    "cpu",
    "cancel",
    "concurrency",
];
fn clean(r: &Value) -> bool {
    r["cleanup_state"] == "verified" && r["creation_uncertain"] == false
}
fn success(r: &Value) -> bool {
    clean(r) && r["reason"] == "completed" && r["exit_code"] == 0 && r["checks_passed"] == true
}
pub fn assess(name: &str, r: &Value) -> Result<bool> {
    ensure!(CHECKS.contains(&name), "unknown qualification check");
    if name == "concurrency" {
        let Some(receipts) = r["receipts"].as_array() else {
            return Ok(false);
        };
        if receipts.len() != 2
            || !receipts
                .iter()
                .all(|v| success(v) && v["result"]["neighbor_contained"] == true)
        {
            return Ok(false);
        }
        let parse = |v: &Value, k: &str| {
            v[k].as_str()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        };
        let (Some(a), Some(b), Some(c), Some(d)) = (
            parse(&receipts[0], "execution_started_at"),
            parse(&receipts[0], "execution_finished_at"),
            parse(&receipts[1], "execution_started_at"),
            parse(&receipts[1], "execution_finished_at"),
        ) else {
            return Ok(false);
        };
        return Ok(a <= b && c <= d && a < d && c < b);
    }
    if !clean(r) {
        return Ok(false);
    }
    Ok(match name {
        "containment" => success(r) && r["result"]["contained"] == true,
        "timeout" => {
            r["reason"] == "timeout" && r["execution_ms"].as_u64().is_some_and(|v| v < 8000)
        }
        "output" => {
            r["reason"] == "output_limit"
                && r["stdout"].as_str().unwrap_or("").len()
                    + r["stderr"].as_str().unwrap_or("").len()
                    <= 65536
        }
        "memory" => {
            matches!(
                r["reason"].as_str(),
                Some("execution_failed" | "memory_limit")
            ) && r["exit_code"].as_i64().is_some_and(|v| v != 0)
                && r["stdout"]
                    .as_str()
                    .is_some_and(|s| s.contains("allocating") && !s.contains("allocation escaped"))
        }
        // Grew live past its 256 MiB start, then died at its 512 MiB ceiling, never above it.
        "scaling" => {
            let stdout = r["stdout"].as_str().unwrap_or("");
            let scaling = &r["scaling"];
            r["reason"] == "memory_limit"
                && scaling["available"] == true
                && scaling["start"]["memory_mb"] == 256
                && scaling["ceiling"]["memory_mb"] == 512
                && scaling["final"]["memory_mb"] == 512
                && scaling["events"].as_array().is_some_and(|e| !e.is_empty())
                && scaling["peak_memory_mb"]
                    .as_u64()
                    .is_some_and(|p| p > 256 && p <= 512)
                && stdout.contains("holding 288 MiB")
                && !stdout.contains("allocation escaped")
        }
        "scratch" => success(r) && r["result"]["scratch_limited"] == true,
        "pids" => {
            success(r)
                && r["result"]["pids_limited"] == true
                && r["result"]["children"]
                    .as_u64()
                    .is_some_and(|n| n > 0 && n < 64)
        }
        "cpu" => {
            let samples: Vec<f64> = r["resource_samples"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|s| {
                    s["docker_stats"]["CPUPerc"]
                        .as_str()?
                        .trim_end_matches('%')
                        .parse()
                        .ok()
                })
                .collect();
            success(r)
                && samples.len() >= 2
                && samples.iter().any(|p| *p > 1.0)
                && samples
                    .iter()
                    .all(|p| p.is_finite() && (0.0..=50.0).contains(p))
        }
        "cancel" => r["reason"] == "cancelled",
        _ => false,
    })
}
pub fn validate_report(report: &Value, health: &Value, at: DateTime<Utc>) -> Result<()> {
    ensure!(
        report["schema"] == "agent-rush.qualification.v1" && report["qualified"] == true,
        "worker has no passing qualification report"
    );
    for key in ["host_fingerprint", "image", "local_image_id"] {
        ensure!(
            health[key].as_str().is_some_and(|v| !v.is_empty())
                && report["health"][key] == health[key],
            "qualification does not match current {key}"
        );
    }
    let finished = DateTime::parse_from_rfc3339(
        report["finished_at"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("report timestamp missing"))?,
    )?
    .with_timezone(&Utc);
    ensure!(
        finished <= at + ChronoDuration::minutes(1) && at - finished < ChronoDuration::hours(2),
        "qualification expired or timestamp is invalid"
    );
    let checks = report["checks"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("qualification checks missing"))?;
    ensure!(
        checks.len() == CHECKS.len(),
        "qualification checks incomplete"
    );
    for name in CHECKS {
        ensure!(
            checks
                .iter()
                .filter(|c| c["name"] == name && c["passed"] == true)
                .count()
                == 1,
            "qualification check {name} absent, duplicated or failed"
        );
        let check = checks
            .iter()
            .find(|c| c["name"] == name)
            .expect("unique check established");
        ensure!(
            assess(name, &check["receipt"])?,
            "qualification evidence contradicts passing {name} flag"
        );
    }
    Ok(())
}
pub async fn run(runner: &DockerRunner, cancel: Arc<AtomicBool>) -> Value {
    let started = now();
    let health = match runner.health().await {
        Ok(health) => health,
        Err(error) => {
            return json!({"schema":"agent-rush.qualification.v1","qualified":false,"started_at":started,"finished_at":now(),"checks":CHECKS.iter().map(|n|json!({"name":n,"passed":false,"status":"not_run_host_rejected"})).collect::<Vec<_>>(),"host_error":error.to_string(),"containers_created":0});
        }
    };
    if ResourcePolicy::detect(1024).map_or(true, |p| p.usable_memory_mb < 1024) {
        return json!({"schema":"agent-rush.qualification.v1","qualified":false,"health":health,"started_at":started,"finished_at":now(),"checks":[],"host_error":"At least 1024 MiB usable memory required for bounded qualification"});
    }
    let nonce = uuid::Uuid::new_v4();
    let mut checks = Vec::new();
    let mut blocked = false;
    for name in CHECKS {
        if blocked || cancel.load(Ordering::SeqCst) {
            checks.push(json!({"name":name,"passed":false,"status":"not_run_after_cleanup_failure_or_cancellation"}));
            continue;
        }
        let id = format!("qualify-{nonce}-{name}");
        let code = match name {
            "containment" => include_str!("../qualification/containment.rs"),
            "timeout" | "cancel" => "fn main(){loop{std::hint::spin_loop();}}",
            "output" => "fn main(){loop{println!(\"{}\",\"x\".repeat(4096));}}",
            "memory" => include_str!("../qualification/memory.rs"),
            "scaling" => include_str!("../qualification/scaling.rs"),
            "scratch" => include_str!("../qualification/scratch.rs"),
            "pids" => include_str!("../qualification/pids.rs"),
            "cpu" => include_str!("../qualification/cpu.rs"),
            _ => include_str!("../qualification/neighbor.rs"),
        };
        let limits = Limits {
            memory_mb: if matches!(name, "memory" | "concurrency" | "scaling") {
                256
            } else {
                512
            },
            timeout_seconds: if name == "timeout" { 5 } else { 60 },
            cpu_millis: if name == "cpu" { 250 } else { 1000 },
            memory_ceiling_mb: if name == "scaling" { 512 } else { 0 },
            cpu_ceiling_millis: 0,
        };
        let result = if name == "concurrency" {
            let first_id = format!("{id}-a");
            let second_id = format!("{id}-b");
            let (a, b) = tokio::join!(
                runner.run(&first_id, code, limits.clone(), cancel.clone()),
                runner.run(&second_id, code, limits, cancel.clone())
            );
            // Retain both outcomes, including when one creation/cleanup fails.
            Ok(
                json!({"receipts":[a.unwrap_or_else(|e|json!({"error":e.to_string(),"cleanup_state":"failed"})),b.unwrap_or_else(|e|json!({"error":e.to_string(),"cleanup_state":"failed"}))]}),
            )
        } else if name == "cancel" {
            let flag = Arc::new(AtomicBool::new(false));
            let trigger = flag.clone();
            let external = cancel.clone();
            let timer = tokio::spawn(async move {
                for _ in 0..20 {
                    if external.load(Ordering::SeqCst) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                trigger.store(true, Ordering::SeqCst);
            });
            let result = runner.run(&id, code, limits, flag).await;
            timer.abort();
            result
        } else {
            runner.run(&id, code, limits, cancel.clone()).await
        };
        let receipt = result
            .unwrap_or_else(|error| json!({"error":error.to_string(),"cleanup_state":"failed"}));
        let passed = assess(name, &receipt).unwrap_or(false);
        blocked = if name == "concurrency" {
            receipt["receipts"]
                .as_array()
                .is_none_or(|a| a.iter().any(|r| !clean(r)))
        } else {
            !clean(&receipt)
        };
        checks.push(json!({"name":name,"passed":passed,"receipt":receipt}));
    }
    json!({"schema":"agent-rush.qualification.v1","qualified":checks.iter().all(|v|v["passed"]==true),"health":health,"started_at":started,"finished_at":now(),"checks":checks,"measurement_note":"CPU probe requests 0.25 vCPU; requires at least two actual samples, positive load and no sample above 50% of one core. A bounded enforcement probe, not a peak-memory/CPU performance guarantee."})
}
