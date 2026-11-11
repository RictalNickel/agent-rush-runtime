//! The owner's self-editable agent configuration: versioned, revertible, and always
//! validated by trusted code against the built-in tool registry and the operator's MCP list.
use crate::{ledger::Ledger, session_store::now_ms};
use anyhow::{Result, bail, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// The built-in tool registry. Custom tools may not reuse these names.
pub const BUILTIN_TOOLS: &[&str] = &[
    "bash",
    "read_file",
    "write_file",
    "edit_file",
    "list_files",
    "grep",
    "glob",
    "browser",
    "web_search",
    "web_fetch",
    "convert_file",
    "security_scan",
    "skill",
    "agent_config",
    "mcp",
    "reply",
];
pub const MAX_ADDENDUM_BYTES: usize = 8 * 1024;
pub const MAX_CUSTOM_TOOLS: usize = 20;
const MAX_TEMPLATE_BYTES: usize = 4096;
const MAX_REASON_BYTES: usize = 500;
pub const REASONING_EFFORTS: &[&str] = &["low", "medium", "high"];

pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS agent_configs(owner TEXT NOT NULL, version INTEGER NOT NULL, data TEXT NOT NULL, reason TEXT NOT NULL, author TEXT NOT NULL, created_at INTEGER NOT NULL, PRIMARY KEY(owner, version));",
    )?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomTool {
    pub name: String,
    pub description: String,
    /// A bash command with `{{arg}}` placeholders; each value is shell-quoted on expansion.
    pub command_template: String,
}
impl CustomTool {
    /// Placeholder names in template order, without duplicates.
    pub fn args(&self) -> Result<Vec<String>> {
        template_args(&self.command_template)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentConfig {
    #[serde(default)]
    pub system_addendum: String,
    #[serde(default)]
    pub disabled_tools: Vec<String>,
    #[serde(default)]
    pub custom_tools: Vec<CustomTool>,
    #[serde(default)]
    pub mcp_servers: Vec<String>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}

/// `^[a-z][a-z0-9_]{0,39}$`
fn valid_tool_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 40
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
}

/// Parses `{{name}}` placeholders. Names follow the tool-name rule; `tool` is reserved.
pub fn template_args(template: &str) -> Result<Vec<String>> {
    let mut names = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            bail!("unclosed {{{{ placeholder in command_template")
        };
        let name = after[..end].trim();
        ensure!(
            valid_tool_name(name) && name != "tool",
            "invalid placeholder {{{{{name}}}}}: use lowercase names like {{{{path}}}}"
        );
        if !names.iter().any(|n| n == name) {
            names.push(name.to_string());
        }
        rest = &after[end + 2..];
    }
    Ok(names)
}

impl AgentConfig {
    pub fn validate(&self, builtin_tools: &[&str], allowed_mcp: &[String]) -> Result<()> {
        self.check(builtin_tools, Some(allowed_mcp))
    }
    /// Every limit except the MCP allowlist, which only the caller knows. The session
    /// engine still intersects `mcp_servers` with the operator's list at use time.
    pub(crate) fn check(
        &self,
        builtin_tools: &[&str],
        allowed_mcp: Option<&[String]>,
    ) -> Result<()> {
        ensure!(
            self.system_addendum.len() <= MAX_ADDENDUM_BYTES,
            "system_addendum exceeds 8 KiB"
        );
        let mut seen = BTreeSet::new();
        for tool in &self.disabled_tools {
            ensure!(
                builtin_tools.contains(&tool.as_str()),
                "disabled_tools: unknown built-in tool {tool:?}"
            );
            ensure!(tool != "reply", "the reply tool cannot be disabled");
            ensure!(
                seen.insert(tool.as_str()),
                "disabled_tools: duplicate {tool:?}"
            );
        }
        ensure!(
            self.custom_tools.len() <= MAX_CUSTOM_TOOLS,
            "at most {MAX_CUSTOM_TOOLS} custom tools"
        );
        let mut names = BTreeSet::new();
        for tool in &self.custom_tools {
            ensure!(
                valid_tool_name(&tool.name),
                "custom tool name {:?} must match ^[a-z][a-z0-9_]{{0,39}}$",
                tool.name
            );
            ensure!(
                !builtin_tools.contains(&tool.name.as_str()),
                "custom tool {:?} clashes with a built-in tool",
                tool.name
            );
            ensure!(
                names.insert(tool.name.as_str()),
                "duplicate custom tool {:?}",
                tool.name
            );
            ensure!(
                !tool.description.trim().is_empty() && tool.description.len() <= 1024,
                "custom tool {:?}: description must be 1-1024 bytes",
                tool.name
            );
            ensure!(
                !tool.command_template.trim().is_empty()
                    && tool.command_template.len() <= MAX_TEMPLATE_BYTES,
                "custom tool {:?}: command_template must be 1-4096 bytes",
                tool.name
            );
            tool.args()?;
        }
        let mut servers = BTreeSet::new();
        for server in &self.mcp_servers {
            ensure!(servers.insert(server), "duplicate MCP server {server:?}");
            if let Some(allowed) = allowed_mcp {
                ensure!(
                    allowed.contains(server),
                    "MCP server {server:?} is not on the operator's allowed list"
                );
            }
        }
        if let Some(effort) = &self.reasoning_effort {
            ensure!(
                REASONING_EFFORTS.contains(&effort.as_str()),
                "reasoning_effort must be low, medium or high"
            );
        }
        Ok(())
    }
}

