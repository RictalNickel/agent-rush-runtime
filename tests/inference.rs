use agent_rush::{
    agent::{AgentContext, Role},
    config::{Settings, parse_models},
    inference::{
        GatewayModel, InferenceGateway, build_request, decode_response, validate_pricing,
        vultr_cost, with_reasoning_effort,
    },
    ledger::Ledger,
    session::SessionModel,
};
use axum::{Json, Router, routing::post};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

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
    assert_eq!(r["max_tokens"], 16384);
    let r = with_reasoning_effort(r, Role::Coder);
    // Observed live: uncapped xhigh spent all 16384 tokens reasoning and never answered.
    // The cap always leaves half the output budget for the answer.
    assert_eq!(r["reasoning"], json!({"effort":"xhigh","max_tokens":8192}));
    assert!(r.get("reasoning_effort").is_none());
    assert_eq!(
        with_reasoning_effort(r.clone(), Role::Orchestrator)["reasoning"]["effort"],
        "medium"
    );
    assert_eq!(r["stream"], false);
}

#[test]
fn usage_is_priced_even_when_the_answer_is_unusable() {
    let body = json!({"choices":[{"finish_reason":"length","message":{"content":"","reasoning":"long chain"}}],"usage":{"prompt_tokens":1000,"completion_tokens":16384}});
    let cost = vultr_cost(&body["usage"], 0.10, 0.35).unwrap();
    assert!((cost - (1000.0 * 0.10 + 16384.0 * 0.35) / 1e6).abs() < 1e-12);
    assert!(decode_response(body).is_err());
    assert!(vultr_cost(&json!({}), 0.10, 0.35).is_none());
}

#[test]
fn missing_usage_or_empty_choices_cannot_be_claimed_as_success() {
    assert!(decode_response(json!({"choices":[]})).is_err());
    assert!(decode_response(json!({"choices":[{"message":{"content":"hello"}}]})).is_err());
    let r=decode_response(json!({"id":"real-provider-id","model":"test","choices":[{"message":{"content":"hello"}}],"usage":{"prompt_tokens":30,"completion_tokens":10}})).unwrap();
    assert_eq!(r.content, "hello");
    assert_eq!(r.input_tokens, 30);
}

