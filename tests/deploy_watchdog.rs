use agent_rush::deploy::{
    lifecycle::{DeploymentManifest, ManifestFile, ResourceApi, ResourceKind, ResourceRole},
    watchdog::{HeartbeatFile, WatchReport, tick},
};
use chrono::{DateTime, Duration, Utc};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Mutex};
// Fork/exec briefly inherits flock descriptors until CLOEXEC. Keep process-spawning
// checks separate from file-lock checks so their results do not depend on that window.
static PROCESS_AND_FILE_TESTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn manifest(at: DateTime<Utc>) -> DeploymentManifest {
    DeploymentManifest::new("ord", at, 0.2, 2.0, 0.0, "a".repeat(64)).unwrap()
}
#[derive(Default)]
struct Cloud {
    wrong_scope: bool,
    pending: bool,
    fail_delete: bool,
    fail_delete_id: Option<String>,
    rows: Mutex<HashMap<String, Value>>,
    deletes: Mutex<Vec<String>>,
}
impl ResourceApi for Cloud {
    async fn credential_scope(&self) -> anyhow::Result<String> {
        Ok(if self.wrong_scope { "b" } else { "a" }.repeat(64))
    }
    async fn lookup(&self, _kind: ResourceKind, id: &str) -> anyhow::Result<Option<Value>> {
        Ok(self.rows.lock().unwrap().get(id).cloned())
    }
    async fn delete(&self, _kind: ResourceKind, id: &str) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.fail_delete && self.fail_delete_id.as_deref() != Some(id),
            "fixture deletion failed"
        );
        self.deletes.lock().unwrap().push(id.into());
        if !self.pending {
            self.rows.lock().unwrap().remove(id);
        }
        Ok(())
    }
}
fn tracked(at: DateTime<Utc>) -> (DeploymentManifest, Cloud) {
    let mut m = manifest(at);
    m.track(ResourceKind::Instance, "vm-1", ResourceRole::Control)
        .unwrap();
    let c = Cloud::default();
    c.rows.lock().unwrap().insert("vm-1".into(),json!({"id":"vm-1","region":"ord","label":m.label(ResourceRole::Control),"tags":[m.tag()],"date_created":at+Duration::seconds(1)}));
    (m, c)
}
#[tokio::test]
async fn watchdog_authenticates_but_never_deletes_before_expiry() {
    let _serial = PROCESS_AND_FILE_TESTS.lock().await;
    let at = Utc::now();
    let (mut m, c) = tracked(at);
    let r = tick(&c, &mut m, at + Duration::seconds(2), false, true)
        .await
        .unwrap();
    assert_eq!(r.phase, "armed");
    r.ensure_armed(&m, at + Duration::seconds(2)).unwrap();
    assert!(c.deletes.lock().unwrap().is_empty());
    let bad = Cloud {
        wrong_scope: true,
        ..Cloud::default()
    };
    assert!(
        tick(&bad, &mut m, at + Duration::seconds(3), false, true)
            .await
            .is_err()
    );
    assert!(!m.resources[0].deleted);
}
#[tokio::test]
async fn expiry_and_explicit_stop_verify_absence_and_finish() {
    let _serial = PROCESS_AND_FILE_TESTS.lock().await;
    for forced in [false, true] {
        let at = Utc::now();
        let (mut m, c) = tracked(at);
        let when = at
            + if forced {
                Duration::seconds(3)
            } else {
                Duration::hours(2)
            };
        let r = tick(&c, &mut m, when, forced, true).await.unwrap();
        assert_eq!(r.phase, "complete");
        assert_eq!(r.confirmed_absent, 1);
        assert!(m.resources[0].deleted);
        assert!(r.ensure_armed(&m, when).is_err());
    }
}
#[tokio::test]
async fn empty_initial_manifest_arms_until_deadline_instead_of_exiting_early() {
    let _serial = PROCESS_AND_FILE_TESTS.lock().await;
    let at = Utc::now();
    let mut m = manifest(at);
    let c = Cloud::default();
    assert_eq!(
        tick(&c, &mut m, at, false, true).await.unwrap().phase,
        "armed"
    );
    assert_eq!(
        tick(&c, &mut m, at + Duration::hours(2), false, true)
            .await
            .unwrap()
            .phase,
        "complete"
    );
}
#[tokio::test]
async fn pending_or_failed_deletion_cannot_be_reported_complete() {
    let _serial = PROCESS_AND_FILE_TESTS.lock().await;
    let at = Utc::now();
    let (mut m, mut c) = tracked(at);
    c.pending = true;
    let r = tick(&c, &mut m, at + Duration::hours(2), false, true)
        .await
        .unwrap();
    assert_eq!(r.phase, "cleaning");
    assert!(!m.resources[0].deleted);
    c.pending = false;
    c.fail_delete = true;
    assert!(
        tick(&c, &mut m, at + Duration::hours(2), false, true)
            .await
            .is_err()
    );
    assert!(!m.resources[0].deleted);
}
#[tokio::test]
async fn stale_one_shot_reused_pid_and_modified_identity_are_not_ready() {
    let _serial = PROCESS_AND_FILE_TESTS.lock().await;
    let at = Utc::now();
    let mut m = manifest(at);
    let c = Cloud::default();
    let r = tick(&c, &mut m, at, false, true).await.unwrap();
    assert!(r.ensure_armed(&m, at + Duration::seconds(31)).is_err());
    assert!(r.ensure_armed(&m, at - Duration::seconds(3)).is_err());
    let mut changed = m.clone();
    changed.region = "ewr".into();
    assert!(r.ensure_armed(&changed, at).is_err());
    let once = tick(&c, &mut m, at, false, false).await.unwrap();
    assert!(once.ensure_armed(&m, at).is_err());
    let mut dead = r.clone();
    dead.witness.as_mut().unwrap().start_ticks += 1;
    assert!(dead.ensure_armed(&m, at).is_err());
}
#[tokio::test]
async fn heartbeat_is_private_atomic_project_bound_and_exclusively_watched() {
    let _serial = PROCESS_AND_FILE_TESTS.lock().await;
    let at = Utc::now();
    let m = manifest(at);
    let dir = tempfile::tempdir().unwrap();
    let mp = dir.path().join("manifest.json");
    drop(ManifestFile::create(&mp, &m).unwrap());
    let hp = dir.path().join("watch.json");
    let store = HeartbeatFile::open(&hp, &mp, &m).unwrap();
    assert!(HeartbeatFile::open(&dir.path().join("other-watch.json"), &mp, &m).is_err());
    let r = WatchReport::error(&m, at, true, "fixture failure").unwrap();
    store.write(&r).unwrap();
    assert_eq!(HeartbeatFile::read(&hp).unwrap().phase, "error");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&hp).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let foreign = manifest(at);
    let bad = WatchReport::error(&foreign, at, true, "foreign").unwrap();
    assert!(store.write(&bad).is_err());
}
#[test]
fn preview_does_not_need_credentials_or_valid_app_configuration() {
    let _serial = PROCESS_AND_FILE_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mp = dir.path().join("manifest.json");
    drop(ManifestFile::create(&mp, &manifest(Utc::now())).unwrap());
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_agent-rush"))
        .args(["vultr-watch", "--manifest"])
        .arg(&mp)
        .env_remove("VULTR_API_KEY")
        .env("AR_INPUT_USD_PER_MILLION", "invalid-app-price")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["phase"], "preview");
    assert_eq!(value["watchdog_armed"], false);
}

