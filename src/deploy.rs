//! Trusted infrastructure inspection. No runtime agent imports credentials or provisions resources.
use anyhow::{Context, Result, ensure};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

pub mod credentials;
pub mod lifecycle;
pub mod watchdog;

async fn decode_response(response: reqwest::Response) -> Result<Value> {
    ensure!(
        response.content_length().is_none_or(|n| n <= 4_194_304),
        "Vultr response too large"
    );
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        ensure!(
            bytes.len() + chunk.len() <= 4_194_304,
            "Vultr response too large"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice(&bytes)?)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeploymentBudget {
    pub ceiling_usd: f64,
    pub reserve_usd: f64,
    pub max_hourly_usd: f64,
    pub max_lifetime_hours: f64,
}
impl Default for DeploymentBudget {
    fn default() -> Self {
        Self {
            ceiling_usd: 300.0,
            reserve_usd: 50.0,
            max_hourly_usd: 0.30,
            max_lifetime_hours: 2.0,
        }
    }
}
impl DeploymentBudget {
    pub fn check(&self, hourly: f64, hours: f64, spent: f64) -> Result<f64> {
        ensure!(
            [hourly, hours, spent].iter().all(|v| v.is_finite()),
            "unknown or invalid cost"
        );
        ensure!(
            hourly > 0.0 && hourly <= self.max_hourly_usd,
            "hourly price exceeds policy"
        );
        ensure!(
            hours > 0.0 && hours <= self.max_lifetime_hours,
            "test lifetime exceeds policy"
        );
        ensure!(spent >= 0.0, "invalid previous spending");
        let projected = hourly * hours;
        ensure!(
            spent + projected <= self.ceiling_usd - self.reserve_usd,
            "insufficient budget after reserve"
        );
        Ok(projected)
    }
}
pub fn select_plan(plans: &[Value], cpus: u64, memory_mb: u64) -> Result<Value> {
    plans
        .iter()
        .filter(|p| {
            p["vcpu_count"].as_u64().unwrap_or(0) >= cpus
                && p["ram"].as_u64().unwrap_or(0) >= memory_mb
                && p["monthly_cost"]
                    .as_f64()
                    .is_some_and(|v| v.is_finite() && v > 0.0)
        })
        .min_by(|a, b| {
            a["monthly_cost"]
                .as_f64()
                .unwrap()
                .total_cmp(&b["monthly_cost"].as_f64().unwrap())
        })
        .cloned()
        .context("No suitable priced plan available")
}
pub struct VultrClient {
    client: reqwest::Client,
    key: String,
}
impl VultrClient {
    pub fn new(key: String) -> Result<Self> {
        ensure!(
            !key.trim().is_empty(),
            "Vultr infrastructure credential missing"
        );
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            key,
        })
    }
    async fn get(&self, path: &str) -> Result<Value> {
        ensure!(
            path.starts_with("/v2/") && !path.contains("..") && !path.contains('#'),
            "invalid infrastructure path"
        );
        let response = self
            .client
            .get(format!("https://api.vultr.com{path}"))
            .bearer_auth(&self.key)
            .send()
            .await
            .context("Vultr API unreachable through configured network route")?;
        ensure!(
            response.status().is_success(),
            "Vultr API returned HTTP {}",
            response.status().as_u16()
        );
        decode_response(response).await
    }
    pub async fn inspect(&self, region: &str) -> Result<Value> {
        ensure!(
            !region.is_empty()
                && region
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            "invalid region"
        );
        let account = self.get("/v2/account").await?;
        let available = self
            .get(&format!("/v2/regions/{region}/availability"))
            .await?;
        let plans = self.get("/v2/plans?per_page=500").await?;
        let available = available["available_plans"]
            .as_array()
            .context("availability response missing plan list")?;
        let plans: Vec<Value> = plans["plans"]
            .as_array()
            .context("plans response missing list")?
            .iter()
            .filter(|p| available.iter().any(|id| id == &p["id"]))
            .cloned()
            .collect();
        let control = select_plan(&plans, 2, 4096)?;
        let worker = select_plan(&plans, 4, 8192)?;
        // Vultr bills compute hourly up to a monthly cap. Monthly/672 is a conservative estimate,
        // not a provider price guarantee; final creation must verify billing details and taxes.
        let hourly = (control["monthly_cost"].as_f64().unwrap()
            + worker["monthly_cost"].as_f64().unwrap())
            / 672.0;
        let projected = DeploymentBudget::default().check(hourly, 2.0, 0.0)?;
        Ok(
            json!({"region":region,"account_balance":account["account"]["balance"],"pending_charges":account["account"]["pending_charges"],"control_plan":control,"worker_plan":worker,"estimated_combined_hourly_usd":hourly,"two_hour_estimate_usd":projected,"status":"plan_only_no_resources_created","requires_before_provisioning":["verified available credit and actual project spend","local qualification passing","verified hourly prices, inference pricing and any minimum charges","working authenticated deployment and independent teardown route","secure inference credential and selected model","HTTPS hostname and private VPC setup"]}),
        )
    }
}
