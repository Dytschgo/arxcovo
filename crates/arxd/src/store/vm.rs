//! The VM profile has its own activity schema and database version. The
//! shared and service openers have no route into this module.
use super::{check_size, validate_principal};
use crate::vm::{
    Action, Completion, ExpiryClaim, Id, Receipt, RecoveryClaim, Refusal, RequestId, Status,
};
use anyhow::{Result, bail, ensure};
use rusqlite::{
    Connection, ErrorCode, OptionalExtension, Transaction, TransactionBehavior, params,
};
use std::{fs, path::Path, sync::Mutex, time::Duration};

const SCHEMA: &str = "
CREATE TABLE activity (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    principal TEXT NOT NULL CHECK(length(CAST(principal AS BLOB)) BETWEEN 1 AND 64
        AND principal NOT GLOB '*[^a-zA-Z0-9_-]*'),
    actor TEXT NOT NULL CHECK(actor IN ('agent','daemon')),
    vm_owner TEXT CHECK(length(CAST(vm_owner AS BLOB)) BETWEEN 1 AND 64
        AND vm_owner NOT GLOB '*[^a-zA-Z0-9_-]*'),
    vm BLOB NOT NULL CHECK(length(vm)=16),
    action INTEGER NOT NULL CHECK(action BETWEEN 1 AND 10),
    started_ms INTEGER NOT NULL CHECK(started_ms BETWEEN 0 AND 253402300799999),
    status INTEGER NOT NULL DEFAULT 0 CHECK(status BETWEEN 0 AND 4),
    reason INTEGER CHECK(reason BETWEEN 1 AND 18),
    finished_ms INTEGER CHECK(finished_ms BETWEEN 0 AND 253402300799999),
    cleanup_confirmed INTEGER CHECK(cleanup_confirmed IN (0,1)),
    CHECK((status=0)=(finished_ms IS NULL)),
    CHECK((status=4)=(reason IS NOT NULL)),
    CHECK((actor='daemon')=(action IN (7,9,10)) AND (actor='agent' OR principal='arxd')),
    CHECK((vm_owner IS NOT NULL)=(action IN (7,10)))
) STRICT;
CREATE INDEX activity_principal ON activity(principal,id);
CREATE UNIQUE INDEX pending_vm ON activity(vm) WHERE status=0 AND action<=6;
CREATE TABLE vm_retry_clock (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    high_water_ms INTEGER NOT NULL CHECK(high_water_ms BETWEEN 0 AND 253402300799999)
) STRICT;
INSERT INTO vm_retry_clock VALUES(1,0);
CREATE TABLE vm_operations (
    principal TEXT NOT NULL CHECK(length(CAST(principal AS BLOB)) BETWEEN 1 AND 64
        AND principal NOT GLOB '*[^a-zA-Z0-9_-]*'),
    request_key BLOB NOT NULL CHECK(length(request_key)=16),
    vm BLOB NOT NULL CHECK(length(vm)=16),
    action INTEGER NOT NULL CHECK(action BETWEEN 1 AND 6),
    activity_id INTEGER NOT NULL UNIQUE CHECK(activity_id>0),
    valid_through_ms INTEGER NOT NULL CHECK(valid_through_ms>=0),
    status INTEGER NOT NULL DEFAULT 0 CHECK(status BETWEEN 0 AND 3),
    PRIMARY KEY(principal,request_key)
) STRICT, WITHOUT ROWID;
CREATE TABLE vm_templates (
    id BLOB PRIMARY KEY CHECK(length(id)=16),
    guest INTEGER NOT NULL CHECK(guest IN (1,2)),
    recipe_digest BLOB NOT NULL UNIQUE CHECK(length(recipe_digest)=32),
    disk_digest BLOB NOT NULL CHECK(length(disk_digest)=32),
    vars_digest BLOB NOT NULL CHECK(length(vars_digest)=32),
    bytes INTEGER NOT NULL CHECK(bytes>0)
) STRICT, WITHOUT ROWID;
-- 0 creating; 1 stopped; 2 running; 3 recovery; 4 teardown required.
-- Every row is a managed TEST instance. Shared/static assets have no row here.
CREATE TABLE vm_instances (
    id BLOB PRIMARY KEY CHECK(length(id)=16),
    template BLOB NOT NULL REFERENCES vm_templates(id),
    principal TEXT NOT NULL CHECK(length(CAST(principal AS BLOB)) BETWEEN 1 AND 64
        AND principal NOT GLOB '*[^a-zA-Z0-9_-]*'),
    expires_ms INTEGER NOT NULL CHECK(expires_ms BETWEEN 0 AND 253402300799999),
    state INTEGER NOT NULL DEFAULT 0 CHECK(state BETWEEN 0 AND 4),
    cleanup_id INTEGER UNIQUE REFERENCES activity(id),
    CHECK(cleanup_id IS NULL OR state IN (3,4)),
    CHECK(state<>3 OR cleanup_id IS NOT NULL)
) STRICT, WITHOUT ROWID;
CREATE INDEX vm_expiry ON vm_instances(expires_ms,id);
CREATE INDEX vm_owner ON vm_instances(principal,id);
CREATE TABLE vm_snapshots (
    vm BLOB NOT NULL REFERENCES vm_instances(id) ON DELETE CASCADE,
    id BLOB NOT NULL CHECK(length(id)=16),
    ready INTEGER NOT NULL DEFAULT 0 CHECK(ready IN (0,1)),
    PRIMARY KEY(vm,id)
) STRICT, WITHOUT ROWID;
-- Keep byte claims independent of audit retention. A claim with a snapshot
-- belongs to that snapshot. A claim without one belongs to the VM.
CREATE TABLE vm_disk_claims (
    activity_id INTEGER PRIMARY KEY,
    vm BLOB NOT NULL REFERENCES vm_instances(id) ON DELETE CASCADE,
    snapshot BLOB,
    persistent INTEGER NOT NULL CHECK(persistent>=0),
    staging INTEGER NOT NULL CHECK(staging>=0),
    CHECK(persistent>0 OR staging>0),
    FOREIGN KEY(vm,snapshot) REFERENCES vm_snapshots(vm,id) ON DELETE CASCADE
) STRICT;
PRAGMA user_version=5;";

