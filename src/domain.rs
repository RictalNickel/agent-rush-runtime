use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct JobRequest {
    pub prompt: String,
    pub profile: String,
    /// Upper bounds the operator allows for this job. The job's lease reserves this
    /// memory; the orchestrator picks the run ceiling within it, and runs start smaller.
    pub memory_mb: u32,
    pub timeout_seconds: u32,
}

impl Default for JobRequest {
    fn default() -> Self {
        Self {
            prompt: String::new(),
            profile: "cli".into(),
            memory_mb: crate::domain::MAX_MEMORY_MB,
            timeout_seconds: crate::domain::MAX_TIMEOUT_SECONDS,
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
    #[serde(default)]
    pub dropped_events: u64,
}
impl Job {
    pub fn record_event(&mut self, event: Value) {
        let drop_count = (self.events.len() + 1).saturating_sub(256);
        if drop_count > 0 {
            self.events.drain(..drop_count);
            self.dropped_events += drop_count as u64;
        }
        self.events.push(event);
    }
}

/// Sandbox limits. A run starts at `memory_mb` and `cpu_millis`; the worker raises them
/// live under measured pressure, never above the ceilings. Ceilings of 0 mean "no growth".
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub memory_mb: u32,
    pub timeout_seconds: u32,
    pub cpu_millis: u32,
    pub memory_ceiling_mb: u32,
    pub cpu_ceiling_millis: u32,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            memory_mb: 512,
            timeout_seconds: 30,
            cpu_millis: 1000,
            memory_ceiling_mb: 0,
            cpu_ceiling_millis: 0,
        }
    }
}
/// Hard maxima fixed by trusted code; no agent or request can exceed them.
pub const MAX_MEMORY_MB: u32 = 1024;
pub const MAX_TIMEOUT_SECONDS: u32 = 120;
pub const MAX_CPU_MILLIS: u32 = 2000;
impl Limits {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            (128..=MAX_MEMORY_MB).contains(&self.memory_mb),
            "memory outside policy"
        );
        anyhow::ensure!(
            (1..=MAX_TIMEOUT_SECONDS).contains(&self.timeout_seconds),
            "timeout outside policy"
        );
        anyhow::ensure!(
            (100..=MAX_CPU_MILLIS).contains(&self.cpu_millis),
            "CPU outside policy"
        );
        anyhow::ensure!(
            self.memory_ceiling_mb == 0
                || (self.memory_mb..=MAX_MEMORY_MB).contains(&self.memory_ceiling_mb),
            "memory ceiling outside policy"
        );
        anyhow::ensure!(
            self.cpu_ceiling_millis == 0
                || (self.cpu_millis..=MAX_CPU_MILLIS).contains(&self.cpu_ceiling_millis),
            "CPU ceiling outside policy"
        );
        Ok(())
    }
    /// The most memory this run may ever hold; admission reserves this, not the start.
    pub fn memory_ceiling(&self) -> u32 {
        self.memory_mb.max(self.memory_ceiling_mb)
    }
    pub fn cpu_ceiling(&self) -> u32 {
        self.cpu_millis.max(self.cpu_ceiling_millis)
    }
}

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
