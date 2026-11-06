//! Control-plane side of the box contract: calls the worker's `/v1/boxes…` routes over the
//! existing private link (mTLS is configured on the `reqwest::Client` passed in).
use crate::{
    boxes::BoxError,
    sandbox_api::{
        BoxClient, BoxInfo, BoxSpec, DEFAULT_EXEC_TIMEOUT_SECONDS, DirEntry, ExecRequest,
        ExecResult, FileRead, MAX_EXEC_TIMEOUT_SECONDS, MAX_FILE_BYTES, WriteResult,
    },
};
use anyhow::{Context, Result, ensure};
use axum::body::Bytes;
use futures_util::{StreamExt, future::BoxFuture};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::time::Duration;

#[derive(Clone)]
pub struct HttpBoxClient {
    http: reqwest::Client,
    base_url: String,
    token: String,
}

impl HttpBoxClient {
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
    /// Fail with the worker's typed error (`BoxError`) when it sent one, so callers can
    /// tell a lost box (`BoxError::NotFound`) from a missing path or a refusal.
    async fn check(response: reqwest::Response) -> Result<reqwest::Response> {
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let body: Value = response.json().await.unwrap_or(Value::Null);
        let message = body["error"]
            .as_str()
            .unwrap_or("worker returned no error message")
            .to_string();
        if let Some(code) = body["code"].as_str()
            && let Some(error) = BoxError::from_code(code, message.clone())
        {
            return Err(error.into());
        }
        anyhow::bail!("box worker returned HTTP {}: {message}", status.as_u16())
    }
    async fn json<T: DeserializeOwned>(response: reqwest::Response) -> Result<T> {
        let bytes = read_bounded(Self::check(response).await?, 4 * 1024 * 1024).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }
}

async fn read_bounded(response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    let mut stream = response.bytes_stream();
    let mut out = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        ensure!(
            out.len() + chunk.len() <= limit,
            "box worker response exceeds {limit} bytes"
        );
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

fn path_query(box_id: &str) -> Result<String> {
    ensure!(
        box_id.len() == 32 && box_id.bytes().all(|b| b.is_ascii_hexdigit()),
        BoxError::NotFound
    );
    Ok(format!("/v1/boxes/{box_id}"))
}

impl BoxClient for HttpBoxClient {
    fn create<'a>(&'a self, spec: &'a BoxSpec) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let response = self
                .request(reqwest::Method::POST, "/v1/boxes", Duration::from_secs(180))
                .json(spec)
                .send()
                .await
                .context("box worker unreachable")?;
            let value: Value = Self::json(response).await?;
            value["box_id"]
                .as_str()
                .map(String::from)
                .context("box worker returned no box_id")
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
                .unwrap_or(DEFAULT_EXEC_TIMEOUT_SECONDS)
                .min(MAX_EXEC_TIMEOUT_SECONDS);
            let response = self
                .request(
                    reqwest::Method::POST,
                    &format!("{}/exec", path_query(box_id)?),
                    Duration::from_secs(seconds as u64 + 60),
                )
                .json(request)
                .send()
                .await
                .context("box worker unreachable")?;
            Self::json(response).await
        })
    }
    fn read_file<'a>(&'a self, box_id: &'a str, path: &'a str) -> BoxFuture<'a, Result<FileRead>> {
        Box::pin(async move {
            let response = self
                .request(
                    reqwest::Method::GET,
                    &format!("{}/files", path_query(box_id)?),
                    Duration::from_secs(180),
                )
                .query(&[("path", path)])
                .send()
                .await
                .context("box worker unreachable")?;
            let response = Self::check(response).await?;
            let kind = response
                .headers()
                .get("x-box-entry")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            let bytes = read_bounded(response, MAX_FILE_BYTES + 1024 * 1024).await?;
            match kind.as_str() {
                "file" => {
                    ensure!(bytes.len() <= MAX_FILE_BYTES, "file exceeds 25 MiB");
                    Ok(FileRead::File(bytes))
                }
                "dir" => Ok(FileRead::Dir(serde_json::from_slice::<Vec<DirEntry>>(
                    &bytes,
                )?)),
                _ => anyhow::bail!("box worker returned an unknown entry kind"),
            }
        })
    }
    fn write_file<'a>(
        &'a self,
        box_id: &'a str,
        path: &'a str,
        bytes: &'a [u8],
    ) -> BoxFuture<'a, Result<WriteResult>> {
        self.write_file_bytes(box_id, path, Bytes::copy_from_slice(bytes))
    }
    /// Sends the bytes as the request body as they are (no copy).
    fn write_file_bytes<'a>(
        &'a self,
        box_id: &'a str,
        path: &'a str,
        bytes: Bytes,
    ) -> BoxFuture<'a, Result<WriteResult>> {
        Box::pin(async move {
            ensure!(
                bytes.len() <= MAX_FILE_BYTES,
                BoxError::TooLarge("file exceeds 25 MiB".into())
            );
            let response = self
                .request(
                    reqwest::Method::PUT,
                    &format!("{}/files", path_query(box_id)?),
                    Duration::from_secs(180),
                )
                .query(&[("path", path)])
                .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                .body(bytes)
                .send()
                .await
                .context("box worker unreachable")?;
            Self::json(response).await
        })
    }
    fn destroy<'a>(&'a self, box_id: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let response = self
                .request(
                    reqwest::Method::DELETE,
                    &path_query(box_id)?,
                    Duration::from_secs(150),
                )
                .send()
                .await
                .context("box worker unreachable")?;
            let value: Value = Self::json(response).await?;
            ensure!(
                value["removed"] == true && value["volume_removed"] == true,
                "box {box_id} was not fully removed"
            );
            Ok(())
        })
    }
    fn list<'a>(&'a self) -> BoxFuture<'a, Result<Vec<BoxInfo>>> {
        Box::pin(async move {
            let response = self
                .request(reqwest::Method::GET, "/v1/boxes", Duration::from_secs(30))
                .send()
                .await
                .context("box worker unreachable")?;
            Self::json(response).await
        })
    }
}
