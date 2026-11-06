//! Session workspace boxes on the worker: one long-lived gVisor container per agent
//! session, with a named volume at /workspace. Implements the `sandbox_api` contract;
//! `node_router` serves it at `/v1/boxes…` behind the worker's bearer and mTLS.
//!
//! Every command runs as uid 1000 inside the box. Paths are checked twice: lexically here,
//! then again inside the box after `realpath -m` has resolved symlinks, so neither `..`
//! nor a symlink can lead a file call outside /workspace or /tmp.
use crate::{
    domain::now,
    sandbox_api::{
        BoxClient, BoxInfo, BoxSpec, DEFAULT_EXEC_TIMEOUT_SECONDS, DirEntry, ExecRequest,
        ExecResult, FileRead, MAX_EXEC_OUTPUT_BYTES, MAX_EXEC_TIMEOUT_SECONDS, MAX_FILE_BYTES,
        WriteResult,
    },
    worker::{DockerRunner, cgroup_dir, cgroup_value, next_cpu_millis, next_memory_mb},
};
use anyhow::{Context, Result, ensure};
use futures_util::future::BoxFuture;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const DEFAULT_NETWORK: &str = "ar-box";
pub const DEFAULT_IDLE_SECONDS: u64 = 1800;
pub const DEFAULT_CEILING_MB: u32 = 4096;
pub const CPU_CEILING_MILLIS: u32 = 2000;
pub const MAX_BOXES: usize = 8;
/// Leases in the worker's shared reservation map that belong to boxes, not one-shot runs.
pub const LEASE_PREFIX: &str = "box:";
const LABEL: &str = "agent-rush.box=true";
const BOX_UID: &str = "1000:1000";
const MAX_COMMAND_BYTES: usize = 256 * 1024;
const MAX_LISTING: usize = 5000;
/// Public resolvers. Docker's embedded DNS (127.0.0.11) is unreachable from gVisor's own
/// network stack, so each box gets this file read-only at /etc/resolv.conf instead.
const RESOLV_CONF: &str = "nameserver 1.1.1.1\nnameserver 8.8.8.8\noptions timeout:2 attempts:2\n";

/// Failures callers act on; the node router maps each to an HTTP status.
#[derive(Debug, thiserror::Error)]
pub enum BoxError {
    /// The box does not exist (never created, destroyed, reaped, or lost with its container).
    #[error("box not found")]
    NotFound,
    /// The box exists but the path in it does not.
    #[error("{0}")]
    NoSuchPath(String),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    TooLarge(String),
    #[error("worker box capacity exhausted")]
    Capacity,
    #[error("{0}")]
    Unavailable(String),
}
fn invalid(message: impl Into<String>) -> BoxError {
    BoxError::Invalid(message.into())
}

/// Normalize a box path: relative paths are taken from /workspace, `.` and `..` are
/// resolved lexically, and the result must stay under /workspace or /tmp.
pub fn resolve_path(p: &str) -> Result<String> {
    ensure!(!p.contains('\0'), invalid("path contains a NUL byte"));
    ensure!(p.len() <= 4096, invalid("path is too long"));
    let joined = if p.starts_with('/') {
        p.to_string()
    } else {
        format!("/workspace/{p}")
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(invalid(format!("path {p:?} leaves the filesystem root")).into());
                }
            }
            part => parts.push(part),
        }
    }
    let path = format!("/{}", parts.join("/"));
    ensure!(
        allowed(&path),
        invalid(format!(
            "path {p:?} resolves to {path}, outside /workspace and /tmp"
        ))
    );
    Ok(path)
}
fn allowed(path: &str) -> bool {
    ["/workspace", "/tmp"]
        .iter()
        .any(|root| path == *root || path.strip_prefix(root).is_some_and(|r| r.starts_with('/')))
}

/// Lossy UTF-8 text of at most `max` bytes, cut on a character boundary. The flag says
/// whether anything was dropped. Binary input becomes replacement characters, never a panic.
pub fn truncate_utf8(bytes: &[u8], max: usize) -> (String, bool) {
    let mut out = String::with_capacity(max.min(bytes.len()));
    for chunk in bytes.utf8_chunks() {
        for c in chunk.valid().chars() {
            if out.len() + c.len_utf8() > max {
                return (out, true);
            }
            out.push(c);
        }
        if !chunk.invalid().is_empty() {
            if out.len() + char::REPLACEMENT_CHARACTER.len_utf8() > max {
                return (out, true);
            }
            out.push(char::REPLACEMENT_CHARACTER);
        }
    }
    (out, false)
}

pub fn container_name(box_id: &str) -> String {
    format!("arbox-{box_id}")
}

/// `docker run` arguments for a box. The resolver file is mounted separately.
pub fn docker_create_args(spec: &BoxSpec, image: &str, network: &str, box_id: &str) -> Vec<String> {
    let name = container_name(box_id);
    [
        "run".into(),
        "-d".into(),
        "--pull=never".into(),
        format!("--name={name}"),
        "--hostname=box".into(),
        format!("--label={LABEL}"),
        format!("--label=agent-rush.box.id={box_id}"),
        format!("--label=agent-rush.box.session={}", spec.session_id),
        format!("--label=agent-rush.box.owner={}", spec.owner),
        format!(
            "--label=agent-rush.box.created={}",
            chrono::Utc::now().timestamp()
        ),
        "--runtime=runsc".into(),
        format!("--network={network}"),
        "--read-only".into(),
        "--cap-drop=ALL".into(),
        "--security-opt=no-new-privileges".into(),
        "--pids-limit=512".into(),
        format!("--user={BOX_UID}"),
        format!("--memory={}m", spec.memory_mb),
        format!("--memory-swap={}m", spec.memory_mb),
        format!("--cpus={:.3}", spec.cpu_millis as f64 / 1000.0),
        "--cpu-shares=512".into(),
        "--ulimit=nofile=4096:4096".into(),
        "--log-driver=none".into(),
        "--init".into(),
        "--tmpfs=/tmp:rw,nosuid,nodev,size=1024m,mode=1777".into(),
        "--tmpfs=/home/agent:rw,nosuid,nodev,size=1024m,uid=1000,gid=1000,mode=0755".into(),
        "--workdir=/workspace".into(),
        "-v".into(),
        format!("{name}:/workspace"),
        image.into(),
        "sleep".into(),
        "infinity".into(),
    ]
    .into()
}

