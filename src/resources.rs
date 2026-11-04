//! Admission envelopes, distinct from measured consumption. CPU ceilings are shared upper bounds.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

pub const MAX_CONCURRENT: usize = 2;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourcePolicy {
    pub physical_memory_mb: u32,
    pub headroom_mb: u32,
    pub usable_memory_mb: u32,
    pub max_concurrent: usize,
    pub cpu_shares: u32,
    pub task_max_memory_mb: u32,
    pub task_max_cpu_millis: u32,
    pub scratch_mb: u32,
    pub pids: u32,
}
impl ResourcePolicy {
    pub fn new(total_mb: u32, configured_mb: u32) -> Result<Self> {
        ensure!(
            total_mb >= 512 && configured_mb > 0,
            "worker needs at least 512 MiB and a positive budget"
        );
        let reserve = (total_mb / 4).max(256);
        Ok(Self {
            physical_memory_mb: total_mb,
            headroom_mb: reserve,
            usable_memory_mb: configured_mb.min(total_mb - reserve),
            max_concurrent: MAX_CONCURRENT,
            cpu_shares: 512,
            task_max_memory_mb: 1024,
            task_max_cpu_millis: 2000,
            scratch_mb: 80,
            pids: 64,
        })
    }
    pub fn detect(configured_mb: u32) -> Result<Self> {
        let info = std::fs::read_to_string("/proc/meminfo")?;
        let total_kib: u64 = info
            .lines()
            .find_map(|line| line.strip_prefix("MemTotal:"))
            .context("host memory unavailable")?
            .split_whitespace()
            .next()
            .context("host memory unavailable")?
            .parse()?;
        let mut total_mb = total_kib / 1024;
        // A process-level cgroup bound can only tighten the physical-node budget.
        if let Ok(limit) = std::fs::read_to_string("/sys/fs/cgroup/memory.max")
            && let Ok(bytes) = limit.trim().parse::<u64>()
        {
            total_mb = total_mb.min(bytes / 1_048_576);
        }
        Self::new(total_mb.min(u32::MAX as u64) as u32, configured_mb)
    }
}
