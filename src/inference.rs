use crate::{
    agent::{AgentContext, ContextBudget, Role},
    config::{ModelCatalog, Provider, Settings},
    ledger::Ledger,
};
use anyhow::{Context, Result, ensure};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

pub const ENDPOINT: &str = "https://api.vultrinference.com/v1/chat/completions";
pub const ANTHROPIC_ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const INPUT_BYTES: usize = 32000;
/// Vultr output ceiling. GLM reasoning tokens count against it, so it leaves room for both.
const OUTPUT_TOKENS: u64 = 16384;
/// Cap on GLM reasoning tokens, so every role keeps half the output budget for its answer.
const REASONING_TOKENS: u64 = OUTPUT_TOKENS / 2;
/// Per-call output ceiling for every Claude role.
pub const ANTHROPIC_MAX_TOKENS: u64 = 100_000;
/// Haiku roles compact server-side once their input reaches this many tokens.
pub const COMPACTION_TRIGGER_TOKENS: u64 = 100_000;
/// Context budget for Claude roles: large enough that compaction, not local
/// truncation, is what normally bounds a Haiku conversation.
pub const ANTHROPIC_BUDGET: ContextBudget = ContextBudget {
    max_bytes: 1_200_000,
    max_messages: 300,
};

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
/// Attach the role's reasoning effort, with a token cap, to a Vultr request. Uncapped,
/// GLM 5.3 Flash at high effort can spend its whole output budget reasoning and return
/// empty content.
pub fn with_reasoning_effort(mut request: Value, role: Role) -> Value {
    request["reasoning"] =
        json!({"effort": anthropic_role(role).effort, "max_tokens": REASONING_TOKENS});
    request
}
/// Price an OpenAI-style usage block. Read before the answer is decoded, so a call whose
/// answer is unusable is still charged what it actually cost.
pub fn vultr_cost(usage: &Value, input_per_million: f64, output_per_million: f64) -> Option<f64> {
    let input = usage["prompt_tokens"].as_u64()?;
    let output = usage["completion_tokens"].as_u64()?;
    Some((input as f64 * input_per_million + output as f64 * output_per_million) / 1_000_000.0)
}
pub fn decode_response(body: Value) -> Result<Completion> {
    let message = &body["choices"][0]["message"];
    let content = message["content"].as_str().unwrap_or("");
    ensure!(
        !content.is_empty() || !message["reasoning"].is_string(),
        "model returned reasoning but no answer (finish_reason {})",
        body["choices"][0]["finish_reason"]
            .as_str()
            .unwrap_or("unknown")
    );
    ensure!(
        message["content"].is_string(),
        "provider returned no text completion"
    );
    // The same bound as the Anthropic path. 32 KiB was too small: a turn that writes a
    // whole module and its tests in one tool call failed the session.
    ensure!(
        !content.is_empty() && content.len() <= 400_000,
        "invalid model output length ({} bytes, finish_reason {})",
        content.len(),
        body["choices"][0]["finish_reason"]
            .as_str()
            .unwrap_or("unknown")
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

/// How one agent role calls Claude. Chosen by trusted code, never by model output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoleModel {
    pub model: &'static str,
    pub effort: &'static str,
    /// `Some("5m")` pins the cache TTL explicitly; `None` uses the API default.
    pub cache_ttl: Option<&'static str>,
    pub compaction: bool,
}
pub fn anthropic_role(role: Role) -> RoleModel {
    match role {
        Role::Orchestrator | Role::Solo => RoleModel {
            model: "claude-opus-5-5",
            effort: "medium",
            cache_ttl: None,
            compaction: false,
        },
        Role::Worker => RoleModel {
            model: "claude-haiku-5-5",
            effort: "high",
            cache_ttl: Some("5m"),
            compaction: true,
        },
        Role::Coder => RoleModel {
            model: "claude-haiku-5-5",
            effort: "xhigh",
            cache_ttl: None,
            compaction: true,
        },
        Role::Reviewer => RoleModel {
            model: "claude-opus-5-5",
            effort: "high",
            cache_ttl: None,
            compaction: false,
        },
    }
}

/// Anthropic list prices in USD per million tokens (platform pricing, Oct 2026).
/// Haiku 5.5 bills prompts over 100k tokens at the higher tier.
#[derive(Debug, Clone, Copy)]
pub struct Tariff {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
}
pub fn anthropic_tariff(model: &str, long_prompt: bool) -> Option<Tariff> {
    match (model, long_prompt) {
        ("claude-opus-5-5", _) => Some(Tariff {
            input: 4.0,
            output: 20.0,
            cache_read: 0.20,
        }),
        ("claude-haiku-5-5", false) => Some(Tariff {
            input: 0.10,
            output: 0.50,
            cache_read: 0.01,
        }),
        ("claude-haiku-5-5", true) => Some(Tariff {
            input: 0.50,
            output: 2.50,
            cache_read: 0.05,
        }),
        _ => None,
    }
}

pub fn build_anthropic_request(context: &AgentContext) -> Result<(Value, bool)> {
    let spec = anthropic_role(context.role);
    ensure!(
        context.messages.len() >= 2 && context.messages[0]["role"] == "system",
        "context must start with its role policy"
    );
    let system = context.messages[0]["content"]
        .as_str()
        .context("role policy must be text")?;
    let mut cache = json!({"type":"ephemeral"});
    if let Some(ttl) = spec.cache_ttl {
        cache["ttl"] = json!(ttl);
    }
    let mut request = json!({
        "model": spec.model,
        "max_tokens": ANTHROPIC_MAX_TOKENS,
        "system": [{"type":"text","text":system,"cache_control":cache}],
        "messages": &context.messages[1..],
        "output_config": {"effort": spec.effort},
        "cache_control": cache,
    });
    if spec.compaction {
        request["context_management"] = json!({"edits":[{
            "type": "compact_20260112",
            "trigger": {"type":"input_tokens","value":COMPACTION_TRIGGER_TOKENS},
            "instructions": "Summarize this agent conversation for continuation. Keep the original task, every requirement, the latest code in full, sandbox evidence (exit codes, failing checks, error text) and what remains to be done. Omit pleasantries."
        }]});
    }
    Ok((request, spec.compaction))
}

#[derive(Debug)]
pub struct AnthropicCompletion {
    pub text: String,
    pub compaction: Option<Value>,
    pub provider_id: String,
    pub model: String,
    pub usage: Value,
}
pub fn decode_anthropic(body: Value) -> Result<AnthropicCompletion> {
    let stop = body["stop_reason"].as_str().unwrap_or("");
    ensure!(
        stop != "refusal",
        "model declined the request ({})",
        body["stop_details"]["category"]
            .as_str()
            .unwrap_or("unspecified")
    );
    let blocks = body["content"]
        .as_array()
        .context("provider returned no content")?;
    let text: String = blocks
        .iter()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let compaction = blocks
        .iter()
        .find(|b| b["type"] == "compaction" && b["content"].is_string())
        .cloned();
    ensure!(
        !text.trim().is_empty() && text.len() <= 400_000,
        "invalid model output length (stop reason {stop})"
    );
    ensure!(body["usage"].is_object(), "missing provider usage");
    Ok(AnthropicCompletion {
        text,
        compaction,
        provider_id: body["id"].as_str().unwrap_or("unavailable").into(),
        model: body["model"].as_str().unwrap_or("unavailable").into(),
        usage: body["usage"].clone(),
    })
}

/// Billed tokens across all sampling iterations, including a compaction pass.
#[derive(Debug, Default, PartialEq)]
pub struct BilledTokens {
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
}
pub fn billed_tokens(usage: &Value) -> BilledTokens {
    let n = |v: &Value, k: &str| v[k].as_u64().unwrap_or(0);
    let (input, output) = match usage["iterations"].as_array() {
        Some(iterations) if !iterations.is_empty() => iterations.iter().fold((0, 0), |acc, i| {
            (acc.0 + n(i, "input_tokens"), acc.1 + n(i, "output_tokens"))
        }),
        _ => (n(usage, "input_tokens"), n(usage, "output_tokens")),
    };
    BilledTokens {
        input,
        output,
        cache_write: n(usage, "cache_creation_input_tokens"),
        cache_read: n(usage, "cache_read_input_tokens"),
    }
}
/// Cost in USD. Unknown models (e.g. a server-side fallback) return None, and the caller
/// keeps the full reservation rather than guessing a price.
pub fn anthropic_cost(model: &str, usage: &Value) -> Option<f64> {
    let t = billed_tokens(usage);
    let long = t.input + t.cache_write + t.cache_read > 100_000;
    let rate = anthropic_tariff(model, long)?;
    Some(
        (t.input as f64 * rate.input
            + t.cache_write as f64 * rate.input * 1.25
            + t.cache_read as f64 * rate.cache_read
            + t.output as f64 * rate.output)
            / 1_000_000.0,
    )
}
/// Upper bound for one call: worst-case input for the serialized context (about one token
/// per three bytes), full output, and a second pass when compaction can run.
pub fn anthropic_reservation(role: Role, context_bytes: usize) -> f64 {
    let spec = anthropic_role(role);
    let rate = anthropic_tariff(spec.model, true).expect("role models are priced");
    let input = (context_bytes / 3 + 4096) as f64;
    let once =
        (input * rate.input * 1.25 + ANTHROPIC_MAX_TOKENS as f64 * rate.output) / 1_000_000.0;
    if spec.compaction { once * 2.0 } else { once }
}

#[derive(Clone)]
pub struct InferenceGateway {
    client: reqwest::Client,
    settings: Arc<Settings>,
    ledger: Arc<Ledger>,
    /// The models a session may name, with the tariffs its calls are priced at.
    catalog: Arc<ModelCatalog>,
    vultr_endpoint: String,
}
impl InferenceGateway {
    pub fn new(settings: Arc<Settings>, ledger: Arc<Ledger>) -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(600))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            catalog: Arc::new(settings.model_catalog()),
            settings,
            ledger,
            vultr_endpoint: ENDPOINT.into(),
        })
    }
    /// Sends Vultr calls to a local stand-in instead of Vultr. Tests only.
    #[doc(hidden)]
    pub fn with_vultr_endpoint(mut self, url: &str) -> Self {
        self.vultr_endpoint = url.into();
        self
    }
    pub fn context_budget(&self) -> ContextBudget {
        match self.settings.inference_provider {
            Provider::Anthropic => ANTHROPIC_BUDGET,
            Provider::Vultr => crate::agent::COMPACT_BUDGET,
        }
    }
    /// Returns the assistant text, an optional compaction block, and a usage receipt.
    pub async fn complete(&self, context: &AgentContext) -> Result<(String, Option<Value>, Value)> {
        self.complete_with_effort(context, None).await
    }
    /// `complete` with a reasoning effort chosen by trusted code (an agent config
    /// preference of low, medium or high) instead of the role default.
    pub async fn complete_with_effort(
        &self,
        context: &AgentContext,
        effort: Option<&str>,
    ) -> Result<(String, Option<Value>, Value)> {
        self.complete_with_model(context, effort, None).await
    }
    /// `complete_with_effort` on a session's model: an ID from the operator's model list,
    /// priced at its listed tariff. `None` is the default model. Anthropic chooses its
    /// model per role and ignores it.
    pub async fn complete_with_model(
        &self,
        context: &AgentContext,
        effort: Option<&str>,
        model: Option<&str>,
    ) -> Result<(String, Option<Value>, Value)> {
        if let Some(effort) = effort {
            ensure!(
                crate::agent_config::REASONING_EFFORTS.contains(&effort),
                "unsupported reasoning effort"
            );
        }
        match self.settings.inference_provider {
            Provider::Vultr => {
                let (completion, receipt) = self.complete_vultr(context, effort, model).await?;
                Ok((completion.content, None, receipt))
            }
            Provider::Anthropic => self.complete_anthropic(context, effort).await,
        }
    }
    async fn read_bounded(response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            ensure!(
                bytes.len() + chunk.len() <= limit,
                "provider response exceeds bound"
            );
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
    async fn complete_anthropic(
        &self,
        context: &AgentContext,
        effort: Option<&str>,
    ) -> Result<(String, Option<Value>, Value)> {
        ensure!(
            self.settings.inference_configured(),
            "Anthropic API key must be configured"
        );
        let (mut request, compaction) = build_anthropic_request(context)?;
        if let Some(effort) = effort {
            request["output_config"]["effort"] = json!(effort);
        }
        let call_id = uuid::Uuid::new_v4().to_string();
        let reservation =
            anthropic_reservation(context.role, serde_json::to_vec(&context.messages)?.len());
        self.ledger
            .reserve_spend(&call_id, reservation, self.settings.inference_budget_usd)?;
        // What the spend ledger holds for this call; an error past this point carries it
        // so callers that account spend (sessions) never under-report a failed call.
        let mut charged = reservation;
        let outcome = async {
            let mut call = self
                .client
                .post(ANTHROPIC_ENDPOINT)
                .header("x-api-key", &self.settings.anthropic_key)
                .header("anthropic-version", "2023-06-01");
            if compaction {
                call = call.header("anthropic-beta", "compact-2026-01-12");
            }
            let response = call
                .json(&request)
                .send()
                .await
                .context("Anthropic request failed; reservation retained")?;
            let status = response.status();
            let bytes = Self::read_bounded(response, 4 * 1024 * 1024).await?;
            if !status.is_success() {
                let detail: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
                // A structured API error is not billed; anything else keeps the reservation.
                let unbilled = detail["type"] == "error";
                if unbilled {
                    self.ledger.settle_spend(&call_id, 0.0)?;
                    charged = 0.0;
                }
                anyhow::bail!(
                    "Anthropic returned HTTP {} ({}); {}",
                    status.as_u16(),
                    crate::agent::clip(detail["error"]["message"].as_str().unwrap_or(""), 300),
                    if unbilled {
                        "not billed"
                    } else {
                        "reservation retained"
                    }
                );
            }
            // Settle from usage before decoding, so a refusal or malformed turn is still
            // charged its actual cost instead of holding the worst-case reservation.
            let body: Value = serde_json::from_slice(&bytes)?;
            let spec = anthropic_role(context.role);
            // A dated snapshot ID in the response is priced as the role's configured model.
            let cost = anthropic_cost(body["model"].as_str().unwrap_or(""), &body["usage"])
                .or_else(|| anthropic_cost(spec.model, &body["usage"]));
            let accounted = cost.unwrap_or(reservation);
            // Record what was actually billed even when it breaks the envelope, then stop.
            self.ledger.settle_spend(&call_id, accounted)?;
            charged = accounted;
            ensure!(
                accounted <= reservation,
                "provider usage exceeds cost reservation; stop and reconcile billing"
            );
            let completion = decode_anthropic(body)?;
            let tokens = billed_tokens(&completion.usage);
            let receipt = json!({
                "call_id": call_id,
                "provider": "Anthropic",
                "origin": "https://api.anthropic.com/v1",
                "role": context.role,
                "model": completion.model,
                "effort": effort.unwrap_or(spec.effort),
                "cache_ttl": spec.cache_ttl.unwrap_or("default"),
                "provider_request_id": completion.provider_id,
                "input_tokens": tokens.input,
                "output_tokens": tokens.output,
                "cache_read_input_tokens": tokens.cache_read,
                "cache_creation_input_tokens": tokens.cache_write,
                "compacted": completion.compaction.is_some(),
                "accounted_usd": accounted,
                "price_source": if cost.is_some() { "anthropic list price" } else { "unpriced model; full reservation kept" },
                "at": crate::domain::now(),
            });
            Ok((completion.text, completion.compaction, receipt))
        }
        .await;
        outcome.map_err(|e| crate::session::ChargedError::wrap(e, charged))
    }
    async fn complete_vultr(
        &self,
        context: &AgentContext,
        effort: Option<&str>,
        model: Option<&str>,
    ) -> Result<(Completion, Value)> {
        ensure!(
            self.settings.inference_configured(),
            "Vultr inference key, model and verified pricing must be configured"
        );
        // The model and its tariff come from the operator's configuration, never from
        // model output: an ID outside the list is refused before anything is reserved.
        // Without a list every call runs the default model, as before models were chosen,
        // even for a session recorded under an earlier default.
        let chosen = model.filter(|m| !m.is_empty() && !self.settings.inference_models.is_empty());
        let (model, input, output) = match chosen {
            None => (
                self.settings.inference_model.as_str(),
                self.settings
                    .inference_input_per_million
                    .context("input price required")?,
                self.settings
                    .inference_output_per_million
                    .context("output price required")?,
            ),
            Some(id) => {
                let price = self
                    .catalog
                    .price(id)
                    .with_context(|| format!("model {id:?} is not allowed on this deployment"))?;
                (
                    price.id.as_str(),
                    price.input_usd_per_million,
                    price.output_usd_per_million,
                )
            }
        };
        validate_pricing(input, output)?;
        let mut request =
            with_reasoning_effort(build_request(model, &context.messages)?, context.role);
        if let Some(effort) = effort {
            request["reasoning"]["effort"] = json!(effort);
        }
        let call_id = uuid::Uuid::new_v4().to_string();
        // Reserve the complete maximum accepted usage, including unexpected provider accounting.
        let reservation = (64000.0 * input + OUTPUT_TOKENS as f64 * output) / 1_000_000.0;
        self.ledger.reserve_spend(
            &call_id,
            reservation.max(0.000001),
            self.settings.inference_budget_usd,
        )?;
        // What the spend ledger holds for this call; errors past this point carry it.
        let mut charged = reservation.max(0.000001);
        let outcome = async {
            // HTTP 429 means Vultr refused the call without running it: wait (its
            // Retry-After, or a growing backoff) and send again. If it keeps refusing, the
            // reservation is released, since nothing was billed.
            let mut attempt = 0;
            let response = loop {
                let response = self
                    .client
                    .post(&self.vultr_endpoint)
                    .bearer_auth(&self.settings.inference_key)
                    .json(&request)
                    .send()
                    .await
                    .context("Vultr inference request failed")?;
                if response.status() != reqwest::StatusCode::TOO_MANY_REQUESTS {
                    break response;
                }
                attempt += 1;
                if attempt >= RATE_LIMIT_ATTEMPTS {
                    self.ledger.settle_spend(&call_id, 0.0)?;
                    charged = 0.0;
                    anyhow::bail!(
                        "Vultr inference is rate limiting this model (HTTP 429 on {attempt} \
                         attempts); nothing was charged, try again shortly"
                    );
                }
                tokio::time::sleep(rate_limit_delay(&response, attempt)).await;
            };
            ensure!(
                response.status().is_success(),
                "Vultr inference returned HTTP {}; reservation retained",
                response.status().as_u16()
            );
            let bytes = Self::read_bounded(response, 262144).await?;
            let body: Value = serde_json::from_slice(&bytes)?;
            // Settle from usage before decoding, as on the Claude path; missing usage keeps
            // the conservative reservation.
            let cost = vultr_cost(&body["usage"], input, output).unwrap_or(reservation);
            self.ledger.settle_spend(&call_id, cost)?;
            charged = cost;
            ensure!(
                cost <= reservation,
                "provider usage exceeds cost reservation; stop and reconcile billing"
            );
            let completion = decode_response(body)?;
            // `model` is what the provider reported; `requested_model` is the configured
            // ID the call was sent as and priced at.
            let receipt = json!({"call_id":call_id,"provider":"Vultr Serverless Inference","origin":"https://api.vultrinference.com/v1","role":context.role,"effort":effort.unwrap_or(anthropic_role(context.role).effort),"model":completion.model,"requested_model":model,"input_usd_per_million":input,"output_usd_per_million":output,"provider_request_id":completion.provider_id,"input_tokens":completion.input_tokens,"output_tokens":completion.output_tokens,"accounted_usd":cost,"at":crate::domain::now()});
            Ok((completion, receipt))
        }
        .await;
        outcome.map_err(|e| crate::session::ChargedError::wrap(e, charged))
    }
}