pub(crate) const MAX_PENDING: i64 = 8;
const MAX_RECEIPTS: i64 = 128;
const MAX_LEDGER: i64 = 512;
// Leave space for finishing every pending operation, all eight expiry claims
// and their cleanup updates. New admission checks this after all its inserts.
const HEADROOM_PAGES: u64 = 64;

pub(crate) struct VmStore {
    db: Mutex<Connection>,
    _lock: fs::File,
}

impl VmStore {
    pub(crate) fn open(dir: &Path, now: i64) -> Result<Self> {
        crate::config::checked_path(dir)?;
        crate::security::private(dir)?;
        ensure!(dir.is_dir(), "VM state directory required");

        let lock_path = dir.join("activity.lock");
        check_existing_file(&lock_path)?;
        let mut options = fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options.open(lock_path)?;
        lock.try_lock()?;

        let path = dir.join("activity.sqlite3");
        let exists = check_existing_file(&path)?;
        for suffix in ["-journal", "-wal", "-shm"] {
            check_existing_file(&dir.join(format!("activity.sqlite3{suffix}")))?;
        }
        #[cfg(unix)]
        if !exists {
            use std::os::unix::fs::OpenOptionsExt;
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)?;
        }
        #[cfg(windows)]
        let _ = exists;
        let mut db = Connection::open(path)?;
        db.busy_timeout(Duration::from_millis(250))?;
        check_size(&db)?;
        let version: u32 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
        ensure!(matches!(version, 0 | 5), "unsupported VM state version");
        db.execute_batch(
            "PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA temp_store=MEMORY;
            PRAGMA foreign_keys=ON; PRAGMA max_page_count=1024;",
        )?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        match tx.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))? {
            0 => {
                ensure!(
                    count(&tx, "SELECT count(*) FROM sqlite_schema", [])? == 0,
                    "VM state must start with an empty database"
                );
                tx.execute_batch(SCHEMA)?;
            }
            5 => {}
            _ => bail!("unsupported VM state version"),
        }
        recover(&tx, now)?;
        check_size(&tx)?;
        tx.commit()?;
        Ok(Self {
            db: Mutex::new(db),
            _lock: lock,
        })
    }

    pub(crate) fn transact<T>(&self, run: impl FnOnce(&Transaction<'_>) -> Result<T>) -> Result<T> {
        let mut db = self
            .db
            .lock()
            .map_err(|_| anyhow::anyhow!("VM state unavailable"))?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let result = run(&tx)?;
        tx.commit()?;
        Ok(result)
    }
}

