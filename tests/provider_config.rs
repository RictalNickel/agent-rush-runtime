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

use agent_rush::config::{ModelPrice, Provider, Settings, parse_models};

fn price(id: &str, input: f64, output: f64) -> ModelPrice {
    ModelPrice {
        id: id.into(),
        input_usd_per_million: input,
        output_usd_per_million: output,
    }
}

#[test]
fn the_model_list_parses_ids_with_colons_and_refuses_guessed_prices() {
    let models = parse_models(" glm-5.3-flash:0.10:0.35 , vendor/glm:5.3:0.75:3.00 ").unwrap();
    assert_eq!(
        models,
        [
            price("glm-5.3-flash", 0.10, 0.35),
            price("vendor/glm:5.3", 0.75, 3.00)
        ]
    );
    for bad in [
        "",
        "glm-5.3",
        "glm-5.3:0.75",
        "glm-5.3::3.00",
        "glm-5.3:0.75:-1",
        "glm-5.3:0.75:NaN",
        "glm 5.3:0.75:3.00",
        "glm-5.3:0.75:3.00,glm-5.3:0.75:3.00",
    ] {
        assert!(parse_models(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn the_default_model_must_be_listed_and_takes_its_listed_price() {
    let mut settings = Settings::local();
    settings.inference_model = "glm-5.3-flash".into();
    let list = parse_models("glm-5.3-flash:0.10:0.35,glm-5.3:0.75:3.00").unwrap();
    settings.set_models(list.clone()).unwrap();
    assert_eq!(settings.inference_input_per_million, Some(0.10));
    assert_eq!(settings.inference_output_per_million, Some(0.35));
    let catalog = settings.model_catalog();
    assert_eq!(catalog.default, "glm-5.3-flash");
    assert!(catalog.per_session);
    assert_eq!(catalog.models, list);
    assert_eq!(catalog.choose(None).unwrap(), "glm-5.3-flash");
    assert_eq!(catalog.choose(Some("glm-5.3")).unwrap(), "glm-5.3");
    let error = catalog.choose(Some("gpt-9")).unwrap_err().to_string();
    assert!(
        error.contains("unknown model \"gpt-9\"") && error.contains("glm-5.3-flash, glm-5.3"),
        "{error}"
    );

    // A default outside the list, or a tariff that disagrees with it, stops startup.
    let mut other = Settings::local();
    other.inference_model = "glm-4".into();
    assert!(other.set_models(list.clone()).is_err());
    let mut other = Settings::local();
    other.inference_model = "glm-5.3-flash".into();
    other.inference_output_per_million = Some(0.50);
    assert!(other.set_models(list).is_err());
}

#[test]
fn without_a_list_the_catalog_is_the_one_default_model() {
    let mut settings = Settings::local();
    assert!(settings.model_catalog().models.is_empty());
    settings.inference_model = "glm-5.3-flash".into();
    settings.inference_input_per_million = Some(0.10);
    settings.inference_output_per_million = Some(0.35);
    let catalog = settings.model_catalog();
    assert_eq!(catalog.models, [price("glm-5.3-flash", 0.10, 0.35)]);
    assert_eq!(catalog.choose(None).unwrap(), "glm-5.3-flash");
    assert!(catalog.choose(Some("glm-5.3")).is_err());

    // Anthropic picks a model per agent role, so a session cannot choose one.
    settings.inference_provider = Provider::Anthropic;
    let catalog = settings.model_catalog();
    assert!(!catalog.per_session);
    assert_eq!(catalog.choose(None).unwrap(), "claude-opus-5-5");
    let error = catalog
        .choose(Some("claude-opus-5-5"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("anthropic"), "{error}");
}
