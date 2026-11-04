//! Trusted management-runner cleanup, independent from application/worker processes.
//! Acts only on the exact acknowledged resources recorded in the private manifest.
use super::lifecycle::{DeploymentManifest, ManifestFile, ResourceApi, teardown};
use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessWitness {
    pub pid: u32,
    pub start_ticks: u64,
}
impl ProcessWitness {
    fn start_ticks(pid: u32) -> Result<u64> {
        let text = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
        ensure!(text.len() < 4096, "invalid process witness");
        let fields = text.rsplit_once(") ").context("invalid process witness")?.1;
        ensure!(
            matches!(
                fields.split_whitespace().next(),
                Some("R" | "S" | "D" | "I" | "W")
            ),
            "watchdog process exited or is stopped"
        );
        // /proc/PID/stat field 22; the first field after the parenthesized name is field 3.
        Ok(fields
            .split_whitespace()
            .nth(19)
            .context("process start timestamp missing")?
            .parse()?)
    }
    fn current() -> Result<Self> {
        let pid = std::process::id();
        Ok(Self {
            pid,
            start_ticks: Self::start_ticks(pid)?,
        })
    }
    fn ensure_live(&self) -> Result<()> {
        ensure!(
            self.pid > 0
                && self.start_ticks > 0
                && Self::start_ticks(self.pid)? == self.start_ticks,
            "watchdog process stopped or PID was reused"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WatchReport {
    pub schema: String,
    pub deployment_id: Uuid,
    pub manifest_identity: String,
    pub observed_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub phase: String,
    pub persistent: bool,
    pub witness: Option<ProcessWitness>,
    pub confirmed_absent: usize,
    pub pending: Vec<String>,
    pub last_error: Option<String>,
}
impl WatchReport {
    fn new(
        m: &DeploymentManifest,
        at: DateTime<Utc>,
        persistent: bool,
        phase: &str,
    ) -> Result<Self> {
        m.validate()?;
        Ok(Self {
            schema: "agent-rush.watchdog.v1".into(),
            deployment_id: m.deployment_id,
            manifest_identity: m.identity_sha256()?,
            observed_at: at,
            expires_at: m.expires_at,
            phase: phase.into(),
            persistent,
            witness: if persistent {
                Some(ProcessWitness::current()?)
            } else {
                None
            },
            confirmed_absent: 0,
            pending: m
                .resources
                .iter()
                .filter(|r| !r.deleted)
                .map(|r| r.id.clone())
                .collect(),
            last_error: None,
        })
    }
    pub fn error(
        m: &DeploymentManifest,
        at: DateTime<Utc>,
        persistent: bool,
        error: &str,
    ) -> Result<Self> {
        let mut report = Self::new(m, at, persistent, "error")?;
        report.last_error = Some(error.chars().take(500).collect());
        Ok(report)
    }
    /// A diagnostic, old heartbeat, completed deployment or reused PID never authorizes creation.
    pub fn ensure_armed(&self, m: &DeploymentManifest, at: DateTime<Utc>) -> Result<()> {
        m.validate()?;
        ensure!(
            self.schema == "agent-rush.watchdog.v1"
                && self.deployment_id == m.deployment_id
                && self.manifest_identity == m.identity_sha256()?
                && self.expires_at == m.expires_at,
            "watchdog belongs to a different deployment identity"
        );
        ensure!(
            self.phase == "armed" && self.persistent && self.last_error.is_none() && !m.closed,
            "continuous watchdog is not armed"
        );
        ensure!(
            self.observed_at <= at + Duration::seconds(2)
                && at - self.observed_at <= Duration::seconds(30)
                && at < m.expires_at,
            "watchdog heartbeat or deployment window expired"
        );
        self.witness
            .as_ref()
            .context("live watchdog witness missing")?
            .ensure_live()
    }
}

pub async fn tick<A: ResourceApi>(
    api: &A,
    m: &mut DeploymentManifest,
    at: DateTime<Utc>,
    force_cleanup: bool,
    persistent: bool,
) -> Result<WatchReport> {
    m.validate()?;
    ensure!(
        m.created_at <= at + Duration::minutes(1),
        "deployment timestamp is in the future"
    );
    let previously_cleaned = !m.resources.is_empty() && m.resources.iter().all(|r| r.deleted);
    if m.closed || force_cleanup || at >= m.expires_at || previously_cleaned {
        // teardown authenticates scope and rechecks every resource, including prior deleted flags.
        let result = teardown(api, m, at).await?;
        let mut report = WatchReport::new(
            m,
            at,
            persistent,
            if result.complete {
                "complete"
            } else {
                "cleaning"
            },
        )?;
        report.confirmed_absent = result.confirmed_absent;
        report.pending = result.pending;
        Ok(report)
    } else {
        ensure!(
            api.credential_scope().await? == m.credential_scope_sha256,
            "watchdog account/credential scope mismatch"
        );
        WatchReport::new(
            m,
            at,
            persistent,
            if persistent { "armed" } else { "checked" },
        )
    }
}

pub async fn tick_file<A: ResourceApi>(
    api: &A,
    path: &Path,
    expected_identity: &str,
    at: DateTime<Utc>,
    force_cleanup: bool,
    persistent: bool,
) -> Result<WatchReport> {
    let store = ManifestFile::open(path)?;
    let mut manifest = store.load()?;
    ensure!(
        manifest.identity_sha256()? == expected_identity,
        "running watchdog refuses modified deployment identity/window"
    );
    if manifest.closed
        || force_cleanup
        || at >= manifest.expires_at
        || (!manifest.resources.is_empty() && manifest.resources.iter().all(|r| r.deleted))
    {
        // Fence tracking durably under the same lock before any cleanup API operation.
        manifest.closed = true;
        store.save(&manifest)?;
    }
    let result = tick(api, &mut manifest, at, force_cleanup, persistent).await;
    // Save verified removals even if a later lookup/deletion failed.
    store.save(&manifest)?;
    result
}

/// An exclusive lease per manifest, distinct from the short manifest update lock.
pub struct HeartbeatFile {
    path: PathBuf,
    identity: String,
    _watch_lock: File,
}
impl HeartbeatFile {
    pub fn open(path: &Path, manifest_path: &Path, m: &DeploymentManifest) -> Result<Self> {
        let lock_path = manifest_path.with_extension("watchdog.lock");
        if let Ok(metadata) = std::fs::symlink_metadata(&lock_path) {
            ensure!(metadata.file_type().is_file(), "invalid watchdog lock");
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options.open(lock_path)?;
        lock.try_lock()
            .context("Another watchdog already owns this deployment")?;
        let manifest = ManifestFile::open(manifest_path)?;
        ensure!(
            manifest.load()?.identity_sha256()? == m.identity_sha256()?,
            "watchdog manifest identity mismatch"
        );
        let identity = m.identity_sha256()?;
        if path.try_exists()? {
            ensure!(
                Self::read(path)?.manifest_identity == identity,
                "heartbeat belongs to another deployment"
            );
        }
        Ok(Self {
            path: path.into(),
            identity,
            _watch_lock: lock,
        })
    }
    pub fn read(path: &Path) -> Result<WatchReport> {
        let metadata = std::fs::symlink_metadata(path)?;
        ensure!(
            metadata.file_type().is_file() && metadata.len() <= 1_048_576,
            "invalid watchdog heartbeat"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            ensure!(
                metadata.permissions().mode() & 0o077 == 0,
                "heartbeat must be owner-only"
            );
        }
        let mut data = Vec::new();
        File::open(path)?.take(1_048_577).read_to_end(&mut data)?;
        let report: WatchReport = serde_json::from_slice(&data)?;
        ensure!(
            report.schema == "agent-rush.watchdog.v1",
            "invalid watchdog schema"
        );
        Ok(report)
    }
    pub fn write(&self, report: &WatchReport) -> Result<()> {
        ensure!(
            report.manifest_identity == self.identity,
            "refusing to write another deployment's heartbeat"
        );
        if self.path.try_exists()? {
            ensure!(
                Self::read(&self.path)?.manifest_identity == self.identity,
                "heartbeat identity changed"
            );
        }
        let parent = self
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        temp.write_all(&serde_json::to_vec_pretty(report)?)?;
        temp.as_file().sync_all()?;
        temp.persist(&self.path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    }
}
