//! Trusted, exact-resource teardown. No creation authority is exposed to runtime agents.
use super::{DeploymentBudget, VultrClient};
use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    future::Future,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Instance,
    Vpc,
    Firewall,
    SshKey,
}
impl ResourceKind {
    fn collection(self) -> &'static str {
        match self {
            Self::Instance => "instances",
            Self::Vpc => "vpcs",
            Self::Firewall => "firewalls",
            Self::SshKey => "ssh-keys",
        }
    }
    fn envelope(self) -> &'static str {
        match self {
            Self::Instance => "instance",
            Self::Vpc => "vpc",
            Self::Firewall => "firewall_group",
            Self::SshKey => "ssh_key",
        }
    }
    fn order(self) -> u8 {
        match self {
            Self::Instance => 0,
            Self::Vpc => 1,
            Self::Firewall => 2,
            Self::SshKey => 3,
        }
    }
    pub fn path(self, id: &str) -> Result<String> {
        validate_id(id)?;
        Ok(format!("/v2/{}/{id}", self.collection()))
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum ResourceRole {
    Control,
    Worker,
    Shared,
}
impl ResourceRole {
    fn name(self) -> &'static str {
        match self {
            Self::Control => "control",
            Self::Worker => "worker",
            Self::Shared => "shared",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackedResource {
    pub kind: ResourceKind,
    pub id: String,
    pub role: ResourceRole,
    pub deleted: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentManifest {
    pub schema: String,
    pub deployment_id: Uuid,
    pub credential_scope_sha256: String,
    pub region: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub hourly_estimate_usd: f64,
    pub resources: Vec<TrackedResource>,
}
fn validate_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 128
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b)),
        "invalid resource identity"
    );
    Ok(())
}
impl DeploymentManifest {
    pub fn new(
        region: &str,
        at: DateTime<Utc>,
        hourly: f64,
        hours: f64,
        spent: f64,
        credential_scope_sha256: String,
    ) -> Result<Self> {
        DeploymentBudget::default().check(hourly, hours, spent)?;
        validate_id(region)?;
        let manifest = Self {
            schema: "agent-rush.deployment.v1".into(),
            deployment_id: Uuid::new_v4(),
            credential_scope_sha256,
            region: region.into(),
            created_at: at,
            expires_at: at + Duration::milliseconds((hours * 3_600_000.0) as i64),
            hourly_estimate_usd: hourly,
            resources: Vec::new(),
        };
        manifest.validate()?;
        Ok(manifest)
    }
    pub fn tag(&self) -> String {
        format!("agent-rush-{}", self.deployment_id)
    }
    pub fn label(&self, role: ResourceRole) -> String {
        format!("{}-{}", self.tag(), role.name())
    }
    pub fn track(&mut self, kind: ResourceKind, id: &str, role: ResourceRole) -> Result<()> {
        validate_id(id)?;
        ensure!(self.resources.len() < 16, "resource limit");
        ensure!(
            !self.resources.iter().any(|r| r.id == id),
            "duplicate resource identity"
        );
        ensure!(
            match kind {
                ResourceKind::Instance | ResourceKind::Firewall => role != ResourceRole::Shared,
                _ => role == ResourceRole::Shared,
            },
            "invalid resource role"
        );
        self.resources.push(TrackedResource {
            kind,
            id: id.into(),
            role,
            deleted: false,
        });
        Ok(())
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema == "agent-rush.deployment.v1" && self.deployment_id.get_version_num() == 4,
            "invalid manifest schema/identity"
        );
        validate_id(&self.region)?;
        ensure!(
            self.credential_scope_sha256.len() == 64
                && self
                    .credential_scope_sha256
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit()),
            "missing verified account/credential scope"
        );
        let life = self.expires_at - self.created_at;
        ensure!(
            life > Duration::zero() && life <= Duration::hours(2),
            "invalid deployment lifetime"
        );
        ensure!(
            self.hourly_estimate_usd.is_finite() && self.hourly_estimate_usd > 0.0,
            "invalid recorded price"
        );
        let mut check = self.clone();
        check.resources.clear();
        for r in &self.resources {
            check.track(r.kind, &r.id, r.role)?;
        }
        Ok(())
    }
    fn verify_resource(
        &self,
        r: &TrackedResource,
        value: &Value,
        now: DateTime<Utc>,
    ) -> Result<()> {
        ensure!(value["id"] == r.id, "provider identity mismatch");
        let label = match r.kind {
            ResourceKind::Instance => "label",
            ResourceKind::SshKey => "name",
            _ => "description",
        };
        ensure!(
            value[label] == self.label(r.role),
            "resource is not owned by this exact deployment"
        );
        if r.kind == ResourceKind::Instance {
            ensure!(
                value["tags"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|v| v == &self.tag())),
                "deployment tag missing"
            );
        }
        if matches!(r.kind, ResourceKind::Instance | ResourceKind::Vpc) {
            ensure!(value["region"] == self.region, "region mismatch");
        }
        let created = value["date_created"]
            .as_str()
            .context("resource creation timestamp missing")?;
        let created = DateTime::parse_from_rfc3339(created)
            .map(|t| t.with_timezone(&Utc))
            .or_else(|_| {
                NaiveDateTime::parse_from_str(created, "%Y-%m-%d %H:%M:%S").map(|t| t.and_utc())
            })?;
        ensure!(
            created >= self.created_at - Duration::minutes(1)
                && created <= now + Duration::minutes(1),
            "resource predates deployment or has invalid creation time"
        );
        if r.kind == ResourceKind::Firewall {
            ensure!(
                value["instance_count"].as_u64() == Some(0),
                "firewall still attached to instances"
            );
        }
        if r.kind == ResourceKind::Vpc {
            ensure!(
                value["attachment_count"].as_u64() == Some(0)
                    && value["nat_gateway_count"].as_u64() == Some(0),
                "VPC still has attachments or NAT gateways"
            );
        }
        Ok(())
    }
    pub fn preview(&self) -> Result<Value> {
        self.validate()?;
        Ok(
            json!({"deployment_id":self.deployment_id,"expires_at":self.expires_at,"dry_run":true,"resources":self.resources,"note":"No API operation performed; live ownership is rechecked before deletion"}),
        )
    }
}

