//! Session workspace boxes on the worker: one gVisor container per agent session, on a
//! workspace that is a sparse ext4 image of fixed size (`disk_mb`), loop-attached and
//! mounted by Docker at /workspace, so a box can never fill the worker's disk. An idle box
//! is parked: its container goes and frees memory, its workspace stays, and the next call
//! starts a new container on it. Implements the `sandbox_api` contract; `node_router`
//! serves it at `/v1/boxes…` behind the worker's bearer and mTLS.
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
use axum::body::Bytes;
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{OwnedSemaphorePermit, Semaphore},
};

pub const DEFAULT_NETWORK: &str = "ar-box";
pub const DEFAULT_IDLE_SECONDS: u64 = 1800;
pub const DEFAULT_CEILING_MB: u32 = 4096;
pub const CPU_CEILING_MILLIS: u32 = 2000;
/// Boxes with a running container at once; parked workspaces do not count.
pub const MAX_BOXES: usize = 8;
pub const DEFAULT_DISK_MB: u32 = 4096;
pub const MIN_DISK_MB: u32 = 256;
pub const MAX_DISK_MB: u32 = 262_144;
/// Parked workspaces are deleted after a week without a call.
pub const DEFAULT_RETENTION_SECONDS: u64 = 7 * 86_400;
pub const DEFAULT_GUARD_MARKER: &str = "/run/agent-rush/box-network.env";
/// Exec and file calls in flight across all boxes, and in one box.
pub const MAX_CONCURRENT_CALLS: usize = 16;
pub const MAX_CALLS_PER_BOX: usize = 4;
/// How long a call waits for a slot before it is refused as busy.
const CALL_WAIT: Duration = Duration::from_secs(30);
/// Leftovers younger than this may belong to a `qualify-box` run in progress.
const STRAY_GRACE_SECONDS: i64 = 900;
/// Which box network a container or volume belongs to. A manager only ever recovers or
/// removes what carries its own network, so a second manager on the host (a test, a
/// qualification on a scratch network) can never touch the worker's boxes.
const NETWORK_LABEL: &str = "agent-rush.box.network";
const MIB: u64 = 1024 * 1024;
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
    /// Too many calls in flight, in this box or on the worker.
    #[error("{0}")]
    Busy(String),
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
        format!("--label={NETWORK_LABEL}={network}"),
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
    /// Holds the resolver file mounted into every box, and `disks/` with each workspace.
    pub state_dir: PathBuf,
    /// A box with no call for this long is parked: its container goes, its workspace stays.
    pub idle_seconds: u64,
    pub ceiling_mb: u32,
    /// Hard size of each box's /workspace, in MiB.
    pub disk_mb: u32,
    /// A parked workspace unused for this long is deleted.
    pub retention_seconds: u64,
    /// Written by deploy/box-network.sh once the egress filter and host input guard are in
    /// place for this boot; no box starts without it.
    pub guard_marker: PathBuf,
}

/// Memory reservations shared by one-shot runs and boxes, keyed by run id or
/// `box:<id>`. Runs reserve their ceiling; running boxes reserve their current limit.
pub(crate) type Leases = Arc<Mutex<HashMap<String, crate::node::ActiveLease>>>;

/// A box is a workspace (its disk image, kept until the session ends or retention runs
/// out) plus, while it is in use, a container on that workspace.
enum Run {
    Running {
        container_id: String,
        scaler: Option<tokio::task::JoinHandle<()>>,
    },
    /// No container: idle, or lost and not yet restarted. The next call starts one.
    Parked,
}

struct Entry {
    info: BoxInfo,
    run: Run,
    /// Limits a new container starts with; live scaling grows from there.
    base_memory_mb: u32,
    base_cpu_millis: u32,
    memory_mb: u32,
    cpu_millis: u32,
    busy: usize,
    /// Calls running in this box at once.
    slots: Arc<Semaphore>,
    /// Serializes starting, parking and destroying this box's container.
    life: Arc<tokio::sync::Mutex<()>>,
    /// Adopted from a worker that predates workspace images: its /workspace is a plain
    /// Docker volume with no quota. It runs until idle or lost, then is destroyed whole,
    /// since there is no image to restart it on.
    legacy: bool,
}
impl Entry {
    fn new(info: BoxInfo, memory_mb: u32, cpu_millis: u32, run: Run) -> Self {
        Self {
            info,
            run,
            base_memory_mb: memory_mb,
            base_cpu_millis: cpu_millis,
            memory_mb,
            cpu_millis,
            busy: 0,
            slots: Arc::new(Semaphore::new(MAX_CALLS_PER_BOX)),
            life: Arc::default(),
            legacy: false,
        }
    }
    fn running(&self) -> bool {
        matches!(self.run, Run::Running { .. })
    }
    fn meta(&self) -> BoxMeta {
        BoxMeta {
            box_id: self.info.box_id.clone(),
            session_id: self.info.session_id.clone(),
            owner: self.info.owner.clone(),
            created_at: self.info.created_at,
            last_used_at: self.info.last_used_at,
            memory_mb: self.base_memory_mb,
            cpu_millis: self.base_cpu_millis,
        }
    }
}

/// What the worker keeps next to each workspace image, so a parked box survives a worker
/// restart or a reboot.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct BoxMeta {
    box_id: String,
    session_id: String,
    owner: String,
    created_at: i64,
    last_used_at: i64,
    memory_mb: u32,
    cpu_millis: u32,
}

