//! Agent sessions, their event log and queued interjections, in the ledger's SQLite.
//! The existing `sessions` table holds login sessions; agent sessions live in `agent_sessions`.
//! Every `*_at` / `at` value is Unix time in milliseconds.
use crate::ledger::Ledger;
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Largest serialized event payload. Tool output is truncated well below this upstream.
pub const MAX_EVENT_BYTES: usize = 256 * 1024;
/// Largest page of events returned by one read.
pub const MAX_EVENT_PAGE: usize = 1000;
/// Largest user message or interjection.
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;
/// Queued interjections per session.
const MAX_PENDING: u32 = 50;
const MAX_TITLE_BYTES: usize = 200;

/// Event kinds written to `session_events`.
pub const EVENT_KINDS: &[&str] = &[
    "user",
    "assistant",
    "tool_call",
    "tool_result",
    "interjection",
    "status",
    "config_change",
    "error",
];

pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS agent_sessions(id TEXT PRIMARY KEY, owner TEXT NOT NULL, title TEXT NOT NULL, status TEXT NOT NULL, box_id TEXT, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, spend_usd REAL NOT NULL DEFAULT 0, last_seq INTEGER NOT NULL DEFAULT 0);
        CREATE INDEX IF NOT EXISTS agent_sessions_owner ON agent_sessions(owner, updated_at);
        CREATE TABLE IF NOT EXISTS session_events(session_id TEXT NOT NULL REFERENCES agent_sessions(id), seq INTEGER NOT NULL, kind TEXT NOT NULL, payload TEXT NOT NULL, at INTEGER NOT NULL, PRIMARY KEY(session_id, seq));
        CREATE TABLE IF NOT EXISTS pending_messages(id INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT NOT NULL REFERENCES agent_sessions(id), text TEXT NOT NULL, at INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS pending_messages_session ON pending_messages(session_id);",
    )?;
    Ok(())
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Idle,
    Running,
    Failed,
    Closed,
}
impl SessionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            SessionStatus::Idle => "idle",
            SessionStatus::Running => "running",
            SessionStatus::Failed => "failed",
            SessionStatus::Closed => "closed",
        }
    }
    fn parse(raw: &str) -> Self {
        match raw {
            "running" => SessionStatus::Running,
            "failed" => SessionStatus::Failed,
            "closed" => SessionStatus::Closed,
            _ => SessionStatus::Idle,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionView {
    pub id: String,
    pub owner: String,
    pub title: String,
    pub status: SessionStatus,
    pub box_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub spend_usd: f64,
    pub last_seq: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionEvent {
    pub session_id: String,
    pub seq: i64,
    pub kind: String,
    pub payload: Value,
    pub at: i64,
}

const VIEW_COLUMNS: &str = "id,owner,title,status,box_id,created_at,updated_at,spend_usd,last_seq";

fn view(row: &Row) -> rusqlite::Result<SessionView> {
    Ok(SessionView {
        id: row.get(0)?,
        owner: row.get(1)?,
        title: row.get(2)?,
        status: SessionStatus::parse(&row.get::<_, String>(3)?),
        box_id: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        spend_usd: row.get(7)?,
        last_seq: row.get(8)?,
    })
}

fn clean_title(title: &str) -> String {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let title = crate::agent::clip(&title, MAX_TITLE_BYTES).to_string();
    if title.is_empty() {
        "New session".into()
    } else {
        title
    }
}

impl Ledger {
    pub fn create_agent_session(&self, owner: &str, title: &str) -> Result<SessionView> {
        Ok(self
            .create_agent_session_within(owner, title, usize::MAX)?
            .expect("no cap"))
    }
    /// Creates a session unless the owner already has `max_open` sessions that are not
    /// closed (then `None`). The count and the insert happen under one database lock.
    pub fn create_agent_session_within(
        &self,
        owner: &str,
        title: &str,
        max_open: usize,
    ) -> Result<Option<SessionView>> {
        ensure!(
            !owner.is_empty() && owner.len() <= 128,
            "invalid session owner"
        );
        let at = now_ms();
        let session = SessionView {
            id: uuid::Uuid::new_v4().to_string(),
            owner: owner.into(),
            title: clean_title(title),
            status: SessionStatus::Idle,
            box_id: None,
            created_at: at,
            updated_at: at,
            spend_usd: 0.0,
            last_seq: 0,
        };
        let db = self.db.lock().unwrap();
        if max_open < usize::MAX {
            let open: i64 = db.query_row(
                "SELECT COUNT(*) FROM agent_sessions WHERE owner=?1 AND status!='closed'",
                [owner],
                |r| r.get(0),
            )?;
            if usize::try_from(open).unwrap_or(usize::MAX) >= max_open {
                return Ok(None);
            }
        }
        db.execute(
            "INSERT INTO agent_sessions(id,owner,title,status,created_at,updated_at) VALUES(?1,?2,?3,'idle',?4,?4)",
            params![session.id, owner, session.title, at],
        )?;
        Ok(Some(session))
    }
    pub fn agent_session(&self, owner: &str, id: &str) -> Result<Option<SessionView>> {
        let db = self.db.lock().unwrap();
        Ok(db
            .query_row(
                &format!("SELECT {VIEW_COLUMNS} FROM agent_sessions WHERE id=?1 AND owner=?2"),
                params![id, owner],
                view,
            )
            .optional()?)
    }
    pub fn agent_sessions(&self, owner: &str) -> Result<Vec<SessionView>> {
        let db = self.db.lock().unwrap();
        let mut statement = db.prepare(&format!(
            "SELECT {VIEW_COLUMNS} FROM agent_sessions WHERE owner=?1 ORDER BY updated_at DESC, rowid DESC LIMIT 200"
        ))?;
        let rows = statement.query_map([owner], view)?;
        rows.map(|row| Ok(row?)).collect()
    }
    /// Permanently removes the owner's session with its events and queued messages, in
    /// one transaction. Returns false when the owner has no such session.
    pub fn delete_agent_session(&self, owner: &str, id: &str) -> Result<bool> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let owned = tx
            .query_row(
                "SELECT 1 FROM agent_sessions WHERE id=?1 AND owner=?2",
                params![id, owner],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !owned {
            return Ok(false);
        }
        tx.execute("DELETE FROM session_events WHERE session_id=?1", [id])?;
        tx.execute("DELETE FROM pending_messages WHERE session_id=?1", [id])?;
        tx.execute(
            "DELETE FROM agent_sessions WHERE id=?1 AND owner=?2",
            params![id, owner],
        )?;
        tx.commit()?;
        Ok(true)
    }
    /// Permanently removes every closed session of the owner, with their events and
    /// queued messages, in one transaction. Returns how many sessions were removed.
    pub fn delete_closed_agent_sessions(&self, owner: &str) -> Result<usize> {
        const CLOSED: &str = "SELECT id FROM agent_sessions WHERE owner=?1 AND status='closed'";
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute(
            &format!("DELETE FROM session_events WHERE session_id IN ({CLOSED})"),
            [owner],
        )?;
        tx.execute(
            &format!("DELETE FROM pending_messages WHERE session_id IN ({CLOSED})"),
            [owner],
        )?;
        let deleted = tx.execute(
            "DELETE FROM agent_sessions WHERE owner=?1 AND status='closed'",
            [owner],
        )?;
        tx.commit()?;
        Ok(deleted)
    }
    /// Sessions left `running` by a previous process; the caller marks them interrupted.
    pub fn running_agent_sessions(&self) -> Result<Vec<SessionView>> {
        let db = self.db.lock().unwrap();
        let mut statement = db.prepare(&format!(
            "SELECT {VIEW_COLUMNS} FROM agent_sessions WHERE status='running'"
        ))?;
        let rows = statement.query_map([], view)?;
        rows.map(|row| Ok(row?)).collect()
    }
    pub fn set_session_status(&self, id: &str, s: SessionStatus) -> Result<()> {
        let changed = self.db.lock().unwrap().execute(
            "UPDATE agent_sessions SET status=?1, updated_at=?2 WHERE id=?3",
            params![s.as_str(), now_ms(), id],
        )?;
        ensure!(changed == 1, "session not found");
        Ok(())
    }
    pub fn set_session_title(&self, id: &str, title: &str) -> Result<()> {
        self.db.lock().unwrap().execute(
            "UPDATE agent_sessions SET title=?1, updated_at=?2 WHERE id=?3",
            params![clean_title(title), now_ms(), id],
        )?;
        Ok(())
    }
    pub fn set_session_box(&self, id: &str, box_id: Option<&str>) -> Result<()> {
        let changed = self.db.lock().unwrap().execute(
            "UPDATE agent_sessions SET box_id=?1, updated_at=?2 WHERE id=?3",
            params![box_id, now_ms(), id],
        )?;
        ensure!(changed == 1, "session not found");
        Ok(())
    }
    pub fn add_session_spend(&self, id: &str, usd: f64) -> Result<()> {
        ensure!(usd.is_finite() && usd >= 0.0, "invalid cost");
        self.db.lock().unwrap().execute(
            "UPDATE agent_sessions SET spend_usd=spend_usd+?1, updated_at=?2 WHERE id=?3",
            params![usd, now_ms(), id],
        )?;
        Ok(())
    }
    /// Appends one event with the next per-session sequence number.
    pub fn append_event(&self, id: &str, kind: &str, payload: Value) -> Result<SessionEvent> {
        ensure!(EVENT_KINDS.contains(&kind), "unknown event kind {kind}");
        let data = serde_json::to_string(&payload)?;
        ensure!(data.len() <= MAX_EVENT_BYTES, "event payload too large");
        let at = now_ms();
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let seq: Option<i64> = tx
            .query_row(
                "UPDATE agent_sessions SET last_seq=last_seq+1, updated_at=?1 WHERE id=?2 RETURNING last_seq",
                params![at, id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(seq) = seq else {
            anyhow::bail!("session not found")
        };
        tx.execute(
            "INSERT INTO session_events(session_id,seq,kind,payload,at) VALUES(?1,?2,?3,?4,?5)",
            params![id, seq, kind, data, at],
        )?;
        tx.commit()?;
        Ok(SessionEvent {
            session_id: id.into(),
            seq,
            kind: kind.into(),
            payload,
            at,
        })
    }
    /// Events with `seq > after`, oldest first, for the owner's session only.
    pub fn session_events(
        &self,
        owner: &str,
        id: &str,
        after: i64,
        limit: usize,
    ) -> Result<Vec<SessionEvent>> {
        let db = self.db.lock().unwrap();
        let mut statement = db.prepare(
            "SELECT e.session_id,e.seq,e.kind,e.payload,e.at FROM session_events e JOIN agent_sessions s ON s.id=e.session_id WHERE e.session_id=?1 AND s.owner=?2 AND e.seq>?3 ORDER BY e.seq ASC LIMIT ?4",
        )?;
        let rows = statement.query_map(
            params![id, owner, after, limit.min(MAX_EVENT_PAGE) as i64],
            event_row,
        )?;
        rows.map(|row| Ok(row?)).collect()
    }
    /// The newest `limit` events of a session, oldest first (used to rebuild model history).
    pub fn recent_session_events(&self, id: &str, limit: usize) -> Result<Vec<SessionEvent>> {
        let db = self.db.lock().unwrap();
        let mut statement = db.prepare(
            "SELECT session_id,seq,kind,payload,at FROM (SELECT * FROM session_events WHERE session_id=?1 ORDER BY seq DESC LIMIT ?2) ORDER BY seq ASC",
        )?;
        let rows = statement.query_map(params![id, limit.min(MAX_EVENT_PAGE) as i64], event_row)?;
        rows.map(|row| Ok(row?)).collect()
    }
    pub fn push_pending(&self, id: &str, text: &str) -> Result<()> {
        ensure!(
            !text.trim().is_empty() && text.len() <= MAX_MESSAGE_BYTES,
            "message must be 1-65536 bytes"
        );
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let count: u32 = tx.query_row(
            "SELECT COUNT(*) FROM pending_messages WHERE session_id=?1",
            [id],
            |r| r.get(0),
        )?;
        ensure!(count < MAX_PENDING, "too many queued messages");
        tx.execute(
            "INSERT INTO pending_messages(session_id,text,at) VALUES(?1,?2,?3)",
            params![id, text, now_ms()],
        )?;
        tx.commit()?;
        Ok(())
    }
    /// Removes and returns every queued message, oldest first. Each is returned once.
    pub fn take_pending(&self, id: &str) -> Result<Vec<String>> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let texts = {
            let mut statement =
                tx.prepare("SELECT text FROM pending_messages WHERE session_id=?1 ORDER BY id")?;
            let rows = statement.query_map([id], |r| r.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        tx.execute("DELETE FROM pending_messages WHERE session_id=?1", [id])?;
        tx.commit()?;
        Ok(texts)
    }
}

fn event_row(row: &Row) -> rusqlite::Result<SessionEvent> {
    let payload: String = row.get(3)?;
    Ok(SessionEvent {
        session_id: row.get(0)?,
        seq: row.get(1)?,
        kind: row.get(2)?,
        payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
        at: row.get(4)?,
    })
}