#[tokio::test]
async fn exited_unreaped_watchdog_process_is_not_alive_for_admission() {
    let _serial = PROCESS_AND_FILE_TESTS.lock().await;
    let at = Utc::now();
    let mut m = manifest(at);
    let c = Cloud::default();
    let mut r = tick(&c, &mut m, at, false, true).await.unwrap();
    let mut child = std::process::Command::new("/bin/true").spawn().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let stat = loop {
        let text = std::fs::read_to_string(format!("/proc/{}/stat", child.id())).unwrap();
        let state = text
            .rsplit_once(") ")
            .unwrap()
            .1
            .split_whitespace()
            .next()
            .unwrap();
        if state == "Z" {
            break text;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.wait();
            panic!("child did not exit");
        }
        std::thread::yield_now();
    };
    r.witness = Some(agent_rush::deploy::watchdog::ProcessWitness {
        pid: child.id(),
        start_ticks: stat
            .rsplit_once(") ")
            .unwrap()
            .1
            .split_whitespace()
            .nth(19)
            .unwrap()
            .parse()
            .unwrap(),
    });
    let armed = r.ensure_armed(&m, Utc::now());
    child.wait().unwrap();
    assert!(armed.is_err(), "Zombie PID is not a live cleanup process");
}

#[tokio::test]
async fn later_api_failure_preserves_confirmed_cleanup_in_the_private_manifest() {
    let _serial = PROCESS_AND_FILE_TESTS.lock().await;
    let at = Utc::now();
    let (mut m, mut c) = tracked(at);
    m.track(ResourceKind::Instance, "vm-2", ResourceRole::Worker)
        .unwrap();
    c.rows.lock().unwrap().insert("vm-2".into(),json!({"id":"vm-2","region":"ord","label":m.label(ResourceRole::Worker),"tags":[m.tag()],"date_created":at+Duration::seconds(1)}));
    c.fail_delete_id = Some("vm-2".into());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deployment.json");
    drop(ManifestFile::create(&path, &m).unwrap());
    assert!(
        agent_rush::deploy::watchdog::tick_file(
            &c,
            &path,
            &m.identity_sha256().unwrap(),
            at + Duration::hours(2),
            false,
            true
        )
        .await
        .is_err()
    );
    let saved = ManifestFile::open(&path).unwrap().load().unwrap();
    assert!(saved.resources[0].deleted);
    assert!(!saved.resources[1].deleted);
    c.fail_delete_id = None;
    let report = agent_rush::deploy::watchdog::tick_file(
        &c,
        &path,
        &m.identity_sha256().unwrap(),
        at + Duration::hours(2),
        false,
        true,
    )
    .await
    .unwrap();
    assert_eq!(report.phase, "complete");
    assert_eq!(report.confirmed_absent, 2);
}