fn validate_spec(spec: &BoxSpec, ceiling_mb: u32) -> Result<()> {
    let label_safe = |v: &str, max: usize| {
        !v.is_empty()
            && v.len() <= max
            && v.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.:@+".contains(&b))
    };
    ensure!(
        label_safe(&spec.session_id, 128),
        invalid("session_id must be 1–128 characters of letters, digits and -_.:@+")
    );
    ensure!(
        label_safe(&spec.owner, 256),
        invalid("owner must be 1–256 characters of letters, digits and -_.:@+")
    );
    ensure!(
        (256..=ceiling_mb).contains(&spec.memory_mb),
        invalid(format!("memory_mb must be 256–{ceiling_mb}"))
    );
    ensure!(
        (100..=CPU_CEILING_MILLIS).contains(&spec.cpu_millis),
        invalid(format!("cpu_millis must be 100–{CPU_CEILING_MILLIS}"))
    );
    Ok(())
}
fn valid_box_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Runs a command, then prints `AREXEC <rc> <timed_out> <stdout bytes> <stderr bytes>`
/// and at most `$2` bytes of each stream. Output goes to files first, so a server left
/// running in the background cannot hold the exec open; `timeout` kills the command's
/// process group, and the box itself bounds /tmp.
const EXEC_SCRIPT: &str = r#"d=$(mktemp -d /tmp/.arexec.XXXXXX) || exit 125
s=$(date +%s)
timeout -k 5 "$1" bash -lc "$3" >"$d/o" 2>"$d/e"
rc=$?
to=0
if [ "$rc" -eq 124 ] || [ "$rc" -eq 137 ]; then [ $(( $(date +%s) - s )) -ge "$1" ] && to=1; fi
printf 'AREXEC %s %s %s %s\n' "$rc" "$to" "$(stat -c %s "$d/o")" "$(stat -c %s "$d/e")"
head -c "$2" "$d/o"
head -c "$2" "$d/e"
rm -rf "$d""#;
/// Shared prologue of the file scripts: resolve symlinks inside the box, then refuse any
/// result outside /workspace and /tmp (exit 3).
const RESOLVE: &str = r#"p=$(realpath -m -- "$1") || exit 3
case "$p" in /workspace|/workspace/*|/tmp|/tmp/*) ;; *) printf 'path resolves to %s, outside /workspace and /tmp' "$p" >&2; exit 3 ;; esac
"#;
const READ_SCRIPT: &str = r#"if [ -d "$p" ]; then
  printf 'ARDIR %s\0' "$p"
  find "$p" -mindepth 1 -maxdepth 1 -printf '%y\t%s\t%f\0' | head -z -n "$3"
  exit 0
fi
[ -e "$p" ] || { printf 'no such file %s' "$p" >&2; exit 4; }
[ -f "$p" ] || { printf '%s is not a regular file' "$p" >&2; exit 5; }
s=$(stat -c %s -- "$p")
[ "$s" -le "$2" ] || { printf '%s is %s bytes, over the %s byte limit' "$p" "$s" "$2" >&2; exit 6; }
printf 'ARFILE %s %s\0' "$s" "$p"
exec cat -- "$p""#;
const WRITE_SCRIPT: &str = r#"[ ! -d "$p" ] || { printf '%s is a directory' "$p" >&2; exit 5; }
mkdir -p -- "$(dirname -- "$p")" || exit 7
cat > "$p" || exit 7
printf 'ARWROTE %s %s\0' "$(stat -c %s -- "$p")" "$p""#;

pub struct BoxConfig {
    /// Digest-pinned workspace image.
    pub image: String,
    pub network: String,
    /// Box qualification report; creation is refused unless it is fresh and passing.
    pub report: PathBuf,
    /// Holds the resolver file mounted into every box.
    pub state_dir: PathBuf,
    pub idle_seconds: u64,
    pub ceiling_mb: u32,
}

/// Memory reservations shared by one-shot runs and boxes, keyed by run id or
/// `box:<id>`. Runs reserve their ceiling; boxes reserve their current limit.
pub(crate) type Leases = Arc<Mutex<HashMap<String, crate::node::ActiveLease>>>;

struct Entry {
    info: BoxInfo,
    container_id: String,
    memory_mb: u32,
    cpu_millis: u32,
    busy: usize,
    scaler: Option<tokio::task::JoinHandle<()>>,
}

#[derive(Clone)]
pub struct BoxManager {
    inner: Arc<Inner>,
}
struct Inner {
    runner: DockerRunner,
    config: BoxConfig,
    resolv: PathBuf,
    boxes: Mutex<HashMap<String, Entry>>,
    leases: Leases,
    capacity_mb: u32,
}

struct DockerOutput {
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: String,
    timed_out: bool,
}

impl BoxManager {
    /// A manager with its own reservations, for `qualify-box` and tests.
    pub fn new(runner: DockerRunner, config: BoxConfig, capacity_mb: u32) -> Result<Self> {
        Self::with_leases(runner, config, Arc::default(), capacity_mb)
    }
    pub(crate) fn with_leases(
        runner: DockerRunner,
        config: BoxConfig,
        leases: Leases,
        capacity_mb: u32,
    ) -> Result<Self> {
        let (_, digest) = config
            .image
            .split_once("@sha256:")
            .context("workspace image must be digest pinned")?;
        ensure!(
            digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid workspace image digest"
        );
        ensure!(
            !config.network.is_empty()
                && config
                    .network
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)),
            "invalid box network name"
        );
        ensure!(
            (1024..=65536).contains(&config.ceiling_mb),
            "box memory ceiling must be 1024–65536 MiB"
        );
        std::fs::create_dir_all(&config.state_dir)?;
        let resolv = config.state_dir.join("resolv.conf");
        std::fs::write(&resolv, RESOLV_CONF)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&resolv, std::fs::Permissions::from_mode(0o644))?;
        }
        Ok(Self {
            inner: Arc::new(Inner {
                runner,
                config,
                resolv,
                boxes: Mutex::new(HashMap::new()),
                leases,
                capacity_mb,
            }),
        })
    }
    pub fn image(&self) -> &str {
        &self.inner.config.image
    }
    pub fn network(&self) -> &str {
        &self.inner.config.network
    }
    pub(crate) async fn network_gateway(&self) -> Result<String> {
        let out = self
            .docker_ok(
                &[
                    "network".into(),
                    "inspect".into(),
                    "--format={{range .IPAM.Config}}{{.Gateway}} {{end}}".into(),
                    self.inner.config.network.clone(),
                ],
                Duration::from_secs(15),
            )
            .await
            .context("box network missing; run deploy/box-network.sh")?;
        let gateway = String::from_utf8_lossy(&out)
            .split_whitespace()
            .find(|g| g.parse::<std::net::Ipv4Addr>().is_ok())
            .map(String::from)
            .context("box network has no IPv4 gateway")?;
        Ok(gateway)
    }
    pub fn count(&self) -> usize {
        self.inner.boxes.lock().unwrap().len()
    }

    async fn docker(
        &self,
        args: &[String],
        stdin: Option<&[u8]>,
        limit: usize,
        timeout: Duration,
    ) -> Result<DockerOutput> {
        let mut child = self
            .inner
            .runner
            .command()
            .args(args)
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let writer = match (stdin, child.stdin.take()) {
            (Some(bytes), Some(mut pipe)) => {
                let bytes = bytes.to_vec();
                Some(tokio::spawn(async move {
                    let _ = pipe.write_all(&bytes).await;
                    let _ = pipe.shutdown().await;
                }))
            }
            _ => None,
        };
        let mut stdout = child.stdout.take().context("docker stdout")?;
        let mut stderr = child.stderr.take().context("docker stderr")?;
        let read = async {
            let mut out = Vec::new();
            let mut err = Vec::new();
            let mut out_reader = (&mut stdout).take(limit as u64);
            let mut err_reader = (&mut stderr).take(65536);
            let (a, b) = tokio::join!(
                out_reader.read_to_end(&mut out),
                err_reader.read_to_end(&mut err)
            );
            a?;
            b?;
            Ok::<_, std::io::Error>((out, err))
        };
        let (stdout, stderr, timed_out) = match tokio::time::timeout(timeout, read).await {
            Ok(result) => {
                let (out, err) = result?;
                (out, err, false)
            }
            Err(_) => {
                let _ = child.kill().await;
                (Vec::new(), Vec::new(), true)
            }
        };
        if let Some(writer) = writer {
            writer.abort();
        }
        let code = match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
            Ok(status) => status?.code(),
            Err(_) => {
                let _ = child.kill().await;
                None
            }
        };
        Ok(DockerOutput {
            code,
            stdout,
            stderr: String::from_utf8_lossy(&stderr).trim().to_string(),
            timed_out,
        })
    }
    async fn docker_ok(&self, args: &[String], timeout: Duration) -> Result<Vec<u8>> {
        let out = self.docker(args, None, 1024 * 1024, timeout).await?;
        ensure!(
            !out.timed_out,
            "docker {} timed out",
            args.first().map(String::as_str).unwrap_or("")
        );
        ensure!(
            out.code == Some(0),
            "docker {} failed: {}",
            args.first().map(String::as_str).unwrap_or(""),
            out.stderr
        );
        Ok(out.stdout)
    }

    fn qualification_gate(&self) -> Result<()> {
        let report =
            crate::qualification::BoxReport::load(&self.inner.config.report).and_then(|report| {
                report.assess(
                    &crate::qualification::box_host_id()?,
                    &self.inner.config.image,
                    chrono::Utc::now().timestamp(),
                )
            });
        report.map_err(|e| {
            tracing::warn!(error=%format!("{e:#}"), "box creation refused: qualification");
            BoxError::Unavailable(
                "Session boxes are not qualified on this worker; run agent-rush qualify-box".into(),
            )
            .into()
        })
    }

    /// Reserve capacity, then create the volume and start the box. Refused unless the box
    /// qualification report is fresh and passing.
    pub async fn create(&self, spec: &BoxSpec) -> Result<String> {
        validate_spec(spec, self.inner.config.ceiling_mb)?;
        self.qualification_gate()?;
        self.start_box(spec).await
    }
    /// Used only by `qualify-box`, which has to start a box before any report exists.
    pub(crate) async fn create_for_qualification(&self, spec: &BoxSpec) -> Result<String> {
        validate_spec(spec, self.inner.config.ceiling_mb)?;
        self.start_box(spec).await
    }
    async fn start_box(&self, spec: &BoxSpec) -> Result<String> {
        let box_id = uuid::Uuid::new_v4().simple().to_string();
        let lease = format!("{LEASE_PREFIX}{box_id}");
        {
            let mut leases = self.inner.leases.lock().unwrap();
            let boxes = leases
                .keys()
                .filter(|k| k.starts_with(LEASE_PREFIX))
                .count();
            let used: u32 = leases.values().map(|l| l.memory_mb).sum();
            if boxes >= MAX_BOXES || used.saturating_add(spec.memory_mb) > self.inner.capacity_mb {
                return Err(BoxError::Capacity.into());
            }
            leases.insert(lease.clone(), crate::node::ActiveLease::new(spec.memory_mb));
        }
        match self.launch(spec, &box_id).await {
            Ok(container_id) => {
                let at = chrono::Utc::now().timestamp();
                let entry = Entry {
                    info: BoxInfo {
                        box_id: box_id.clone(),
                        session_id: spec.session_id.clone(),
                        owner: spec.owner.clone(),
                        created_at: at,
                        last_used_at: at,
                    },
                    container_id: container_id.clone(),
                    memory_mb: spec.memory_mb,
                    cpu_millis: spec.cpu_millis,
                    busy: 0,
                    scaler: None,
                };
                self.inner
                    .boxes
                    .lock()
                    .unwrap()
                    .insert(box_id.clone(), entry);
                self.start_scaler(&box_id, &container_id);
                tracing::info!(box_id=%box_id, session=%spec.session_id, "box created");
                Ok(box_id)
            }
            Err(e) => {
                let _ = self.remove_container(&box_id).await;
                self.inner.leases.lock().unwrap().remove(&lease);
                Err(e)
            }
        }
    }
    async fn launch(&self, spec: &BoxSpec, box_id: &str) -> Result<String> {
        let name = container_name(box_id);
        self.docker_ok(
            &[
                "volume".into(),
                "create".into(),
                format!("--label={LABEL}"),
                format!("--label=agent-rush.box.id={box_id}"),
                name.clone(),
            ],
            Duration::from_secs(30),
        )
        .await?;
        let mut args = docker_create_args(
            spec,
            &self.inner.config.image,
            &self.inner.config.network,
            box_id,
        );
        args.insert(
            1,
            format!(
                "--volume={}:/etc/resolv.conf:ro",
                self.inner.resolv.display()
            ),
        );
        let out = self.docker_ok(&args, Duration::from_secs(120)).await?;
        let id = String::from_utf8_lossy(&out).trim().to_string();
        ensure!(
            id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()),
            "unexpected container id from docker run"
        );
        Ok(id)
    }
    /// Remove a box's container and volume. Returns whether each is now absent.
    async fn remove_container(&self, box_id: &str) -> (bool, bool) {
        let name = container_name(box_id);
        let _ = self
            .docker(
                &["rm".into(), "-f".into(), name.clone()],
                None,
                4096,
                Duration::from_secs(60),
            )
            .await;
        let _ = self
            .docker(
                &["volume".into(), "rm".into(), "-f".into(), name.clone()],
                None,
                4096,
                Duration::from_secs(60),
            )
            .await;
        let absent = |args: Vec<String>| async move {
            self.docker(&args, None, 65536, Duration::from_secs(15))
                .await
                .is_ok_and(|o| !o.timed_out && o.code.is_some_and(|c| c != 0))
        };
        let container_gone = absent(vec![
            "inspect".into(),
            "--type=container".into(),
            name.clone(),
        ])
        .await;
        let volume_gone = absent(vec!["volume".into(), "inspect".into(), name]).await;
        (container_gone, volume_gone)
    }
    /// Destroy a box and its volume; idempotent. Returns (container removed, volume removed).
    pub async fn destroy_box(&self, box_id: &str) -> Result<(bool, bool)> {
        ensure!(valid_box_id(box_id), BoxError::NotFound);
        let entry = self.inner.boxes.lock().unwrap().remove(box_id);
        if let Some(scaler) = entry.and_then(|e| e.scaler) {
            scaler.abort();
        }
        let (container, volume) = self.remove_container(box_id).await;
        if container {
            self.inner
                .leases
                .lock()
                .unwrap()
                .remove(&format!("{LEASE_PREFIX}{box_id}"));
        }
        tracing::info!(box_id=%box_id, container_removed=container, volume_removed=volume, "box destroyed");
        Ok((container, volume))
    }
    /// Bring the map in line with Docker: a stopped box (an out-of-memory kill, say) is
    /// restarted, and a box whose container is gone or will not start is removed, so
    /// `list` never reports a box that cannot run commands.
    pub async fn reconcile(&self) -> Result<()> {
        // Only boxes known before asking Docker can be judged; one created meanwhile is not
        // yet in the answer and must not be mistaken for lost.
        let known: Vec<(String, String)> = self
            .inner
            .boxes
            .lock()
            .unwrap()
            .iter()
            .map(|(id, e)| (id.clone(), e.container_id.clone()))
            .collect();
        let out = self
            .docker_ok(
                &[
                    "ps".into(),
                    "-q".into(),
                    "--no-trunc".into(),
                    "--filter".into(),
                    format!("label={LABEL}"),
                    "--filter".into(),
                    "status=running".into(),
                ],
                Duration::from_secs(30),
            )
            .await?;
        let running = String::from_utf8_lossy(&out).to_string();
        let lost = known
            .into_iter()
            .filter(|(_, container)| !running.split_whitespace().any(|id| id == container));
        for (id, _) in lost {
            if !self.inner.boxes.lock().unwrap().contains_key(&id) {
                continue;
            }
            let restarted = self
                .docker_ok(
                    &["start".into(), container_name(&id)],
                    Duration::from_secs(60),
                )
                .await
                .is_ok();
            if restarted {
                tracing::warn!(box_id=%id, "box was stopped; restarted it");
            } else {
                tracing::warn!(box_id=%id, "box lost; removing its remains");
                let _ = self.destroy_box(&id).await;
            }
        }
        Ok(())
    }
    pub fn list_boxes(&self) -> Vec<BoxInfo> {
        let mut list: Vec<BoxInfo> = self
            .inner
            .boxes
            .lock()
            .unwrap()
            .values()
            .map(|e| e.info.clone())
            .collect();
        list.sort_by_key(|b| b.created_at);
        list
    }

    /// Mark a box in use; the guard keeps the reaper away until the call finishes.
    fn enter(&self, box_id: &str) -> Result<Busy> {
        let mut boxes = self.inner.boxes.lock().unwrap();
        let entry = boxes.get_mut(box_id).ok_or(BoxError::NotFound)?;
        entry.busy += 1;
        entry.info.last_used_at = chrono::Utc::now().timestamp();
        Ok(Busy {
            manager: self.clone(),
            box_id: box_id.into(),
        })
    }
    /// Run a script in the box as the box user. A stopped box (an out-of-memory kill, say)
    /// is restarted once; a box whose container is gone is forgotten and reported missing.
    async fn box_exec(
        &self,
        box_id: &str,
        script_args: &[String],
        cwd: &str,
        stdin: Option<&[u8]>,
        limit: usize,
        timeout: Duration,
    ) -> Result<DockerOutput> {
        let mut args: Vec<String> = vec!["exec".into()];
        if stdin.is_some() {
            args.push("-i".into());
        }
        args.extend([
            "-u".into(),
            BOX_UID.into(),
            "-w".into(),
            cwd.into(),
            container_name(box_id),
        ]);
        args.extend(script_args.iter().cloned());
        for attempt in 0..2 {
            let out = self.docker(&args, stdin, limit, timeout).await?;
            // Only Docker's own errors count: a file script's stderr can quote an agent's
            // path, but always starts with that path or with fixed text of our own.
            let daemon = out.stderr.starts_with("Error response from daemon");
            let missing = daemon && out.stderr.contains("No such container");
            let stopped = daemon && out.stderr.contains("is not running");
            if attempt == 0 && stopped && !missing {
                tracing::warn!(box_id=%box_id, "box was stopped; restarting it");
                if self
                    .docker_ok(
                        &["start".into(), container_name(box_id)],
                        Duration::from_secs(60),
                    )
                    .await
                    .is_ok()
                {
                    continue;
                }
            }
            if missing || stopped {
                tracing::warn!(box_id=%box_id, "box lost; removing its remains");
                let _ = self.destroy_box(box_id).await;
                return Err(BoxError::NotFound.into());
            }
            return Ok(out);
        }
        unreachable!("the loop returns on its second attempt")
    }

    pub async fn exec_box(&self, box_id: &str, request: &ExecRequest) -> Result<ExecResult> {
        ensure!(
            !request.command.trim().is_empty() && request.command.len() <= MAX_COMMAND_BYTES,
            invalid("command must be 1–262144 bytes")
        );
        ensure!(
            request
                .stdin
                .as_ref()
                .is_none_or(|s| s.len() <= MAX_FILE_BYTES),
            BoxError::TooLarge("stdin exceeds 25 MiB".into())
        );
        let cwd = resolve_path(request.cwd.as_deref().unwrap_or("/workspace"))?;
        let seconds = request
            .timeout_seconds
            .unwrap_or(DEFAULT_EXEC_TIMEOUT_SECONDS)
            .clamp(1, MAX_EXEC_TIMEOUT_SECONDS);
        let _busy = self.enter(box_id)?;
        let script = vec![
            "bash".into(),
            "-c".into(),
            EXEC_SCRIPT.into(),
            "arexec".into(),
            seconds.to_string(),
            MAX_EXEC_OUTPUT_BYTES.to_string(),
            request.command.clone(),
        ];
        let stdin = request.stdin.as_deref().unwrap_or("").as_bytes();
        let out = self
            .box_exec(
                box_id,
                &script,
                &cwd,
                Some(stdin),
                2 * MAX_EXEC_OUTPUT_BYTES + 256,
                Duration::from_secs(seconds as u64 + 20),
            )
            .await?;
        if out.timed_out {
            // The in-box `timeout` should have fired first; the box is unresponsive.
            return Ok(ExecResult {
                exit_code: -1,
                stdout: String::new(),
                stderr: "command did not finish; the box stopped responding".into(),
                timed_out: true,
                truncated: false,
            });
        }
        let Some((header, body)) = split_header(&out.stdout, b'\n') else {
            if out.stderr.contains("no such file or directory") && out.stderr.contains("chdir") {
                return Err(invalid(format!("working directory {cwd} does not exist")).into());
            }
            anyhow::bail!("box exec failed: {}", out.stderr);
        };
        let fields: Vec<&str> = header.split(' ').collect();
        let [tag, rc, to, olen, elen] = fields[..] else {
            anyhow::bail!("box exec returned a malformed header");
        };
        ensure!(tag == "AREXEC", "box exec returned a malformed header");
        let (rc, olen, elen): (i64, usize, usize) = (rc.parse()?, olen.parse()?, elen.parse()?);
        let kept_out = olen.min(MAX_EXEC_OUTPUT_BYTES).min(body.len());
        let kept_err = elen.min(MAX_EXEC_OUTPUT_BYTES).min(body.len() - kept_out);
        let (stdout, cut_out) = truncate_utf8(&body[..kept_out], MAX_EXEC_OUTPUT_BYTES);
        let (stderr, cut_err) =
            truncate_utf8(&body[kept_out..kept_out + kept_err], MAX_EXEC_OUTPUT_BYTES);
        Ok(ExecResult {
            exit_code: rc,
            stdout,
            stderr,
            timed_out: to == "1",
            truncated: cut_out
                || cut_err
                || olen > MAX_EXEC_OUTPUT_BYTES
                || elen > MAX_EXEC_OUTPUT_BYTES,
        })
    }

    async fn file_script(
        &self,
        box_id: &str,
        script: &str,
        path: &str,
        extra: &[String],
        stdin: Option<&[u8]>,
        limit: usize,
    ) -> Result<(String, Vec<u8>)> {
        let path = resolve_path(path)?;
        let mut args = vec![
            "bash".into(),
            "-c".into(),
            format!("{RESOLVE}{script}"),
            "arfile".into(),
            path,
        ];
        args.extend(extra.iter().cloned());
        let out = self
            .box_exec(
                box_id,
                &args,
                "/workspace",
                stdin,
                limit,
                Duration::from_secs(120),
            )
            .await?;
        ensure!(!out.timed_out, "box file operation timed out");
        match out.code {
            Some(0) => {}
            Some(3) | Some(5) => return Err(invalid(out.stderr).into()),
            Some(4) => return Err(BoxError::NoSuchPath(out.stderr).into()),
            Some(6) => return Err(BoxError::TooLarge(out.stderr).into()),
            _ => anyhow::bail!("box file operation failed: {}", out.stderr),
        }
        let (header, body) =
            split_header(&out.stdout, 0).context("box file operation returned no header")?;
        Ok((header.to_string(), body.to_vec()))
    }

    pub async fn read_box_file(&self, box_id: &str, path: &str) -> Result<FileRead> {
        let _busy = self.enter(box_id)?;
        let (header, body) = self
            .file_script(
                box_id,
                READ_SCRIPT,
                path,
                &[MAX_FILE_BYTES.to_string(), MAX_LISTING.to_string()],
                None,
                MAX_FILE_BYTES + 4096,
            )
            .await?;
        if let Some(dir) = header.strip_prefix("ARDIR ") {
            recheck(dir)?;
            let mut entries: Vec<DirEntry> = body
                .split(|b| *b == 0)
                .filter(|r| !r.is_empty())
                .filter_map(|record| {
                    let text = String::from_utf8_lossy(record);
                    let mut fields = text.splitn(3, '\t');
                    let (kind, size, name) = (fields.next()?, fields.next()?, fields.next()?);
                    Some(DirEntry {
                        name: name.into(),
                        is_dir: kind == "d",
                        size: size.parse().unwrap_or(0),
                    })
                })
                .collect();
            entries.sort_by(|a, b| a.name.cmp(&b.name));
            return Ok(FileRead::Dir(entries));
        }
        let rest = header
            .strip_prefix("ARFILE ")
            .context("box file operation returned a malformed header")?;
        let (size, resolved) = rest
            .split_once(' ')
            .context("box file operation returned a malformed header")?;
        recheck(resolved)?;
        ensure!(
            size.parse::<usize>().ok() == Some(body.len()),
            "file changed while it was read; try again"
        );
        Ok(FileRead::File(body))
    }

    pub async fn write_box_file(
        &self,
        box_id: &str,
        path: &str,
        bytes: &[u8],
    ) -> Result<WriteResult> {
        ensure!(
            bytes.len() <= MAX_FILE_BYTES,
            BoxError::TooLarge("file exceeds 25 MiB".into())
        );
        let _busy = self.enter(box_id)?;
        let (header, _) = self
            .file_script(box_id, WRITE_SCRIPT, path, &[], Some(bytes), 4096)
            .await?;
        let rest = header
            .strip_prefix("ARWROTE ")
            .context("box write returned a malformed header")?;
        let (size, resolved) = rest
            .split_once(' ')
            .context("box write returned a malformed header")?;
        recheck(resolved)?;
        ensure!(
            size.parse::<usize>().ok() == Some(bytes.len()),
            "box write was incomplete (disk full?)"
        );
        Ok(WriteResult {
            bytes: bytes.len() as u64,
            sha256: hex::encode(Sha256::digest(bytes)),
        })
    }

    /// Live scaling: start small and grow memory and CPU under measured pressure, up to the
    /// operator ceiling, as long as the worker's shared reservations still fit.
    fn start_scaler(&self, box_id: &str, container_id: &str) {
        let Some(dir) = cgroup_dir(container_id) else {
            tracing::warn!(box_id=%box_id, "box cgroup not visible; live scaling off");
            return;
        };
        let manager = self.clone();
        let id = box_id.to_string();
        let task = tokio::spawn(async move {
            let lease = format!("{LEASE_PREFIX}{id}");
            let mut window = (
                Instant::now(),
                cgroup_value(&dir, "cpu.stat", Some("usage_usec")),
            );
            let mut full_logged = false;
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let Some((memory, cpu)) = manager
                    .inner
                    .boxes
                    .lock()
                    .unwrap()
                    .get(&id)
                    .map(|e| (e.memory_mb, e.cpu_millis))
                else {
                    return;
                };
                let mut target = (memory, cpu);
                if let Some(used) = cgroup_value(&dir, "memory.current", None)
                    && let Some(next) =
                        next_memory_mb(memory, used, manager.inner.config.ceiling_mb)
                {
                    let mut leases = manager.inner.leases.lock().unwrap();
                    let total: u32 = leases.values().map(|l| l.memory_mb).sum();
                    if total - memory.min(total) + next <= manager.inner.capacity_mb {
                        if let Some(l) = leases.get_mut(&lease) {
                            l.memory_mb = next;
                        }
                        target.0 = next;
                    } else if !full_logged {
                        full_logged = true;
                        tracing::warn!(box_id=%id, "box memory growth held: worker capacity full");
                    }
                }
                if window.0.elapsed() >= Duration::from_secs(2) {
                    let usage = cgroup_value(&dir, "cpu.stat", Some("usage_usec"));
                    if let (Some(before), Some(after)) = (window.1, usage)
                        && let Some(next) = next_cpu_millis(
                            cpu,
                            after.saturating_sub(before),
                            window.0.elapsed().as_micros() as u64,
                            CPU_CEILING_MILLIS,
                        )
                    {
                        target.1 = next;
                    }
                    window = (Instant::now(), usage);
                }
                if target == (memory, cpu) {
                    continue;
                }
                let updated = manager
                    .docker_ok(
                        &[
                            "update".into(),
                            format!("--memory={}m", target.0),
                            format!("--memory-swap={}m", target.0),
                            format!("--cpus={:.3}", target.1 as f64 / 1000.0),
                            container_name(&id),
                        ],
                        Duration::from_secs(15),
                    )
                    .await;
                let mut boxes = manager.inner.boxes.lock().unwrap();
                match (updated, boxes.get_mut(&id)) {
                    (Ok(_), Some(entry)) => {
                        (entry.memory_mb, entry.cpu_millis) = target;
                        tracing::info!(box_id=%id, memory_mb=target.0, cpu_millis=target.1, "box scaled up");
                    }
                    (Err(e), _) => {
                        tracing::warn!(box_id=%id, error=%e, "box scale-up failed");
                        if let Some(l) = manager.inner.leases.lock().unwrap().get_mut(&lease) {
                            l.memory_mb = memory;
                        }
                    }
                    (Ok(_), None) => return,
                }
            }
        });
        if let Some(entry) = self.inner.boxes.lock().unwrap().get_mut(box_id) {
            entry.scaler = Some(task);
        } else {
            task.abort();
        }
    }

    /// Every 30 s, forever: reconcile with Docker, then destroy boxes with no call for
    /// `idle_seconds`.
    pub fn start_reaper(&self) -> tokio::task::JoinHandle<()> {
        let manager = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                if let Err(e) = manager.reconcile().await {
                    tracing::warn!(error=%e, "box reconciliation failed");
                }
                for id in manager.idle_boxes(chrono::Utc::now().timestamp()) {
                    tracing::info!(box_id=%id, "reaping idle box");
                    if let Err(e) = manager.destroy_box(&id).await {
                        tracing::warn!(box_id=%id, error=%e, "idle box not reaped");
                    }
                }
            }
        })
    }
    fn idle_boxes(&self, at: i64) -> Vec<String> {
        let idle = self.inner.config.idle_seconds as i64;
        self.inner
            .boxes
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, e)| e.busy == 0 && at - e.info.last_used_at >= idle)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// At worker start: adopt running boxes left by a previous worker process, so sessions
    /// keep their workspaces across a restart, and remove stopped boxes and stray volumes.
    pub async fn recover(&self) -> Result<Value> {
        let ids = self
            .docker_ok(
                &[
                    "ps".into(),
                    "-aq".into(),
                    "--no-trunc".into(),
                    "--filter".into(),
                    format!("label={LABEL}"),
                ],
                Duration::from_secs(30),
            )
            .await?;
        let (mut adopted, mut removed) = (Vec::new(), Vec::new());
        for id in String::from_utf8(ids)?.split_whitespace() {
            ensure!(
                id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()),
                "bad inventory id"
            );
            let raw = self
                .docker_ok(
                    &["inspect".into(), "--format={{json .}}".into(), id.into()],
                    Duration::from_secs(30),
                )
                .await?;
            let c: Value = serde_json::from_slice(&raw)?;
            let labels = &c["Config"]["Labels"];
            let box_id = labels["agent-rush.box.id"]
                .as_str()
                .unwrap_or("")
                .to_string();
            if !valid_box_id(&box_id) {
                continue;
            }
            if c["State"]["Running"] != true {
                self.remove_container(&box_id).await;
                removed.push(box_id);
                continue;
            }
            let memory_mb = (c["HostConfig"]["Memory"].as_u64().unwrap_or(0) / 1_048_576) as u32;
            let cpu_millis = (c["HostConfig"]["NanoCpus"].as_u64().unwrap_or(0) / 1_000_000) as u32;
            let created_at = labels["agent-rush.box.created"]
                .as_str()
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(|| chrono::Utc::now().timestamp());
            self.inner.leases.lock().unwrap().insert(
                format!("{LEASE_PREFIX}{box_id}"),
                crate::node::ActiveLease::new(memory_mb),
            );
            self.inner.boxes.lock().unwrap().insert(
                box_id.clone(),
                Entry {
                    info: BoxInfo {
                        box_id: box_id.clone(),
                        session_id: labels["agent-rush.box.session"]
                            .as_str()
                            .unwrap_or("")
                            .into(),
                        owner: labels["agent-rush.box.owner"].as_str().unwrap_or("").into(),
                        created_at,
                        last_used_at: chrono::Utc::now().timestamp(),
                    },
                    container_id: id.into(),
                    memory_mb,
                    cpu_millis,
                    busy: 0,
                    scaler: None,
                },
            );
            self.start_scaler(&box_id, id);
            adopted.push(box_id);
        }
        let volumes = self
            .docker_ok(
                &[
                    "volume".into(),
                    "ls".into(),
                    "-q".into(),
                    "--filter".into(),
                    format!("label={LABEL}"),
                ],
                Duration::from_secs(30),
            )
            .await?;
        for volume in String::from_utf8(volumes)?.split_whitespace() {
            if let Some(box_id) = volume.strip_prefix("arbox-")
                && valid_box_id(box_id)
                && !adopted.iter().any(|a| a == box_id)
            {
                self.remove_container(box_id).await;
                removed.push(box_id.to_string());
            }
        }
        Ok(json!({"adopted":adopted,"removed":removed,"at":now()}))
    }
}

/// Re-check, outside the box, the path the box resolved.
fn recheck(resolved: &str) -> Result<()> {
    ensure!(
        resolve_path(resolved).is_ok_and(|p| p == resolved),
        invalid(format!(
            "path resolves to {resolved}, outside /workspace and /tmp"
        ))
    );
    Ok(())
}
fn split_header(bytes: &[u8], end: u8) -> Option<(&str, &[u8])> {
    let at = bytes.iter().take(8192).position(|b| *b == end)?;
    Some((std::str::from_utf8(&bytes[..at]).ok()?, &bytes[at + 1..]))
}

struct Busy {
    manager: BoxManager,
    box_id: String,
}
impl Drop for Busy {
    fn drop(&mut self) {
        if let Some(entry) = self
            .manager
            .inner
            .boxes
            .lock()
            .unwrap()
            .get_mut(&self.box_id)
        {
            entry.busy = entry.busy.saturating_sub(1);
            entry.info.last_used_at = chrono::Utc::now().timestamp();
        }
    }
}

/// The worker's own box manager is also a `BoxClient`, for in-process use and tests.
impl BoxClient for BoxManager {
    fn create<'a>(&'a self, spec: &'a BoxSpec) -> BoxFuture<'a, Result<String>> {
        Box::pin(BoxManager::create(self, spec))
    }
    fn exec<'a>(
        &'a self,
        box_id: &'a str,
        request: &'a ExecRequest,
    ) -> BoxFuture<'a, Result<ExecResult>> {
        Box::pin(self.exec_box(box_id, request))
    }
    fn read_file<'a>(&'a self, box_id: &'a str, path: &'a str) -> BoxFuture<'a, Result<FileRead>> {
        Box::pin(self.read_box_file(box_id, path))
    }
    fn write_file<'a>(
        &'a self,
        box_id: &'a str,
        path: &'a str,
        bytes: &'a [u8],
    ) -> BoxFuture<'a, Result<WriteResult>> {
        Box::pin(self.write_box_file(box_id, path, bytes))
    }
    fn destroy<'a>(&'a self, box_id: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let (container, volume) = self.destroy_box(box_id).await?;
            ensure!(container && volume, "box {box_id} was not fully removed");
            Ok(())
        })
    }
    fn list<'a>(&'a self) -> BoxFuture<'a, Result<Vec<BoxInfo>>> {
        Box::pin(async move {
            self.reconcile().await?;
            Ok(self.list_boxes())
        })
    }
}

impl BoxError {
    /// The machine-readable code carried in worker error bodies (`{error, code}`).
    pub fn code(&self) -> &'static str {
        match self {
            BoxError::NotFound => "box_not_found",
            BoxError::NoSuchPath(_) => "no_such_path",
            BoxError::Invalid(_) => "invalid",
            BoxError::TooLarge(_) => "too_large",
            BoxError::Capacity => "capacity",
            BoxError::Unavailable(_) => "unavailable",
        }
    }
    /// The inverse of `code`, used by the HTTP client to rebuild the typed error.
    pub fn from_code(code: &str, message: String) -> Option<Self> {
        Some(match code {
            "box_not_found" => BoxError::NotFound,
            "no_such_path" => BoxError::NoSuchPath(message),
            "invalid" => BoxError::Invalid(message),
            "too_large" => BoxError::TooLarge(message),
            "capacity" => BoxError::Capacity,
            "unavailable" => BoxError::Unavailable(message),
            _ => return None,
        })
    }
}

/// Map a box failure to a status, a code and a message safe to return to the control plane.
pub fn error_status(e: &anyhow::Error) -> (u16, &'static str, String) {
    let Some(known) = e.downcast_ref::<BoxError>() else {
        tracing::warn!(error=%format!("{e:#}"), "box operation failed");
        return (
            500,
            "internal",
            "Box operation failed; consult worker logs".into(),
        );
    };
    let status = match known {
        BoxError::NotFound | BoxError::NoSuchPath(_) => 404,
        BoxError::Invalid(_) => 400,
        BoxError::TooLarge(_) => 413,
        BoxError::Capacity => 429,
        BoxError::Unavailable(_) => 503,
    };
    (status, known.code(), known.to_string())
}