/// What the reaper does with a box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reap {
    Keep,
    /// Stop and remove the container; keep the workspace.
    Park,
    /// Delete the workspace (the box is already parked).
    Purge,
}
/// The reaper's decision for one box at `at`: a running box idle for `idle` seconds is
/// parked, a parked one unused for `retention` seconds is purged, a busy box is kept.
pub fn reap_decision(
    running: bool,
    busy: usize,
    last_used_at: i64,
    at: i64,
    idle: u64,
    retention: u64,
) -> Reap {
    let quiet = at.saturating_sub(last_used_at);
    if busy > 0 || quiet < 0 {
        Reap::Keep
    } else if running && quiet >= idle as i64 {
        Reap::Park
    } else if !running && quiet >= retention as i64 {
        Reap::Purge
    } else {
        Reap::Keep
    }
}

/// Whether a new workspace of `disk_mb` fits: the data filesystem's free space, less what
/// every existing workspace (running or parked) may still grow into, must be at least
/// twice the quota plus 2 GiB.
pub fn disk_space_ok(available_bytes: u64, retained_headroom_bytes: u64, disk_mb: u32) -> bool {
    let need = 2 * disk_mb as u64 * MIB + 2 * 1024 * MIB;
    available_bytes.saturating_sub(retained_headroom_bytes) >= need
}
/// Free bytes from `df --output=avail -B1`.
pub fn parse_df_avail(out: &str) -> Option<u64> {
    out.lines().nth(1)?.trim().parse().ok()
}
/// Loop devices from `losetup -j <file>` (`/dev/loopN: [dev]:ino (path)` per line).
pub fn parse_losetup(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|l| l.split_once(':').map(|(dev, _)| dev.trim().to_string()))
        .filter(|dev| valid_loop_device(dev))
        .collect()
}
fn valid_loop_device(dev: &str) -> bool {
    dev.strip_prefix("/dev/loop")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}
/// `mkfs.ext4` arguments for a workspace image: no reserved blocks, the root directory
/// owned by the box user.
pub fn mkfs_args(image: &Path) -> Vec<String> {
    vec![
        "-q".into(),
        "-F".into(),
        "-m".into(),
        "0".into(),
        "-E".into(),
        "root_owner=1000:1000".into(),
        "-L".into(),
        "workspace".into(),
        image.display().to_string(),
    ]
}
/// `docker volume create` arguments: Docker mounts the attached loop device (ext4) when the
/// box starts and unmounts it when the box stops, so /workspace can never outgrow it.
pub fn volume_create_args(box_id: &str, loop_device: &str, network: &str) -> Result<Vec<String>> {
    ensure!(valid_loop_device(loop_device), "invalid loop device");
    Ok(vec![
        "volume".into(),
        "create".into(),
        "--driver=local".into(),
        "--opt=type=ext4".into(),
        format!("--opt=device={loop_device}"),
        "--opt=o=nodev,nosuid".into(),
        format!("--label={LABEL}"),
        format!("--label=agent-rush.box.id={box_id}"),
        format!("--label={NETWORK_LABEL}={network}"),
        container_name(box_id),
    ])
}
/// Whether a labelled box container belongs to the manager of `network`: its network label
/// says so, or (a box from before the label) it is attached to that network.
pub fn container_owned(label: Option<&str>, network_mode: &str, network: &str) -> bool {
    match label {
        Some(label) => label == network,
        None => network_mode == network,
    }
}
/// Whether a box volume with no workspace image and no box is a leftover the manager of
/// `network` may remove: labelled for that network, or unlabelled (from before the label)
/// and older than the grace period, so a box being created right now is never hit.
pub fn volume_stray(label: Option<&str>, network: &str, age_seconds: i64) -> bool {
    match label {
        Some(label) => label == network,
        None => age_seconds >= STRAY_GRACE_SECONDS,
    }
}
/// Checks the marker deploy/box-network.sh writes once the egress filter and the host
/// input guard are installed: it must be from this boot and for this network.
pub fn guard_marker_valid(text: &str, boot_id: &str, network: &str) -> Result<()> {
    let value = |key: &str| {
        text.lines()
            .filter_map(|l| l.split_once('='))
            .find(|(k, _)| k.trim() == key)
            .map(|(_, v)| v.trim())
    };
    ensure!(
        value("boot_id").is_some_and(|b| !b.is_empty() && b == boot_id.trim()),
        "box network guard was installed before the last boot; run deploy/box-network.sh"
    );
    ensure!(
        value("network") == Some(network),
        "box network guard is for another network"
    );
    ensure!(
        value("guard_host_input") == Some("1"),
        "box network guard has no host input rule"
    );
    Ok(())
}
/// Checks the start-up probe's lines (`<ip:port> <result>`): every target must have been
/// refused by the filter (ICMP admin-prohibited, seen in the box as "no route to host").
pub fn guard_probe_ok(out: &str, targets: &[String]) -> Result<()> {
    for target in targets {
        let result = out
            .lines()
            .filter_map(|l| l.split_once(' '))
            .find(|(t, _)| t == target)
            .map(|(_, r)| r.trim());
        ensure!(
            result == Some("rejected"),
            "box reached {target} ({}): the box network filter is not in force",
            result.unwrap_or("no result")
        );
    }
    Ok(())
}
/// The current boot's id; a box report or guard marker from an earlier boot is void.
pub fn boot_id() -> Result<String> {
    Ok(std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .context("boot id unavailable")?
        .trim()
        .to_string())
}

/// Probes each `ip:port` from inside a new box, in parallel, one line each.
const GUARD_PROBE: &str = r#"for t in "$@"; do
  ( if out=$(timeout 6 bash -c "exec 3<>/dev/tcp/${t%:*}/${t##*:}" 2>&1); then r=open
    else case "$out" in *"No route to host"*) r=rejected ;; *refused*) r=refused ;; "") r=timeout ;; *) r=failed ;; esac; fi
    echo "$t $r" ) &
done
wait"#;

