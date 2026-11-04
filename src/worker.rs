use crate::domain::{Limits, now};
use anyhow::{Context, Result, ensure};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader},
    process::Command,
};

pub const OUTPUT_LIMIT: usize = 65536;
pub const IMAGE: &str =
    "rust:1.90.0-bookworm@sha256:3914072ca0c3b8aad871db9169a651ccfce30cf58303e5d6f2db16d1d8a7e58f";
const LABEL: &str = "agent-rush.managed=true";

pub fn validate_host(info: &Value) -> Result<()> {
    ensure!(
        info["Runtimes"]["runsc"].is_object(),
        "runsc runtime unavailable"
    );
    for capability in ["MemoryLimit", "CpuCfsQuota", "PidsLimit"] {
        ensure!(
            info[capability] == true,
            "Host cannot enforce required {capability}; worker refused"
        );
    }
    ensure!(
        matches!(info["CgroupDriver"].as_str(), Some("systemd" | "cgroupfs")),
        "A functioning host cgroup driver is required"
    );
    let args = &info["Runtimes"]["runsc"]["runtimeArgs"];
    ensure!(
        !args.as_array().is_some_and(|a| a.iter().any(|v| v
            .as_str()
            .is_some_and(|s| s == "--ignore-cgroups" || s == "--ignore-cgroups=true"))),
        "runsc --ignore-cgroups is prohibited"
    );
    Ok(())
}

#[derive(Clone)]
pub struct SandboxSpec {
    pub id: String,
    pub image: String,
    pub limits: Limits,
}
impl SandboxSpec {
    pub fn new(id: &str, image: &str, limits: Limits) -> Result<Self> {
        ensure!(
            !id.is_empty()
                && id.len() <= 64
                && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            "invalid task id"
        );
        let (_, digest) = image
            .split_once("@sha256:")
            .context("sandbox image must be digest pinned")?;
        ensure!(
            digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid image digest"
        );
        limits.validate()?;
        Ok(Self {
            id: id.into(),
            image: image.into(),
            limits,
        })
    }
    pub fn name(&self) -> String {
        format!("ar-task-{}", self.id)
    }
    pub fn create_args(&self) -> Vec<String> {
        vec![
            "create".into(),
            "--pull".into(),
            "never".into(),
            "--name".into(),
            self.name(),
            "--label".into(),
            LABEL.into(),
            "--label".into(),
            format!("agent-rush.job={}", self.id),
            "--runtime".into(),
            "runsc".into(),
            "--network".into(),
            "none".into(),
            "--read-only".into(),
            "--cap-drop".into(),
            "ALL".into(),
            "--security-opt".into(),
            "no-new-privileges".into(),
            "--pids-limit".into(),
            "64".into(),
            "--user".into(),
            "65532:65532".into(),
            "--memory".into(),
            format!("{}m", self.limits.memory_mb),
            "--memory-swap".into(),
            format!("{}m", self.limits.memory_mb),
            "--cpus".into(),
            format!("{:.3}", self.limits.cpu_millis as f64 / 1000.0),
            "--ulimit".into(),
            "nofile=128:128".into(),
            "--ulimit".into(),
            "fsize=16777216:16777216".into(),
            "--log-driver".into(),
            "none".into(),
            "--tmpfs".into(),
            "/tmp:rw,noexec,nosuid,nodev,size=16m,mode=1777".into(),
            "--tmpfs".into(),
            "/work:rw,exec,nosuid,nodev,size=64m,mode=1777".into(),
            "--workdir".into(),
            "/work".into(),
            "--env".into(),
            "HOME=/work".into(),
            "--env".into(),
            "RUSTUP_HOME=/usr/local/rustup".into(),
            "--entrypoint".into(),
            "/bin/sleep".into(),
            self.image.clone(),
            "150".into(),
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub name: String,
    pub bytes: usize,
    pub sha256: String,
    pub data_base64: String,
}
pub fn read_artifacts(bytes: &[u8]) -> Result<Vec<Artifact>> {
    ensure!(
        bytes.len() <= 5 * 1024 * 1024,
        "artifact archive exceeds limit"
    );
    let mut archive = tar::Archive::new(bytes);
    let mut result = Vec::new();
    let mut total = 0usize;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            continue;
        }
        ensure!(kind.is_file(), "only regular artifacts allowed");
        let path = entry.path()?.to_string_lossy().into_owned();
        let path = path.strip_prefix("./").unwrap_or(&path);
        ensure!(
            !path.is_empty()
                && path.len() <= 100
                && path != "."
                && path != ".."
                && path
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
            "unsafe artifact path"
        );
        ensure!(
            result.len() < 8 && !result.iter().any(|a: &Artifact| a.name == path),
            "artifact count/duplicate limit"
        );
        ensure!(entry.size() <= 1_048_576, "artifact file exceeds 1 MiB");
        let mut content = Vec::new();
        (&mut entry).take(1_048_577).read_to_end(&mut content)?;
        total += content.len();
        ensure!(total <= 4_194_304, "artifact total exceeds 4 MiB");
        result.push(Artifact {
            name: path.into(),
            bytes: content.len(),
            sha256: hex::encode(Sha256::digest(&content)),
            data_base64: base64::engine::general_purpose::STANDARD.encode(content),
        });
    }
    Ok(result)
}

