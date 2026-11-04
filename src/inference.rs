use crate::{config::Settings, ledger::Ledger};
use anyhow::{Context, Result, ensure};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

pub const ENDPOINT: &str = "https://api.vultrinference.com/v1/chat/completions";
const INPUT_BYTES: usize = 32000;
const OUTPUT_TOKENS: u64 = 4096;

pub fn validate_pricing(input: f64, output: f64) -> Result<()> {
    ensure!(
        input.is_finite() && output.is_finite() && input >= 0.0 && output >= 0.0,
        "verified nonnegative model pricing required"
    );
    Ok(())
}
pub fn build_request(model: &str, messages: &[Value]) -> Result<Value> {
    ensure!(!model.is_empty() && model.len() <= 200, "model required");
    let content = serde_json::to_vec(messages)?;
    ensure!(
        content.len() <= INPUT_BYTES,
        "agent context exceeds fixed input budget"
    );
    ensure!(
        !messages.is_empty() && messages.len() <= 16,
        "invalid message count"
    );
    Ok(
        json!({"model":model,"messages":messages,"max_tokens":OUTPUT_TOKENS,"temperature":0.2,"stream":false,"response_format":{"type":"json_object"}}),
    )
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Completion {
    pub content: String,
    pub provider_id: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
}
pub fn decode_response(body: Value) -> Result<Completion> {
    let content = body["choices"][0]["message"]["content"]
        .as_str()
        .context("provider returned no text completion")?;
    ensure!(
        !content.is_empty() && content.len() <= 32768,
        "invalid model output length"
    );
    let input = body["usage"]["prompt_tokens"]
        .as_u64()
        .context("missing provider usage; retain conservative reservation")?;
    let output = body["usage"]["completion_tokens"]
        .as_u64()
        .context("missing provider usage; retain conservative reservation")?;
    ensure!(
        input <= 64000 && output <= OUTPUT_TOKENS,
        "provider usage exceeds configured envelope"
    );
    Ok(Completion {
        content: content.into(),
        provider_id: body["id"].as_str().unwrap_or("unavailable").into(),
        model: body["model"].as_str().unwrap_or("unavailable").into(),
        input_tokens: input,
        output_tokens: output,
    })
}

#[derive(Clone)]
pub struct InferenceGateway {
    client: reqwest::Client,
    settings: Arc<Settings>,
    ledger: Arc<Ledger>,
}
impl InferenceGateway {
    pub fn new(settings: Arc<Settings>, ledger: Arc<Ledger>) -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(90))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            settings,
            ledger,
        })
    }
    pub async fn complete(&self, messages: &[Value]) -> Result<(Completion, Value)> {
        ensure!(
            self.settings.inference_configured(),
            "Vultr inference key, model and verified pricing must be configured"
        );
        let input = self
            .settings
            .inference_input_per_million
            .context("input price required")?;
        let output = self
            .settings
            .inference_output_per_million
            .context("output price required")?;
        validate_pricing(input, output)?;
        let request = build_request(&self.settings.inference_model, messages)?;
        let call_id = uuid::Uuid::new_v4().to_string();
        // Reserve the complete maximum accepted usage, including unexpected provider accounting.
        let reservation = (64000.0 * input + OUTPUT_TOKENS as f64 * output) / 1_000_000.0;
        self.ledger.reserve_spend(
            &call_id,
            reservation.max(0.000001),
            self.settings.inference_budget_usd,
        )?;
        let response = self
            .client
            .post(ENDPOINT)
            .bearer_auth(&self.settings.inference_key)
            .json(&request)
            .send()
            .await
            .context("Vultr inference request failed")?;
        ensure!(
            response.status().is_success(),
            "Vultr inference returned HTTP {}; reservation retained",
            response.status().as_u16()
        );
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            ensure!(
                bytes.len() + chunk.len() <= 262144,
                "provider response exceeds bound"
            );
            bytes.extend_from_slice(&chunk);
        }
        let completion = decode_response(serde_json::from_slice(&bytes)?)?;
        let cost = (completion.input_tokens as f64 * input
            + completion.output_tokens as f64 * output)
            / 1_000_000.0;
        ensure!(
            cost <= reservation,
            "provider usage exceeds cost reservation; stop and reconcile billing"
        );
        self.ledger.settle_spend(&call_id, cost)?;
        let receipt = json!({"call_id":call_id,"provider":"Vultr Serverless Inference","origin":"https://api.vultrinference.com/v1","model":completion.model,"provider_request_id":completion.provider_id,"input_tokens":completion.input_tokens,"output_tokens":completion.output_tokens,"accounted_usd":cost,"at":crate::domain::now()});
        Ok((completion, receipt))
    }
}
