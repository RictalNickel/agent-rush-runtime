//! Owner-scoped API keys and webhook registrations, in the ledger's SQLite.
//! A key is `ar_` followed by 48 hex characters. Only its SHA-256 is stored, so the key
//! itself is shown once, at creation. Every `*_at` value is Unix time in milliseconds.
use crate::{ledger::Ledger, session_store::now_ms};
use anyhow::{Result, ensure};
use rand::RngCore;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use sha2::{Digest, Sha256};

/// Active (unrevoked) keys one owner may hold.
pub const MAX_KEYS_PER_OWNER: u32 = 50;
/// Webhooks one owner may register.
pub const MAX_WEBHOOKS_PER_OWNER: u32 = 10;
const KEY_PREFIX: &str = "ar_";
const KEY_HEX_LEN: usize = 48;
/// `last_used_at` is written at most this often per key.
const LAST_USED_RESOLUTION_MS: i64 = 60_000;

pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS api_keys(id TEXT PRIMARY KEY, owner TEXT NOT NULL, name TEXT NOT NULL, hash TEXT NOT NULL UNIQUE, prefix TEXT NOT NULL, created_at INTEGER NOT NULL, last_used_at INTEGER, revoked_at INTEGER);
        CREATE INDEX IF NOT EXISTS api_keys_owner ON api_keys(owner);
        CREATE TABLE IF NOT EXISTS webhooks(id TEXT PRIMARY KEY, owner TEXT NOT NULL, url TEXT NOT NULL, secret TEXT NOT NULL, created_at INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS webhooks_owner ON webhooks(owner);",
    )?;
    Ok(())
}

/// What the owner sees about a key. Never contains the key or its hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApiKeyView {
    pub id: String,
    pub name: String,
    /// The first characters of the key, to recognise it (`ar_1a2b3c4d`).
    pub prefix: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
    pub revoked: bool,
}

/// A newly created key. `key` is returned once and never stored.
#[derive(Debug, Clone, Serialize)]
pub struct CreatedKey {
    #[serde(flatten)]
    pub view: ApiKeyView,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Webhook {
    pub id: String,
    pub url: String,
    pub created_at: i64,
}

/// A webhook with its signing secret, for delivery and for the one-time create response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WebhookWithSecret {
    #[serde(flatten)]
    pub webhook: Webhook,
    pub secret: String,
}

pub fn key_hash(key: &str) -> String {
    hex::encode(Sha256::digest(key.as_bytes()))
}

