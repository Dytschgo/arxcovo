use anyhow::Result;
use rusqlite::{Connection, params};
use std::{path::Path, sync::Mutex, time::Duration};

/// Fixed-size metadata only; no arguments, environment, credentials or host paths.
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
        let db = Connection::open(path)?;
        db.busy_timeout(Duration::from_millis(250))?;
        let page_size: u64 = db.pragma_query_value(None, "page_size", |r| r.get(0))?;
        let page_count: u64 = db.pragma_query_value(None, "page_count", |r| r.get(0))?;
        if page_size > 4096 || page_count > 1024 {
            anyhow::bail!("activity store exceeds size bound");
        }
        db.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA max_page_count=1024; CREATE TABLE IF NOT EXISTS activity(id INTEGER PRIMARY KEY AUTOINCREMENT, principal TEXT NOT NULL, tool TEXT NOT NULL, outcome TEXT NOT NULL); UPDATE activity SET outcome='interrupted' WHERE outcome='started';")?;
        Ok(Self {
            db: Mutex::new(db),
            _lock: lock,
        })
    }
    pub fn start(&self, principal: &str, tool: &str) -> Result<i64> {
        if principal.is_empty()
            || principal.len() > 64
            || !principal
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
            || tool != "list_hosts"
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
        self.db
            .lock()
            .map_err(|_| anyhow::anyhow!("activity unavailable"))?
            .execute(
                "UPDATE activity SET outcome=?1 WHERE id=?2",
                params![outcome, id],
            )?;
        Ok(())
    }
}
