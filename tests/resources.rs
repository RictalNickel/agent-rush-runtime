use agent_rush::{domain::JobRequest, ledger::Ledger, resources::ResourcePolicy};
use serde_json::json;
use std::sync::Arc;
fn request(memory: u32) -> JobRequest {
    serde_json::from_value(json!({"prompt":"Compute totals","memory_mb":memory})).unwrap()
}
#[test]
fn headroom_is_reserved_and_capacity_never_exceeds_the_actual_node() {
    let p = ResourcePolicy::new(8192, 9000).unwrap();
    assert_eq!(p.usable_memory_mb, 6144);
    assert_eq!(p.headroom_mb, 2048);
    assert_eq!(p.max_concurrent, 2);
    assert_eq!(
        ResourcePolicy::new(8192, 1024).unwrap().usable_memory_mb,
        1024
    );
    assert!(ResourcePolicy::new(256, 1024).is_err());
}
#[test]
fn slot_and_memory_admission_are_atomic_even_with_concurrent_schedulers() {
    let db = Arc::new(Ledger::memory(8192).unwrap());
    let jobs: Vec<_> = (0..8)
        .map(|i| {
            db.create_job("alice", request(256), &i.to_string())
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
            .filter(|h| h.thread().id() != std::thread::current().id())
            .map(|h| h.join().unwrap() as usize)
            .sum::<usize>(),
        2
    );
    assert_eq!(db.reserved_memory().unwrap(), 512);
}
#[test]
fn advertised_node_capacity_can_only_reduce_controller_admission() {
    let db = Ledger::memory(4096).unwrap();
    let j = db.create_job("alice", request(512), "a").unwrap();
    assert!(db.claim_with_capacity(&j.id, 256).unwrap().is_none());
    assert_eq!(db.reserved_memory().unwrap(), 0);
    assert!(db.claim_with_capacity(&j.id, 512).unwrap().is_some());
}
#[test]
fn scheduler_rotates_owners_and_does_not_depend_on_ui_history() {
    let db = Ledger::memory(4096).unwrap();
    let a = db.create_job("alice", request(256), "a").unwrap();
    db.create_job("alice", request(256), "a2").unwrap();
    let b = db.create_job("bob", request(256), "b").unwrap();
    assert_eq!(db.scheduling_candidates().unwrap()[0].id, a.id);
    db.claim(&a.id).unwrap();
    assert_eq!(db.scheduling_candidates().unwrap()[0].id, b.id);
}
#[test]
fn memory_growth_cannot_change_only_the_ledger_of_a_running_sandbox() {
    let db = Ledger::memory(2048).unwrap();
    let j = db.create_job("alice", request(512), "a").unwrap();
    db.claim(&j.id).unwrap();
    db.event(&j.id, "executing", "Sandbox already running")
        .unwrap();
    assert!(db.grow("alice", &j.id, 768).is_err());
    assert_eq!(db.reserved_memory().unwrap(), 512);
    db.event(&j.id, "verifying", "Previous sandbox cleanup verified")
        .unwrap();
    assert_eq!(db.grow("alice", &j.id, 768).unwrap().memory_mb, 768);
}
