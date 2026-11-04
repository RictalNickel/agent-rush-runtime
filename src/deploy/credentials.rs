//! Trusted setup helpers. Keys are exported privately, never printed or supplied to task code.
use super::VultrClient;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{io::Write, path::Path};

pub struct InferenceSubscription {
    pub id: String,
    pub label: String,
    key: String,
}

pub fn select_subscription(body: &Value, requested: Option<&str>) -> Result<InferenceSubscription> {
    let rows = body["subscriptions"]
        .as_array()
        .context("subscription list missing")?;
    let matches: Vec<&Value> = rows
        .iter()
        .filter(|v| match requested {
            Some(id) => v["id"] == id,
            None => v["label"] == "agent-rush-runtime",
        })
        .collect();
    let selected = if matches.len() == 1 {
        matches[0]
    } else if requested.is_none() && matches.is_empty() && rows.len() == 1 {
        &rows[0]
    } else {
        anyhow::bail!(
            "No unambiguous existing inference subscription; refusing to create or guess one"
        )
    };
    let id = selected["id"].as_str().context("subscription ID missing")?;
    ensure!(
        !id.is_empty()
            && id.len() <= 128
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b)),
        "invalid subscription identity"
    );
    let key = selected["api_key"]
        .as_str()
        .context("subscription API key unavailable")?;
    ensure!(
        (8..=512).contains(&key.len())
            && key.bytes().all(|b| b.is_ascii_graphic())
            && !key.contains('\''),
        "invalid secret encoding"
    );
    Ok(InferenceSubscription {
        id: id.into(),
        label: selected["label"].as_str().unwrap_or("").into(),
        key: key.into(),
    })
}

pub fn select_glm_flash(body: &Value) -> Result<String> {
    let rows = body["data"]
        .as_array()
        .context("Vultr model catalog missing data")?;
    let normalized = |name: &str| {
        name.bytes()
            .filter(|b| !b"-_ ".contains(b))
            .map(|b| b.to_ascii_lowercase() as char)
            .collect::<String>()
    };
    let ids: std::collections::BTreeSet<String> = rows
        .iter()
        .filter_map(|v| v["id"].as_str())
        .filter(|id| normalized(id.rsplit('/').next().unwrap_or(id)) == "glm5.3flash")
        .map(String::from)
        .collect();
    ensure!(
        ids.len() == 1,
        "GLM 5.3 Flash is unavailable or ambiguous in the returned catalog; no fallback selected"
    );
    let id = ids.into_iter().next().unwrap();
    ensure!(
        id.len() <= 200
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_./:".contains(&b)),
        "Invalid catalog model identifier"
    );
    Ok(id)
}

fn write_new_private(path: &Path, content: &str) -> Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .context("Cannot create private credential output; existing files are never overwritten")?;
    let result = (|| -> Result<()> {
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result
}

pub fn export_inference_key(subscription: &InferenceSubscription, path: &Path) -> Result<Value> {
    write_new_private(
        path,
        &format!("VULTR_INFERENCE_API_KEY='{}'\n", subscription.key),
    )?;
    Ok(
        json!({"subscription_id":subscription.id,"label":subscription.label.replace(&subscription.key,"[redacted]"),"credential_file":path,"secret_printed":false,"subscription_created":false,"inference_calls":0}),
    )
}

pub fn export_model_configuration(catalog: &Value, path: &Path) -> Result<Value> {
    let id = select_glm_flash(catalog)?;
    write_new_private(path, &format!("AR_INFERENCE_MODEL='{id}'\n"))?;
    Ok(
        json!({"model_id":id,"configuration_file":path,"provider_rates_need_verification":true,"paid_inference_enabled":false,"model_requests":0}),
    )
}

pub async fn discover_model_configuration(inference_key: &str, path: &Path) -> Result<Value> {
    ensure!(
        !inference_key.is_empty(),
        "Inference credential is not configured"
    );
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let response = client
        .get("https://api.vultrinference.com/v1/models")
        .bearer_auth(inference_key)
        .send()
        .await?;
    ensure!(
        response.status().is_success(),
        "Vultr model catalog HTTP {}",
        response.status().as_u16()
    );
    let catalog = super::decode_response(response).await?;
    export_model_configuration(&catalog, path)
}

impl VultrClient {
    pub async fn export_existing_inference_key(
        &self,
        requested: Option<&str>,
        path: &Path,
    ) -> Result<Value> {
        let body = self.get("/v2/inference").await?;
        export_inference_key(&select_subscription(&body, requested)?, path)
    }
}
