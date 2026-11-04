use agent_rush::deploy::lifecycle::{
    DeploymentManifest, ManifestFile, ResourceApi, ResourceKind, ResourceRole, teardown,
};
use chrono::{DateTime, Duration, Utc};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Mutex};

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-11-04T05:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}
fn manifest() -> DeploymentManifest {
    DeploymentManifest::new("ord", now(), 0.2, 2.0, 0.0, "a".repeat(64)).unwrap()
}
#[derive(Default)]
struct Cloud {
    resources: Mutex<HashMap<String, Value>>,
    pending: bool,
    wrong_scope: bool,
    pending_id: Option<String>,
    lookup_error_id: Option<String>,
    deleted_ids: Mutex<Vec<String>>,
}
impl ResourceApi for Cloud {
    async fn credential_scope(&self) -> anyhow::Result<String> {
        Ok(if self.wrong_scope { "b" } else { "a" }.repeat(64))
    }
    async fn lookup(&self, _kind: ResourceKind, id: &str) -> anyhow::Result<Option<Value>> {
        anyhow::ensure!(
            self.lookup_error_id.as_deref() != Some(id),
            "fixture lookup failure"
        );
        Ok(self.resources.lock().unwrap().get(id).cloned())
    }
    async fn delete(&self, _kind: ResourceKind, id: &str) -> anyhow::Result<()> {
        self.deleted_ids.lock().unwrap().push(id.into());
        if !self.pending && self.pending_id.as_deref() != Some(id) {
            self.resources.lock().unwrap().remove(id);
        }
        Ok(())
    }
}
fn vm(m: &DeploymentManifest, id: &str, role: ResourceRole) -> Value {
    json!({"id":id,"region":"ord","label":m.label(role),"tags":[m.tag()],"date_created":"2026-11-04T05:00:01Z"})
}

#[test]
fn lifetime_budget_ids_and_duplicate_resources_are_bounded() {
    assert!(DeploymentManifest::new("ord", now(), 0.31, 2.0, 0.0, "a".repeat(64)).is_err());
    assert!(DeploymentManifest::new("ord", now(), 0.2, 3.0, 0.0, "a".repeat(64)).is_err());
    assert!(DeploymentManifest::new("ord", now(), 0.2, 2.0, 249.9, "a".repeat(64)).is_err());
    let mut m = manifest();
    assert!(
        m.track(ResourceKind::Instance, "../other", ResourceRole::Control)
            .is_err()
    );
    m.track(ResourceKind::Instance, "vm-1", ResourceRole::Control)
        .unwrap();
    assert!(
        m.track(ResourceKind::Instance, "vm-1", ResourceRole::Worker)
            .is_err()
    );
    assert!(
        m.track(ResourceKind::Instance, "vm-2", ResourceRole::Shared)
            .is_err()
    );
}

#[tokio::test]
async fn cleanup_never_deletes_a_preexisting_or_wrong_project_resource() {
    let mut m = manifest();
    m.track(ResourceKind::Instance, "vm-1", ResourceRole::Control)
        .unwrap();
    let c = Cloud::default();
    let mut wrong = vm(&m, "vm-1", ResourceRole::Control);
    wrong["tags"] = json!(["other-project"]);
    c.resources.lock().unwrap().insert("vm-1".into(), wrong);
    assert!(
        teardown(&c, &mut m, now() + Duration::minutes(5))
            .await
            .is_err()
    );
    assert!(c.resources.lock().unwrap().contains_key("vm-1"));
    assert!(!m.resources[0].deleted);
}

#[tokio::test]
async fn expired_cleanup_is_verified_and_repeatable_without_touching_unrelated_resources() {
    let mut m = manifest();
    m.track(ResourceKind::Instance, "vm-1", ResourceRole::Control)
        .unwrap();
    m.hourly_estimate_usd = 999.0; // A bad cost outcome must never prevent deleting owned resources.
    let c = Cloud::default();
    c.resources
        .lock()
        .unwrap()
        .insert("vm-1".into(), vm(&m, "vm-1", ResourceRole::Control));
    c.resources
        .lock()
        .unwrap()
        .insert("unrelated".into(), json!({"id":"unrelated"}));
    let report = teardown(&c, &mut m, now() + Duration::hours(3))
        .await
        .unwrap();
    assert!(report.complete);
    assert_eq!(c.resources.lock().unwrap().len(), 1);
    assert!(c.resources.lock().unwrap().contains_key("unrelated"));
    assert!(
        teardown(&c, &mut m, now() + Duration::hours(4))
            .await
            .unwrap()
            .complete
    );
}

#[tokio::test]
async fn vpc_with_unrelated_attachments_is_not_deleted() {
    let mut m = manifest();
    m.track(ResourceKind::Vpc, "vpc-1", ResourceRole::Shared)
        .unwrap();
    let c = Cloud::default();
    c.resources.lock().unwrap().insert("vpc-1".into(),json!({"id":"vpc-1","region":"ord","description":m.label(ResourceRole::Shared),"date_created":"2026-11-04T05:00:01Z","attachment_count":1,"nat_gateway_count":0}));
    assert!(
        teardown(&c, &mut m, now() + Duration::minutes(2))
            .await
            .is_err()
    );
    assert!(c.resources.lock().unwrap().contains_key("vpc-1"));
}

