use agent_rush::{
    inference::{build_request, decode_response, validate_pricing},
    ledger::Ledger,
};
use serde_json::json;

#[test]
fn inference_reservations_are_atomic_and_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.db");
    let db = Ledger::open(&path, 1024).unwrap();
    db.reserve_spend("call1", 0.4, 0.5).unwrap();
    assert!(db.reserve_spend("call2", 0.2, 0.5).is_err());
    drop(db);
    let db = Ledger::open(&path, 1024).unwrap();
    assert_eq!(db.spend_total().unwrap(), 0.4);
    db.settle_spend("call1", 0.1).unwrap();
    db.reserve_spend("call2", 0.2, 0.5).unwrap();
    assert!((db.spend_total().unwrap() - 0.3).abs() < 0.000001);
}

#[test]
fn invalid_pricing_and_oversized_prompts_fail_closed() {
    assert!(validate_pricing(f64::NAN, 1.0).is_err());
    assert!(validate_pricing(-1.0, 1.0).is_err());
    assert!(
        build_request(
            "model",
            &[json!({"role":"user","content":"x".repeat(50000)})]
        )
        .is_err()
    );
    let r = build_request(
        "model",
        &[json!({"role":"user","content":"Compute totals"})],
    )
    .unwrap();
    assert_eq!(r["max_tokens"], 4096);
    assert_eq!(r["stream"], false);
}

#[test]
fn missing_usage_or_empty_choices_cannot_be_claimed_as_success() {
    assert!(decode_response(json!({"choices":[]})).is_err());
    assert!(decode_response(json!({"choices":[{"message":{"content":"hello"}}]})).is_err());
    let r=decode_response(json!({"id":"real-provider-id","model":"test","choices":[{"message":{"content":"hello"}}],"usage":{"prompt_tokens":30,"completion_tokens":10}})).unwrap();
    assert_eq!(r.content, "hello");
    assert_eq!(r.input_tokens, 30);
}
