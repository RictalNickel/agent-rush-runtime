//! Native equivalents of Agent Zero memory tools and persisted contexts. No embeddings/provider calls.
use crate::{agent::AgentContext, domain::now, ledger::Ledger};
use anyhow::{Result, ensure};
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: String,
    pub title: String,
    pub text: String,
    pub created_at: String,
}

impl Ledger {
    pub fn memory_save(&self, owner: &str, title: &str, text: &str) -> Result<Memory> {
        ensure!(
            !owner.is_empty() && owner.len() <= 128,
            "invalid memory owner"
        );
        ensure!(
            (1..=120).contains(&title.trim().len()),
            "memory title must be 1–120 bytes"
        );
        ensure!(
            (1..=8192).contains(&text.trim().len()),
            "memory text must be 1–8192 bytes"
        );
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let count: u32 = tx.query_row(
            "SELECT COUNT(*) FROM memories WHERE owner=?1",
            [owner],
            |row| row.get(0),
        )?;
        ensure!(
            count < 64,
            "owner memory quota reached; delete an obsolete note first"
        );
        let memory = Memory {
            id: uuid::Uuid::new_v4().to_string(),
            title: title.trim().into(),
            text: text.trim().into(),
            created_at: now(),
        };
        tx.execute(
            "INSERT INTO memories VALUES(?1,?2,?3,?4,?5)",
            params![
                memory.id,
                owner,
                memory.title,
                memory.text,
                memory.created_at
            ],
        )?;
        tx.commit()?;
        Ok(memory)
    }
    pub fn memory_search(&self, owner: &str, query: &str) -> Result<Vec<Memory>> {
        ensure!(
            (1..=256).contains(&query.trim().len()),
            "memory query must be 1–256 bytes"
        );
        let db = self.db.lock().unwrap();
        let mut statement = db.prepare("SELECT id,title,text,created_at FROM memories WHERE owner=?1 AND instr(lower(title || char(10) || text),lower(?2))>0 ORDER BY rowid DESC LIMIT 5")?;
        let rows = statement.query_map(params![owner, query.trim()], |r| {
            Ok(Memory {
                id: r.get(0)?,
                title: r.get(1)?,
                text: r.get(2)?,
                created_at: r.get(3)?,
            })
        })?;
        rows.map(|row| Ok(row?)).collect()
    }
    pub fn memory_delete(&self, owner: &str, id: &str) -> Result<bool> {
        uuid::Uuid::parse_str(id)?;
        Ok(self.db.lock().unwrap().execute(
            "DELETE FROM memories WHERE owner=?1 AND id=?2",
            params![owner, id],
        )? == 1)
    }
    pub fn save_context(&self, owner: &str, context: &AgentContext) -> Result<()> {
        context.validate()?;
        let data = serde_json::to_string(context)?;
        ensure!(data.len() <= 32768, "context snapshot exceeds bound");
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let owned: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM jobs WHERE id=?1 AND owner=?2)",
            params![context.task_id, owner],
            |r| r.get(0),
        )?;
        ensure!(owned, "context job ownership mismatch");
        if let Some(parent) = &context.parent_agent_id {
            let parent_owned:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_contexts WHERE agent_id=?1 AND job_id=?2 AND owner=?3)",params![parent,context.task_id,owner],|r|r.get(0))?;
            ensure!(
                parent_owned,
                "subordinate parent not registered under this job"
            );
        }
        let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_contexts WHERE agent_id=?1 AND job_id=?2 AND owner=?3)",params![context.agent_id,context.task_id,owner],|r|r.get(0))?;
        if !exists {
            let count: u32 = tx.query_row(
                "SELECT COUNT(*) FROM agent_contexts WHERE job_id=?1",
                [&context.task_id],
                |r| r.get(0),
            )?;
            ensure!(count < 3, "context count limit reached");
            tx.execute(
                "INSERT INTO agent_contexts VALUES(?1,?2,?3,?4)",
                params![context.agent_id, context.task_id, owner, data],
            )?;
        } else {
            tx.execute(
                "UPDATE agent_contexts SET data=?1 WHERE agent_id=?2 AND job_id=?3 AND owner=?4",
                params![data, context.agent_id, context.task_id, owner],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn contexts(&self, owner: &str, job_id: &str) -> Result<Vec<AgentContext>> {
        let db = self.db.lock().unwrap();
        let mut statement = db.prepare(
            "SELECT data FROM agent_contexts WHERE owner=?1 AND job_id=?2 ORDER BY rowid",
        )?;
        let rows = statement.query_map(params![owner, job_id], |r| r.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }
}
