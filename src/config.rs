use anyhow::{Result, ensure};
use std::{env, path::PathBuf};

/// Internal provider tariff input: blank means not discovered, never an assumed zero price.
pub fn provider_rate(raw: Option<&str>) -> Result<Option<f64>> {
    let Some(raw) = raw.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    let rate: f64 = raw.parse()?;
    ensure!(rate.is_finite() && rate >= 0.0, "invalid provider tariff");
    Ok(Some(rate))
}

#[derive(Clone)]
pub struct Settings {
    pub data_dir: PathBuf,
    pub bind: String,
    pub operator_password: String,
    pub secure_cookies: bool,
    pub worker_url: Option<String>,
    pub worker_token: String,
    pub worker_ca_path: Option<PathBuf>,
    pub worker_identity_path: Option<PathBuf>,
    pub inference_key: String,
    pub inference_model: String,
    pub inference_input_per_million: Option<f64>,
    pub inference_output_per_million: Option<f64>,
    pub pricing_verified: bool,
    pub inference_budget_usd: f64,
    pub capacity_mb: u32,
    pub sandbox_image: String,
}
impl Settings {
    pub fn local() -> Self {
        Self {
            data_dir: PathBuf::from("runtime"),
            bind: "127.0.0.1:8080".into(),
            operator_password: String::new(),
            secure_cookies: false,
            worker_url: None,
            worker_token: String::new(),
            worker_ca_path: None,
            worker_identity_path: None,
            inference_key: String::new(),
            inference_model: String::new(),
            inference_input_per_million: None,
            inference_output_per_million: None,
            pricing_verified: false,
            inference_budget_usd: 5.0,
            capacity_mb: 2048,
            sandbox_image: String::new(),
        }
    }
    pub fn from_env() -> Result<Self> {
        let mut s = Self::local();
        if let Ok(v) = env::var("AR_DATA_DIR") {
            s.data_dir = v.into();
        }
        if let Ok(v) = env::var("AR_BIND") {
            s.bind = v;
        }
        s.operator_password = env::var("AR_OPERATOR_PASSWORD").unwrap_or_default();
        s.secure_cookies =
            env::var("AR_SECURE_COOKIES").unwrap_or_else(|_| "true".into()) != "false";
        s.worker_url = env::var("AR_WORKER_URL").ok().filter(|v| !v.is_empty());
        if let Some(url) = &s.worker_url {
            let parsed = reqwest::Url::parse(url)?;
            ensure!(
                parsed.scheme() == "https"
                    || (parsed.scheme() == "http"
                        && matches!(parsed.host_str(), Some("127.0.0.1" | "localhost"))),
                "worker requires HTTPS or loopback"
            );
            ensure!(
                parsed.username().is_empty() && parsed.password().is_none(),
                "worker URL must not embed credentials"
            );
        }
        s.worker_token = env::var("AR_WORKER_TOKEN").unwrap_or_default();
        s.worker_ca_path = env::var("AR_WORKER_CA_PATH").ok().map(PathBuf::from);
        s.worker_identity_path = env::var("AR_WORKER_IDENTITY_PATH").ok().map(PathBuf::from);
        s.inference_key = env::var("VULTR_INFERENCE_API_KEY").unwrap_or_default();
        s.inference_model = env::var("AR_INFERENCE_MODEL").unwrap_or_default();
        s.inference_input_per_million =
            provider_rate(env::var("AR_INPUT_USD_PER_MILLION").ok().as_deref())?;
        s.inference_output_per_million =
            provider_rate(env::var("AR_OUTPUT_USD_PER_MILLION").ok().as_deref())?;
        s.pricing_verified = env::var("AR_PRICING_VERIFIED").is_ok_and(|v| v == "true");
        if let Ok(v) = env::var("AR_INFERENCE_BUDGET_USD") {
            s.inference_budget_usd = v.parse()?;
        }
        ensure!(
            s.inference_budget_usd.is_finite() && (0.0..=25.0).contains(&s.inference_budget_usd),
            "inference budget must be 0–25 USD"
        );
        for price in [
            s.inference_input_per_million,
            s.inference_output_per_million,
        ]
        .into_iter()
        .flatten()
        {
            ensure!(price.is_finite() && price >= 0.0, "invalid model pricing");
        }
        if let Ok(v) = env::var("AR_CAPACITY_MB") {
            s.capacity_mb = v.parse()?;
        }
        s.sandbox_image = env::var("AR_SANDBOX_IMAGE").unwrap_or_default();
        Ok(s)
    }
    pub fn inference_configured(&self) -> bool {
        !self.inference_key.is_empty()
            && !self.inference_model.is_empty()
            && self.inference_input_per_million.is_some()
            && self.inference_output_per_million.is_some()
            && self.pricing_verified
    }
}
