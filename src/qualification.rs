//! Real, fixed containment/resource probes. A green unit suite never substitutes for this report.
use crate::{
    boxes::BoxManager,
    domain::{Limits, now},
    resources::ResourcePolicy,
    sandbox_api::{BoxSpec, ExecRequest},
    worker::DockerRunner,
};
use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
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

pub const BOX_CHECKS: [&str; 8] = [
    "metadata_blocked",
    "private_blocked",
    "worker_listener_blocked",
    "smtp_blocked",
    "public_https_allowed",
    "no_credentials",
    "non_root_readonly",
    "volume_destroyed",
];
pub const BOX_REPORT_SCHEMA: &str = "agent-rush.box-qualification.v1";
/// A box report admits boxes for two hours, like the job report.
pub const BOX_REPORT_TTL_SECONDS: i64 = 7200;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BoxCheck {
    pub name: String,
    pub passed: bool,
    #[serde(default)]
    pub detail: Value,
}
/// Evidence that this host's session boxes are contained. `BoxManager::create` refuses
/// to start a box unless a report for this host and image is fresh and fully passing.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BoxReport {
    pub schema: String,
    pub qualified: bool,
    pub host: String,
    pub image: String,
    pub network: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub checks: Vec<BoxCheck>,
    #[serde(default)]
    pub notes: Vec<String>,
}
impl BoxReport {
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let metadata = std::fs::symlink_metadata(path)
            .with_context(|| format!("box qualification report {} missing", path.display()))?;
        ensure!(
            metadata.file_type().is_file() && metadata.len() <= 1024 * 1024,
            "invalid box qualification file"
        );
        Ok(serde_json::from_slice(&std::fs::read(path)?)?)
    }
    /// Fails unless the report is for this host and image, current at `at` (unix seconds),
    /// and every one of the eight checks is present exactly once and passed.
    pub fn assess(&self, host: &str, image: &str, at: i64) -> Result<()> {
        ensure!(
            self.schema == BOX_REPORT_SCHEMA && self.qualified,
            "box qualification report is not passing"
        );
        ensure!(self.host == host, "box qualification is for another host");
        ensure!(
            self.image == image,
            "box qualification is for another workspace image"
        );
        ensure!(
            self.created_at <= at + 60
                && self.expires_at > at
                && self.expires_at - self.created_at <= BOX_REPORT_TTL_SECONDS,
            "box qualification expired or its timestamps are invalid"
        );
        ensure!(
            self.checks.len() == BOX_CHECKS.len(),
            "box qualification checks incomplete"
        );
        for name in BOX_CHECKS {
            let mut named = self.checks.iter().filter(|c| c.name == name);
            ensure!(
                named.next().is_some_and(|c| c.passed) && named.next().is_none(),
                "box qualification check {name} absent, duplicated or failed"
            );
        }
        Ok(())
    }
}
/// This host's identity in box reports: a digest of its machine id.
pub fn box_host_id() -> Result<String> {
    let id = std::fs::read_to_string("/etc/machine-id").context("machine id unavailable")?;
    Ok(hex::encode(Sha256::digest(id.trim().as_bytes())))
}

/// What `qualify-box` probes beyond its fixed list.
pub struct BoxProbes {
    /// Private endpoints (`ip:port`) the host itself can reach; a box must not.
    pub private: Vec<String>,
    /// Ports the worker listens on; each is probed on every host address.
    pub listener_ports: Vec<u16>,
    /// The worker's bearer token, compared by digest with every box environment value.
    pub worker_token: Option<String>,
}

fn host_addresses() -> Result<Vec<String>> {
    let out = std::process::Command::new("ip")
        .args(["-4", "-o", "addr", "show", "scope", "global"])
        .output()
        .context("listing host addresses")?;
    ensure!(out.status.success(), "listing host addresses failed");
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            l.split_whitespace()
                .nth(3)?
                .split('/')
                .next()
                .map(String::from)
        })
        .collect())
}
async fn host_reaches(target: &str) -> bool {
    matches!(
        tokio::time::timeout(
            Duration::from_secs(3),
            tokio::net::TcpStream::connect(target)
        )
        .await,
        Ok(Ok(_))
    )
}