fn check_existing_file(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            crate::config::checked_path(path)?;
            ensure!(meta.is_file(), "regular VM state file required");
            crate::security::private(path)?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn count(db: &Connection, sql: &str, args: impl rusqlite::Params) -> Result<i64> {
    Ok(db.query_row(sql, args, |row| row.get(0))?)
}

pub(crate) fn headroom(db: &Connection) -> Result<bool> {
    let pages: u64 = db.pragma_query_value(None, "page_count", |row| row.get(0))?;
    let free: u64 = db.pragma_query_value(None, "freelist_count", |row| row.get(0))?;
    let max: u64 = db.pragma_query_value(None, "max_page_count", |row| row.get(0))?;
    Ok(pages
        .checked_sub(free)
        .and_then(|used| max.checked_sub(used))
        .is_some_and(|remaining| remaining >= HEADROOM_PAGES))
}

#[derive(Debug)]
pub(crate) struct StateHeadroom;

impl std::fmt::Display for StateHeadroom {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("VM state headroom exhausted")
    }
}

impl std::error::Error for StateHeadroom {}

pub(crate) fn require_headroom(db: &Connection) -> Result<()> {
    ensure!(headroom(db)?, StateHeadroom);
    Ok(())
}

pub(crate) fn full(error: &anyhow::Error) -> bool {
    error.is::<StateHeadroom>()
        || matches!(
            error.downcast_ref::<rusqlite::Error>(),
            Some(rusqlite::Error::SqliteFailure(err, _)) if err.code == ErrorCode::DiskFull
        )
}

pub(crate) fn activity(
    db: &Connection,
    principal: &str,
    vm: Id,
    action: Action,
    now: i64,
) -> Result<i64> {
    validate_principal(principal)?;
    db.execute(
        "INSERT INTO activity(principal,vm,action,started_ms,actor,vm_owner)
         VALUES(?1,?2,?3,?4,?5,CASE WHEN ?3 IN (7,10)
         THEN (SELECT principal FROM vm_instances WHERE id=?2) END)",
        params![
            principal,
            vm,
            action as i64,
            now,
            if matches!(
                action,
                Action::Expire | Action::SealTemplate | Action::Recover
            ) {
                "daemon"
            } else {
                "agent"
            }
        ],
    )?;
    Ok(db.last_insert_rowid())
}

pub(crate) fn refusal(
    db: &Connection,
    principal: &str,
    vm: Id,
    action: Action,
    now: i64,
    reason: Refusal,
) -> Result<()> {
    if headroom(db)? {
        validate_principal(principal)?;
        db.execute(
            "INSERT INTO activity(principal,vm,action,started_ms,status,reason,finished_ms,actor)
             VALUES(?1,?2,?3,?4,4,?5,?4,'agent')",
            params![principal, vm, action as i64, now, reason as i64],
        )?;
        prune(db)?;
        require_headroom(db)?;
    }
    Ok(())
}

