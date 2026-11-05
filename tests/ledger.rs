use agent_rush::{domain::JobRequest, ledger::Ledger};
use std::sync::Arc;

/// These tests exercise admission arithmetic with a 512 MiB lease, not the request default.
fn request(prompt: &str) -> JobRequest {
    serde_json::from_value(serde_json::json!({"prompt": prompt, "memory_mb": 512})).unwrap()
}

#[test]
fn idempotency_conflict_and_owner_isolation() {
    let db = Ledger::memory(768).unwrap();
    let a = db
        .create_job("alice", request("Compute revenue totals"), "same")
        .unwrap();
    assert_eq!(
        db.create_job("alice", request("Compute revenue totals"), "same")
            .unwrap()
            .id,
        a.id
    );
    assert!(
        db.create_job("alice", request("Another request"), "same")
            .is_err()
    );
    assert!(db.get_job("bob", &a.id).unwrap().is_none());
    assert!(db.list_jobs("bob").unwrap().is_empty());
}

#[test]
fn simultaneous_admission_cannot_overcommit() {
    let db = Arc::new(Ledger::memory(768).unwrap());
    let jobs: Vec<_> = (0..8)
        .map(|i| {
            db.create_job("alice", request("Compute revenue totals"), &i.to_string())
                .unwrap()
        })
        .collect();
    let threads: Vec<_> = jobs
        .into_iter()
        .map(|j| {
            let db = db.clone();
            std::thread::spawn(move || db.claim(&j.id).unwrap().is_some())
        })
        .collect();
    assert_eq!(
        threads
            .into_iter()
            .filter_map(|h| h.join().ok())
            .filter(|x| *x)
            .count(),
        1
    );
    assert_eq!(db.reserved_memory().unwrap(), 512);
}

#[test]
fn cancelled_lease_survives_restart_until_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("jobs.db");
    let db = Ledger::open(&path, 1024).unwrap();
    let j = db
        .create_job("alice", request("Calculate totals"), "one")
        .unwrap();
    db.claim(&j.id).unwrap();
    assert!(db.cancel("alice", &j.id).unwrap().cancel_requested);
    drop(db);
    let db = Ledger::open(&path, 1024).unwrap();
    assert_eq!(db.reserved_memory().unwrap(), 512);
    db.finish(
        &j.id,
        serde_json::json!({"reason":"cancelled", "cleanup_state":"failed"}),
    )
    .unwrap();
    assert_eq!(db.reserved_memory().unwrap(), 512);
    db.finish(
        &j.id,
        serde_json::json!({"reason":"cancelled", "cleanup_state":"verified"}),
    )
    .unwrap();
    assert_eq!(db.reserved_memory().unwrap(), 0);
}

#[test]
fn success_requires_actual_checks_and_cleanup() {
    let db = Ledger::memory(1024).unwrap();
    let j = db
        .create_job("alice", request("Calculate totals"), "one")
        .unwrap();
    db.claim(&j.id).unwrap();
    db.finish(&j.id, serde_json::json!({"reason":"completed", "exit_code":0, "cleanup_state":"verified", "checks_passed":false})).unwrap();
    assert_eq!(db.get_job("alice", &j.id).unwrap().unwrap().state, "failed");
}

#[test]
fn growth_requires_capacity_and_policy() {
    let db = Ledger::memory(1024).unwrap();
    let j = db
        .create_job("alice", request("Calculate totals"), "one")
        .unwrap();
    db.claim(&j.id).unwrap();
    assert_eq!(db.grow("alice", &j.id, 768).unwrap().memory_mb, 768);
    assert!(db.grow("alice", &j.id, 2048).is_err());
    assert_eq!(db.reserved_memory().unwrap(), 768);
}

#[test]
fn untrusted_requests_cannot_set_authority_or_unbounded_resources() {
    for payload in [
        serde_json::json!({"prompt":"test", "owner":"admin"}),
        serde_json::json!({"prompt":"test", "profile":"privileged"}),
        serde_json::json!({"prompt":"test", "memory_mb":100000}),
    ] {
        let parsed = serde_json::from_value::<JobRequest>(payload);
        assert!(parsed.is_err() || parsed.unwrap().validate().is_err());
    }
}

#[test]
fn repeated_active_cancellation_is_idempotent_and_does_not_strand_finalization() {
    let db = Ledger::memory(1024).unwrap();
    let j = db
        .create_job("alice", request("Calculate totals"), "one")
        .unwrap();
    db.claim(&j.id).unwrap();
    for _ in 0..300 {
        db.cancel("alice", &j.id).unwrap();
    }
    assert!(db.get_job("alice", &j.id).unwrap().unwrap().events.len() < 10);
    db.event(&j.id, "cleaning", "Cancellation acknowledged")
        .unwrap();
    db.finish(
        &j.id,
        serde_json::json!({"reason":"cancelled","cleanup_state":"verified"}),
    )
    .unwrap();
    assert_eq!(db.reserved_memory().unwrap(), 0);
}

#[test]
fn scheduler_queue_cannot_be_hidden_by_newer_terminal_history() {
    let db = Ledger::memory(1024).unwrap();
    let oldest = db
        .create_job("alice", request("Old queued task"), "old")
        .unwrap();
    for i in 0..110 {
        let job = db
            .create_job("alice", request("New cancelled task"), &format!("new-{i}"))
            .unwrap();
        db.cancel("alice", &job.id).unwrap();
    }
    assert!(
        !db.list_jobs("alice")
            .unwrap()
            .iter()
            .any(|j| j.id == oldest.id)
    );
    assert_eq!(db.queued_jobs("alice").unwrap()[0].id, oldest.id);
}

#[test]
fn diagnostic_history_is_bounded_without_breaking_finalization() {
    let db = Ledger::memory(1024).unwrap();
    let j = db
        .create_job("alice", request("Calculate totals"), "one")
        .unwrap();
    db.claim(&j.id).unwrap();
    for _ in 0..300 {
        db.event(&j.id, "progress", "Measured checkpoint").unwrap();
    }
    let j = db.get_job("alice", &j.id).unwrap().unwrap();
    assert_eq!(j.events.len(), 256);
    assert!(j.dropped_events > 0);
    db.finish(
        &j.id,
        serde_json::json!({"reason":"cancelled","cleanup_state":"verified"}),
    )
    .unwrap();
    assert_eq!(db.reserved_memory().unwrap(), 0);
}
