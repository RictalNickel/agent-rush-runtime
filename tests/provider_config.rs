use agent_rush::config::provider_rate;

#[test]
fn missing_or_blank_provider_prices_keep_inference_unconfigured() {
    assert_eq!(provider_rate(None).unwrap(), None);
    assert_eq!(provider_rate(Some("")).unwrap(), None);
    assert_eq!(provider_rate(Some("  ")).unwrap(), None);
}

#[test]
fn verified_numeric_rate_must_be_finite_and_nonnegative() {
    assert_eq!(provider_rate(Some(" 0.25 ")).unwrap(), Some(0.25));
    for value in ["-1", "NaN", "inf", "automatic"] {
        assert!(provider_rate(Some(value)).is_err());
    }
}

#[test]
fn entered_numbers_alone_do_not_certify_a_provider_tariff() {
    let mut config = agent_rush::config::Settings::local();
    config.inference_key = "fixture-provider-key".into();
    config.inference_model = "vendor/GLM-5.3-Flash".into();
    config.inference_input_per_million = Some(0.0);
    config.inference_output_per_million = Some(0.0);
    assert!(!config.inference_configured());
    config.pricing_verified = true;
    assert!(config.inference_configured());
}
