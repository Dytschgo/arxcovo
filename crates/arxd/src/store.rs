use anyhow::Result;
use rusqlite::{Connection, params};
use std::{path::Path, sync::Mutex, time::Duration};

/// Private local identity and bounded activity; no credentials or host paths.
pub struct Activity {
    db: Mutex<Connection>,
    _lock: std::fs::File,
}
impl Activity {
    pub fn open(dir: &Path) -> Result<Self> {
        crate::config::checked_path(dir)?;
        crate::security::private(dir)?;
        let lock_path = dir.join("activity.lock");
        if std::fs::symlink_metadata(&lock_path).is_ok() {
            crate::config::checked_path(&lock_path)?;
            crate::security::private(&lock_path)?;
        }
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options.open(&lock_path)?;
        lock.try_lock()?;
        let path = dir.join("activity.sqlite3");
        if std::fs::symlink_metadata(&path).is_ok() {
            crate::config::checked_path(&path)?;
            crate::security::private(&path)?;
        }
        #[cfg(unix)]
        if !path.exists() {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)?;
        }
        for suffix in ["-journal", "-wal", "-shm"] {
            let companion = dir.join(format!("activity.sqlite3{suffix}"));
            if std::fs::symlink_metadata(&companion).is_ok() {
                crate::config::checked_path(&companion)?;
                crate::security::private(&companion)?;
            }
        }
        let mut db = Connection::open(path)?;
        db.busy_timeout(Duration::from_millis(250))?;
        let page_size: u64 = db.pragma_query_value(None, "page_size", |r| r.get(0))?;
        let page_count: u64 = db.pragma_query_value(None, "page_count", |r| r.get(0))?;
        if page_size > 4096 || page_count > 1024 {
            anyhow::bail!("activity store exceeds size bound");
        }
        db.execute_batch(
            "PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA max_page_count=1024;",
        )?;
        let tx = db.transaction()?;
        let version: u32 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
        match version {
            0 => {
                // No IF NOT EXISTS: unexpected identity metadata must fail closed.
                // This also migrates pre-identity activity stores without losing events.
                tx.execute_batch("CREATE TABLE local_host_identity(singleton INTEGER PRIMARY KEY CHECK(singleton=1), host_id TEXT NOT NULL);")?;
                let mut random = [0u8; 32];
                getrandom::fill(&mut random)
                    .map_err(|_| anyhow::anyhow!("host identity randomness unavailable"))?;
                tx.execute(
                    "INSERT INTO local_host_identity VALUES(1, ?1)",
                    [hex::encode(random)],
                )?;
                tx.pragma_update(None, "user_version", 1)?;
            }
            1 => {}
            _ => anyhow::bail!("unsupported local state version"),
        }
        read_host_id(&tx)?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS activity(id INTEGER PRIMARY KEY AUTOINCREMENT, principal TEXT NOT NULL, tool TEXT NOT NULL, outcome TEXT NOT NULL); UPDATE activity SET outcome='interrupted' WHERE outcome='started';")?;
        tx.commit()?;
        Ok(Self {
            db: Mutex::new(db),
            _lock: lock,
        })
    }
    /// Application host identity only, unrelated to Raft node IDs or certificates.
    pub fn host_id(&self) -> Result<String> {
        let db = self
            .db
            .lock()
            .map_err(|_| anyhow::anyhow!("activity unavailable"))?;
        read_host_id(&db)
    }
    pub fn start(&self, principal: &str, tool: &str) -> Result<i64> {
        if principal.is_empty()
            || principal.len() > 64
            || !principal
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
            || !["list_hosts", "unknown_tool"].contains(&tool)
        {
            anyhow::bail!("invalid activity metadata");
        }
        let mut db = self
            .db
            .lock()
            .map_err(|_| anyhow::anyhow!("activity unavailable"))?;
        let tx = db.transaction()?;
        tx.execute("DELETE FROM activity WHERE id IN (SELECT id FROM activity ORDER BY id DESC LIMIT -1 OFFSET 999)", [])?;
        tx.execute(
            "INSERT INTO activity(principal,tool,outcome) VALUES (?1,?2,'started')",
            params![principal, tool],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(id)
    }
    pub fn finish(&self, id: i64, outcome: &str) -> Result<()> {
        if !["succeeded", "rejected"].contains(&outcome) {
            anyhow::bail!("invalid outcome");
        }
        let mut db = self
            .db
            .lock()
            .map_err(|_| anyhow::anyhow!("activity unavailable"))?;
        let tx = db.transaction()?;
        let changed = tx.execute(
            "UPDATE activity SET outcome=?1 WHERE id=?2 AND outcome='started'",
            params![outcome, id],
        )?;
        if changed != 1 {
            anyhow::bail!("activity completion unavailable");
        }
        tx.commit()?;
        Ok(())
    }
}

fn read_host_id(db: &Connection) -> Result<String> {
    // Bound even malformed values and reject missing/extra rows and non-text data.
    let mut stmt = db.prepare(
        "SELECT singleton, length(host_id), substr(host_id,1,65) FROM local_host_identity LIMIT 2",
    )?;
    let mut rows = stmt.query([])?;
    let row = rows
        .next()?
        .ok_or_else(|| anyhow::anyhow!("missing host identity"))?;
    let singleton: i64 = row.get(0)?;
    let length: i64 = row.get(1)?;
    let id: String = row.get(2)?;
    if singleton != 1
        || length != 64
        || id.len() != 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || rows.next()?.is_some()
    {
        anyhow::bail!("invalid host identity");
    }
    Ok(id)
}
