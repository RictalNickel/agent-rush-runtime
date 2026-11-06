use anyhow::{Context, Result, ensure};
use serde::Serialize;
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

/// Most models `AR_INFERENCE_MODELS` may list.
pub const MAX_MODELS: usize = 16;

/// One model the operator allows sessions to use, with its verified tariff.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ModelPrice {
    pub id: String,
    pub input_usd_per_million: f64,
    pub output_usd_per_million: f64,
}

/// A provider model ID: 1-200 ASCII letters, digits and `._-/:`.
pub fn valid_model_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 200
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-/:".contains(c))
}

/// Parses `AR_INFERENCE_MODELS`: comma-separated `id:input:output` entries, prices in USD
/// per million tokens. A price is never blank or assumed; the ID may itself hold colons.
pub fn parse_models(raw: &str) -> Result<Vec<ModelPrice>> {
    let mut models: Vec<ModelPrice> = Vec::new();
    for entry in raw.split(',').map(str::trim).filter(|e| !e.is_empty()) {
        let mut parts = entry.rsplitn(3, ':');
        let (Some(output), Some(input), Some(id)) = (parts.next(), parts.next(), parts.next())
        else {
            anyhow::bail!(
                "model entry {entry:?} must be id:input_usd_per_million:output_usd_per_million"
            );
        };
        let id = id.trim();
        ensure!(valid_model_id(id), "invalid model ID {id:?}");
        ensure!(
            !models.iter().any(|m| m.id == id),
            "model {id:?} is listed twice"
        );
        let price = |raw: &str| {
            provider_rate(Some(raw))
                .ok()
                .flatten()
                .with_context(|| format!("model {id:?} needs two finite, nonnegative prices"))
        };
        models.push(ModelPrice {
            id: id.into(),
            input_usd_per_million: price(input)?,
            output_usd_per_million: price(output)?,
        });
    }
    ensure!(!models.is_empty(), "AR_INFERENCE_MODELS lists no models");
    ensure!(
        models.len() <= MAX_MODELS,
        "AR_INFERENCE_MODELS lists more than {MAX_MODELS} models"
    );
    Ok(models)
}