/// Attempts a call makes while Vultr answers HTTP 429: about two and a half minutes of
/// backoff. Six attempts (about 45 s) were not enough for Flash with 72 sessions at once.
const RATE_LIMIT_ATTEMPTS: u32 = 12;

/// How long to wait after the `attempt`th 429: the provider's `Retry-After` seconds when
/// given (at most 30), otherwise 2, 4, 8, 16 s, plus up to a second of jitter so parallel
/// sessions do not retry in step.
fn rate_limit_delay(response: &reqwest::Response, attempt: u32) -> std::time::Duration {
    let jitter =
        std::time::Duration::from_millis(u64::from(uuid::Uuid::new_v4().as_bytes()[0]) * 4);
    let after = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok());
    match after {
        Some(0) => std::time::Duration::ZERO,
        Some(seconds) => std::time::Duration::from_secs(seconds.min(30)) + jitter,
        None => std::time::Duration::from_secs(1 << attempt.min(4)) + jitter,
    }
}

/// Adapts the inference gateway to sessions: role `Solo`, the gateway's context budget and
/// its existing spend reservation and settlement.
pub struct GatewayModel {
    pub gateway: InferenceGateway,
}
impl GatewayModel {
    async fn call(
        &self,
        system: &str,
        messages: &[Value],
        effort: Option<&str>,
        model: Option<&str>,
    ) -> Result<crate::session::ModelReply> {
        let mut context =
            AgentContext::with_role("session", "", Role::Solo, self.gateway.context_budget());
        context.messages = std::iter::once(json!({"role":"system","content":system}))
            .chain(messages.iter().cloned())
            .collect();
        let (text, _compaction, receipt) = self
            .gateway
            .complete_with_model(&context, effort, model)
            .await?;
        let tokens = |keys: &[&str]| keys.iter().filter_map(|k| receipt[*k].as_u64()).sum();
        Ok(crate::session::ModelReply {
            text,
            cost_usd: receipt["accounted_usd"].as_f64().unwrap_or(0.0),
            // Anthropic reports cached prompt tokens apart; they are prompt tokens too.
            input_tokens: tokens(&[
                "input_tokens",
                "cache_read_input_tokens",
                "cache_creation_input_tokens",
            ]),
            output_tokens: tokens(&["output_tokens"]),
        })
    }
}
impl crate::session::SessionModel for GatewayModel {
    fn complete<'a>(
        &'a self,
        system: &'a str,
        messages: &'a [Value],
    ) -> futures_util::future::BoxFuture<'a, Result<crate::session::ModelReply>> {
        Box::pin(self.call(system, messages, None, None))
    }
    fn complete_with_effort<'a>(
        &'a self,
        system: &'a str,
        messages: &'a [Value],
        effort: Option<&'a str>,
    ) -> futures_util::future::BoxFuture<'a, Result<crate::session::ModelReply>> {
        Box::pin(self.call(system, messages, effort, None))
    }
    fn complete_with_model<'a>(
        &'a self,
        model: &'a str,
        system: &'a str,
        messages: &'a [Value],
        effort: Option<&'a str>,
    ) -> futures_util::future::BoxFuture<'a, Result<crate::session::ModelReply>> {
        Box::pin(self.call(system, messages, effort, Some(model)))
    }
    fn budget(&self) -> ContextBudget {
        self.gateway.context_budget()
    }
}