/// A local stand-in for Vultr's chat completions: every answer reports 10000 prompt and
/// 2000 completion tokens and echoes the requested model. Returns its URL and the bodies
/// it received.
async fn fake_vultr() -> (String, Arc<Mutex<Vec<Value>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let app = Router::new().route(
        "/chat",
        post(move |Json(body): Json<Value>| {
            let log = log.clone();
            async move {
                log.lock().unwrap().push(body.clone());
                Json(json!({
                    "id": "provider-call",
                    "model": body["model"],
                    "choices": [{"message": {"content": "{\"tool\":\"reply\",\"text\":\"ok\"}"}}],
                    "usage": {"prompt_tokens": 10000, "completion_tokens": 2000},
                }))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/chat", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, seen)
}

fn glm_gateway(url: &str, budget: f64) -> (InferenceGateway, Arc<Ledger>) {
    let mut settings = Settings::local();
    settings.inference_key = "fixture-provider-key".into();
    settings.inference_model = "glm-5.3-flash".into();
    settings.pricing_verified = true;
    settings.inference_budget_usd = budget;
    settings
        .set_models(parse_models("glm-5.3-flash:0.10:0.35,glm-5.3:0.75:3.00").unwrap())
        .unwrap();
    let ledger = Arc::new(Ledger::memory(1024).unwrap());
    let gateway = InferenceGateway::new(Arc::new(settings), ledger.clone())
        .unwrap()
        .with_vultr_endpoint(url);
    (gateway, ledger)
}

#[tokio::test]
async fn each_session_model_is_sent_and_charged_at_its_own_tariff() {
    let (url, seen) = fake_vultr().await;
    let (gateway, ledger) = glm_gateway(&url, 5.0);
    let model = GatewayModel {
        gateway: gateway.clone(),
    };
    let messages = [json!({"role":"user","content":"hi"})];
    let full = model
        .complete_with_model("glm-5.3", "system", &messages, None)
        .await
        .unwrap();
    let flash = model
        .complete_with_model("glm-5.3-flash", "system", &messages, None)
        .await
        .unwrap();
    let default = model.complete("system", &messages).await.unwrap();
    let full_usd = (10000.0 * 0.75 + 2000.0 * 3.00) / 1e6;
    let flash_usd = (10000.0 * 0.10 + 2000.0 * 0.35) / 1e6;
    assert!(
        (full.cost_usd - full_usd).abs() < 1e-12,
        "{}",
        full.cost_usd
    );
    assert!((flash.cost_usd - flash_usd).abs() < 1e-12);
    assert!((default.cost_usd - flash_usd).abs() < 1e-12);
    assert_eq!((full.input_tokens, full.output_tokens), (10000, 2000));
    // The global ledger settled each call at its model's price.
    assert!((ledger.spend_total().unwrap() - (full_usd + 2.0 * flash_usd)).abs() < 1e-12);
    let sent: Vec<Value> = seen
        .lock()
        .unwrap()
        .iter()
        .map(|b| b["model"].clone())
        .collect();
    assert_eq!(
        sent,
        [
            json!("glm-5.3"),
            json!("glm-5.3-flash"),
            json!("glm-5.3-flash")
        ]
    );

    // The receipt names the model the call was sent as and the tariff it was priced at.
    let mut context = AgentContext::with_role("t", "", Role::Solo, gateway.context_budget());
    context.messages = vec![
        json!({"role":"system","content":"system"}),
        json!({"role":"user","content":"hi"}),
    ];
    let (_, _, receipt) = gateway
        .complete_with_model(&context, None, Some("glm-5.3"))
        .await
        .unwrap();
    assert_eq!(receipt["requested_model"], "glm-5.3");
    assert_eq!(receipt["model"], "glm-5.3");
    assert_eq!(receipt["input_usd_per_million"], 0.75);
    assert_eq!(receipt["output_usd_per_million"], 3.0);

    // A model outside the operator's list is refused before anything is reserved or sent.
    let before = ledger.spend_total().unwrap();
    let error = model
        .complete_with_model("gpt-9", "system", &messages, None)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("not allowed"), "{error}");
    assert_eq!(ledger.spend_total().unwrap(), before);
    assert_eq!(seen.lock().unwrap().len(), 4);
}

#[tokio::test]
async fn without_a_model_list_every_call_runs_the_default_model() {
    let (url, seen) = fake_vultr().await;
    let mut settings = Settings::local();
    settings.inference_key = "fixture-provider-key".into();
    settings.inference_model = "glm-5.3-flash".into();
    settings.inference_input_per_million = Some(0.10);
    settings.inference_output_per_million = Some(0.35);
    settings.pricing_verified = true;
    let ledger = Arc::new(Ledger::memory(1024).unwrap());
    let gateway = InferenceGateway::new(Arc::new(settings), ledger)
        .unwrap()
        .with_vultr_endpoint(&url);
    let model = GatewayModel { gateway };
    // A session recorded under an earlier default still runs, on today's default.
    let reply = model
        .complete_with_model(
            "glm-4.6",
            "system",
            &[json!({"role":"user","content":"hi"})],
            None,
        )
        .await
        .unwrap();
    assert!((reply.cost_usd - (10000.0 * 0.10 + 2000.0 * 0.35) / 1e6).abs() < 1e-12);
    assert_eq!(seen.lock().unwrap()[0]["model"], "glm-5.3-flash");
}

#[tokio::test]
async fn the_budget_reservation_uses_the_session_models_tariff() {
    let (url, seen) = fake_vultr().await;
    // A worst-case full GLM 5.3 call reserves about $0.097, a Flash call about $0.012.
    let (gateway, _ledger) = glm_gateway(&url, 0.05);
    let model = GatewayModel { gateway };
    let messages = [json!({"role":"user","content":"hi"})];
    let error = model
        .complete_with_model("glm-5.3", "system", &messages, None)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("budget"), "{error}");
    assert!(seen.lock().unwrap().is_empty());
    model
        .complete_with_model("glm-5.3-flash", "system", &messages, None)
        .await
        .unwrap();
}

#[test]
fn reasoning_without_an_answer_is_a_clear_failure() {
    // Observed live from GLM 5.3 Flash without a reasoning effort: all tokens spent reasoning.
    let body = json!({"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":"","reasoning":"long chain"}}],"usage":{"prompt_tokens":86,"completion_tokens":2454}});
    let error = decode_response(body).unwrap_err().to_string();
    assert!(error.contains("reasoning but no answer"), "{error}");
}
