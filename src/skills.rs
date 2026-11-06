//! Owner-scoped skills in the spirit of Anthropic's `SKILL.md`: a name and a one-line
//! description are indexed in the system prompt, and the body is loaded on demand.
use crate::{ledger::Ledger, session_store::now_ms};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

pub const MAX_SKILL_BODY_BYTES: usize = 64 * 1024;
pub const MAX_SKILL_DESCRIPTION_BYTES: usize = 1024;
pub const MAX_SKILLS_PER_OWNER: u32 = 200;

pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS skills(owner TEXT NOT NULL, name TEXT NOT NULL, description TEXT NOT NULL, body TEXT NOT NULL, updated_at INTEGER NOT NULL, PRIMARY KEY(owner, name));",
    )?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub body: String,
    pub updated_at: i64,
}
impl Skill {
    /// The skill as a `SKILL.md` document with YAML front matter.
    pub fn to_markdown(&self) -> String {
        format!(
            "---\nname: {}\ndescription: {}\n---\n\n{}\n",
            self.name,
            self.description.replace('\n', " "),
            self.body.trim_end()
        )
    }
}

/// `^[a-z0-9][a-z0-9-]{0,63}$`
pub fn valid_skill_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

pub fn validate_skill(name: &str, description: &str, body: &str) -> Result<()> {
    ensure!(
        valid_skill_name(name),
        "skill name must match ^[a-z0-9][a-z0-9-]{{0,63}}$"
    );
    ensure!(
        !description.trim().is_empty() && description.len() <= MAX_SKILL_DESCRIPTION_BYTES,
        "skill description must be 1-1024 bytes"
    );
    ensure!(
        !body.trim().is_empty() && body.len() <= MAX_SKILL_BODY_BYTES,
        "skill body must be 1-65536 bytes"
    );
    Ok(())
}

/// Parses a `SKILL.md` document (front matter with `name` and `description`, then the body).
pub fn parse_skill_md(text: &str) -> Result<(String, String, String)> {
    let rest = text
        .trim_start()
        .strip_prefix("---")
        .context("SKILL.md must start with --- front matter")?;
    let (front, body) = rest
        .split_once("\n---")
        .context("SKILL.md front matter is not closed")?;
    let field = |key: &str| {
        front
            .lines()
            .find_map(|line| line.trim().strip_prefix(key)?.strip_prefix(':'))
            .map(|v| v.trim().trim_matches('"').to_string())
    };
    let name = field("name").context("SKILL.md front matter needs name")?;
    let description = field("description").context("SKILL.md front matter needs description")?;
    let body = body.trim_start_matches('-').trim().to_string();
    validate_skill(&name, &description, &body)?;
    Ok((name, description, body))
}

/// Skills every owner sees until they save one with the same name.
pub fn builtin_skills() -> Vec<Skill> {
    [
        (
            "web-research",
            "Research a question on the web and answer with cited sources.",
            "1. Break the question into 2-4 search queries.\n2. Run web_search for each; prefer primary sources (official docs, papers, vendor pages) over aggregators.\n3. web_fetch the 3-6 most relevant results; skim with max_chars around 20000.\n4. Cross-check key facts in at least two sources; note disagreements and dates.\n5. Save longer findings to /workspace/research/<topic>.md as you go.\n6. Reply with a short answer first, then the supporting points, each with its source URL.",
        ),
        (
            "app-testing",
            "Run a web app inside the workspace and test it end to end with the browser.",
            "1. Install dependencies and build (npm ci / pip install -r requirements.txt / cargo build).\n2. Start the server in the background: nohup <cmd> > /tmp/app.log 2>&1 & then poll with curl -sf http://localhost:PORT until it answers (give up after ~30 s and read /tmp/app.log).\n3. browser goto the URL, then snapshot to get [ref=N] markers.\n4. Exercise the main flows with click/type/press, snapshotting after each step; check console for errors.\n5. Take a screenshot of the final state for the user (it lands in /workspace/.screenshots).\n6. Fix failures and re-test. Reply with what passed, what failed and the screenshot path as an attachment.",
        ),
        (
            "file-conversion",
            "Convert documents, images, audio and video between formats.",
            "Use convert_file with input and output paths; the extensions choose the converter (pandoc for md/html/docx/epub, LibreOffice for office formats and PDF, ImageMagick for images, ffmpeg for audio and video).\n- md -> pdf: convert to docx or html first if a direct conversion fails.\n- Check the result exists and is non-empty: bash ls -l <out> (and pdfinfo for PDFs).\n- For batches, loop in bash over convert <in> <out>.\n- Attach the outputs in reply.",
        ),
    ]
    .into_iter()
    .map(|(name, description, body)| Skill {
        name: name.into(),
        description: description.into(),
        body: body.into(),
        updated_at: 0,
    })
    .collect()
}

impl Ledger {
    pub fn skills(&self, owner: &str) -> Result<Vec<Skill>> {
        let db = self.db.lock().unwrap();
        let mut statement = db.prepare(
            "SELECT name,description,body,updated_at FROM skills WHERE owner=?1 ORDER BY name",
        )?;
        let rows = statement.query_map([owner], |r| {
            Ok(Skill {
                name: r.get(0)?,
                description: r.get(1)?,
                body: r.get(2)?,
                updated_at: r.get(3)?,
            })
        })?;
        rows.map(|row| Ok(row?)).collect()
    }
    pub fn skill(&self, owner: &str, name: &str) -> Result<Option<Skill>> {
        let db = self.db.lock().unwrap();
        Ok(db
            .query_row(
                "SELECT name,description,body,updated_at FROM skills WHERE owner=?1 AND name=?2",
                params![owner, name],
                |r| {
                    Ok(Skill {
                        name: r.get(0)?,
                        description: r.get(1)?,
                        body: r.get(2)?,
                        updated_at: r.get(3)?,
                    })
                },
            )
            .optional()?)
    }
    /// Creates or replaces a skill.
    pub fn save_skill(
        &self,
        owner: &str,
        name: &str,
        description: &str,
        body: &str,
    ) -> Result<Skill> {
        validate_skill(name, description, body)?;
        let skill = Skill {
            name: name.into(),
            description: description.trim().into(),
            body: body.into(),
            updated_at: now_ms(),
        };
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM skills WHERE owner=?1 AND name=?2)",
            params![owner, name],
            |r| r.get(0),
        )?;
        if !exists {
            let count: u32 =
                tx.query_row("SELECT COUNT(*) FROM skills WHERE owner=?1", [owner], |r| {
                    r.get(0)
                })?;
            ensure!(
                count < MAX_SKILLS_PER_OWNER,
                "skill limit reached ({MAX_SKILLS_PER_OWNER}); delete one first"
            );
        }
        tx.execute(
            "INSERT INTO skills(owner,name,description,body,updated_at) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(owner,name) DO UPDATE SET description=excluded.description, body=excluded.body, updated_at=excluded.updated_at",
            params![owner, skill.name, skill.description, skill.body, skill.updated_at],
        )?;
        tx.commit()?;
        Ok(skill)
    }
    pub fn delete_skill(&self, owner: &str, name: &str) -> Result<bool> {
        Ok(self.db.lock().unwrap().execute(
            "DELETE FROM skills WHERE owner=?1 AND name=?2",
            params![owner, name],
        )? == 1)
    }
}