#[tokio::test]
async fn completion_atomically_closes_tracking_and_cannot_race_an_acknowledged_id() {
    let _serial = PROCESS_AND_FILE_TESTS.lock().await;
    let at = Utc::now();
    let m = manifest(at);
    let c = Cloud::default();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deployment.json");
    drop(ManifestFile::create(&path, &m).unwrap());
    let report = agent_rush::deploy::watchdog::tick_file(
        &c,
        &path,
        &m.identity_sha256().unwrap(),
        at + Duration::hours(2),
        false,
        true,
    )
    .await
    .unwrap();
    assert_eq!(report.phase, "complete");
    let store = ManifestFile::open(&path).unwrap();
    let mut after = store.load().unwrap();
    assert!(
        after
            .track(
                ResourceKind::Instance,
                "late-acknowledged-vm",
                ResourceRole::Worker
            )
            .is_err(),
        "completed deployment must not accept tracking"
    );
    let mut stale = m.clone();
    stale
        .track(
            ResourceKind::Instance,
            "stale-tracker",
            ResourceRole::Worker,
        )
        .unwrap();
    assert!(
        store.save(&stale).is_err(),
        "stale snapshots cannot reopen a completed deployment"
    );
}

#[tokio::test]
async fn early_cleanup_intent_survives_failure_and_restart_before_original_expiry() {
    let _serial = PROCESS_AND_FILE_TESTS.lock().await;
    let at = Utc::now();
    let (mut m, mut c) = tracked(at);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deployment.json");
    drop(ManifestFile::create(&path, &m).unwrap());
    c.fail_delete = true;
    assert!(
        agent_rush::deploy::watchdog::tick_file(
            &c,
            &path,
            &m.identity_sha256().unwrap(),
            at + Duration::seconds(2),
            true,
            true
        )
        .await
        .is_err()
    );
    m = ManifestFile::open(&path).unwrap().load().unwrap();
    assert!(
        m.track(ResourceKind::Instance, "new-vm", ResourceRole::Worker)
            .is_err()
    );
    c.fail_delete = false;
    let r = agent_rush::deploy::watchdog::tick_file(
        &c,
        &path,
        &m.identity_sha256().unwrap(),
        at + Duration::seconds(3),
        false,
        true,
    )
    .await
    .unwrap();
    assert_eq!(r.phase, "complete");
}