pub(crate) fn finish(
    db: &Connection,
    id: i64,
    status: Status,
    now: i64,
    cleanup: Option<bool>,
) -> Result<()> {
    ensure!(db.execute(
        "UPDATE activity SET status=?2,finished_ms=?3,cleanup_confirmed=?4 WHERE id=?1 AND status=0",
        params![id, status.code(), now, cleanup],
    )? == 1, "VM activity already terminal");
    db.execute(
        "UPDATE vm_operations SET status=?2 WHERE activity_id=?1",
        params![id, status.code()],
    )?;
    prune(db)
}

fn prune(db: &Connection) -> Result<()> {
    db.execute(
        "DELETE FROM activity WHERE id IN
         (SELECT id FROM activity WHERE status<>0 ORDER BY id DESC LIMIT -1 OFFSET ?1)",
        [MAX_RECEIPTS],
    )?;
    Ok(())
}

pub(crate) fn retry_time(db: &Connection, now: i64) -> Result<i64> {
    Ok(db.query_row(
        "UPDATE vm_retry_clock SET high_water_ms=max(high_water_ms,?1) RETURNING high_water_ms",
        [now],
        |row| row.get(0),
    )?)
}

pub(crate) fn retry_receipt(
    db: &Connection,
    principal: &str,
    key: RequestId,
) -> Result<Option<(i64, Id, Receipt)>> {
    Ok(db.query_row(
        "SELECT action,vm,activity_id,status FROM vm_operations WHERE principal=?1 AND request_key=?2",
        params![principal, key.key],
        |row| Ok((row.get(0)?, row.get(1)?, Receipt {
            activity_id: row.get(2)?, status: Status::from_code(row.get(3)?)?,
        })),
    ).optional()?)
}

pub(crate) fn reserve_ledger_slot(db: &Connection, now: i64) -> Result<bool> {
    db.execute(
        "DELETE FROM vm_operations WHERE status<>0 AND valid_through_ms<?1",
        [now],
    )?;
    Ok(count(db, "SELECT count(*) FROM vm_operations", [])? < MAX_LEDGER)
}

pub(crate) fn disk_bytes(db: &Connection) -> Result<Option<u64>> {
    let mut stmt = db.prepare(
        "SELECT bytes FROM vm_templates
        UNION ALL SELECT persistent FROM vm_disk_claims
        UNION ALL SELECT staging FROM vm_disk_claims",
    )?;
    let mut rows = stmt.query([])?;
    let mut total = 0u64;
    while let Some(row) = rows.next()? {
        let Some(sum) = total.checked_add(row.get::<_, u64>(0)?) else {
            return Ok(None);
        };
        total = sum;
    }
    Ok(Some(total))
}

