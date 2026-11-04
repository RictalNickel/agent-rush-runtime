use crate::{
    agent::AgentContext,
    api::AppState,
    domain::{Job, Limits},
    engine::{Backend, Engine},
    inference::InferenceGateway,
};
use anyhow::{Context, Result, ensure};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
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
        ensure!(
            state.settings.worker_token.len() >= 32,
            "worker credential missing"
        );
        Ok(Self {
            client: state.http.clone(),
            base: state
                .settings
                .worker_url
                .clone()
                .context("worker URL missing")?,
            token: state.settings.worker_token.clone(),
        })
    }
    async fn ready(&self) -> Option<u32> {
        let response = self
            .client
            .get(format!("{}/v1/health", self.base))
            .bearer_auth(&self.token)
            .send()
            .await
            .ok()?;
        let health = read_json(response).await.ok()?;
        health["resource_policy"]["usable_memory_mb"]
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .filter(|v| *v > 0)
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
    if !state.settings.inference_configured() || state.settings.worker_url.is_none() {
        return Ok(0);
    }
    let node = NodeClient::new(state)?;
    let Some(node_capacity) = node.ready().await else {
        return Ok(0);
    };
    let jobs = state.ledger.scheduling_candidates()?;
    for job in &jobs {
        if let Some(job) = state.ledger.claim_with_capacity(&job.id, node_capacity)? {
            let state = state.clone();
            let node = node.clone();
            tokio::spawn(async move {
                if let Err(e) = run_job(&state, &node, &job, node_capacity).await {
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
struct RuntimeBackend<'a> {
    state: &'a AppState,
    node: &'a NodeClient,
    job: &'a Job,
    gateway: InferenceGateway,
    active_run: Mutex<Option<String>>,
}
impl Backend for RuntimeBackend<'_> {
    async fn complete(&self, context: &AgentContext) -> Result<(String, Value)> {
        let (completion, usage) = self.gateway.complete(&context.messages).await?;
        Ok((completion.content, usage))
    }
    async fn execute(&self, id: &str, code: &str, limits: Limits) -> Result<Value> {
        *self.active_run.lock().unwrap() = Some(id.into());
        self.node.submit(id, code, limits.clone()).await?;
        // Account separately for bounded image creation/start/export/cleanup overhead.
        let deadline = Instant::now() + Duration::from_secs(limits.timeout_seconds as u64 + 150);
        loop {
            ensure!(
                Instant::now() < deadline,
                "worker response deadline exceeded"
            );
            let value = self.node.get(id).await?;
            if value["state"] == "finished" {
                let receipt = value["receipt"].clone();
                if receipt["cleanup_state"] == "verified" {
                    *self.active_run.lock().unwrap() = None;
                }
                return Ok(receipt);
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
    fn cancelled(&self) -> bool {
        self.state
            .ledger
            .get_job(&self.job.owner, &self.job.id)
            .ok()
            .flatten()
            .is_none_or(|job| job.cancel_requested)
    }
}
async fn run_job(state: &AppState, node: &NodeClient, job: &Job, node_capacity: u32) -> Result<()> {
    let backend = RuntimeBackend {
        state,
        node,
        job,
        gateway: InferenceGateway::new(state.settings.clone(), state.ledger.clone())?,
        active_run: Mutex::new(None),
    };
    let outcome = Engine::new(&state.ledger, job, &backend, node_capacity)
        .run()
        .await;
    let active = backend.active_run.lock().unwrap().clone();
    let clean = if let Some(id) = active {
        node.cancel(&id).await.unwrap_or(false)
    } else {
        outcome
            .receipt
            .as_ref()
            .is_none_or(|r| r["cleanup_state"] == "verified")
    };
    let mut receipt = outcome.receipt.unwrap_or_else(
        || json!({"exit_code":null,"checks_passed":false,"stdout":"","stderr":"","artifacts":[]}),
    );
    if let Some(error) = outcome.error {
        state.ledger.event(
            &job.id,
            "error",
            &format!("Task could not complete: {error}"),
        )?;
        receipt["reason"] = json!(if backend.cancelled() {
            "cancelled"
        } else {
            "controller_error"
        });
        receipt["checks_passed"] = json!(false);
    }
    receipt["cleanup_state"] = json!(if clean { "verified" } else { "failed" });
    receipt["inference_calls"] = json!(outcome.inference_calls);
    receipt["model_calls"] = json!(outcome.model_calls);
    receipt["explanation"] = json!(outcome.final_message);
    state.ledger.finish(&job.id, receipt)?;
    Ok(())
}
