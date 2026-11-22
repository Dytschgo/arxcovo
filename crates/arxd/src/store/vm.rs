//! The VM profile has its own activity schema and database version. The
//! shared and service openers have no route into this module.
use super::{check_size, validate_principal};
use crate::vm::{
    Action, Completion, ExpiredVm, ExpiryClaim, Id, RETRY_WINDOW_MS, Receipt, RecoveryClaim,
    Refusal, RequestId, Status,
};
use anyhow::{Result, bail, ensure};
use rusqlite::{
    Connection, ErrorCode, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, ffi,
    params,
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
    terminal_order INTEGER UNIQUE CHECK(terminal_order>0),
    cleanup_confirmed INTEGER CHECK(cleanup_confirmed IN (0,1)),
    CHECK((status=0)=(finished_ms IS NULL)),
    CHECK((status=0)=(terminal_order IS NULL)),
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
    action INTEGER NOT NULL CHECK(action BETWEEN 1 AND 6 OR action=8),
    activity_id INTEGER NOT NULL UNIQUE CHECK(activity_id>0),
    valid_through_ms INTEGER NOT NULL CHECK(valid_through_ms>=0),
    status INTEGER NOT NULL DEFAULT 0 CHECK(status BETWEEN 0 AND 3),
    lease_end_ms INTEGER CHECK(lease_end_ms BETWEEN 0 AND 253402300799999),
    CHECK((action=8)=(lease_end_ms IS NOT NULL)),
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
CREATE TABLE vm_tombstones (
    id BLOB PRIMARY KEY CHECK(length(id)=16),
    principal TEXT NOT NULL CHECK(length(CAST(principal AS BLOB)) BETWEEN 1 AND 64
        AND principal NOT GLOB '*[^a-zA-Z0-9_-]*'),
    activity_id INTEGER NOT NULL UNIQUE REFERENCES activity(id),
    expired_at_ms INTEGER NOT NULL CHECK(expired_at_ms BETWEEN 0 AND 253402300799999)
) STRICT, WITHOUT ROWID;
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

pub(crate) const MAX_CALLER_PENDING: i64 = 8;
// One cleanup slot per live instance, shared by expiry and recovery. Cleanup
// stays available at caller saturation. At most 16 pending rows are visible.
pub(crate) const MAX_INSTANCES: i64 = 8;
const MAX_RECEIPTS: i64 = 128;
const MAX_LEDGER: i64 = 512;
const MAX_TOMBSTONES: i64 = 32;
const TOMBSTONE_LIFETIME_MS: i64 = 24 * 60 * 60 * 1000;
// Leave space for finishing every pending operation, all eight expiry claims
// and their cleanup updates. New admission checks this after all its inserts.
const HEADROOM_PAGES: u64 = 64;

pub(crate) struct VmStore {
    db: Mutex<Connection>,
    _database: fs::File,
    _lock: fs::File,
}

impl VmStore {
    pub(crate) fn open(dir: &Path, now: i64) -> Result<Self> {
        Self::open_inner(dir, now, open_sqlite)
    }

    fn open_inner(
        dir: &Path,
        now: i64,
        connect: impl FnOnce(&Path) -> Result<Connection>,
    ) -> Result<Self> {
        crate::config::checked_path(dir)?;
        crate::security::private(dir)?;
        ensure!(dir.is_dir(), "VM state directory required");

        let lock_path = dir.join("activity.lock");
        check_existing_file(&lock_path)?;
        let lock = state_options().write(true).create(true).open(&lock_path)?;
        lock.try_lock()?;
        verify_state_identity(&lock, &lock_path)?;

        let path = dir.join("activity.sqlite3");
        let exists = check_existing_file(&path)?;
        for suffix in ["-journal", "-wal", "-shm"] {
            check_existing_file(&dir.join(format!("activity.sqlite3{suffix}")))?;
        }
        let database = if exists {
            state_options().open(&path)?
        } else {
            state_options().write(true).create_new(true).open(&path)?
        };
        // The guard stays unlocked because SQLite owns the database locks.
        verify_state_identity(&database, &path)?;
        let expected = state_identity(&database)?;
        let mut db = connect(&path)?;
        ensure!(
            sqlite_main_identity(&mut db)? == expected && state_identity(&database)? == expected,
            "SQLite VM main database identity changed"
        );
        verify_state_identity(&database, &path)?;
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
            _database: database,
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

fn open_sqlite(path: &Path) -> Result<Connection> {
    Ok(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?)
}

fn check_existing_file(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            crate::config::checked_path(path)?;
            ensure!(meta.is_file(), "regular VM state file required");
            crate::security::private(path)?;
            state_identity(&state_options().open(path)?)?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn state_options() -> fs::OpenOptions {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        // Deny deletion/replacement while an identity guard is held.
        options
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options
}

fn verify_state_identity(file: &fs::File, path: &Path) -> Result<()> {
    ensure!(check_existing_file(path)?, "VM state file disappeared");
    ensure!(
        state_identity(file)? == state_identity(&state_options().open(path)?)?,
        "VM state file identity changed"
    );
    Ok(())
}

#[cfg(unix)]
fn state_identity(file: &fs::File) -> Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let m = file.metadata()?;
    ensure!(
        m.is_file() && m.nlink() == 1,
        "single-link regular VM state file required"
    );
    Ok((m.dev(), m.ino()))
}

#[cfg(windows)]
fn state_identity(file: &fs::File) -> Result<(u64, u64)> {
    use std::os::windows::io::AsRawHandle;
    windows_identity(file.as_raw_handle())
}

#[cfg(windows)]
fn windows_identity(handle: windows_sys::Win32::Foundation::HANDLE) -> Result<(u64, u64)> {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_TYPE_DISK, GetFileInformationByHandle, GetFileType,
    };
    ensure!(
        !handle.is_null() && handle != INVALID_HANDLE_VALUE,
        "invalid VM state handle"
    );
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // Both the retained Rust file and SQLite's borrowed handle outlive this query.
    ensure!(
        unsafe {
            GetFileType(handle) == FILE_TYPE_DISK
                && GetFileInformationByHandle(handle, &mut info) != 0
        },
        "unavailable VM state file identity"
    );
    ensure!(
        info.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) == 0
            && info.nNumberOfLinks == 1,
        "single-link regular VM state file required"
    );
    Ok((
        u64::from(info.dwVolumeSerialNumber),
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    ))
}

fn sqlite_main_identity(db: &mut Connection) -> Result<(u64, u64)> {
    use std::ffi::CStr;

    // The bundled extension qualifies both the native VFS and the main file's
    // I/O methods before we use either platform's file-control contract.
    // The exclusive connection borrow spans qualification and measurement.
    let handle = unsafe { db.handle() };
    ensure!(
        unsafe {
            ffi::sqlite3_libversion_number() == 3050002
                && CStr::from_ptr(ffi::sqlite3_sourceid()).to_bytes() == b"2025-06-28 14:00:48 2af157d77fb1304a74176eaee7fbc7c7e932d946bf25325e9c26c91db19e3079"
                && ffi::sqlite3_arxcovo_bound_abi_v1() == 1
                && ffi::sqlite3_arxcovo_qualify_main_v1(handle) == ffi::SQLITE_OK
        },
        "unqualified SQLite VM main database"
    );
    #[cfg(unix)]
    {
        const MAGIC: u32 = 0x41525849;
        const _: () = assert!(size_of::<ffi::ArxIdentityV1>() == 80);
        let mut identity = ffi::ArxIdentityV1 {
            magic: MAGIC,
            abi_version: 1,
            struct_bytes: 80,
            status: u32::MAX,
            os_errno: 0,
            vfs_kind: 0,
            dev_lo: 0,
            dev_hi: 0,
            ino_lo: 0,
            ino_hi: 0,
            nlink_lo: 0,
            nlink_hi: 0,
            mode: 0,
            flags: 0,
            reserved: [0; 6],
        };
        // The pinned Unix operation fstats the actual descriptor and writes
        // exactly the versioned result structure supplied here.
        let rc = unsafe {
            ffi::sqlite3_file_control(
                handle,
                c"main".as_ptr(),
                ffi::ARX_FCNTL_IDENTITY_V1,
                std::ptr::from_mut(&mut identity).cast(),
            )
        };
        let vfs_kind = if cfg!(target_os = "macos") { 2 } else { 1 };
        ensure!(
            rc == ffi::SQLITE_OK
                && identity.magic == MAGIC
                && identity.abi_version == 1
                && identity.struct_bytes == 80
                && identity.status == 0
                && identity.os_errno == 0
                && identity.vfs_kind == vfs_kind
                && identity.nlink_lo == 1
                && identity.nlink_hi == 0
                && identity.mode & 0o170077 == 0o100000
                && identity.flags == 0
                && identity.reserved == [0; 6],
            "unavailable SQLite VM main database identity"
        );
        Ok((
            (u64::from(identity.dev_hi) << 32) | u64::from(identity.dev_lo),
            (u64::from(identity.ino_hi) << 32) | u64::from(identity.ino_lo),
        ))
    }
    #[cfg(windows)]
    {
        let mut borrowed: windows_sys::Win32::Foundation::HANDLE = std::ptr::null_mut();
        let rc = unsafe {
            ffi::sqlite3_file_control(
                handle,
                c"main".as_ptr(),
                ffi::SQLITE_FCNTL_WIN32_GET_HANDLE,
                std::ptr::from_mut(&mut borrowed).cast(),
            )
        };
        ensure!(
            rc == ffi::SQLITE_OK,
            "unavailable SQLite VM main database handle"
        );
        // SQLite retains ownership. Inspect the handle without adopting or closing it.
        windows_identity(borrowed)
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
            "INSERT INTO activity(principal,vm,action,started_ms,status,reason,finished_ms,actor,terminal_order)
             VALUES(?1,?2,?3,?4,4,?5,?4,'agent',
               (SELECT coalesce(max(terminal_order),0)+1 FROM activity))",
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
    ensure!(
        db.execute(
            "UPDATE activity SET status=?2,finished_ms=?3,cleanup_confirmed=?4,
           terminal_order=(SELECT coalesce(max(terminal_order),0)+1 FROM activity)
         WHERE id=?1 AND status=0",
            params![id, status.code(), now, cleanup],
        )? == 1,
        "VM activity already terminal"
    );
    db.execute(
        "UPDATE vm_operations SET status=?2 WHERE activity_id=?1",
        params![id, status.code()],
    )?;
    prune(db)
}

fn prune(db: &Connection) -> Result<()> {
    db.execute(
        "DELETE FROM activity WHERE id IN
         (SELECT id FROM activity WHERE status<>0
          ORDER BY (EXISTS(SELECT 1 FROM vm_tombstones WHERE activity_id=activity.id)
                 OR EXISTS(SELECT 1 FROM vm_instances WHERE cleanup_id=activity.id)) DESC,
                   terminal_order DESC LIMIT -1 OFFSET ?1)",
        [MAX_RECEIPTS],
    )?;
    Ok(())
}

fn retry_time(db: &Connection, now: i64) -> Result<i64> {
    Ok(db.query_row(
        "UPDATE vm_retry_clock SET high_water_ms=max(high_water_ms,?1) RETURNING high_water_ms",
        [now],
        |row| row.get(0),
    )?)
}

pub(crate) struct Binding<'a> {
    pub principal: &'a str,
    pub key: RequestId,
    pub vm: Id,
    pub action: Action,
}

pub(crate) struct Replay {
    pub receipt: Receipt,
    pub lease_end: Option<i64>,
}

impl Binding<'_> {
    pub(crate) fn replay(
        &self,
        db: &Connection,
        now: i64,
        closed: bool,
    ) -> Result<Result<Option<Replay>, Refusal>> {
        let retry_now = retry_time(db, now)?;
        if self.valid_through() < retry_now {
            return Ok(Err(Refusal::StaleKey));
        }
        let previous = db
            .query_row(
                "SELECT action,vm,activity_id,status,lease_end_ms FROM vm_operations
             WHERE principal=?1 AND request_key=?2",
                params![self.principal, self.key.key],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Id>(1)?,
                        Receipt {
                            activity_id: row.get(2)?,
                            status: Status::from_code(row.get(3)?)?,
                        },
                        row.get(4)?,
                    ))
                },
            )
            .optional()?;
        if let Some((action, vm, receipt, lease_end)) = previous {
            return Ok(if action == self.action as i64 && vm == self.vm {
                Ok(Some(Replay { receipt, lease_end }))
            } else {
                Err(Refusal::KeyCollision)
            });
        }
        if self.key.issued_at_ms > now {
            return Ok(Err(Refusal::StaleKey));
        }
        if closed {
            return Ok(Err(Refusal::Shutdown));
        }
        if !headroom(db)? {
            return Ok(Err(Refusal::StateHeadroom));
        }
        db.execute(
            "DELETE FROM vm_operations WHERE status<>0 AND valid_through_ms<?1",
            [retry_now],
        )?;
        if count(db, "SELECT count(*) FROM vm_operations", [])? >= MAX_LEDGER {
            return Ok(Err(Refusal::ReplayFull));
        }
        Ok(Ok(None))
    }

    fn valid_through(&self) -> i64 {
        self.key.issued_at_ms + RETRY_WINDOW_MS
    }

    pub(crate) fn record(&self, db: &Connection, now: i64, lease_end: Option<i64>) -> Result<i64> {
        let id = activity(db, self.principal, self.vm, self.action, now)?;
        db.execute(
            "INSERT INTO vm_operations(principal,request_key,vm,action,activity_id,valid_through_ms,lease_end_ms)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![self.principal, self.key.key, self.vm, self.action as i64, id, self.valid_through(), lease_end],
        )?;
        Ok(id)
    }
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
    purge_tombstones(db, now)?;
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
    let mut stmt =
        db.prepare("SELECT id FROM activity WHERE status=0 AND action<=6 ORDER BY id LIMIT ?1")?;
    let pending = stmt
        .query_map([MAX_CALLER_PENDING], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for id in pending {
        finish(db, id, Status::Interrupted, now, Some(false))?;
    }
    Ok(())
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
    purge_tombstones(db, now)?;
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
            if let Some(previous) = previous {
                db.execute(
                    "UPDATE vm_instances SET state=4,cleanup_id=NULL WHERE id=?1",
                    [vm],
                )?;
                finish(db, previous, Status::Interrupted, now, Some(false))?;
            }
            let activity_id = activity(db, "arxd", vm, Action::Expire, now)?;
            db.execute(
                "UPDATE vm_instances SET state=4,cleanup_id=?2 WHERE id=?1",
                params![vm, activity_id],
            )?;
            Ok(ExpiryClaim { vm, activity_id })
        })
        .collect()
}

