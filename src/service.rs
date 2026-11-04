use crate::{
    agent::{AgentContext, ToolCall, parse_tool},
    api::AppState,
    domain::{Job, Limits},
    inference::InferenceGateway,
};
use anyhow::{Context, Result, bail, ensure};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone)]
struct NodeClient {
    client: reqwest::Client,
    base: String,
    token: String,
}
impl NodeClient {
    fn new(state: &AppState) -> Result<Self> {
        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .redirect(reqwest::redirect::Policy::none());
        if let Ok(path) = std::env::var("AR_WORKER_CA_PATH") {
            builder = builder
                .add_root_certificate(reqwest::Certificate::from_pem(&std::fs::read(path)?)?);
        }
        if let Ok(path) = std::env::var("AR_WORKER_IDENTITY_PATH") {
            builder = builder.identity(reqwest::Identity::from_pem(&std::fs::read(path)?)?);
        }
        ensure!(
            state.settings.worker_token.len() >= 32,
            "worker credential missing"
        );
        Ok(Self {
            client: builder.build()?,
            base: state
                .settings
                .worker_url
                .clone()
                .context("worker URL missing")?,
            token: state.settings.worker_token.clone(),
        })
    }
    async fn ready(&self) -> bool {
        self.client
            .get(format!("{}/v1/health", self.base))
            .bearer_auth(&self.token)
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
    }
    async fn get(&self, id: &str) -> Result<Value> {
        read_json(
            self.client
                .get(format!("{}/v1/runs/{id}", self.base))
                .bearer_auth(&self.token)
                .send()
                .await?,
        )
        .await
    }
    async fn submit(&self, id: &str, code: &str, limits: Limits) -> Result<()> {
        let response = self
            .client
            .post(format!("{}/v1/runs", self.base))
            .bearer_auth(&self.token)
            .json(&json!({"id":id,"code":code,"limits":limits}))
            .send()
            .await?;
        ensure!(
            response.status().is_success(),
            "worker rejected execution: HTTP {}",
            response.status().as_u16()
        );
        Ok(())
    }
    async fn cancel(&self, id: &str) -> Result<bool> {
        let response = self
            .client
            .delete(format!("{}/v1/runs/{id}", self.base))
            .bearer_auth(&self.token)
            .send()
            .await?;
        let result = read_json(response).await?;
        if result["cleanup_state"] == "verified" {
            return Ok(true);
        }
        for _ in 0..40 {
            tokio::time::sleep(Duration::from_millis(250)).await;
            if self
                .get(id)
                .await
                .ok()
                .is_some_and(|r| r["receipt"]["cleanup_state"] == "verified")
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
async fn read_json(response: reqwest::Response) -> Result<Value> {
    ensure!(
        response.status().is_success(),
        "worker returned HTTP {}",
        response.status().as_u16()
    );
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        ensure!(
            bytes.len() + chunk.len() <= 8 * 1024 * 1024,
            "worker response too large"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice(&bytes)?)
}
pub async fn run_once(state: &AppState) -> Result<usize> {
    if !state.settings.inference_configured()
        || state.settings.worker_url.is_none()
        || state.ledger.active_jobs()?.len() >= 2
    {
        return Ok(0);
    }
    let node = NodeClient::new(state)?;
    if !node.ready().await {
        return Ok(0);
    }
    let jobs = state.ledger.list_jobs("operator")?;
    for job in jobs.iter().rev().filter(|j| j.state == "queued") {
        if let Some(job) = state.ledger.claim(&job.id)? {
            let state = state.clone();
            let node = node.clone();
            tokio::spawn(async move {
                if let Err(e) = run_job(&state, &node, &job).await {
                    tracing::error!(job=%job.id,error=%e,"controller job failed");
                }
            });
            return Ok(1);
        }
    }
    Ok(0)
}

pub async fn scheduler(state: AppState, stop: Arc<AtomicBool>) -> Result<()> {
    reconcile(&state).await?;
    while !stop.load(Ordering::SeqCst) {
        if let Err(e) = run_once(&state).await {
            tracing::warn!(error=%e,"scheduler paused");
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    for job in state.ledger.active_jobs()? {
        state.ledger.cancel(&job.owner, &job.id)?;
    }
    for _ in 0..60 {
        if state.ledger.active_jobs()?.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Ok(())
}
pub async fn reconcile(state: &AppState) -> Result<()> {
    for job in state.ledger.active_jobs()? {
        let last = job
            .events
            .iter()
            .rev()
            .find(|e| e["kind"] == "node_run")
            .and_then(|e| e["message"].as_str());
        let clean = if let Some(id) = last {
            match NodeClient::new(state) {
                Ok(node) => node.cancel(id).await.unwrap_or(false),
                Err(_) => false,
            }
        } else {
            true
        };
        state.ledger.finish(&job.id,json!({"reason":"controller_restarted","checks_passed":false,"cleanup_state":if clean{"verified"}else{"failed"},"artifacts":[]}))?;
    }
    Ok(())
}
fn cancelled(state: &AppState, job: &Job) -> bool {
    state
        .ledger
        .get_job(&job.owner, &job.id)
        .ok()
        .flatten()
        .is_some_and(|j| j.cancel_requested)
}
async fn wait_cancel(state: &AppState, job: &Job) {
    loop {
        if cancelled(state, job) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}
async fn run_job(state: &AppState, node: &NodeClient, job: &Job) -> Result<()> {
    let gateway = InferenceGateway::new(state.settings.clone(), state.ledger.clone())?;
    let mut context = AgentContext::root(&job.id, &job.request.prompt);
    let mut receipts = Vec::new();
    let mut latest = None;
    let mut node_run = None;
    let mut delegated = false;
    let started = Instant::now();
    let result:Result<()>=async {
        for turn in 0..4 {
            ensure!(started.elapsed()<Duration::from_secs(300),"task lifetime exceeded");
            if cancelled(state,job){bail!("cancelled");}
            let completion=tokio::select!{result=gateway.complete(&context.messages)=>result?, _=wait_cancel(state,job)=>bail!("cancelled")};
            receipts.push(completion.1);
            context.messages.push(json!({"role":"assistant","content":completion.0.content}));
            match parse_tool(&completion.0.content)? {
                ToolCall::Response{message}=>{
                    state.ledger.event(&job.id,"explanation",&message)?;
                    ensure!(context.verified(),"agent stopped without verified execution");return Ok(());
                }
                ToolCall::Delegate{task}=>{
                    ensure!(!delegated && turn<3,"subordinate limit reached");delegated=true;
                    let child=context.subordinate(&task)?;
                    let completion=tokio::select!{result=gateway.complete(&child.messages)=>result?, _=wait_cancel(state,job)=>bail!("cancelled")};
                    receipts.push(completion.1);
                    let tool=parse_tool(&completion.0.content)?;
                    match tool {ToolCall::Sandbox{code,summary}=>{
                        state.ledger.event(&job.id,"subordinate",&format!("Logical subordinate {} proposed: {}",child.agent_id,summary))?;
                        context.messages.push(json!({"role":"user","content":format!("Subordinate proposed source; verify by executing with sandbox tool: {code}")}));
                    },ToolCall::Response{message}=>{context.messages.push(json!({"role":"user","content":format!("Subordinate suggestion (unverified): {message}")}));},_=>bail!("nested delegation denied")}
                }
                ToolCall::Sandbox{code,summary}=>{
                    let attempt=format!("{}-{turn}",job.id);node_run=Some(attempt.clone());
                    state.ledger.event(&job.id,"executing",&summary)?;
                    state.ledger.event(&job.id,"node_run",&attempt)?;
                    node.submit(&attempt,&code,Limits{memory_mb:job.memory_mb,timeout_seconds:job.request.timeout_seconds,cpu_millis:1000}).await?;
                    let deadline=Instant::now()+Duration::from_secs(job.request.timeout_seconds as u64+45);
                    let receipt=loop {
                        if cancelled(state,job){bail!("cancelled");}
                        ensure!(Instant::now()<deadline,"worker response deadline exceeded");
                        let result=node.get(&attempt).await?;
                        if result["state"]=="finished"{break result["receipt"].clone();}
                        tokio::time::sleep(Duration::from_millis(250)).await;
                    };
                    context.observe(&receipt);latest=Some(receipt);node_run=None;
                    state.ledger.event(&job.id,"verifying",if context.verified(){"Rust process exited zero; result JSON and cleanup checks passed"}else{"Execution checks failed; agent may retry within the fixed turn limit"})?;
                    if context.verified(){return Ok(());}
                    ensure!(latest.as_ref().is_some_and(|r|r["cleanup_state"]=="verified"),"previous worker cleanup unresolved");
                }
            }
        }
        bail!("maximum agent turns reached")
    }.await;
    let clean = if let Some(id) = node_run {
        node.cancel(&id).await.unwrap_or(false)
    } else {
        latest
            .as_ref()
            .is_none_or(|r| r["cleanup_state"] == "verified")
    };
    let mut receipt = latest.unwrap_or_else(
        || json!({"exit_code":null,"checks_passed":false,"stdout":"","stderr":"","artifacts":[]}),
    );
    if let Err(e) = result {
        state.ledger.event(
            &job.id,
            "error",
            &format!("Task could not complete: {}", safe_error(&e.to_string())),
        )?;
        receipt["reason"] = json!(if cancelled(state, job) {
            "cancelled"
        } else {
            "controller_error"
        });
        receipt["checks_passed"] = json!(false);
    }
    receipt["cleanup_state"] = json!(if clean { "verified" } else { "failed" });
    receipt["inference_calls"] = json!(receipts);
    state.ledger.finish(&job.id, receipt)?;
    Ok(())
}
fn safe_error(message: &str) -> String {
    message.chars().take(500).collect()
}