/// True when the value has the shape of an API key (`ar_` + 48 lowercase hex).
pub fn looks_like_key(value: &str) -> bool {
    value.strip_prefix(KEY_PREFIX).is_some_and(|hex| {
        hex.len() == KEY_HEX_LEN
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

fn clean_name(name: &str) -> Result<String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    ensure!(name.len() <= 100, "name must be at most 100 bytes");
    Ok(if name.is_empty() {
        "API key".into()
    } else {
        name
    })
}

fn key_view(row: &rusqlite::Row) -> rusqlite::Result<ApiKeyView> {
    Ok(ApiKeyView {
        id: row.get(0)?,
        name: row.get(1)?,
        prefix: row.get(2)?,
        created_at: row.get(3)?,
        last_used_at: row.get(4)?,
        revoked: row.get::<_, Option<i64>>(5)?.is_some(),
    })
}

impl Ledger {
    pub fn create_api_key(&self, owner: &str, name: &str) -> Result<CreatedKey> {
        ensure!(!owner.is_empty() && owner.len() <= 128, "invalid key owner");
        let name = clean_name(name)?;
        let key = format!("{KEY_PREFIX}{}", random_hex(KEY_HEX_LEN / 2));
        let view = ApiKeyView {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            prefix: key[..KEY_PREFIX.len() + 8].to_string(),
            created_at: now_ms(),
            last_used_at: None,
            revoked: false,
        };
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let active: u32 = tx.query_row(
            "SELECT COUNT(*) FROM api_keys WHERE owner=?1 AND revoked_at IS NULL",
            [owner],
            |r| r.get(0),
        )?;
        ensure!(
            active < MAX_KEYS_PER_OWNER,
            "key limit reached ({MAX_KEYS_PER_OWNER}); revoke one first"
        );
        tx.execute(
            "INSERT INTO api_keys(id,owner,name,hash,prefix,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                view.id,
                owner,
                view.name,
                key_hash(&key),
                view.prefix,
                view.created_at
            ],
        )?;
        tx.commit()?;
        Ok(CreatedKey { view, key })
    }
    pub fn api_keys(&self, owner: &str) -> Result<Vec<ApiKeyView>> {
        let db = self.db.lock().unwrap();
        let mut statement = db.prepare(
            "SELECT id,name,prefix,created_at,last_used_at,revoked_at FROM api_keys WHERE owner=?1 ORDER BY created_at DESC",
        )?;
        let rows = statement.query_map([owner], key_view)?;
        rows.map(|row| Ok(row?)).collect()
    }
    /// Revokes the owner's key. False when the owner has no such active key.
    pub fn revoke_api_key(&self, owner: &str, id: &str) -> Result<bool> {
        Ok(self.db.lock().unwrap().execute(
            "UPDATE api_keys SET revoked_at=?3 WHERE id=?1 AND owner=?2 AND revoked_at IS NULL",
            params![id, owner, now_ms()],
        )? == 1)
    }
    /// The owner of an active key, recording its use. None for unknown or revoked keys.
    pub fn api_key_owner(&self, key: &str) -> Result<Option<String>> {
        if !looks_like_key(key) {
            return Ok(None);
        }
        let hash = key_hash(key);
        let db = self.db.lock().unwrap();
        let row: Option<(String, String)> = db
            .query_row(
                "SELECT id,owner FROM api_keys WHERE hash=?1 AND revoked_at IS NULL",
                [&hash],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((id, owner)) = row else {
            return Ok(None);
        };
        let now = now_ms();
        db.execute(
            "UPDATE api_keys SET last_used_at=?2 WHERE id=?1 AND (last_used_at IS NULL OR last_used_at<?3)",
            params![id, now, now - LAST_USED_RESOLUTION_MS],
        )?;
        Ok(Some(owner))
    }

    /// Stores a webhook (its URL already checked by the caller) with a fresh secret.
    pub fn create_webhook(&self, owner: &str, url: &str) -> Result<WebhookWithSecret> {
        ensure!(url.len() <= 2048, "webhook URL must be at most 2048 bytes");
        let hook = WebhookWithSecret {
            webhook: Webhook {
                id: uuid::Uuid::new_v4().to_string(),
                url: url.into(),
                created_at: now_ms(),
            },
            secret: format!("whsec_{}", random_hex(32)),
        };
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let count: u32 = tx.query_row(
            "SELECT COUNT(*) FROM webhooks WHERE owner=?1",
            [owner],
            |r| r.get(0),
        )?;
        ensure!(
            count < MAX_WEBHOOKS_PER_OWNER,
            "webhook limit reached ({MAX_WEBHOOKS_PER_OWNER}); delete one first"
        );
        tx.execute(
            "INSERT INTO webhooks(id,owner,url,secret,created_at) VALUES(?1,?2,?3,?4,?5)",
            params![
                hook.webhook.id,
                owner,
                hook.webhook.url,
                hook.secret,
                hook.webhook.created_at
            ],
        )?;
        tx.commit()?;
        Ok(hook)
    }
    pub fn webhooks(&self, owner: &str) -> Result<Vec<Webhook>> {
        Ok(self
            .webhooks_with_secrets(owner)?
            .into_iter()
            .map(|h| h.webhook)
            .collect())
    }
    pub(crate) fn webhooks_with_secrets(&self, owner: &str) -> Result<Vec<WebhookWithSecret>> {
        let db = self.db.lock().unwrap();
        let mut statement = db.prepare(
            "SELECT id,url,created_at,secret FROM webhooks WHERE owner=?1 ORDER BY created_at",
        )?;
        let rows = statement.query_map([owner], |r| {
            Ok(WebhookWithSecret {
                webhook: Webhook {
                    id: r.get(0)?,
                    url: r.get(1)?,
                    created_at: r.get(2)?,
                },
                secret: r.get(3)?,
            })
        })?;
        rows.map(|row| Ok(row?)).collect()
    }
    pub fn delete_webhook(&self, owner: &str, id: &str) -> Result<bool> {
        Ok(self.db.lock().unwrap().execute(
            "DELETE FROM webhooks WHERE id=?1 AND owner=?2",
            params![id, owner],
        )? == 1)
    }
    /// The owner of any agent session, for trusted background work such as webhooks.
    pub(crate) fn agent_session_owner(&self, id: &str) -> Result<Option<String>> {
        Ok(self
            .db
            .lock()
            .unwrap()
            .query_row("SELECT owner FROM agent_sessions WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?)
    }
}