pub(crate) fn purge_tombstones(db: &Connection, now: i64) -> Result<()> {
    db.execute(
        "DELETE FROM vm_tombstones WHERE expired_at_ms<=?1",
        [now - TOMBSTONE_LIFETIME_MS],
    )?;
    Ok(())
}

pub(crate) fn record_expired(db: &Connection, claim: ExpiryClaim, now: i64) -> Result<()> {
    purge_tombstones(db, now)?;
    db.execute(
        "INSERT INTO vm_tombstones(id,principal,activity_id,expired_at_ms)
        SELECT id,principal,cleanup_id,?2 FROM vm_instances WHERE id=?1",
        params![claim.vm, now],
    )?;
    db.execute(
        "DELETE FROM vm_tombstones WHERE id IN
        (SELECT id FROM vm_tombstones ORDER BY (id=?2) DESC, expired_at_ms DESC, activity_id DESC
         LIMIT -1 OFFSET ?1)",
        params![MAX_TOMBSTONES, claim.vm],
    )?;
    db.execute("DELETE FROM vm_instances WHERE id=?1", [claim.vm])?;
    finish(db, claim.activity_id, Status::Succeeded, now, Some(true))
}

pub(crate) fn expired_status(
    db: &Connection,
    principal: &str,
    vm: Id,
) -> Result<Option<ExpiredVm>> {
    Ok(db
        .query_row(
            "SELECT expired_at_ms,activity_id FROM vm_tombstones WHERE id=?1 AND principal=?2",
            params![vm, principal],
            |row| {
                Ok(ExpiredVm {
                    expired_at_ms: row.get(0)?,
                    activity_id: row.get(1)?,
                })
            },
        )
        .optional()?)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::application::tests::private_temp;

    #[test]
    fn substituted_sqlite_main_refuses_before_reconciliation() {
        let now = 1_800_000_000_000;
        let original = private_temp();
        let replacement = private_temp();
        for dir in [&original, &replacement] {
            VmStore::open(dir.path(), now)
                .unwrap()
                .transact(|db| {
                    Binding {
                        principal: "fixture",
                        key: RequestId::parse("00000000000000000000000000000001").unwrap(),
                        vm: [1; 16],
                        action: Action::Script,
                    }
                    .record(db, now, None)?;
                    Ok(())
                })
                .unwrap();
        }

        let other_path = replacement.path().join("activity.sqlite3");
        let parked = original.path().join("parked.sqlite3");
        let result = VmStore::open_inner(original.path(), now, |path| {
            #[cfg(unix)]
            {
                // Put both names back before returning, so a before/after
                // pathname-only check would accept the substituted connection.
                fs::rename(path, &parked)?;
                fs::rename(&other_path, path)?;
                let opened = open_sqlite(path)?;
                fs::rename(path, &other_path)?;
                fs::rename(&parked, path)?;
                Ok(opened)
            }
            #[cfg(windows)]
            {
                // The retained guard denies rename here. Present a connection
                // to the other database to exercise the independent handle check.
                let error = fs::rename(path, &parked).unwrap_err();
                assert_eq!(
                    error.raw_os_error(),
                    Some(windows_sys::Win32::Foundation::ERROR_SHARING_VIOLATION as i32)
                );
                open_sqlite(&other_path)
            }
        });
        let Err(error) = result else {
            panic!("SQLite opened an unexpected main database");
        };
        assert_eq!(
            error.to_string(),
            "SQLite VM main database identity changed"
        );
        for dir in [&original, &replacement] {
            let raw = open_sqlite(&dir.path().join("activity.sqlite3")).unwrap();
            assert_eq!(
                count(&raw, "SELECT status FROM vm_operations", []).unwrap(),
                0
            );
            assert_eq!(count(&raw, "SELECT status FROM activity", []).unwrap(), 0);
        }
    }
}