pub trait ResourceApi: Sync {
    fn credential_scope(&self) -> impl Future<Output = Result<String>> + Send;
    fn lookup(
        &self,
        kind: ResourceKind,
        id: &str,
    ) -> impl Future<Output = Result<Option<Value>>> + Send;
    fn delete(&self, kind: ResourceKind, id: &str) -> impl Future<Output = Result<()>> + Send;
}
#[derive(Debug, Serialize)]
pub struct TeardownReport {
    pub complete: bool,
    pub confirmed_absent: usize,
    pub pending: Vec<String>,
}
pub async fn teardown<A: ResourceApi>(
    api: &A,
    m: &mut DeploymentManifest,
    now: DateTime<Utc>,
) -> Result<TeardownReport> {
    m.validate()?;
    ensure!(
        m.created_at <= now + Duration::minutes(1),
        "deployment timestamp is in the future"
    );
    ensure!(
        api.credential_scope().await? == m.credential_scope_sha256,
        "authenticated account/credential scope differs from deployment; reconcile before cleanup"
    );
    let mut order: Vec<usize> = (0..m.resources.len()).collect();
    order.sort_by_key(|i| m.resources[*i].kind.order());
    let mut report = TeardownReport {
        complete: false,
        confirmed_absent: 0,
        pending: Vec::new(),
    };
    let mut first_error = None;
    // Cleanup is deliberately allowed after expiry or budget exhaustion.
    for index in order {
        let r = m.resources[index].clone();
        if r.kind != ResourceKind::Instance
            && m.resources
                .iter()
                .any(|item| item.kind == ResourceKind::Instance && !item.deleted)
        {
            break;
        }
        m.resources[index].deleted = false;
        let result = async {
            if let Some(value) = api.lookup(r.kind, &r.id).await? {
                m.verify_resource(&r, &value, now)?;
                api.delete(r.kind, &r.id).await?;
                if api.lookup(r.kind, &r.id).await?.is_some() {
                    return Ok(false);
                }
            }
            Ok::<bool, anyhow::Error>(true)
        }
        .await;
        match result {
            Ok(true) => {
                m.resources[index].deleted = true;
                report.confirmed_absent += 1;
            }
            Ok(false) => report.pending.push(r.id),
            Err(error) => {
                report.pending.push(r.id);
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
        // Continue independent VM cleanup even after pending/error results. Preserve dependency order.
        if r.kind != ResourceKind::Instance && !m.resources[index].deleted {
            break;
        }
    }
    if let Some(error) = first_error {
        return Err(
            error.context("Cleanup incomplete; confirmed progress retained, retry required")
        );
    }
    report.complete = m.resources.iter().all(|r| r.deleted);
    Ok(report)
}

pub struct ManifestFile {
    path: PathBuf,
    _lock: File,
}
impl ManifestFile {
    fn lock(path: &Path) -> Result<Self> {
        let lock_path = path.with_extension("lock");
        if let Ok(m) = std::fs::symlink_metadata(&lock_path) {
            ensure!(m.file_type().is_file(), "invalid lock file");
        }
        let mut opts = OpenOptions::new();
        opts.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let lock = opts.open(lock_path)?;
        lock.try_lock()
            .context("Deployment is already being changed by another process")?;
        Ok(Self {
            path: path.into(),
            _lock: lock,
        })
    }
    pub fn create(path: &Path, m: &DeploymentManifest) -> Result<Self> {
        m.validate()?;
        let store = Self::lock(path)?;
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut file = opts.open(path)?;
        file.write_all(&serde_json::to_vec_pretty(m)?)?;
        file.sync_all()?;
        Ok(store)
    }
    pub fn open(path: &Path) -> Result<Self> {
        let store = Self::lock(path)?;
        store.load()?;
        Ok(store)
    }
    pub fn load(&self) -> Result<DeploymentManifest> {
        let meta = std::fs::symlink_metadata(&self.path)?;
        ensure!(
            meta.file_type().is_file() && meta.len() <= 1_048_576,
            "invalid manifest file"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            ensure!(
                meta.permissions().mode() & 0o077 == 0,
                "manifest must be owner-only"
            );
        }
        let mut data = String::new();
        File::open(&self.path)?
            .take(1_048_577)
            .read_to_string(&mut data)?;
        let m: DeploymentManifest = serde_json::from_str(&data)?;
        m.validate()?;
        Ok(m)
    }
    pub fn save(&self, m: &DeploymentManifest) -> Result<()> {
        m.validate()?;
        ensure!(
            self.load()?.deployment_id == m.deployment_id,
            "refusing to replace another deployment"
        );
        let parent = self
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        temp.write_all(&serde_json::to_vec_pretty(m)?)?;
        temp.as_file().sync_all()?;
        temp.persist(&self.path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    }
}

impl ResourceApi for VultrClient {
    async fn credential_scope(&self) -> Result<String> {
        let account = self.get("/v2/account").await?;
        let email = account["account"]["email"]
            .as_str()
            .filter(|v| !v.trim().is_empty() && v.len() <= 320)
            .context("authenticated account identity missing")?;
        // Bind both the authenticated account and the exact high-entropy management key.
        // Key rotation/account changes require explicit reconciliation, never silent rebinding.
        let mut digest = Sha256::new();
        digest.update(b"agent-rush.vultr-scope.v1\0");
        digest.update(email.as_bytes());
        digest.update(b"\0");
        digest.update(self.key.as_bytes());
        Ok(format!("{:x}", digest.finalize()))
    }
    async fn lookup(&self, kind: ResourceKind, id: &str) -> Result<Option<Value>> {
        let response = self
            .client
            .get(format!("https://api.vultr.com{}", kind.path(id)?))
            .bearer_auth(&self.key)
            .send()
            .await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        ensure!(
            response.status().is_success(),
            "Vultr lookup HTTP {}",
            response.status().as_u16()
        );
        let body = super::decode_response(response).await?;
        let value = body
            .get(kind.envelope())
            .context("resource envelope missing")?;
        ensure!(value.is_object(), "invalid resource response");
        let mut value = value.clone();
        if kind == ResourceKind::Vpc {
            for (suffix, field, target) in [
                ("attachments", "attachments", "attachment_count"),
                ("nat-gateway", "nat_gateways", "nat_gateway_count"),
            ] {
                let details = self
                    .get(&format!("/v2/vpcs/{id}/{suffix}?per_page=100"))
                    .await?;
                let rows = details[field]
                    .as_array()
                    .context("VPC dependency list missing")?;
                let total = details["meta"]["total"]
                    .as_u64()
                    .context("VPC dependency count missing")?;
                let next = details["meta"]["links"]["next"].as_str().unwrap_or("");
                ensure!(
                    rows.is_empty() && total == 0 && next.is_empty(),
                    "VPC has dependencies or incomplete inventory"
                );
                value[target] = json!(0);
            }
        }
        Ok(Some(value))
    }
    async fn delete(&self, kind: ResourceKind, id: &str) -> Result<()> {
        let response = self
            .client
            .delete(format!("https://api.vultr.com{}", kind.path(id)?))
            .bearer_auth(&self.key)
            .send()
            .await?;
        ensure!(
            response.status().is_success() || response.status() == reqwest::StatusCode::NOT_FOUND,
            "Vultr deletion HTTP {}",
            response.status().as_u16()
        );
        Ok(())
    }
}