/// Preserve all charges on uncertainty, including on restart.
pub(crate) fn recover(db: &Connection, now: i64) -> Result<()> {
    db.execute("UPDATE vm_instances SET state=4 WHERE state=0", [])?;
    let mut stmt = db.prepare(
        "SELECT id FROM vm_instances WHERE state IN (1,2)
        AND (state=2 OR id IN (SELECT vm FROM activity WHERE status=0 AND action<=6)) LIMIT 8",
    )?;
    let interrupted = stmt
        .query_map([], |row| row.get::<_, Id>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for vm in interrupted {
        begin_recovery(db, vm, now)?;
    }
    db.execute("UPDATE vm_operations SET status=3 WHERE status=0", [])?;
    db.execute(
        "UPDATE activity SET status=3,finished_ms=?1,cleanup_confirmed=0
        WHERE status=0 AND action<=6",
        [now],
    )?;
    prune(db)
}

fn begin_recovery(db: &Connection, vm: Id, now: i64) -> Result<()> {
    let id = activity(db, "arxd", vm, Action::Recover, now)?;
    db.execute(
        "UPDATE vm_instances SET state=3,cleanup_id=?2 WHERE id=?1",
        params![vm, id],
    )?;
    Ok(())
}

pub(crate) fn settle_operation(
    db: &Connection,
    id: i64,
    now: i64,
    completion: Completion,
) -> Result<Status> {
    let (vm, action, state): (Id, i64, i64) = db.query_row(
        "SELECT a.vm,a.action,v.state FROM activity a JOIN vm_instances v ON a.vm=v.id
         WHERE a.id=?1 AND a.status=0 AND a.action<=6",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    if completion == Completion::CleanupUnconfirmed || state == 4 {
        if state == 0 {
            db.execute("UPDATE vm_instances SET state=4 WHERE id=?1", [vm])?;
        } else if state != 4 {
            begin_recovery(db, vm, now)?;
        }
        finish(db, id, Status::Interrupted, now, Some(false))?;
        return Ok(Status::Interrupted);
    }

    let status = if completion == Completion::Succeeded {
        Status::Succeeded
    } else {
        Status::Failed
    };
    let state = match (completion, action) {
        (Completion::Succeeded, x) if x == Action::Start as i64 => 2,
        (Completion::Succeeded, x) if x == Action::Stop as i64 || x == Action::Clone as i64 => 1,
        // Failure still requires teardown proof for partially created instances.
        (Completion::Failed, x) if x == Action::Clone as i64 => 4,
        _ => state,
    };
    db.execute(
        "UPDATE vm_instances SET state=?2 WHERE id=?1",
        params![vm, state],
    )?;
    let success = status == Status::Succeeded;
    db.execute(
        "UPDATE vm_snapshots SET ready=1 WHERE ?2 AND (vm,id) IN
        (SELECT vm,snapshot FROM vm_disk_claims WHERE activity_id=?1)",
        params![id, success],
    )?;
    db.execute(
        "DELETE FROM vm_snapshots WHERE NOT ?2 AND (vm,id) IN
        (SELECT vm,snapshot FROM vm_disk_claims WHERE activity_id=?1)",
        params![id, success],
    )?;
    release_staging(db, id)?;
    finish(db, id, status, now, Some(true))?;
    Ok(status)
}

fn release_staging(db: &Connection, id: i64) -> Result<()> {
    db.execute(
        "DELETE FROM vm_disk_claims WHERE activity_id=?1 AND persistent=0",
        [id],
    )?;
    db.execute(
        "UPDATE vm_disk_claims SET staging=0 WHERE activity_id=?1",
        [id],
    )?;
    Ok(())
}

pub(crate) fn expiry_claims(db: &Connection, now: i64) -> Result<Vec<ExpiryClaim>> {
    let mut stmt = db.prepare(
        "SELECT id,cleanup_id FROM vm_instances WHERE (state<>4 OR cleanup_id IS NULL)
        AND (state=4 OR expires_ms<=?1) ORDER BY expires_ms,id LIMIT 8",
    )?;
    let due = stmt
        .query_map([now], |row| {
            Ok((row.get::<_, Id>(0)?, row.get::<_, Option<i64>>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    due.into_iter()
        .map(|(vm, previous)| {
            let activity_id = activity(db, "arxd", vm, Action::Expire, now)?;
            db.execute(
                "UPDATE vm_instances SET state=4,cleanup_id=?2 WHERE id=?1",
                params![vm, activity_id],
            )?;
            if let Some(previous) = previous {
                finish(db, previous, Status::Interrupted, now, Some(false))?;
            }
            Ok(ExpiryClaim { vm, activity_id })
        })
        .collect()
}

pub(crate) fn expiry_intents(db: &Connection) -> Result<Vec<ExpiryClaim>> {
    let mut stmt = db.prepare(
        "SELECT id,cleanup_id FROM vm_instances WHERE state=4 AND cleanup_id IS NOT NULL ORDER BY id LIMIT 8",
    )?;
    Ok(stmt
        .query_map([], |row| {
            Ok(ExpiryClaim {
                vm: row.get(0)?,
                activity_id: row.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(crate) fn recovery_intents(db: &Connection) -> Result<Vec<RecoveryClaim>> {
    let mut stmt =
        db.prepare("SELECT id,cleanup_id FROM vm_instances WHERE state=3 ORDER BY id LIMIT 8")?;
    Ok(stmt
        .query_map([], |row| {
            Ok(RecoveryClaim {
                vm: row.get(0)?,
                activity_id: row.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}