#[derive(Clone)]
pub struct BoxManager {
    inner: Arc<Inner>,
}
struct Inner {
    runner: DockerRunner,
    config: BoxConfig,
    resolv: PathBuf,
    disks: PathBuf,
    boxes: Mutex<HashMap<String, Entry>>,
    leases: Leases,
    capacity_mb: u32,
    /// Exec and file calls running in all boxes at once.
    calls: Arc<Semaphore>,
    /// Boxes whose removal failed; their leases stay until a retry succeeds.
    retry: Mutex<HashSet<String>>,
    gateway: Mutex<Option<String>>,
}

struct DockerOutput {
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: String,
    timed_out: bool,
}

/// What a box call holds while it runs: the busy mark (keeps the reaper away) and a slot.
struct Call {
    _busy: Busy,
    _slot: OwnedSemaphorePermit,
    _global: OwnedSemaphorePermit,
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
        ensure!(
            (MIN_DISK_MB..=MAX_DISK_MB).contains(&config.disk_mb),
            "box disk quota must be {MIN_DISK_MB}–{MAX_DISK_MB} MiB"
        );
        ensure!(
            config.retention_seconds >= config.idle_seconds,
            "box workspace retention must be at least the idle time"
        );
        std::fs::create_dir_all(&config.state_dir)?;
        let resolv = config.state_dir.join("resolv.conf");
        std::fs::write(&resolv, RESOLV_CONF)?;
        let disks = config.state_dir.join("disks");
        std::fs::create_dir_all(&disks)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&resolv, std::fs::Permissions::from_mode(0o644))?;
            std::fs::set_permissions(&disks, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(Self {
            inner: Arc::new(Inner {
                runner,
                config,
                resolv,
                disks,
                boxes: Mutex::new(HashMap::new()),
                leases,
                capacity_mb,
                calls: Arc::new(Semaphore::new(MAX_CONCURRENT_CALLS)),
                retry: Mutex::default(),
                gateway: Mutex::default(),
            }),
        })
    }
    pub fn image(&self) -> &str {
        &self.inner.config.image
    }
    pub fn network(&self) -> &str {
        &self.inner.config.network
    }
    pub fn disk_mb(&self) -> u32 {
        self.inner.config.disk_mb
    }
    pub(crate) async fn network_gateway(&self) -> Result<String> {
        if let Some(gateway) = self.inner.gateway.lock().unwrap().clone() {
            return Ok(gateway);
        }
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
        *self.inner.gateway.lock().unwrap() = Some(gateway.clone());
        Ok(gateway)
    }
    /// Boxes known to this worker, running or parked.
    pub fn count(&self) -> usize {
        self.inner.boxes.lock().unwrap().len()
    }
    pub fn running_count(&self) -> usize {
        self.inner
            .boxes
            .lock()
            .unwrap()
            .values()
            .filter(|e| e.running())
            .count()
    }

    async fn docker(
        &self,
        args: &[String],
        stdin: Option<Bytes>,
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
            (Some(bytes), Some(mut pipe)) => Some(tokio::spawn(async move {
                let _ = pipe.write_all(&bytes).await;
                let _ = pipe.shutdown().await;
            })),
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
    /// Run a host tool (mkfs.ext4, losetup, df) with a clean environment.
    async fn tool(&self, program: &str, args: &[String]) -> Result<String> {
        let mut command = tokio::process::Command::new(program);
        command
            .env_clear()
            .env(
                "PATH",
                "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
            )
            .env("LC_ALL", "C")
            .args(args)
            .stdin(Stdio::null())
            .kill_on_drop(true);
        let out = tokio::time::timeout(Duration::from_secs(60), command.output())
            .await
            .with_context(|| format!("{program} timed out"))?
            .with_context(|| format!("running {program}"))?;
        ensure!(
            out.status.success(),
            "{program} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn qualification_gate(&self) -> Result<()> {
        let report =
            crate::qualification::BoxReport::load(&self.inner.config.report).and_then(|report| {
                report.assess(
                    &crate::qualification::box_host_id()?,
                    &self.inner.config.image,
                    &self.inner.config.network,
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
    /// The egress filter and host input guard must be installed for this boot (reboots and
    /// firewall resets drop them; the root unit agent-rush-box-network reinstalls them).
    pub(crate) fn guard_marker_check(&self) -> Result<()> {
        let text = std::fs::read_to_string(&self.inner.config.guard_marker).with_context(|| {
            format!(
                "box network guard marker {} missing",
                self.inner.config.guard_marker.display()
            )
        })?;
        guard_marker_valid(&text, &boot_id()?, &self.inner.config.network)
    }
    fn guard_gate(&self) -> Result<()> {
        self.guard_marker_check().map_err(|e| {
            tracing::warn!(error=%format!("{e:#}"), "box start refused: network guard");
            BoxError::Unavailable(
                "The session box network filter is not in place on this worker".into(),
            )
            .into()
        })
    }
    fn disk_gate(&self) -> Result<()> {
        let available = std::process::Command::new("df")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .args(["--output=avail", "-B1", "--"])
            .arg(&self.inner.disks)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| parse_df_avail(&String::from_utf8_lossy(&o.stdout)));
        let headroom = self.retained_headroom();
        match available {
            Some(free) if disk_space_ok(free, headroom, self.inner.config.disk_mb) => Ok(()),
            other => {
                tracing::warn!(
                    available = ?other,
                    retained_headroom = headroom,
                    disk_mb = self.inner.config.disk_mb,
                    "box creation refused: worker disk"
                );
                Err(BoxError::Capacity.into())
            }
        }
    }
    /// What existing workspaces may still grow into: each image's size less its allocation.
    fn retained_headroom(&self) -> u64 {
        use std::os::unix::fs::MetadataExt;
        std::fs::read_dir(&self.inner.disks)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "img"))
            .filter_map(|e| e.metadata().ok())
            .map(|m| m.len().saturating_sub(m.blocks() * 512))
            .sum()
    }

    /// Reserve capacity, then create the workspace and start the box. Refused unless the box
    /// qualification report is fresh and passing, the network guard is in place for this
    /// boot, and the worker's disk has room.
    pub async fn create(&self, spec: &BoxSpec) -> Result<String> {
        validate_spec(spec, self.inner.config.ceiling_mb)?;
        self.qualification_gate()?;
        self.guard_gate()?;
        self.disk_gate()?;
        self.create_box(spec, self.inner.config.disk_mb, true).await
    }
    /// Used only by `qualify-box`, which has to start a box before any report exists, on a
    /// small workspace it can fill quickly. The report records what the probes observe.
    pub(crate) async fn create_for_qualification(
        &self,
        spec: &BoxSpec,
        disk_mb: u32,
    ) -> Result<String> {
        validate_spec(spec, self.inner.config.ceiling_mb)?;
        self.disk_gate()?;
        self.create_box(spec, disk_mb, false).await
    }
    async fn create_box(&self, spec: &BoxSpec, disk_mb: u32, probe: bool) -> Result<String> {
        let box_id = uuid::Uuid::new_v4().simple().to_string();
        let at = chrono::Utc::now().timestamp();
        let mut entry = Entry::new(
            BoxInfo {
                box_id: box_id.clone(),
                session_id: spec.session_id.clone(),
                owner: spec.owner.clone(),
                created_at: at,
                last_used_at: at,
            },
            spec.memory_mb,
            spec.cpu_millis,
            Run::Parked,
        );
        if let Err(e) = self.make_disk(&box_id, disk_mb).await {
            self.delete_disk(&box_id).await;
            return Err(e);
        }
        self.write_meta(&entry.meta());
        let life = entry.life.clone();
        let guard = life.lock().await;
        match self.launch(&box_id, &entry.meta(), probe).await {
            Ok(container_id) => {
                entry.run = Run::Running {
                    container_id: container_id.clone(),
                    scaler: None,
                };
                self.inner
                    .boxes
                    .lock()
                    .unwrap()
                    .insert(box_id.clone(), entry);
                drop(guard);
                self.start_scaler(&box_id, &container_id);
                tracing::info!(box_id=%box_id, session=%spec.session_id, disk_mb, "box created");
                Ok(box_id)
            }
            Err(e) => {
                drop(guard);
                self.delete_disk(&box_id).await;
                Err(e)
            }
        }
    }
    fn disk_path(&self, box_id: &str) -> PathBuf {
        self.inner.disks.join(format!("{box_id}.img"))
    }
    fn meta_path(&self, box_id: &str) -> PathBuf {
        self.inner.disks.join(format!("{box_id}.json"))
    }
    fn write_meta(&self, meta: &BoxMeta) {
        let path = self.meta_path(&meta.box_id);
        let tmp = path.with_extension("json.tmp");
        let written = serde_json::to_vec(meta)
            .map_err(anyhow::Error::from)
            .and_then(|bytes| Ok(std::fs::write(&tmp, bytes)?))
            .and_then(|()| Ok(std::fs::rename(&tmp, &path)?));
        if let Err(e) = written {
            tracing::warn!(box_id=%meta.box_id, error=%e, "box metadata not saved");
        }
    }
    fn read_meta(&self, box_id: &str) -> Option<BoxMeta> {
        let bytes = std::fs::read(self.meta_path(box_id)).ok()?;
        serde_json::from_slice::<BoxMeta>(&bytes)
            .ok()
            .filter(|m| m.box_id == box_id)
    }
    /// A sparse ext4 image of `disk_mb` MiB whose root belongs to the box user.
    async fn make_disk(&self, box_id: &str, disk_mb: u32) -> Result<()> {
        let path = self.disk_path(box_id);
        {
            let mut options = std::fs::OpenOptions::new();
            options.create_new(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let file = options.open(&path).context("creating workspace image")?;
            file.set_len(disk_mb as u64 * MIB)?;
        }
        self.tool("mkfs.ext4", &mkfs_args(&path)).await?;
        // An empty workspace: lost+found is root's and only clutters the agent's listing.
        let _ = self
            .tool(
                "debugfs",
                &[
                    "-w".into(),
                    "-R".into(),
                    "rmdir lost+found".into(),
                    path.display().to_string(),
                ],
            )
            .await;
        Ok(())
    }
    async fn loops_of(&self, box_id: &str) -> Result<Vec<String>> {
        let path = self.disk_path(box_id);
        Ok(parse_losetup(
            &self
                .tool("losetup", &["-j".into(), path.display().to_string()])
                .await?,
        ))
    }
    /// Attach the workspace image to a loop device (reusing one already attached).
    async fn attach_disk(&self, box_id: &str) -> Result<String> {
        let path = self.disk_path(box_id);
        ensure!(path.is_file(), "workspace image missing");
        if let Some(dev) = self.loops_of(box_id).await?.into_iter().next() {
            return Ok(dev);
        }
        let dev = self
            .tool(
                "losetup",
                &["--find".into(), "--show".into(), path.display().to_string()],
            )
            .await?
            .trim()
            .to_string();
        ensure!(valid_loop_device(&dev), "losetup returned no loop device");
        Ok(dev)
    }
    /// Detach every loop device of this workspace. True when none remains.
    async fn detach_disk(&self, box_id: &str) -> bool {
        if !self.disk_path(box_id).exists() {
            return true;
        }
        let Ok(devices) = self.loops_of(box_id).await else {
            return false;
        };
        for dev in &devices {
            let _ = self.tool("losetup", &["-d".into(), dev.clone()]).await;
        }
        self.loops_of(box_id).await.is_ok_and(|d| d.is_empty())
    }
    /// Delete the workspace image and its metadata. True when both are gone.
    async fn delete_disk(&self, box_id: &str) -> bool {
        if !self.detach_disk(box_id).await {
            return false;
        }
        for path in [self.disk_path(box_id), self.meta_path(box_id)] {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return false,
            }
        }
        true
    }

    /// Start a container on an existing workspace. Reserves the box's memory first and
    /// releases it again on any failure. With `probe`, the new box must find the network
    /// filter in force (host and metadata refused) before anyone may use it.
    async fn launch(&self, box_id: &str, meta: &BoxMeta, probe: bool) -> Result<String> {
        let lease = format!("{LEASE_PREFIX}{box_id}");
        {
            let mut leases = self.inner.leases.lock().unwrap();
            let boxes = leases
                .keys()
                .filter(|k| k.starts_with(LEASE_PREFIX) && **k != lease)
                .count();
            let used: u32 = leases
                .iter()
                .filter(|(k, _)| **k != lease)
                .map(|(_, l)| l.memory_mb)
                .sum();
            if boxes >= MAX_BOXES || used.saturating_add(meta.memory_mb) > self.inner.capacity_mb {
                return Err(BoxError::Capacity.into());
            }
            leases.insert(lease.clone(), crate::node::ActiveLease::new(meta.memory_mb));
        }
        let started = self.start_container(box_id, meta, probe).await;
        if started.is_err() {
            let (container, _) = self.stop_container(box_id).await;
            if container {
                self.inner.leases.lock().unwrap().remove(&lease);
            } else {
                self.inner.retry.lock().unwrap().insert(box_id.into());
            }
        }
        started
    }
    async fn start_container(&self, box_id: &str, meta: &BoxMeta, probe: bool) -> Result<String> {
        let name = container_name(box_id);
        let device = self.attach_disk(box_id).await?;
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
        self.docker_ok(
            &volume_create_args(box_id, &device, &self.inner.config.network)?,
            Duration::from_secs(30),
        )
        .await?;
        let spec = BoxSpec {
            session_id: meta.session_id.clone(),
            owner: meta.owner.clone(),
            memory_mb: meta.memory_mb,
            cpu_millis: meta.cpu_millis,
        };
        let mut args = docker_create_args(
            &spec,
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
        if probe {
            self.probe_guard(box_id).await?;
        }
        Ok(id)
    }
    /// From inside the new box: the host (its gateway) and the metadata service must both
    /// be refused by the filter. Proves the rules are in force now, not just at boot.
    async fn probe_guard(&self, box_id: &str) -> Result<()> {
        let gateway = self.network_gateway().await?;
        let targets = vec![format!("{gateway}:22"), "169.254.169.254:80".to_string()];
        let mut args = vec![
            "exec".into(),
            "-u".into(),
            BOX_UID.into(),
            container_name(box_id),
            "bash".into(),
            "-c".into(),
            GUARD_PROBE.into(),
            "arguard".into(),
        ];
        args.extend(targets.iter().cloned());
        let out = self
            .docker(&args, None, 4096, Duration::from_secs(20))
            .await?;
        guard_probe_ok(&String::from_utf8_lossy(&out.stdout), &targets).map_err(|e| {
            tracing::error!(box_id=%box_id, error=%format!("{e:#}"), "box start refused: network guard probe");
            BoxError::Unavailable(
                "The session box network filter is not in force on this worker".into(),
            )
            .into()
        })
    }
    /// Remove a box's container and volume and detach its workspace; the image stays.
    /// Returns whether the container is gone and whether the volume and loop are.
    async fn stop_container(&self, box_id: &str) -> (bool, bool) {
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
        let detached = volume_gone && self.detach_disk(box_id).await;
        (container_gone, detached)
    }
    /// Take the box's container away and keep its workspace. Call with its life lock held.
    /// The lease is released once the container is gone; if it is not, nothing changes.
    /// A legacy box has no workspace image to keep, so it is destroyed instead.
    async fn park_locked(&self, box_id: &str) -> bool {
        let legacy = {
            let mut boxes = self.inner.boxes.lock().unwrap();
            match boxes.get(box_id) {
                Some(entry) if entry.legacy => boxes.remove(box_id),
                _ => None,
            }
        };
        if let Some(entry) = legacy {
            if let Run::Running {
                scaler: Some(scaler),
                ..
            } = entry.run
            {
                scaler.abort();
            }
            let (container, workspace) = self.teardown(box_id).await;
            tracing::info!(box_id=%box_id, container_removed=container, volume_removed=workspace, "legacy box removed (no workspace image to keep)");
            return container;
        }
        let (container, _) = self.stop_container(box_id).await;
        if !container {
            return false;
        }
        let meta = {
            let mut boxes = self.inner.boxes.lock().unwrap();
            boxes.get_mut(box_id).map(|entry| {
                if let Run::Running {
                    scaler: Some(scaler),
                    ..
                } = std::mem::replace(&mut entry.run, Run::Parked)
                {
                    scaler.abort();
                }
                entry.memory_mb = entry.base_memory_mb;
                entry.cpu_millis = entry.base_cpu_millis;
                entry.meta()
            })
        };
        self.inner
            .leases
            .lock()
            .unwrap()
            .remove(&format!("{LEASE_PREFIX}{box_id}"));
        if let Some(meta) = meta {
            self.write_meta(&meta);
        }
        true
    }
    /// Make sure the box has a running container, starting one on its workspace if it is
    /// parked. Concurrent callers wait for one start.
    async fn ensure_running(&self, box_id: &str) -> Result<()> {
        let life = {
            let boxes = self.inner.boxes.lock().unwrap();
            let entry = boxes.get(box_id).ok_or(BoxError::NotFound)?;
            if entry.running() {
                return Ok(());
            }
            entry.life.clone()
        };
        let _guard = life.lock().await;
        let meta = {
            let boxes = self.inner.boxes.lock().unwrap();
            let entry = boxes.get(box_id).ok_or(BoxError::NotFound)?;
            if entry.running() {
                return Ok(());
            }
            entry.meta()
        };
        self.qualification_gate()?;
        self.guard_gate()?;
        let container_id = self.launch(box_id, &meta, true).await?;
        let kept = match self.inner.boxes.lock().unwrap().get_mut(box_id) {
            Some(entry) => {
                entry.run = Run::Running {
                    container_id: container_id.clone(),
                    scaler: None,
                };
                Some(entry.meta())
            }
            None => None,
        };
        let Some(meta) = kept else {
            // Destroyed while starting: its destroy is waiting on this lock and removes it.
            return Err(BoxError::NotFound.into());
        };
        self.write_meta(&meta);
        self.start_scaler(box_id, &container_id);
        tracing::info!(box_id=%box_id, "box restarted on its workspace");
        Ok(())
    }
    /// The container is gone or will not start: park the box, then start it afresh.
    async fn restart_lost(&self, box_id: &str) -> Result<()> {
        let life = {
            let boxes = self.inner.boxes.lock().unwrap();
            boxes.get(box_id).ok_or(BoxError::NotFound)?.life.clone()
        };
        {
            let _guard = life.lock().await;
            ensure!(
                self.park_locked(box_id).await,
                BoxError::Unavailable("box container could not be removed".into())
            );
        }
        self.ensure_running(box_id).await
    }
    /// Remove the container, volume, loop device and workspace image of a box id, known or
    /// not. Returns (container removed, workspace removed). The lease is released only when
    /// both are; otherwise the box is retried by `reconcile`.
    async fn teardown(&self, box_id: &str) -> (bool, bool) {
        let (container, _) = self.stop_container(box_id).await;
        let workspace = container && self.delete_disk(box_id).await;
        if container && workspace {
            self.inner
                .leases
                .lock()
                .unwrap()
                .remove(&format!("{LEASE_PREFIX}{box_id}"));
            self.inner.retry.lock().unwrap().remove(box_id);
        } else {
            tracing::warn!(box_id=%box_id, container_removed=container, workspace_removed=workspace, "box removal incomplete; will retry");
            self.inner.retry.lock().unwrap().insert(box_id.into());
        }
        (container, workspace)
    }
    /// Destroy a box and its workspace; idempotent. Returns (container removed, workspace
    /// removed).
    pub async fn destroy_box(&self, box_id: &str) -> Result<(bool, bool)> {
        ensure!(valid_box_id(box_id), BoxError::NotFound);
        let entry = self.inner.boxes.lock().unwrap().remove(box_id);
        let _guard = match &entry {
            Some(entry) => Some(entry.life.clone().lock_owned().await),
            None => None,
        };
        if let Some(Run::Running {
            scaler: Some(scaler),
            ..
        }) = entry.map(|e| e.run)
        {
            scaler.abort();
        }
        let (container, workspace) = self.teardown(box_id).await;
        tracing::info!(box_id=%box_id, container_removed=container, workspace_removed=workspace, "box destroyed");
        Ok((container, workspace))
    }
    /// Bring the map in line with Docker: retry unfinished removals; restart a stopped box
    /// (an out-of-memory kill, say), and park one whose container is gone or will not start,
    /// so its workspace survives and the next call starts it afresh.
    pub async fn reconcile(&self) -> Result<()> {
        let pending: Vec<String> = self.inner.retry.lock().unwrap().iter().cloned().collect();
        for id in pending {
            if !self.inner.boxes.lock().unwrap().contains_key(&id) {
                self.teardown(&id).await;
            }
        }
        // Only boxes known before asking Docker can be judged; one started meanwhile is not
        // yet in the answer and must not be mistaken for lost.
        let known: Vec<(String, String)> = self
            .inner
            .boxes
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(id, e)| match &e.run {
                Run::Running { container_id, .. } => Some((id.clone(), container_id.clone())),
                Run::Parked => None,
            })
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
        for (id, container) in lost {
            let Some(life) = self
                .inner
                .boxes
                .lock()
                .unwrap()
                .get(&id)
                .map(|e| e.life.clone())
            else {
                continue;
            };
            let _guard = life.lock().await;
            let still = matches!(
                self.inner.boxes.lock().unwrap().get(&id).map(|e| &e.run),
                Some(Run::Running { container_id, .. }) if *container_id == container
            );
            if !still {
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
                tracing::warn!(box_id=%id, "box container lost; parked, workspace kept");
                self.park_locked(&id).await;
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

    /// Mark a box in use (the reaper leaves it alone until the call finishes), wait for a
    /// slot in the box and in the worker, then make sure it has a running container.
    async fn enter(&self, box_id: &str) -> Result<Call> {
        let (busy, slots) = {
            let mut boxes = self.inner.boxes.lock().unwrap();
            let entry = boxes.get_mut(box_id).ok_or(BoxError::NotFound)?;
            entry.busy += 1;
            entry.info.last_used_at = chrono::Utc::now().timestamp();
            (
                Busy {
                    manager: self.clone(),
                    box_id: box_id.into(),
                },
                entry.slots.clone(),
            )
        };
        let busy_error =
            |what: &str| BoxError::Busy(format!("{what}; retry when a running call has finished"));
        let slot = tokio::time::timeout(CALL_WAIT, slots.acquire_owned())
            .await
            .map_err(|_| busy_error("this box is running its maximum of calls"))?
            .map_err(|_| BoxError::NotFound)?;
        let global = tokio::time::timeout(CALL_WAIT, self.inner.calls.clone().acquire_owned())
            .await
            .map_err(|_| busy_error("the worker is running its maximum of box calls"))?
            .context("worker call slots closed")?;
        self.ensure_running(box_id).await?;
        Ok(Call {
            _busy: busy,
            _slot: slot,
            _global: global,
        })
    }
    /// Run a script in the box as the box user. A stopped box (an out-of-memory kill, say)
    /// is restarted once; a box whose container is gone gets a new one on its workspace.
    async fn box_exec(
        &self,
        box_id: &str,
        script_args: &[String],
        cwd: &str,
        stdin: Option<Bytes>,
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
            let out = self.docker(&args, stdin.clone(), limit, timeout).await?;
            // Only Docker's own errors count: a file script's stderr can quote an agent's
            // path, but always starts with that path or with fixed text of our own.
            let daemon = out.stderr.starts_with("Error response from daemon");
            let missing = daemon && out.stderr.contains("No such container");
            let stopped = daemon && out.stderr.contains("is not running");
            if !(missing || stopped) {
                return Ok(out);
            }
            if attempt == 1 {
                break;
            }
            if stopped
                && self
                    .docker_ok(
                        &["start".into(), container_name(box_id)],
                        Duration::from_secs(60),
                    )
                    .await
                    .is_ok()
            {
                tracing::warn!(box_id=%box_id, "box was stopped; restarted it");
                continue;
            }
            tracing::warn!(box_id=%box_id, "box container lost; starting a new one on its workspace");
            self.restart_lost(box_id).await?;
        }
        Err(BoxError::Unavailable("box container keeps stopping".into()).into())
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
        let _call = self.enter(box_id).await?;
        let script = vec![
            "bash".into(),
            "-c".into(),
            EXEC_SCRIPT.into(),
            "arexec".into(),
            seconds.to_string(),
            MAX_EXEC_OUTPUT_BYTES.to_string(),
            request.command.clone(),
        ];
        let stdin = Bytes::from(request.stdin.clone().unwrap_or_default());
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
        stdin: Option<Bytes>,
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
        let _call = self.enter(box_id).await?;
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

    /// Write a file; the body is handed to `docker exec` as is, never copied.
    pub async fn write_box_file(
        &self,
        box_id: &str,
        path: &str,
        bytes: Bytes,
    ) -> Result<WriteResult> {
        ensure!(
            bytes.len() <= MAX_FILE_BYTES,
            BoxError::TooLarge("file exceeds 25 MiB".into())
        );
        let _call = self.enter(box_id).await?;
        let (header, _) = self
            .file_script(box_id, WRITE_SCRIPT, path, &[], Some(bytes.clone()), 4096)
            .await
            .map_err(|e| match e.downcast_ref::<BoxError>() {
                None if format!("{e:#}").contains("No space left on device") => {
                    BoxError::TooLarge("the box workspace is full".into()).into()
                }
                _ => e,
            })?;
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
            sha256: hex::encode(Sha256::digest(&bytes)),
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
                    .filter(|e| e.running())
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
                match (updated, boxes.get_mut(&id).filter(|e| e.running())) {
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
        let mut boxes = self.inner.boxes.lock().unwrap();
        match boxes.get_mut(box_id).map(|e| &mut e.run) {
            Some(Run::Running {
                container_id: current,
                scaler,
            }) if current == container_id => *scaler = Some(task),
            _ => task.abort(),
        }
    }

    /// Every 30 s, forever: reconcile with Docker, park boxes with no call for
    /// `idle_seconds` (their workspaces stay), and delete workspaces unused for
    /// `retention_seconds`.
    pub fn start_reaper(&self) -> tokio::task::JoinHandle<()> {
        let manager = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                if let Err(e) = manager.reconcile().await {
                    tracing::warn!(error=%e, "box reconciliation failed");
                }
                manager.reap(chrono::Utc::now().timestamp()).await;
            }
        })
    }
    fn decide(&self, box_id: &str, at: i64) -> Reap {
        let config = &self.inner.config;
        self.inner
            .boxes
            .lock()
            .unwrap()
            .get(box_id)
            .map_or(Reap::Keep, |e| {
                reap_decision(
                    e.running(),
                    e.busy,
                    e.info.last_used_at,
                    at,
                    config.idle_seconds,
                    config.retention_seconds,
                )
            })
    }
    /// One reaper pass at `at`: park idle boxes, delete workspaces past retention.
    pub async fn reap(&self, at: i64) {
        let ids: Vec<String> = self.inner.boxes.lock().unwrap().keys().cloned().collect();
        for id in ids {
            if self.decide(&id, at) == Reap::Keep {
                continue;
            }
            let Some(life) = self
                .inner
                .boxes
                .lock()
                .unwrap()
                .get(&id)
                .map(|e| e.life.clone())
            else {
                continue;
            };
            let _guard = life.lock().await;
            // A call may have started while this waited: decide again, under the lock that
            // every start, park and destroy takes, with the clock read now.
            match self.decide(&id, chrono::Utc::now().timestamp()) {
                Reap::Keep => {}
                Reap::Park => {
                    if self.park_locked(&id).await {
                        tracing::info!(box_id=%id, "idle box parked; workspace kept");
                    } else {
                        tracing::warn!(box_id=%id, "idle box not parked; will retry");
                    }
                }
                Reap::Purge => {
                    let removed = {
                        let mut boxes = self.inner.boxes.lock().unwrap();
                        let purge = boxes.get(&id).is_some_and(|e| e.busy == 0 && !e.running());
                        purge && boxes.remove(&id).is_some()
                    };
                    if removed {
                        let (_, workspace) = self.teardown(&id).await;
                        tracing::info!(box_id=%id, workspace_removed=workspace, "workspace past retention deleted");
                    }
                }
            }
        }
    }

    /// At worker start: adopt running boxes left by a previous worker process, park stopped
    /// ones (after a reboot, all of them) on their workspaces, and remove containers,
    /// volumes and images that belong to no workspace.
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
        let now_s = chrono::Utc::now().timestamp();
        let (mut adopted, mut parked, mut removed) = (Vec::new(), Vec::new(), Vec::new());
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
            if !valid_box_id(&box_id)
                || !container_owned(
                    labels[NETWORK_LABEL].as_str(),
                    c["HostConfig"]["NetworkMode"].as_str().unwrap_or(""),
                    &self.inner.config.network,
                )
            {
                continue; // another manager's box (another network): never touched
            }
            let created_at = labels["agent-rush.box.created"]
                .as_str()
                .and_then(|v| v.parse().ok())
                .unwrap_or(now_s);
            // A qualification box belongs to a `qualify-box` process that removes it itself.
            if labels["agent-rush.box.owner"] == "qualification"
                && now_s - created_at < STRAY_GRACE_SECONDS
            {
                continue;
            }
            let running = c["State"]["Running"] == true;
            let memory_mb = (c["HostConfig"]["Memory"].as_u64().unwrap_or(0) / 1_048_576) as u32;
            let cpu_millis = (c["HostConfig"]["NanoCpus"].as_u64().unwrap_or(0) / 1_000_000) as u32;
            let has_image = self.disk_path(&box_id).is_file();
            let meta = self.read_meta(&box_id).filter(|_| has_image);
            let (meta, legacy) = match meta {
                Some(meta) if meta.owner != "qualification" => (meta, false),
                // A running box from before workspace images keeps running (its session
                // keeps its files) until idle or lost; it is then removed whole.
                None if !has_image
                    && running
                    && labels[NETWORK_LABEL].is_null()
                    && labels["agent-rush.box.owner"] != "qualification" =>
                {
                    let text = |key: &str| labels[key].as_str().unwrap_or("").to_string();
                    let meta = BoxMeta {
                        box_id: box_id.clone(),
                        session_id: text("agent-rush.box.session"),
                        owner: text("agent-rush.box.owner"),
                        created_at,
                        last_used_at: now_s,
                        memory_mb,
                        cpu_millis,
                    };
                    (meta, true)
                }
                _ => {
                    // No usable workspace: a stopped legacy box, or a broken one.
                    self.teardown(&box_id).await;
                    removed.push(box_id);
                    continue;
                }
            };
            if !running {
                continue; // parked with the other workspaces below
            }
            self.inner.leases.lock().unwrap().insert(
                format!("{LEASE_PREFIX}{box_id}"),
                crate::node::ActiveLease::new(memory_mb),
            );
            let mut entry = Entry::new(
                BoxInfo {
                    box_id: box_id.clone(),
                    session_id: meta.session_id.clone(),
                    owner: meta.owner.clone(),
                    created_at: meta.created_at,
                    last_used_at: now_s,
                },
                meta.memory_mb,
                meta.cpu_millis,
                Run::Running {
                    container_id: id.into(),
                    scaler: None,
                },
            );
            (entry.memory_mb, entry.cpu_millis) = (memory_mb, cpu_millis);
            entry.legacy = legacy;
            if legacy {
                tracing::warn!(box_id=%box_id, "adopted a box without a workspace quota (from an older worker); it is removed when idle");
            }
            self.inner
                .boxes
                .lock()
                .unwrap()
                .insert(box_id.clone(), entry);
            self.start_scaler(&box_id, id);
            adopted.push(box_id);
        }
        // Every other workspace image is parked, or deleted if it has no metadata.
        let images: Vec<String> = std::fs::read_dir(&self.inner.disks)?
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.strip_suffix(".img").map(String::from)
            })
            .filter(|id| valid_box_id(id) && !adopted.contains(id))
            .collect();
        for box_id in images {
            let meta = self.read_meta(&box_id);
            let age = std::fs::metadata(self.disk_path(&box_id))
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .map_or(0, |d| d.as_secs() as i64);
            match meta {
                Some(meta) if meta.owner != "qualification" => {
                    let (container, _) = self.stop_container(&box_id).await;
                    if !container {
                        continue;
                    }
                    self.inner.boxes.lock().unwrap().insert(
                        box_id.clone(),
                        Entry::new(
                            BoxInfo {
                                box_id: box_id.clone(),
                                session_id: meta.session_id,
                                owner: meta.owner,
                                created_at: meta.created_at,
                                last_used_at: meta.last_used_at,
                            },
                            meta.memory_mb,
                            meta.cpu_millis,
                            Run::Parked,
                        ),
                    );
                    parked.push(box_id);
                }
                _ if age >= STRAY_GRACE_SECONDS => {
                    self.teardown(&box_id).await;
                    removed.push(box_id);
                }
                _ => {}
            }
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
            let Some(box_id) = volume.strip_prefix("arbox-") else {
                continue;
            };
            if !valid_box_id(box_id)
                || self.disk_path(box_id).exists()
                || removed.iter().any(|r| r == box_id)
                || self.inner.boxes.lock().unwrap().contains_key(box_id)
            {
                continue;
            }
            let raw = self
                .docker_ok(
                    &[
                        "volume".into(),
                        "inspect".into(),
                        "--format={{json .}}".into(),
                        volume.into(),
                    ],
                    Duration::from_secs(30),
                )
                .await?;
            let v: Value = serde_json::from_slice(&raw)?;
            let age = v["CreatedAt"]
                .as_str()
                .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
                .map_or(0, |t| now_s - t.timestamp());
            if volume_stray(
                v["Labels"][NETWORK_LABEL].as_str(),
                &self.inner.config.network,
                age,
            ) {
                self.teardown(box_id).await;
                removed.push(box_id.to_string());
            }
        }
        Ok(json!({"adopted":adopted,"parked":parked,"removed":removed,"at":now()}))
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
        Box::pin(self.write_box_file(box_id, path, Bytes::copy_from_slice(bytes)))
    }
    fn destroy<'a>(&'a self, box_id: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let (container, workspace) = self.destroy_box(box_id).await?;
            ensure!(container && workspace, "box {box_id} was not fully removed");
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
            BoxError::Busy(_) => "busy",
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
            "busy" => BoxError::Busy(message),
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
        BoxError::Capacity | BoxError::Busy(_) => 429,
        BoxError::Unavailable(_) => 503,
    };
    (status, known.code(), known.to_string())
}
