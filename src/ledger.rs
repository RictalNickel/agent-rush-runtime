use crate::domain::{Job, JobRequest, now};
use anyhow::{Result, bail, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::{path::Path, sync::Mutex};
use uuid::Uuid;

pub struct Ledger {
    pub(crate) db: Mutex<Connection>,
    capacity_mb: u32,
}

impl Ledger {
    pub fn memory(capacity_mb: u32) -> Result<Self> {
        Self::init(Connection::open_in_memory()?, capacity_mb)
    }
    pub fn open(path: &Path, capacity_mb: u32) -> Result<Self> {
        Self::init(Connection::open(path)?, capacity_mb)
    }
    fn init(db: Connection, capacity_mb: u32) -> Result<Self> {
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS jobs(id TEXT PRIMARY KEY, owner TEXT NOT NULL, idem TEXT NOT NULL, request TEXT NOT NULL, data TEXT NOT NULL, leased_mb INTEGER NOT NULL DEFAULT 0, UNIQUE(owner,idem));
            CREATE TABLE IF NOT EXISTS sessions(hash TEXT PRIMARY KEY, owner TEXT NOT NULL, csrf TEXT NOT NULL, expires INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS spend(id TEXT PRIMARY KEY, reserved REAL NOT NULL, actual REAL, at TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS memories(id TEXT PRIMARY KEY, owner TEXT NOT NULL, title TEXT NOT NULL, text TEXT NOT NULL, created_at TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS memories_owner ON memories(owner);
            CREATE TABLE IF NOT EXISTS agent_contexts(agent_id TEXT PRIMARY KEY, job_id TEXT NOT NULL REFERENCES jobs(id), owner TEXT NOT NULL, data TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS scheduler_meta(id INTEGER PRIMARY KEY CHECK(id=1), last_owner TEXT NOT NULL);
            INSERT OR IGNORE INTO scheduler_meta VALUES(1,'');")?;
        crate::session_store::migrate(&db)?;
        crate::skills::migrate(&db)?;
        crate::agent_config::migrate(&db)?;
        crate::api_keys::migrate(&db)?;
        Ok(Self {
            db: Mutex::new(db),
            capacity_mb,
        })
    }
    pub fn create_job(&self, owner: &str, request: JobRequest, key: &str) -> Result<Job> {
        request.validate().map_err(anyhow::Error::msg)?;
        ensure!(
            !key.is_empty() && key.len() <= 128,
            "idempotency key required (max 128 characters)"
        );
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let serialized = serde_json::to_string(&request)?;
        let previous: Option<(String, String)> = tx
            .query_row(
                "SELECT request,data FROM jobs WHERE owner=?1 AND idem=?2",
                params![owner, key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((old, data)) = previous {
            ensure!(old == serialized, "idempotency conflict");
            return Ok(serde_json::from_str(&data)?);
        }
        let active: u32 = tx.query_row("SELECT COUNT(*) FROM jobs WHERE owner=?1 AND json_extract(data,'$.state') IN ('queued','planning','executing','verifying','cleaning')", [owner], |r| r.get(0))?;
        ensure!(active < 32, "queue capacity reached");
        let job = Job {
            id: Uuid::new_v4().to_string(),
            owner: owner.into(),
            memory_mb: request.memory_mb,
            request,
            state: "queued".into(),
            cancel_requested: false,
            created_at: now(),
            updated_at: now(),
            receipt: None,
            dropped_events: 0,
            events: vec![
                json!({"at":now(),"kind":"queued","message":"Task accepted under fixed CLI policy"}),
            ],
        };
        tx.execute(
            "INSERT INTO jobs(id,owner,idem,request,data) VALUES(?1,?2,?3,?4,?5)",
            params![job.id, owner, key, serialized, serde_json::to_string(&job)?],
        )?;
        tx.commit()?;
        Ok(job)
    }
    pub fn get_job(&self, owner: &str, id: &str) -> Result<Option<Job>> {
        let db = self.db.lock().unwrap();
        let data: Option<String> = db
            .query_row(
                "SELECT data FROM jobs WHERE id=?1 AND owner=?2",
                params![id, owner],
                |r| r.get(0),
            )
            .optional()?;
        data.map(|v| Ok(serde_json::from_str(&v)?)).transpose()
    }
    pub fn list_jobs(&self, owner: &str) -> Result<Vec<Job>> {
        let db = self.db.lock().unwrap();
        let paths = (0..8)
            .map(|i| format!("'$.receipt.artifacts[{i}].data_base64'"))
            .collect::<Vec<_>>()
            .join(",");
        let query = format!(
            "SELECT json_remove(data,{paths}) FROM jobs WHERE owner=?1 ORDER BY rowid DESC LIMIT 100"
        );
        let mut stmt = db.prepare(&query)?;
        let rows = stmt.query_map([owner], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    pub fn active_jobs(&self) -> Result<Vec<Job>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT data FROM jobs WHERE leased_mb > 0")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    pub fn queued_jobs(&self, owner: &str) -> Result<Vec<Job>> {
        let db = self.db.lock().unwrap();
        let mut stmt=db.prepare("SELECT data FROM jobs WHERE owner=?1 AND json_extract(data,'$.state')='queued' ORDER BY rowid ASC LIMIT 32")?;
        let rows = stmt.query_map([owner], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    pub fn reserved_memory(&self) -> Result<u32> {
        Ok(self.db.lock().unwrap().query_row(
            "SELECT COALESCE(SUM(leased_mb),0) FROM jobs",
            [],
            |r| r.get(0),
        )?)
    }
    pub fn claim(&self, id: &str) -> Result<Option<Job>> {
        self.claim_with_capacity(id, self.capacity_mb)
    }
    pub fn scheduling_candidates(&self) -> Result<Vec<Job>> {
        let db = self.db.lock().unwrap();
        let mut statement = db.prepare("SELECT data FROM jobs WHERE rowid IN (SELECT MIN(rowid) FROM jobs WHERE json_extract(data,'$.state')='queued' GROUP BY owner) ORDER BY (owner=(SELECT last_owner FROM scheduler_meta WHERE id=1)) ASC,rowid ASC LIMIT 32")?;
        let rows = statement.query_map([], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    pub fn capacity_mb(&self) -> u32 {
        self.capacity_mb
    }
    pub fn claim_with_capacity(&self, id: &str, node_capacity_mb: u32) -> Result<Option<Job>> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let data: Option<String> = tx
            .query_row("SELECT data FROM jobs WHERE id=?1", [id], |r| r.get(0))
            .optional()?;
        let Some(data) = data else { return Ok(None) };
        let mut job: Job = serde_json::from_str(&data)?;
        if job.state != "queued" || job.cancel_requested {
            return Ok(None);
        }
        let slots: usize =
            tx.query_row("SELECT COUNT(*) FROM jobs WHERE leased_mb>0", [], |r| {
                r.get(0)
            })?;
        if slots >= crate::resources::MAX_CONCURRENT {
            return Ok(None);
        }
        let used: u32 = tx.query_row("SELECT COALESCE(SUM(leased_mb),0) FROM jobs", [], |r| {
            r.get(0)
        })?;
        if used.saturating_add(job.memory_mb) > self.capacity_mb.min(node_capacity_mb) {
            return Ok(None);
        }
        job.state = "planning".into();
        job.updated_at = now();
        job.record_event(json!({"at":now(),"kind":"admitted","message":"Memory lease admitted atomically","memory_mb":job.memory_mb}));
        tx.execute(
            "UPDATE jobs SET data=?1,leased_mb=?2 WHERE id=?3",
            params![serde_json::to_string(&job)?, job.memory_mb, id],
        )?;
        tx.execute(
            "UPDATE scheduler_meta SET last_owner=?1 WHERE id=1",
            [&job.owner],
        )?;
        tx.commit()?;
        Ok(Some(job))
    }
    fn mutate<F>(&self, id: &str, f: F) -> Result<Job>
    where
        F: FnOnce(&mut Job, u32) -> Result<u32>,
    {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let (data, leased): (String, u32) =
            tx.query_row("SELECT data,leased_mb FROM jobs WHERE id=?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        let mut job: Job = serde_json::from_str(&data)?;
        let lease = f(&mut job, leased)?;
        job.updated_at = now();
        tx.execute(
            "UPDATE jobs SET data=?1,leased_mb=?2 WHERE id=?3",
            params![serde_json::to_string(&job)?, lease, id],
        )?;
        tx.commit()?;
        Ok(job)
    }
    pub fn event(&self, id: &str, kind: &str, message: &str) -> Result<Job> {
        ensure!(message.len() <= 4096, "event too large");
        self.mutate(id, |job, lease| {
            job.record_event(json!({"at":now(),"kind":kind,"message":message}));
            if ["executing", "verifying", "cleaning"].contains(&kind) {
                job.state = kind.into();
            }
            Ok(lease)
        })
    }
    pub fn cancel(&self, owner: &str, id: &str) -> Result<Job> {
        ensure!(self.get_job(owner, id)?.is_some(), "not found");
        self.mutate(id,|job,lease| { if job.cancel_requested || ["succeeded","failed","cancelled"].contains(&job.state.as_str()) { return Ok(lease); } job.cancel_requested=true; if lease==0 { job.state="cancelled".into(); } job.record_event(json!({"at":now(),"kind":"cancel_requested","message":"Cancellation requested; active lease retained until cleanup"})); Ok(lease) })
    }
    pub fn finish(&self, id: &str, receipt: Value) -> Result<Job> {
        self.mutate(id,|job,lease| {
            let clean = receipt["cleanup_state"]=="verified";
            let success = clean && receipt["reason"]=="completed" && receipt["exit_code"]==0 && receipt["checks_passed"]==true;
            job.state = if !clean {"cleaning"} else if job.cancel_requested || receipt["reason"]=="cancelled" {"cancelled"} else if success {"succeeded"} else {"failed"}.into();
            job.record_event(json!({"at":now(),"kind":job.state,"message": if clean {"Worker cleanup verified; lease released"} else {"Cleanup unresolved; lease retained"}}));
            job.receipt=Some(receipt); Ok(if clean {0} else {lease})
        })
    }
    pub fn grow(&self, owner: &str, id: &str, target: u32) -> Result<Job> {
        self.grow_with_capacity(owner, id, target, self.capacity_mb)
    }
    pub fn grow_with_capacity(
        &self,
        owner: &str,
        id: &str,
        target: u32,
        node_capacity_mb: u32,
    ) -> Result<Job> {
        ensure!((128..=1024).contains(&target), "memory exceeds policy");
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let (data, old): (String, u32) = tx.query_row(
            "SELECT data,leased_mb FROM jobs WHERE id=?1 AND owner=?2",
            params![id, owner],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let mut job: Job = serde_json::from_str(&data)?;
        ensure!(old > 0 && target >= old, "only active leases can grow");
        ensure!(
            matches!(job.state.as_str(), "planning" | "verifying"),
            "memory growth applies only between sandbox attempts"
        );
        let used: u32 = tx.query_row("SELECT COALESCE(SUM(leased_mb),0) FROM jobs", [], |r| {
            r.get(0)
        })?;
        ensure!(
            used - old + target <= self.capacity_mb.min(node_capacity_mb),
            "insufficient capacity"
        );
        job.memory_mb = target;
        job.updated_at = now();
        job.record_event(json!({"at":now(),"kind":"memory_granted","memory_mb":target,"message":"Atomic increase reserved for the next sandbox attempt"}));
        tx.execute(
            "UPDATE jobs SET data=?1,leased_mb=?2 WHERE id=?3",
            params![serde_json::to_string(&job)?, target, id],
        )?;
        tx.commit()?;
        Ok(job)
    }
    pub fn session(&self, hash: &str) -> Result<Option<(String, String)>> {
        Ok(self
            .db
            .lock()
            .unwrap()
            .query_row(
                "SELECT owner,csrf FROM sessions WHERE hash=?1 AND expires>?2",
                params![hash, chrono::Utc::now().timestamp()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }
    pub fn add_session(&self, hash: &str, csrf: &str) -> Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "DELETE FROM sessions WHERE expires<=?1",
            [chrono::Utc::now().timestamp()],
        )?;
        db.execute(
            "INSERT INTO sessions VALUES(?1,'operator',?2,?3)",
            params![hash, csrf, chrono::Utc::now().timestamp() + 3600],
        )?;
        Ok(())
    }
    pub fn remove_session(&self, hash: &str) -> Result<()> {
        self.db
            .lock()
            .unwrap()
            .execute("DELETE FROM sessions WHERE hash=?1", [hash])?;
        Ok(())
    }
    pub fn reserve_spend(&self, id: &str, amount: f64, limit: f64) -> Result<()> {
        ensure!(
            amount.is_finite() && amount > 0.0 && limit.is_finite() && limit >= 0.0,
            "invalid cost"
        );
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let total: f64 = tx.query_row(
            "SELECT COALESCE(SUM(COALESCE(actual,reserved)),0) FROM spend",
            [],
            |r| r.get(0),
        )?;
        if total + amount > limit {
            bail!("inference budget exhausted");
        }
        tx.execute(
            "INSERT INTO spend(id,reserved,at) VALUES(?1,?2,?3)",
            params![id, amount, now()],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn settle_spend(&self, id: &str, amount: f64) -> Result<()> {
        ensure!(amount.is_finite() && amount >= 0.0, "invalid cost");
        self.db.lock().unwrap().execute(
            "UPDATE spend SET actual=?1 WHERE id=?2",
            params![amount, id],
        )?;
        Ok(())
    }
    pub fn spend_total(&self) -> Result<f64> {
        Ok(self.db.lock().unwrap().query_row(
            "SELECT COALESCE(SUM(COALESCE(actual,reserved)),0) FROM spend",
            [],
            |r| r.get(0),
        )?)
    }
}
