//! Contract between the control plane and the worker's session boxes.
//! The worker implements it over HTTP (`src/boxes.rs`); the control plane
//! consumes it through `Arc<dyn BoxClient>` (`src/box_client.rs`, fakes in tests).

use anyhow::Result;
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};

/// Largest file moved through the files endpoints, in bytes.
pub const MAX_FILE_BYTES: usize = 25 * 1024 * 1024;
/// Largest stdout or stderr kept from one exec, in bytes.
pub const MAX_EXEC_OUTPUT_BYTES: usize = 256 * 1024;
pub const DEFAULT_EXEC_TIMEOUT_SECONDS: u32 = 120;
pub const MAX_EXEC_TIMEOUT_SECONDS: u32 = 600;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BoxSpec {
    pub session_id: String,
    pub owner: String,
    pub memory_mb: u32,
    pub cpu_millis: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecRequest {
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdin: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecResult {
    pub exit_code: i64,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileRead {
    File(Vec<u8>),
    Dir(Vec<DirEntry>),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WriteResult {
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BoxInfo {
    pub box_id: String,
    pub session_id: String,
    pub owner: String,
    pub created_at: i64,
    pub last_used_at: i64,
}

/// Paths are relative to `/workspace` or absolute under `/workspace` or `/tmp`.
pub trait BoxClient: Send + Sync {
    fn create<'a>(&'a self, spec: &'a BoxSpec) -> BoxFuture<'a, Result<String>>;
    fn exec<'a>(
        &'a self,
        box_id: &'a str,
        request: &'a ExecRequest,
    ) -> BoxFuture<'a, Result<ExecResult>>;
    fn read_file<'a>(&'a self, box_id: &'a str, path: &'a str) -> BoxFuture<'a, Result<FileRead>>;
    fn write_file<'a>(
        &'a self,
        box_id: &'a str,
        path: &'a str,
        bytes: &'a [u8],
    ) -> BoxFuture<'a, Result<WriteResult>>;
    /// `write_file` with owned, reference-counted bytes, so a client that sends them on
    /// (the HTTP client) needs no copy. Defaults to `write_file`.
    fn write_file_bytes<'a>(
        &'a self,
        box_id: &'a str,
        path: &'a str,
        bytes: axum::body::Bytes,
    ) -> BoxFuture<'a, Result<WriteResult>> {
        Box::pin(async move { self.write_file(box_id, path, &bytes).await })
    }
    fn destroy<'a>(&'a self, box_id: &'a str) -> BoxFuture<'a, Result<()>>;
    fn list<'a>(&'a self) -> BoxFuture<'a, Result<Vec<BoxInfo>>>;
}