impl Ledger {
    /// The latest version and config; version 0 is the built-in default.
    pub fn agent_config(&self, owner: &str) -> Result<(i64, AgentConfig)> {
        let db = self.db.lock().unwrap();
        let row: Option<(i64, String)> = db
            .query_row(
                "SELECT version,data FROM agent_configs WHERE owner=?1 ORDER BY version DESC LIMIT 1",
                [owner],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match row {
            Some((version, data)) => Ok((version, serde_json::from_str(&data)?)),
            None => Ok((0, AgentConfig::default())),
        }
    }
    pub fn agent_config_version(&self, owner: &str, version: i64) -> Result<Option<AgentConfig>> {
        if version == 0 {
            return Ok(Some(AgentConfig::default()));
        }
        let db = self.db.lock().unwrap();
        let data: Option<String> = db
            .query_row(
                "SELECT data FROM agent_configs WHERE owner=?1 AND version=?2",
                params![owner, version],
                |r| r.get(0),
            )
            .optional()?;
        data.map(|d| Ok(serde_json::from_str(&d)?)).transpose()
    }
    /// Stores a new version and returns its number. `author` is `user` or `agent`.
    /// Callers validate the MCP allowlist with `AgentConfig::validate` first.
    pub fn update_agent_config(
        &self,
        owner: &str,
        cfg: &AgentConfig,
        reason: &str,
        author: &str,
    ) -> Result<i64> {
        ensure!(
            matches!(author, "user" | "agent"),
            "author must be user or agent"
        );
        ensure!(
            !owner.is_empty() && owner.len() <= 128,
            "invalid config owner"
        );
        cfg.check(BUILTIN_TOOLS, None)?;
        let reason = crate::agent::clip(reason.trim(), MAX_REASON_BYTES);
        let data = serde_json::to_string(cfg)?;
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let version: i64 = tx.query_row(
            "SELECT COALESCE(MAX(version),0)+1 FROM agent_configs WHERE owner=?1",
            [owner],
            |r| r.get(0),
        )?;
        tx.execute(
            "INSERT INTO agent_configs(owner,version,data,reason,author,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
            params![owner, version, data, reason, author, now_ms()],
        )?;
        tx.commit()?;
        Ok(version)
    }
    /// `(version, reason, author, created_at)`, newest first.
    pub fn agent_config_history(&self, owner: &str) -> Result<Vec<(i64, String, String, i64)>> {
        let db = self.db.lock().unwrap();
        let mut statement = db.prepare(
            "SELECT version,reason,author,created_at FROM agent_configs WHERE owner=?1 ORDER BY version DESC LIMIT 200",
        )?;
        let rows = statement.query_map([owner], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?;
        rows.map(|row| Ok(row?)).collect()
    }
    /// Re-applies an earlier version as a new version (author `user`).
    pub fn revert_agent_config(&self, owner: &str, version: i64) -> Result<i64> {
        let Some(cfg) = self.agent_config_version(owner, version)? else {
            bail!("agent config version {version} not found")
        };
        self.update_agent_config(owner, &cfg, &format!("revert to version {version}"), "user")
    }
}