/// Start a box, run the fixed probes in it, destroy it, and check its volume is gone.
pub async fn run_box(manager: &BoxManager, probes: &BoxProbes) -> Result<BoxReport> {
    let created_at = Utc::now().timestamp();
    let host = box_host_id()?;
    for target in &probes.private {
        ensure!(
            target.parse::<std::net::SocketAddrV4>().is_ok(),
            "private probe {target} must be ip:port"
        );
    }
    let gateway = manager.network_gateway().await?;
    let mut addresses = host_addresses()?;
    addresses.push(gateway.clone());
    addresses.sort();
    addresses.dedup();
    let listeners: Vec<String> = addresses
        .iter()
        .flat_map(|a| {
            probes
                .listener_ports
                .iter()
                .map(move |p| format!("{a}:{p}"))
        })
        .collect();
    let mut notes = vec![
        "Box traffic to the host's own addresses is delivered locally and governed by the host firewall, not by DOCKER-USER; worker_listener_blocked observes that firewall.".to_string(),
    ];
    let mut private_controls = Vec::new();
    for target in &probes.private {
        let reachable = host_reaches(target).await;
        private_controls.push(json!({"target":target,"reachable_from_host":reachable}));
        if !reachable {
            notes.push(format!(
                "{target} is not reachable from the host either, so it is a weak control"
            ));
        }
    }
    // Whether the host itself can submit mail shows the box's refusal is the filter's doing.
    let mut smtp_controls = Vec::new();
    for target in ["smtp.gmail.com:587", "smtp.gmail.com:465"] {
        smtp_controls
            .push(json!({"target":target,"reachable_from_host":host_reaches(target).await}));
    }
    let spec = BoxSpec {
        session_id: format!("qualify-{}", uuid::Uuid::new_v4().simple()),
        owner: "qualification".into(),
        memory_mb: 1024,
        cpu_millis: 1000,
    };
    let box_id = manager.create_for_qualification(&spec).await?;
    let probe = manager
        .exec_box(
            &box_id,
            &ExecRequest {
                command: format!(
                    "bash -s -- --gateway {gateway} --private '{}' --listeners '{}'",
                    probes.private.join(","),
                    listeners.join(",")
                ),
                cwd: None,
                timeout_seconds: Some(300),
                stdin: Some(include_str!("../qualification/box_checks.sh").into()),
            },
        )
        .await;
    let (container_removed, volume_removed) =
        manager.destroy_box(&box_id).await.unwrap_or((false, false));
    let probe = probe?;
    let token_digest = probes
        .worker_token
        .as_deref()
        .filter(|t| !t.is_empty())
        .map(|t| hex::encode(Sha256::digest(t.as_bytes())));
    let mut checks: Vec<BoxCheck> = Vec::new();
    for line in probe.stdout.lines() {
        let Ok(mut check) = serde_json::from_str::<BoxCheck>(line) else {
            continue;
        };
        if !BOX_CHECKS.contains(&check.name.as_str())
            || check.name == "volume_destroyed"
            || checks.iter().any(|c| c.name == check.name)
        {
            continue;
        }
        if check.name == "private_blocked" {
            check.detail["host_controls"] = json!(private_controls);
        }
        if check.name == "smtp_blocked" {
            check.detail["host_controls"] = json!(smtp_controls);
        }
        if check.name == "no_credentials" {
            // Compare digests here so neither the token nor its digest enters the box.
            let hashes = check.detail["env_value_sha256"].take();
            let leaked = token_digest.as_ref().is_some_and(|digest| {
                hashes
                    .as_array()
                    .is_some_and(|a| a.iter().any(|h| h == digest.as_str()))
            });
            if let Some(detail) = check.detail.as_object_mut() {
                detail.remove("env_value_sha256");
            }
            check.detail["worker_token_compared"] = json!(token_digest.is_some());
            check.detail["worker_token_in_env"] = json!(leaked);
            check.passed = check.passed && !leaked;
        }
        checks.push(check);
    }
    for name in BOX_CHECKS {
        if name != "volume_destroyed" && !checks.iter().any(|c| c.name == name) {
            checks.push(BoxCheck {
                name: name.into(),
                passed: false,
                detail: json!({"error":"probe did not report","exit_code":probe.exit_code,"stderr":probe.stderr}),
            });
        }
    }
    checks.push(BoxCheck {
        name: "volume_destroyed".into(),
        passed: container_removed && volume_removed,
        detail: json!({"box_id":box_id,"container_removed":container_removed,"volume_removed":volume_removed}),
    });
    checks.sort_by_key(|c| BOX_CHECKS.iter().position(|n| *n == c.name));
    Ok(BoxReport {
        schema: BOX_REPORT_SCHEMA.into(),
        qualified: checks.len() == BOX_CHECKS.len() && checks.iter().all(|c| c.passed),
        host,
        image: manager.image().into(),
        network: manager.network().into(),
        created_at,
        expires_at: created_at + BOX_REPORT_TTL_SECONDS,
        checks,
        notes,
    })
}