/// The models sessions may run on, as trusted configuration sets them. A model never
/// comes from model output.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ModelCatalog {
    pub provider: String,
    /// The model of a session that does not choose one; empty when none is configured.
    pub default: String,
    /// Models with their tariffs. With Anthropic this is informational: roles pick models.
    pub models: Vec<ModelPrice>,
    /// Whether a session may choose its model (Vultr only).
    pub per_session: bool,
}
impl ModelCatalog {
    pub fn price(&self, id: &str) -> Option<&ModelPrice> {
        self.models.iter().find(|m| m.id == id)
    }
    /// The model a new session runs on: the requested one, which must be listed, or the
    /// default.
    pub fn choose(&self, requested: Option<&str>) -> Result<String> {
        let Some(id) = requested else {
            return Ok(self.default.clone());
        };
        ensure!(
            self.per_session,
            "this deployment's inference provider ({}) cannot run a chosen model per session; leave out model",
            self.provider
        );
        ensure!(
            self.price(id).is_some(),
            "unknown model {id:?}; allowed models: {}",
            if self.models.is_empty() {
                "none configured".to_string()
            } else {
                self.models
                    .iter()
                    .map(|m| m.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        );
        Ok(id.to_string())
    }
}

/// Which inference service every agent role uses. Selected by the operator, never by a model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    /// Vultr Serverless Inference (OpenAI-compatible); required by the Blast Radius Zero track.
    Vultr,
    /// Anthropic Messages API with per-role Claude models.
    Anthropic,
}
impl Provider {
    pub fn parse(raw: &str) -> Result<Self> {
        match raw.trim() {
            "" | "vultr" => Ok(Provider::Vultr),
            "anthropic" => Ok(Provider::Anthropic),
            other => anyhow::bail!("unknown inference provider {other:?}"),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Provider::Vultr => "vultr",
            Provider::Anthropic => "anthropic",
        }
    }
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
    pub inference_provider: Provider,
    pub anthropic_key: String,
    pub inference_key: String,
    pub inference_model: String,
    pub inference_input_per_million: Option<f64>,
    pub inference_output_per_million: Option<f64>,
    /// Models a Vultr session may choose (`AR_INFERENCE_MODELS`); empty means only
    /// `inference_model` at the `AR_*_USD_PER_MILLION` tariffs.
    pub inference_models: Vec<ModelPrice>,
    pub pricing_verified: bool,
    pub inference_budget_usd: f64,
    pub capacity_mb: u32,
    pub sandbox_image: String,
    pub qualification_report: Option<PathBuf>,
    /// Operator-configured remote MCP servers (`AR_MCP_SERVERS_FILE`); credentials stay here.
    pub mcp_servers_file: Option<PathBuf>,
    /// Optional local voice service (`AR_VOICE_URL`); `/v1/voice/*` is 404 without it.
    pub voice_url: Option<String>,
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
            inference_provider: Provider::Vultr,
            anthropic_key: String::new(),
            inference_key: String::new(),
            inference_model: String::new(),
            inference_input_per_million: None,
            inference_output_per_million: None,
            inference_models: Vec::new(),
            pricing_verified: false,
            inference_budget_usd: 5.0,
            capacity_mb: 2048,
            sandbox_image: String::new(),
            qualification_report: None,
            mcp_servers_file: None,
            voice_url: None,
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
        s.inference_provider =
            Provider::parse(&env::var("AR_INFERENCE_PROVIDER").unwrap_or_default())?;
        s.anthropic_key = env::var("ANTHROPIC_API_KEY").unwrap_or_default();
        s.inference_key = env::var("VULTR_INFERENCE_API_KEY").unwrap_or_default();
        s.inference_model = env::var("AR_INFERENCE_MODEL").unwrap_or_default();
        s.inference_input_per_million =
            provider_rate(env::var("AR_INPUT_USD_PER_MILLION").ok().as_deref())?;
        s.inference_output_per_million =
            provider_rate(env::var("AR_OUTPUT_USD_PER_MILLION").ok().as_deref())?;
        if s.inference_provider == Provider::Vultr
            && let Some(raw) = env::var("AR_INFERENCE_MODELS")
                .ok()
                .filter(|v| !v.trim().is_empty())
        {
            s.set_models(parse_models(&raw)?)?;
        }
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
        s.qualification_report = env::var("AR_QUALIFICATION_REPORT")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from);
        s.mcp_servers_file = env::var("AR_MCP_SERVERS_FILE")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from);
        s.voice_url = env::var("AR_VOICE_URL")
            .ok()
            .map(|v| v.trim().trim_end_matches('/').to_string())
            .filter(|v| !v.is_empty());
        if let Some(url) = &s.voice_url {
            let parsed = reqwest::Url::parse(url)?;
            ensure!(
                matches!(parsed.scheme(), "http" | "https"),
                "voice URL must use http or https"
            );
            ensure!(
                parsed.username().is_empty() && parsed.password().is_none(),
                "voice URL must not embed credentials"
            );
        }
        Ok(s)
    }
    /// Sets the models sessions may choose. The default model must be one of them, and
    /// its listed tariff becomes the default tariff; an `AR_*_USD_PER_MILLION` value that
    /// disagrees with the list stops startup rather than picking one.
    pub fn set_models(&mut self, models: Vec<ModelPrice>) -> Result<()> {
        let default = models
            .iter()
            .find(|m| m.id == self.inference_model)
            .with_context(|| {
                format!(
                    "AR_INFERENCE_MODEL {:?} must be one of the AR_INFERENCE_MODELS",
                    self.inference_model
                )
            })?;
        for (set, listed, name) in [
            (
                self.inference_input_per_million,
                default.input_usd_per_million,
                "AR_INPUT_USD_PER_MILLION",
            ),
            (
                self.inference_output_per_million,
                default.output_usd_per_million,
                "AR_OUTPUT_USD_PER_MILLION",
            ),
        ] {
            ensure!(
                set.is_none_or(|v| v == listed),
                "{name} differs from the AR_INFERENCE_MODELS price of {}",
                default.id
            );
        }
        self.inference_input_per_million = Some(default.input_usd_per_million);
        self.inference_output_per_million = Some(default.output_usd_per_million);
        self.inference_models = models;
        Ok(())
    }
    /// The models sessions may run on. Vultr: the `AR_INFERENCE_MODELS` list, or the one
    /// default model when its tariffs are set. Anthropic: the role model, not choosable.
    pub fn model_catalog(&self) -> ModelCatalog {
        match self.inference_provider {
            Provider::Anthropic => {
                let model = crate::inference::anthropic_role(crate::agent::Role::Solo).model;
                ModelCatalog {
                    provider: Provider::Anthropic.name().into(),
                    default: model.into(),
                    models: crate::inference::anthropic_tariff(model, false)
                        .map(|t| ModelPrice {
                            id: model.into(),
                            input_usd_per_million: t.input,
                            output_usd_per_million: t.output,
                        })
                        .into_iter()
                        .collect(),
                    per_session: false,
                }
            }
            Provider::Vultr => {
                let models = if !self.inference_models.is_empty() {
                    self.inference_models.clone()
                } else {
                    match (
                        self.inference_input_per_million,
                        self.inference_output_per_million,
                    ) {
                        (Some(input), Some(output)) if !self.inference_model.is_empty() => {
                            vec![ModelPrice {
                                id: self.inference_model.clone(),
                                input_usd_per_million: input,
                                output_usd_per_million: output,
                            }]
                        }
                        _ => Vec::new(),
                    }
                };
                ModelCatalog {
                    provider: Provider::Vultr.name().into(),
                    default: self.inference_model.clone(),
                    models,
                    per_session: true,
                }
            }
        }
    }
    pub fn inference_configured(&self) -> bool {
        if self.inference_provider == Provider::Anthropic {
            // Claude roles are priced from Anthropic's published list prices.
            return self.anthropic_key.len() >= 20;
        }
        !self.inference_key.is_empty()
            && !self.inference_model.is_empty()
            && self.inference_input_per_million.is_some()
            && self.inference_output_per_million.is_some()
            && self.pricing_verified
    }
}
