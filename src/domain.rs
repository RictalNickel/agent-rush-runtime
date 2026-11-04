use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct JobRequest {
    pub prompt: String,
    pub profile: String,
    pub memory_mb: u32,
    pub timeout_seconds: u32,
}

impl Default for JobRequest {
    fn default() -> Self {
        Self {
            prompt: String::new(),
            profile: "cli".into(),
            memory_mb: 512,
            timeout_seconds: 30,
        }
    }
}

impl JobRequest {
    pub fn validate(&self) -> Result<(), String> {
        if !(3..=8000).contains(&self.prompt.trim().len()) {
            return Err("Task must contain 3–8000 characters".into());
        }
        if self.profile != "cli" {
            return Err("Only the qualified CLI profile is supported".into());
        }
        if ![256, 512, 768, 1024].contains(&self.memory_mb) {
            return Err("Memory must be 256, 512, 768 or 1024 MiB".into());
        }
        if !(1..=120).contains(&self.timeout_seconds) {
            return Err("Timeout must be 1–120 seconds".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub owner: String,
    pub request: JobRequest,
    pub state: String,
    pub memory_mb: u32,
    pub cancel_requested: bool,
    pub created_at: String,
    pub updated_at: String,
    pub receipt: Option<Value>,
    pub events: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub memory_mb: u32,
    pub timeout_seconds: u32,
    pub cpu_millis: u32,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            memory_mb: 512,
            timeout_seconds: 30,
            cpu_millis: 1000,
        }
    }
}
impl Limits {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            (128..=1024).contains(&self.memory_mb),
            "memory outside policy"
        );
        anyhow::ensure!(
            (1..=120).contains(&self.timeout_seconds),
            "timeout outside policy"
        );
        anyhow::ensure!(
            (100..=2000).contains(&self.cpu_millis),
            "CPU outside policy"
        );
        Ok(())
    }
}

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