#[derive(Clone)]
pub struct DockerRunner {
    image: String,
    docker_config: Arc<tempfile::TempDir>,
}
impl DockerRunner {
    pub fn new(image: &str) -> Result<Self> {
        SandboxSpec::new("validation", image, Limits::default())?;
        Ok(Self {
            image: image.into(),
            docker_config: Arc::new(tempfile::tempdir()?),
        })
    }
    fn command(&self) -> Command {
        // An empty client config prevents Docker from silently injecting host proxy/secret defaults into tasks.
        let mut cmd = Command::new("/usr/local/bin/docker");
        if !std::path::Path::new("/usr/local/bin/docker").exists() {
            cmd = Command::new("/usr/bin/docker");
        }
        cmd.env_clear()
            .env(
                "PATH",
                "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
            )
            .arg("--config")
            .arg(self.docker_config.path())
            .arg("--host=unix:///var/run/docker.sock")
            .kill_on_drop(true);
        cmd
    }
    async fn output(&self, args: &[String], limit: usize) -> Result<Vec<u8>> {
        let mut child = self
            .command()
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut stdout = child.stdout.take().context("stdout")?;
        let mut stderr = child.stderr.take().context("stderr")?;
        let out = async {
            let mut b = Vec::new();
            (&mut stdout)
                .take(limit as u64 + 1)
                .read_to_end(&mut b)
                .await?;
            ensure!(b.len() <= limit, "Docker response exceeds bound");
            Ok::<_, anyhow::Error>(b)
        };
        let err = async {
            let mut b = Vec::new();
            (&mut stderr).take(2049).read_to_end(&mut b).await?;
            Ok::<_, anyhow::Error>(b)
        };
        let operation = args.first().map(String::as_str).unwrap_or("unknown");
        let timeout = if operation == "create" {
            90
        } else if operation == "start" {
            30
        } else {
            10
        };
        let result = tokio::time::timeout(Duration::from_secs(timeout), async {
            tokio::try_join!(out, err)
        })
        .await
        .with_context(|| format!("Docker {operation} response deadline exceeded"))??;
        let status = tokio::time::timeout(Duration::from_secs(3), child.wait()).await??;
        ensure!(
            status.success(),
            "Docker operation failed: {}",
            String::from_utf8_lossy(&result.1)
        );
        Ok(result.0)
    }
    pub async fn health(&self) -> Result<Value> {
        let bytes = self
            .output(
                &["info".into(), "--format".into(), "{{json .}}".into()],
                1024 * 1024,
            )
            .await?;
        let info: Value = serde_json::from_slice(&bytes)?;
        validate_host(&info)?;
        let image = self
            .output(
                &[
                    "image".into(),
                    "inspect".into(),
                    self.image.clone(),
                    "--format".into(),
                    "{{.Id}}".into(),
                ],
                4096,
            )
            .await?;
        Ok(
            json!({"runtime":"runsc","image":self.image,"local_image_id":String::from_utf8_lossy(&image).trim(),"memory_limit_supported":info["MemoryLimit"],"cpu_quota_supported":info["CpuCfsQuota"],"security_qualified":false,"note":"Availability check only; run containment qualification before public deployment"}),
        )
    }
    pub async fn cleanup(&self, id: &str) -> Result<bool> {
        let spec = SandboxSpec::new(id, &self.image, Limits::default())?;
        let _ = self
            .output(&["rm".into(), "-f".into(), spec.name()], 4096)
            .await;
        let inventory = self
            .output(
                &[
                    "ps".into(),
                    "-a".into(),
                    "--format".into(),
                    "{{.Names}}".into(),
                ],
                65536,
            )
            .await?;
        Ok(!String::from_utf8(inventory)?
            .lines()
            .any(|name| name.trim() == spec.name()))
    }
    pub async fn recover(&self) -> Result<Vec<String>> {
        let ids = self
            .output(
                &[
                    "ps".into(),
                    "-aq".into(),
                    "--filter".into(),
                    format!("label={LABEL}"),
                ],
                65536,
            )
            .await?;
        let mut removed = Vec::new();
        for id in String::from_utf8(ids)?.split_whitespace() {
            ensure!(
                id.len() <= 64 && id.bytes().all(|b| b.is_ascii_hexdigit()),
                "bad inventory id"
            );
            self.output(&["rm".into(), "-f".into(), id.into()], 4096)
                .await?;
            removed.push(id.into());
        }
        Ok(removed)
    }
    pub async fn run(
        &self,
        id: &str,
        code: &str,
        limits: Limits,
        cancel: Arc<AtomicBool>,
    ) -> Result<Value> {
        self.health()
            .await
            .context("Host resource enforcement preflight failed")?;
        ensure!(
            !code.is_empty() && code.len() <= 32768,
            "source must be 1–32768 bytes"
        );
        let spec = SandboxSpec::new(id, &self.image, limits)?;
        let started = now();
        let clock = Instant::now();
        let mut creation_acknowledged = false;
        let result = self
            .execute(&spec, code, cancel, &mut creation_acknowledged)
            .await;
        let cleanup = self.cleanup(id).await.unwrap_or(false) && creation_acknowledged;
        if let Err(e) = &result {
            tracing::warn!(job=%id,error=%format!("{e:#}"),"sandbox operation failed");
        }
        let mut receipt = match result {
            Ok(v) => v,
            Err(_) => {
                json!({"reason":"worker_error","exit_code":null,"stdout":"","stderr":"Worker operation failed; consult trusted node logs","checks_passed":false,"artifacts":[]})
            }
        };
        receipt["job_id"] = json!(id);
        receipt["started_at"] = json!(started);
        receipt["finished_at"] = json!(now());
        receipt["duration_ms"] = json!(clock.elapsed().as_millis());
        receipt["runtime"] = json!("runsc");
        receipt["image"] = json!(self.image);
        receipt["limits"] = json!(spec.limits);
        receipt["cleanup_state"] = json!(if cleanup { "verified" } else { "failed" });
        receipt["creation_uncertain"] = json!(!creation_acknowledged);
        Ok(receipt)
    }
    async fn execute(
        &self,
        spec: &SandboxSpec,
        code: &str,
        cancel: Arc<AtomicBool>,
        creation_acknowledged: &mut bool,
    ) -> Result<Value> {
        self.output(&spec.create_args(), 4096).await?;
        *creation_acknowledged = true;
        self.output(&["start".into(), spec.name()], 4096).await?;
        let args=vec!["exec".into(),"-i".into(),spec.name(),"/bin/sh".into(),"-c".into(),"umask 077; mkdir -p /work/artifacts; cat > /work/task.rs; rustc --edition=2024 -C debuginfo=0 /work/task.rs -o /work/task && exec /work/task".into()];
        let mut child = self
            .command()
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let execution_clock = Instant::now();
        let mut stdin = child.stdin.take().context("sandbox stdin")?;
        let code = code.as_bytes().to_vec();
        let input = tokio::spawn(async move {
            stdin.write_all(&code).await?;
            stdin.shutdown().await
        });
        let total = Arc::new(AtomicUsize::new(0));
        let exceeded = Arc::new(AtomicBool::new(false));
        let stdout = tokio::spawn(capture(
            child.stdout.take().context("sandbox stdout")?,
            total.clone(),
            exceeded.clone(),
        ));
        let stderr = tokio::spawn(capture(
            child.stderr.take().context("sandbox stderr")?,
            total,
            exceeded.clone(),
        ));
        let mut stats_child = self
            .command()
            .args([
                "stats",
                "--no-trunc",
                "--format",
                "{{json .}}",
                &spec.name(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stats_stdout = stats_child.stdout.take().context("stats stdout")?;
        let stats_task = tokio::spawn(async move {
            let mut samples = Vec::new();
            let mut lines = BufReader::new(stats_stdout.take(262144)).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if samples.len() >= 128 {
                    break;
                }
                if let Ok(value) = serde_json::from_str::<Value>(&line) {
                    samples.push(json!({"at":now(),"docker_stats":value}));
                }
            }
            samples
        });
        let deadline = Instant::now() + Duration::from_secs(spec.limits.timeout_seconds as u64);
        let (exit, reason) = loop {
            if cancel.load(Ordering::SeqCst) {
                let _ = child.kill().await;
                break (None, "cancelled");
            }
            if exceeded.load(Ordering::SeqCst) {
                let _ = child.kill().await;
                break (None, "output_limit");
            }
            if Instant::now() >= deadline {
                let _ = child.kill().await;
                break (None, "timeout");
            }
            if let Some(status) = child.try_wait()? {
                break (
                    status.code(),
                    if status.success() {
                        "completed"
                    } else {
                        "execution_failed"
                    },
                );
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        };
        let execution_ms = execution_clock.elapsed().as_millis();
        input.abort();
        let _ = stats_child.kill().await;
        let samples = tokio::time::timeout(Duration::from_secs(2), stats_task)
            .await
            .ok()
            .and_then(|r| r.ok())
            .unwrap_or_default();
        if reason != "completed" && reason != "execution_failed" {
            let _ = self.output(&["kill".into(), spec.name()], 4096).await;
        }
        let (out, out_complete) = finish_capture(stdout).await;
        let (err, err_complete) = finish_capture(stderr).await;
        let artifacts = if reason == "completed" {
            // gVisor tmpfs lives in its guest filesystem and cannot be read with Docker cp.
            // A fixed read-only tar binary exports bytes; the host never extracts paths.
            let archive = self
                .output(
                    &[
                        "exec".into(),
                        spec.name(),
                        "/bin/tar".into(),
                        "-cf".into(),
                        "-".into(),
                        "-C".into(),
                        "/work/artifacts".into(),
                        ".".into(),
                    ],
                    5 * 1024 * 1024,
                )
                .await?;
            read_artifacts(&archive)?
        } else {
            Vec::new()
        };
        let result_json = artifacts
            .iter()
            .find(|a| a.name == "result.json")
            .and_then(|a| {
                base64::engine::general_purpose::STANDARD
                    .decode(&a.data_base64)
                    .ok()
            })
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
        let (reason, checks, passed) = reconcile_output(
            exit,
            reason,
            exceeded.load(Ordering::SeqCst),
            result_json.as_ref().is_some_and(Value::is_object),
            out_complete && err_complete,
        );
        Ok(
            json!({"reason":reason,"exit_code":exit,"execution_ms":execution_ms,"stdout":String::from_utf8_lossy(&out),"stderr":String::from_utf8_lossy(&err),"output_truncated":exceeded.load(Ordering::SeqCst),"checks":checks,"checks_passed":passed,"artifacts":artifacts,"result":result_json,"resource_samples":samples,"resource_sampling_note":"Observed Docker samples when available, not peak-memory attestation"}),
        )
    }
}

pub fn reconcile_output(
    exit: Option<i32>,
    reason: &str,
    overflow: bool,
    json_valid: bool,
    capture_complete: bool,
) -> (String, Vec<Value>, bool) {
    let reason = if overflow { "output_limit" } else { reason };
    let checks = vec![
        json!({"name":"process_exit_zero","passed":exit==Some(0)}),
        json!({"name":"result_json_is_object","passed":json_valid}),
        json!({"name":"output_within_limit","passed":!overflow}),
        json!({"name":"output_capture_complete","passed":capture_complete}),
    ];
    let passed = reason == "completed" && checks.iter().all(|c| c["passed"] == true);
    (reason.into(), checks, passed)
}
async fn finish_capture(mut task: tokio::task::JoinHandle<Vec<u8>>) -> (Vec<u8>, bool) {
    match tokio::time::timeout(Duration::from_secs(2), &mut task).await {
        Ok(Ok(bytes)) => (bytes, true),
        _ => {
            task.abort();
            (Vec::new(), false)
        }
    }
}
async fn capture<R: AsyncRead + Unpin>(
    mut reader: R,
    total: Arc<AtomicUsize>,
    exceeded: Arc<AtomicBool>,
) -> Vec<u8> {
    let mut result = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match reader.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let previous = total.fetch_add(n, Ordering::SeqCst);
                let allowed = OUTPUT_LIMIT.saturating_sub(previous).min(n);
                result.extend_from_slice(&buf[..allowed]);
                if previous + n > OUTPUT_LIMIT {
                    exceeded.store(true, Ordering::SeqCst);
                    break;
                }
            }
        }
    }
    result
}
