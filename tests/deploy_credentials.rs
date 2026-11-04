use agent_rush::deploy::credentials::{
    export_inference_key, select_glm_flash, select_subscription,
};
use serde_json::json;

#[test]
fn model_selection_uses_exact_version_and_variant_without_fallback() {
    let catalog = json!({"data":[{"id":"vendor/GLM-5.2-Flash"},{"id":"vendor/GLM-5.3-Air"},{"id":"vendor/GLM-5.3-Flash-Preview"},{"id":"vendor/GLM-5.3-Flash"}]});
    assert_eq!(select_glm_flash(&catalog).unwrap(), "vendor/GLM-5.3-Flash");
    assert!(select_glm_flash(&json!({"data":[{"id":"glm-5.2-flash"}]})).is_err());
    assert!(
        select_glm_flash(&json!({"data":[{"id":"a/glm-5.3-flash"},{"id":"b/glm-5.3-flash"}]}))
            .is_err()
    );
}

#[test]
fn existing_subscription_selection_is_project_scoped_or_unambiguous() {
    let body = json!({"subscriptions":[{"id":"other","label":"other-project","api_key":"fixture-other"},{"id":"ours","label":"agent-rush-runtime","api_key":"fixture-ours"}]});
    let selected = select_subscription(&body, None).unwrap();
    assert_eq!(selected.id, "ours");
    assert!(select_subscription(&body, Some("missing")).is_err());
    assert!(select_subscription(&json!({"subscriptions":[{"id":"a","label":"a","api_key":"fixture-a"},{"id":"b","label":"b","api_key":"fixture-b"}]}),None).is_err());
}

#[test]
fn key_export_is_private_redacted_and_does_not_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("inference.env");
    let selected=select_subscription(&json!({"subscriptions":[{"id":"ours","label":"agent-rush-runtime","api_key":"fixture-test-key"}]}),None).unwrap();
    let metadata = export_inference_key(&selected, &path).unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "VULTR_INFERENCE_API_KEY='fixture-test-key'\n"
    );
    assert!(!metadata.to_string().contains("fixture-test-key"));
    assert!(export_inference_key(&selected, &path).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn malformed_secret_does_not_inject_environment_entries() {
    assert!(select_subscription(&json!({"subscriptions":[{"id":"ours","label":"agent-rush-runtime","api_key":"fake\nEVIL=yes"}]}),None).is_err());
}

#[test]
fn a_different_major_version_cannot_match_by_removing_decimal_punctuation() {
    assert!(select_glm_flash(&json!({"data":[{"id":"vendor/GLM-53-Flash"}]})).is_err());
}

#[test]
fn export_metadata_redacts_a_secret_repeated_in_a_user_label() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("inference.env");
    let chosen=select_subscription(&json!({"subscriptions":[{"id":"ours","label":"label-with-fixture-test-key","api_key":"fixture-test-key"}]}),None).unwrap();
    assert!(
        !export_inference_key(&chosen, &path)
            .unwrap()
            .to_string()
            .contains("fixture-test-key")
    );
}

#[test]
fn model_setup_exports_exact_id_without_guessing_provider_prices() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.env");
    let report = agent_rush::deploy::credentials::export_model_configuration(
        &json!({"data":[{"id":"vendor/GLM-5.3-Flash"}]}),
        &path,
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "AR_INFERENCE_MODEL='vendor/GLM-5.3-Flash'\n"
    );
    assert_eq!(report["paid_inference_enabled"], false);
    assert_eq!(report["provider_rates_need_verification"], true);
    assert!(select_glm_flash(&json!({"data":[{"id":"bad'namespace/GLM-5.3-Flash"}]})).is_err());
}