#[tokio::test]
async fn pending_vm_deletion_blocks_network_dependency_deletion() {
    let mut m = manifest();
    m.track(ResourceKind::Vpc, "vpc-1", ResourceRole::Shared)
        .unwrap();
    m.track(ResourceKind::Instance, "vm-1", ResourceRole::Control)
        .unwrap();
    let c = Cloud {
        pending: true,
        ..Cloud::default()
    };
    c.resources
        .lock()
        .unwrap()
        .insert("vm-1".into(), vm(&m, "vm-1", ResourceRole::Control));
    c.resources.lock().unwrap().insert("vpc-1".into(),json!({"id":"vpc-1","region":"ord","description":m.label(ResourceRole::Shared),"date_created":"2026-11-04T05:00:01Z"}));
    let report = teardown(&c, &mut m, now() + Duration::minutes(1))
        .await
        .unwrap();
    assert!(!report.complete);
    assert_eq!(report.pending, vec!["vm-1"]);
    assert!(c.resources.lock().unwrap().contains_key("vpc-1"));
}

#[test]
fn manifest_file_is_private_locked_and_preserves_identity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deployment.json");
    let m = manifest();
    let store = ManifestFile::create(&path, &m).unwrap();
    assert!(ManifestFile::open(&path).is_err());
    assert_eq!(store.load().unwrap().deployment_id, m.deployment_id);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let other = manifest();
    assert!(store.save(&other).is_err());
    drop(store);
    // A concurrent command test can fork just before drop and briefly inherit the CLOEXEC fd.
    // Verify release after that exec boundary rather than assuming an instantaneous global release.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match ManifestFile::open(&path) {
            Ok(_) => break,
            Err(error) => {
                assert!(std::time::Instant::now() < deadline, "{error:#}");
                std::thread::yield_now();
            }
        }
    }
}

#[tokio::test]
async fn reappearing_pending_resource_cannot_reuse_a_previous_confirmed_flag() {
    let mut m = manifest();
    m.track(ResourceKind::Instance, "vm-1", ResourceRole::Control)
        .unwrap();
    m.resources[0].deleted = true;
    let c = Cloud {
        pending: true,
        ..Cloud::default()
    };
    c.resources
        .lock()
        .unwrap()
        .insert("vm-1".into(), vm(&m, "vm-1", ResourceRole::Control));
    let report = teardown(&c, &mut m, now() + Duration::minutes(2))
        .await
        .unwrap();
    assert!(!report.complete);
    assert!(!m.resources[0].deleted);
}

#[test]
fn cleanup_preview_does_not_need_cloud_keys_or_valid_app_price_settings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deployment.json");
    let m = manifest();
    drop(ManifestFile::create(&path, &m).unwrap());
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_agent-rush"))
        .args(["vultr-teardown", "--manifest"])
        .arg(&path)
        .env_remove("VULTR_API_KEY")
        .env(
            "AR_INPUT_USD_PER_MILLION",
            "deliberately-invalid-app-setting",
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["dry_run"], true);
}

#[tokio::test]
async fn wrong_account_scope_cannot_turn_invisible_resources_into_cleanup_success() {
    let mut m = manifest();
    m.track(ResourceKind::Instance, "vm-1", ResourceRole::Control)
        .unwrap();
    let c = Cloud {
        wrong_scope: true,
        ..Cloud::default()
    };
    assert!(
        teardown(&c, &mut m, now() + Duration::minutes(1))
            .await
            .is_err()
    );
    assert!(!m.resources[0].deleted);
    assert!(c.deleted_ids.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_pending_instance_does_not_delay_other_independent_instances() {
    let mut m = manifest();
    m.track(ResourceKind::Instance, "control", ResourceRole::Control)
        .unwrap();
    m.track(ResourceKind::Instance, "worker", ResourceRole::Worker)
        .unwrap();
    m.track(ResourceKind::Vpc, "vpc-1", ResourceRole::Shared)
        .unwrap();
    let c = Cloud {
        pending_id: Some("control".into()),
        ..Cloud::default()
    };
    c.resources
        .lock()
        .unwrap()
        .insert("control".into(), vm(&m, "control", ResourceRole::Control));
    c.resources
        .lock()
        .unwrap()
        .insert("worker".into(), vm(&m, "worker", ResourceRole::Worker));
    let report = teardown(&c, &mut m, now() + Duration::minutes(1))
        .await
        .unwrap();
    assert!(!report.complete);
    assert_eq!(*c.deleted_ids.lock().unwrap(), vec!["control", "worker"]);
    assert_eq!(report.pending, vec!["control"]);
    assert!(m.resources[1].deleted);
    assert!(!m.resources[2].deleted);
}

#[tokio::test]
async fn an_instance_lookup_error_still_allows_other_instance_cleanup() {
    let mut m = manifest();
    m.track(ResourceKind::Instance, "control", ResourceRole::Control)
        .unwrap();
    m.track(ResourceKind::Instance, "worker", ResourceRole::Worker)
        .unwrap();
    m.track(ResourceKind::Vpc, "vpc-1", ResourceRole::Shared)
        .unwrap();
    let c = Cloud {
        lookup_error_id: Some("control".into()),
        ..Cloud::default()
    };
    c.resources
        .lock()
        .unwrap()
        .insert("worker".into(), vm(&m, "worker", ResourceRole::Worker));
    assert!(
        teardown(&c, &mut m, now() + Duration::minutes(1))
            .await
            .is_err()
    );
    assert_eq!(*c.deleted_ids.lock().unwrap(), vec!["worker"]);
    assert!(!m.resources[0].deleted);
    assert!(m.resources[1].deleted);
    assert!(!m.resources[2].deleted);
}
