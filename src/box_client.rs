//! `BoxClient` over the worker's authenticated private link (`/v1/boxes...`, see
//! `src/sandbox_api.rs` and the box contract table in the agent backend design).
use crate::sandbox_api::{
    BoxClient, BoxInfo, BoxSpec, DirEntry, ExecRequest, ExecResult, FileRead,
    MAX_EXEC_TIMEOUT_SECONDS, MAX_FILE_BYTES, WriteResult,
};
use anyhow::{Result, anyhow, bail};
use futures_util::future::BoxFuture;
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;

const CREATE_TIMEOUT: Duration = Duration::from_secs(180);
const FILE_TIMEOUT: Duration = Duration::from_secs(120);
const SHORT_TIMEOUT: Duration = Duration::from_secs(60);

pub struct HttpBoxClient {
    http: reqwest::Client,
    base_url: String,
    token: String,
}

impl HttpBoxClient {
    /// `http` carries the worker's mTLS identity; each request sets its own timeout.
    pub fn new(http: reqwest::Client, base_url: String, token: String) -> Self {
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            token,
        }
    }
    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }
    fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        timeout: Duration,
    ) -> reqwest::RequestBuilder {
        self.http
            .request(method, self.url(path))
            .bearer_auth(&self.token)
            .timeout(timeout)
    }
}

/// Turns a non-success worker response into an error carrying the worker's message.
async fn checked(response: reqwest::Response) -> Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    let message = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| v["error"].as_str().map(String::from))
        .unwrap_or_else(|| crate::agent::clip(body.trim(), 500).to_string());
    if status == reqwest::StatusCode::NOT_FOUND && message.is_empty() {
        bail!("not found");
    }
    bail!("{message} (worker returned {})", status.as_u16())
}

fn box_path(id: &str) -> Result<String> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        bail!("invalid box id");
    }
    Ok(format!("/v1/boxes/{id}"))
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Listing {
    Wrapped { entries: Vec<DirEntry> },
    Plain(Vec<DirEntry>),
}
#[derive(Deserialize)]
#[serde(untagged)]
enum BoxList {
    Wrapped { boxes: Vec<BoxInfo> },
    Plain(Vec<BoxInfo>),
}

impl BoxClient for HttpBoxClient {
    fn create<'a>(&'a self, spec: &'a BoxSpec) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let response = self
                .request(reqwest::Method::POST, "/v1/boxes", CREATE_TIMEOUT)
                .json(spec)
                .send()
                .await?;
            let body: Value = checked(response).await?.json().await?;
            body["box_id"]
                .as_str()
                .map(String::from)
                .ok_or_else(|| anyhow!("worker returned no box_id"))
        })
    }
    fn exec<'a>(
        &'a self,
        box_id: &'a str,
        request: &'a ExecRequest,
    ) -> BoxFuture<'a, Result<ExecResult>> {
        Box::pin(async move {
            let seconds = request
                .timeout_seconds
                .unwrap_or(crate::sandbox_api::DEFAULT_EXEC_TIMEOUT_SECONDS)
                .min(MAX_EXEC_TIMEOUT_SECONDS) as u64;
            let response = self
                .request(
                    reqwest::Method::POST,
                    &format!("{}/exec", box_path(box_id)?),
                    Duration::from_secs(seconds + 30),
                )
                .json(request)
                .send()
                .await?;
            Ok(checked(response).await?.json().await?)
        })
    }
    fn read_file<'a>(&'a self, box_id: &'a str, path: &'a str) -> BoxFuture<'a, Result<FileRead>> {
        Box::pin(async move {
            let response = self
                .request(
                    reqwest::Method::GET,
                    &format!("{}/files", box_path(box_id)?),
                    FILE_TIMEOUT,
                )
                .query(&[("path", path)])
                .send()
                .await?;
            let response = checked(response).await?;
            let kind = response
                .headers()
                .get("x-box-kind")
                .and_then(|v| v.to_str().ok())
                .map(String::from);
            let json = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.starts_with("application/json"));
            if response
                .content_length()
                .is_some_and(|n| n > MAX_FILE_BYTES as u64)
            {
                bail!("file exceeds 25 MiB");
            }
            let bytes = response.bytes().await?;
            if bytes.len() > MAX_FILE_BYTES {
                bail!("file exceeds 25 MiB");
            }
            if kind.as_deref() == Some("dir") || (kind.is_none() && json) {
                if let Ok(listing) = serde_json::from_slice::<Listing>(&bytes) {
                    return Ok(FileRead::Dir(match listing {
                        Listing::Wrapped { entries } | Listing::Plain(entries) => entries,
                    }));
                }
                if kind.is_some() {
                    bail!("worker returned an invalid directory listing");
                }
            }
            Ok(FileRead::File(bytes.to_vec()))
        })
    }
    fn write_file<'a>(
        &'a self,
        box_id: &'a str,
        path: &'a str,
        bytes: &'a [u8],
    ) -> BoxFuture<'a, Result<WriteResult>> {
        Box::pin(async move {
            let response = self
                .request(
                    reqwest::Method::PUT,
                    &format!("{}/files", box_path(box_id)?),
                    FILE_TIMEOUT,
                )
                .query(&[("path", path)])
                .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                .body(bytes.to_vec())
                .send()
                .await?;
            Ok(checked(response).await?.json().await?)
        })
    }
    fn destroy<'a>(&'a self, box_id: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let response = self
                .request(reqwest::Method::DELETE, &box_path(box_id)?, SHORT_TIMEOUT)
                .send()
                .await?;
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(());
            }
            checked(response).await?;
            Ok(())
        })
    }
    fn list<'a>(&'a self) -> BoxFuture<'a, Result<Vec<BoxInfo>>> {
        Box::pin(async move {
            let response = self
                .request(reqwest::Method::GET, "/v1/boxes", SHORT_TIMEOUT)
                .send()
                .await?;
            let list: BoxList = checked(response).await?.json().await?;
            Ok(match list {
                BoxList::Wrapped { boxes } | BoxList::Plain(boxes) => boxes,
            })
        })
    }
}
