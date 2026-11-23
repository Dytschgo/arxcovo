//! The VM profile has its own activity schema and database version. The
//! shared and service openers have no route into this module.
//! Each VM opener rejects later versions: v5 rejects v6 and v6 rejects v7.
//! Upgrades are forward-only. Upgraded state needs a matching or newer opener.
use super::{check_size, validate_principal};
use crate::vm::{
    Action, ArtifactEvidence, ArtifactSlot, BuildRequest, BuildSlots, Completion, ExpiredVm,
    ExpiryClaim, Guest, Id, OperationRef, OperationStatus, PairSlots, RETRY_WINDOW_MS, Receipt,
    RecoveryClaim, Refusal, RequestId, ScriptResult, Status, Subject, VmStatus,
    paths::FileIdentity,
    reaper::{CleanupArtifact, CleanupKind, CleanupWork},
};
use anyhow::{Result, ensure};
use rusqlite::{
    Connection, ErrorCode, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, ffi,
    params,
};
use std::{fs, path::Path, sync::Mutex, time::Duration};

const SCHEMA_V5: &str = "
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

const VM_SCHEMA_VERSION: u32 = 7;

// Activity IDs also serve as audit order and as references from cleanup and
// tombstone rows. Copy them verbatim and carry the AUTOINCREMENT high-water.
// Action 11 is private build storage. The public Action codes stay as they were.
const MIGRATE_V6: &str = "
CREATE TABLE activity_v6 (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    principal TEXT NOT NULL CHECK(length(CAST(principal AS BLOB)) BETWEEN 1 AND 64
        AND principal NOT GLOB '*[^a-zA-Z0-9_-]*'),
    actor TEXT NOT NULL CHECK(actor IN ('agent','daemon')),
    vm_owner TEXT CHECK(length(CAST(vm_owner AS BLOB)) BETWEEN 1 AND 64
        AND vm_owner NOT GLOB '*[^a-zA-Z0-9_-]*'),
    vm BLOB CHECK(length(vm)=16),
    action INTEGER NOT NULL CHECK(action BETWEEN 1 AND 11),
    started_ms INTEGER NOT NULL CHECK(started_ms BETWEEN 0 AND 253402300799999),
    status INTEGER NOT NULL DEFAULT 0 CHECK(status BETWEEN 0 AND 4),
    reason INTEGER CHECK(reason BETWEEN 1 AND 18),
    finished_ms INTEGER CHECK(finished_ms BETWEEN 0 AND 253402300799999),
    terminal_order INTEGER UNIQUE CHECK(terminal_order>0),
    cleanup_confirmed INTEGER CHECK(cleanup_confirmed IN (0,1)),
    CHECK(vm IS NOT NULL OR action=11),
    CHECK((status=0)=(finished_ms IS NULL)),
    CHECK((status=0)=(terminal_order IS NULL)),
    CHECK((status=4)=(reason IS NOT NULL)),
    CHECK((actor='daemon')=(action IN (7,9,10)) AND (actor='agent' OR principal='arxd')),
    CHECK((vm_owner IS NOT NULL)=(action IN (7,10)))
) STRICT;
INSERT INTO activity_v6 SELECT * FROM activity;
INSERT INTO sqlite_sequence(name,seq) SELECT 'activity_v6',seq FROM sqlite_sequence
    WHERE name='activity' AND NOT EXISTS(SELECT 1 FROM sqlite_sequence WHERE name='activity_v6');
UPDATE sqlite_sequence SET seq=max(seq,coalesce((SELECT seq FROM sqlite_sequence WHERE name='activity'),0))
    WHERE name='activity_v6';
DROP TABLE activity;
ALTER TABLE activity_v6 RENAME TO activity;
CREATE INDEX activity_principal ON activity(principal,id);
CREATE UNIQUE INDEX pending_vm ON activity(vm) WHERE status=0 AND action<=6;

CREATE TABLE vm_operations_v6 (
    principal TEXT NOT NULL CHECK(length(CAST(principal AS BLOB)) BETWEEN 1 AND 64
        AND principal NOT GLOB '*[^a-zA-Z0-9_-]*'),
    request_key BLOB NOT NULL CHECK(length(request_key)=16),
    operation_id BLOB NOT NULL UNIQUE DEFAULT (randomblob(16)) CHECK(length(operation_id)=16),
    vm BLOB CHECK(length(vm)=16),
    action INTEGER NOT NULL CHECK(action BETWEEN 1 AND 6 OR action IN (8,11)),
    activity_id INTEGER NOT NULL UNIQUE CHECK(activity_id>0),
    valid_through_ms INTEGER NOT NULL CHECK(valid_through_ms>=0),
    status INTEGER NOT NULL DEFAULT 0 CHECK(status BETWEEN 0 AND 3),
    lease_end_ms INTEGER CHECK(lease_end_ms BETWEEN 0 AND 253402300799999),
    binding_kind TEXT NOT NULL DEFAULT 'legacy_vm'
        CHECK(binding_kind IN ('legacy_vm','vm','template','recipe')),
    subject_id BLOB CHECK(length(subject_id)=16),
    recipe_id TEXT CHECK(length(recipe_id) BETWEEN 1 AND 64
        AND length(recipe_id)=length(CAST(recipe_id AS BLOB))
        AND recipe_id NOT GLOB '*[^a-z0-9-]*'),
    recipe_digest BLOB CHECK(length(recipe_digest)=32),
    request_digest BLOB CHECK(length(request_digest)=32),
    script_result TEXT CHECK(script_result IN
        ('legacy','exited','unavailable','timed_out','launch_failed','terminated','interrupted')),
    script_exit_code INTEGER CHECK(script_exit_code BETWEEN 0 AND 4294967295),
    CHECK((action=8)=(lease_end_ms IS NOT NULL)),
    CHECK((action=11)=(vm IS NULL)),
    CHECK((action=11)=(binding_kind='recipe')),
    CHECK((binding_kind IN ('vm','template'))=(subject_id IS NOT NULL)),
    CHECK((binding_kind='recipe')=(recipe_id IS NOT NULL)
        AND (binding_kind='recipe')=(recipe_digest IS NOT NULL)),
    CHECK(binding_kind NOT IN ('vm','template') OR (action=1)=(binding_kind='template')),
    CHECK(binding_kind<>'vm' OR subject_id=vm),
    CHECK((binding_kind='template')=(request_digest IS NOT NULL)),
    CHECK(action=2 OR script_result IS NULL),
    CHECK(action<>2 OR status=0 OR script_result IS NOT NULL),
    CHECK(script_result IS NULL OR status<>0 OR script_result='legacy'),
    CHECK((script_result IS 'exited')=(script_exit_code IS NOT NULL)),
    PRIMARY KEY(principal,request_key)
) STRICT, WITHOUT ROWID;
-- Ledger rows can outlive their activity projection. Copy directly from the ledger.
INSERT INTO vm_operations_v6(principal,request_key,vm,action,activity_id,valid_through_ms,
    status,lease_end_ms,script_result)
    SELECT principal,request_key,vm,action,activity_id,valid_through_ms,status,lease_end_ms,
        CASE WHEN action=2 THEN 'legacy' END FROM vm_operations;
DROP TABLE vm_operations;
ALTER TABLE vm_operations_v6 RENAME TO vm_operations;

-- Build charges live independently of instances. Completed build evidence can
-- outlive its retry ledger. The sealed charge lives in the referenced template.
CREATE TABLE vm_build_intents (
    id BLOB PRIMARY KEY CHECK(length(id)=16),
    operation_id BLOB UNIQUE REFERENCES vm_operations(operation_id) ON DELETE SET NULL,
    recipe_id TEXT NOT NULL CHECK(length(recipe_id) BETWEEN 1 AND 64
        AND length(recipe_id)=length(CAST(recipe_id AS BLOB))
        AND recipe_id NOT GLOB '*[^a-z0-9-]*'),
    recipe_digest BLOB NOT NULL CHECK(length(recipe_digest)=32),
    staging_bytes INTEGER NOT NULL CHECK(staging_bytes>=0),
    template_id BLOB UNIQUE REFERENCES vm_templates(id),
    health_verified INTEGER NOT NULL DEFAULT 0 CHECK(health_verified IN (0,1)),
    state TEXT NOT NULL CHECK(state IN ('pending','sealed','pending_disposal','quarantined')),
    CHECK((state='sealed')=(template_id IS NOT NULL)),
    CHECK((state='sealed')=(staging_bytes=0)),
    CHECK(state<>'sealed' OR health_verified=1),
    CHECK(state<>'pending' OR operation_id IS NOT NULL)
) STRICT, WITHOUT ROWID;
CREATE UNIQUE INDEX vm_pending_build ON vm_build_intents(recipe_digest) WHERE state='pending';

CREATE TABLE vm_generations (
    id BLOB PRIMARY KEY CHECK(length(id)=16),
    build_id BLOB REFERENCES vm_build_intents(id),
    vm_id BLOB REFERENCES vm_instances(id),
    generation INTEGER NOT NULL CHECK(generation>0),
    active INTEGER NOT NULL DEFAULT 0 CHECK(active IN (0,1)),
    CHECK((build_id IS NULL)<>(vm_id IS NULL)),
    UNIQUE(build_id,generation),
    UNIQUE(vm_id,generation)
) STRICT, WITHOUT ROWID;
CREATE UNIQUE INDEX vm_active_build_generation ON vm_generations(build_id) WHERE active=1;
CREATE UNIQUE INDEX vm_active_instance_generation ON vm_generations(vm_id) WHERE active=1;

-- object_key is a generated ID used with the role to derive a relative name.
-- File evidence uses opaque binary identities and bounded content digests.
CREATE TABLE vm_artifacts (
    generation_id BLOB NOT NULL REFERENCES vm_generations(id),
    role TEXT NOT NULL CHECK(role IN ('disk','vars','seed','verification_overlay')),
    object_key BLOB NOT NULL UNIQUE CHECK(length(object_key)=16),
    state TEXT NOT NULL CHECK(state IN
        ('planned','created','identified','sealed','active','pending_disposal','quarantined')),
    volume_id BLOB CHECK(length(volume_id)=8),
    directory_id BLOB CHECK(length(directory_id) IN (8,16)),
    file_id BLOB CHECK(length(file_id) IN (8,16)),
    content_digest BLOB CHECK(length(content_digest)=32),
    bytes INTEGER CHECK(bytes>0),
    CHECK((volume_id IS NULL)=(directory_id IS NULL)
        AND (volume_id IS NULL)=(file_id IS NULL)),
    CHECK(state NOT IN ('identified','sealed','active') OR file_id IS NOT NULL),
    CHECK(state<>'sealed' OR (content_digest IS NOT NULL AND bytes IS NOT NULL)),
    PRIMARY KEY(generation_id,role)
) STRICT, WITHOUT ROWID;

ALTER TABLE vm_instances ADD COLUMN cleanup_route TEXT
    CHECK(cleanup_route IN ('staging','lease','legacy_unknown'))
    CHECK(cleanup_route IS NULL OR state=4);
-- The first disk claim is the originating clone: its persistent charge stays
-- until instance deletion. An old claim taken before the deadline also proves
-- staging cleanup. Absent origin evidence, an older claim may be ambiguous.
WITH origin AS (
    SELECT v.id,v.expires_ms,v.cleanup_id,a.started_ms,
        coalesce(o.status,c.status) AS clone_status
    FROM vm_instances v
    LEFT JOIN activity a ON a.id=v.cleanup_id
    LEFT JOIN vm_disk_claims d ON d.activity_id=(SELECT min(activity_id) FROM vm_disk_claims WHERE vm=v.id)
    LEFT JOIN vm_operations o ON o.activity_id=d.activity_id AND o.action=1
    LEFT JOIN activity c ON c.id=d.activity_id AND c.action=1
    WHERE v.state=4
)
UPDATE vm_instances SET cleanup_route=CASE
    WHEN origin.cleanup_id IS NULL OR origin.started_ms<origin.expires_ms
        OR origin.clone_status IN (0,2,3) THEN 'staging'
    WHEN origin.clone_status=1 THEN 'lease'
    ELSE 'legacy_unknown' END
    FROM origin WHERE vm_instances.id=origin.id;
CREATE TRIGGER vm_cleanup_route AFTER UPDATE OF state ON vm_instances
    WHEN NEW.state=4 AND OLD.state<>4 AND NEW.cleanup_route IS NULL
    BEGIN
        UPDATE vm_instances SET cleanup_route=CASE WHEN OLD.state=0 THEN 'staging' ELSE 'lease' END
            WHERE id=NEW.id;
    END;
";

// Caller receipts keep their own cleanup proof because audit rows are pruned.
// Copy still-retained terminal agent proof before recovery can prune more.
// NULL means the proof is unavailable, and the receipt omits cleanup.
const MIGRATE_V7: &str = "
ALTER TABLE vm_operations ADD COLUMN cleanup_confirmed INTEGER
    CHECK(cleanup_confirmed IN (0,1))
    CHECK(cleanup_confirmed IS NULL OR status BETWEEN 1 AND 3);
UPDATE vm_operations AS o SET cleanup_confirmed=a.cleanup_confirmed
    FROM activity AS a
    WHERE a.id=o.activity_id AND a.actor='agent' AND a.principal=o.principal
        AND a.vm IS o.vm AND a.action=o.action AND a.status=o.status
        AND o.status BETWEEN 1 AND 3 AND a.cleanup_confirmed IS NOT NULL;
";

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
        ensure!(
            matches!(version, 0 | 5 | 6 | VM_SCHEMA_VERSION),
            "unsupported VM state version"
        );
        let replace = matches!(version, 0 | 5);
        let upgrade = version != VM_SCHEMA_VERSION;
        db.execute_batch(
            "PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA temp_store=MEMORY;
            PRAGMA foreign_keys=ON; PRAGMA max_page_count=1024;",
        )?;
        // SQLite's table replacement procedure suspends FK enforcement on this
        // connection so the original activity name can retain its dependents.
        // Validate every FK before publishing the replacement and restore
        // enforcement before handing the connection to any caller. The v7
        // column addition replaces no table and keeps enforcement on.
        if replace {
            db.pragma_update(None, "foreign_keys", false)?;
        }
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure!(
            tx.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))? == version,
            "VM state version changed"
        );
        if version == 0 {
            ensure!(
                count(&tx, "SELECT count(*) FROM sqlite_schema", [])? == 0,
                "VM state must start with an empty database"
            );
            tx.execute_batch(SCHEMA_V5)?;
        }
        if replace {
            tx.execute_batch(MIGRATE_V6)?;
        }
        if upgrade {
            tx.execute_batch(MIGRATE_V7)?;
        }
        recover(&tx, now)?;
        check_size(&tx)?;
        if upgrade {
            require_headroom(&tx)?;
            ensure!(
                count(
                    &tx,
                    "SELECT NOT EXISTS(SELECT 1 FROM pragma_foreign_key_check)",
                    []
                )? == 1,
                "invalid VM state foreign keys"
            );
            tx.pragma_update(None, "user_version", VM_SCHEMA_VERSION)?;
        }
        tx.commit()?;
        if replace {
            db.pragma_update(None, "foreign_keys", true)?;
        }
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
    refusal_for(db, principal, Some(vm), action as i64, now, reason)
}

pub(crate) fn refusal_for(
    db: &Connection,
    principal: &str,
    vm: Option<Id>,
    action: i64,
    now: i64,
    reason: Refusal,
) -> Result<()> {
    if headroom(db)? {
        validate_principal(principal)?;
        db.execute(
            "INSERT INTO activity(principal,vm,action,started_ms,status,reason,finished_ms,actor,terminal_order)
             VALUES(?1,?2,?3,?4,4,?5,?4,'agent',
               (SELECT coalesce(max(terminal_order),0)+1 FROM activity))",
            params![principal, vm, action, now, reason as i64],
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
    finish_with_script(db, id, status, now, cleanup, None)
}

fn finish_with_script(
    db: &Connection,
    id: i64,
    status: Status,
    now: i64,
    cleanup: Option<bool>,
    script: Option<ScriptResult>,
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
    let (result, code) = match script {
        Some(ScriptResult::Exited(code)) => (Some("exited"), Some(code)),
        Some(ScriptResult::Unavailable) => (Some("unavailable"), None),
        Some(ScriptResult::TimedOut) => (Some("timed_out"), None),
        Some(ScriptResult::LaunchFailed) => (Some("launch_failed"), None),
        Some(ScriptResult::Terminated) => (Some("terminated"), None),
        None => (None, None),
    };
    db.execute(
        "UPDATE vm_operations SET status=?2,script_result=CASE
            WHEN action=2 AND binding_kind='vm' AND ?2=3 THEN 'interrupted'
            ELSE coalesce(?3,script_result) END,script_exit_code=?4,
            cleanup_confirmed=?5
         WHERE activity_id=?1",
        params![id, status.code(), result, code, cleanup],
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
    pub subject: Subject<'a>,
    pub action: i64,
}

pub(crate) struct Replay {
    pub operation: OperationRef,
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
                "SELECT action,vm,activity_id,status,lease_end_ms,operation_id,binding_kind,
                    subject_id,request_digest,recipe_id,recipe_digest FROM vm_operations
             WHERE principal=?1 AND request_key=?2",
                params![self.principal, self.key.key],
                |row| {
                    let matches = row.get::<_, i64>(0)? == self.action
                        && match (self.subject, row.get::<_, String>(6)?.as_str()) {
                            (Subject::Vm(id), "legacy_vm" | "vm") => {
                                row.get::<_, Option<Id>>(1)? == Some(id)
                            }
                            (Subject::Template(id, digest), "template") => {
                                row.get::<_, Option<Id>>(7)? == Some(id)
                                    && row.get::<_, Option<[u8; 32]>>(8)? == Some(digest)
                            }
                            (Subject::Recipe(id, digest), "recipe") => {
                                row.get::<_, Option<String>>(9)?.as_deref() == Some(id)
                                    && row.get::<_, Option<[u8; 32]>>(10)? == Some(digest)
                            }
                            _ => false,
                        };
                    Ok((
                        matches,
                        Replay {
                            operation: OperationRef {
                                id: row.get(5)?,
                                receipt: Receipt {
                                    activity_id: row.get(2)?,
                                    status: Status::from_code(row.get(3)?)?,
                                },
                            },
                            lease_end: row.get(4)?,
                        },
                    ))
                },
            )
            .optional()?;
        if let Some((matches, prior)) = previous {
            return Ok(if matches {
                Ok(Some(prior))
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

    pub(crate) fn record(
        &self,
        db: &Connection,
        now: i64,
        vm: Option<Id>,
        lease_end: Option<i64>,
    ) -> Result<OperationRef> {
        db.execute(
            "INSERT INTO activity(principal,vm,action,started_ms,actor) VALUES(?1,?2,?3,?4,'agent')",
            params![self.principal, vm, self.action, now],
        )?;
        let activity_id = db.last_insert_rowid();
        let (kind, subject, request, recipe, digest) = match self.subject {
            Subject::Vm(id) => {
                // The existing public clone API still binds its supplied VM ID.
                let kind = if self.action == Action::Clone as i64 {
                    "legacy_vm"
                } else {
                    "vm"
                };
                (kind, (kind == "vm").then_some(id), None, None, None)
            }
            Subject::Template(id, digest) => ("template", Some(id), Some(digest), None, None),
            Subject::Recipe(id, digest) => ("recipe", None, None, Some(id), Some(digest)),
        };
        let id = db.query_row(
            "INSERT INTO vm_operations(principal,request_key,vm,action,activity_id,valid_through_ms,
                lease_end_ms,binding_kind,subject_id,request_digest,recipe_id,recipe_digest,script_result)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,CASE WHEN ?4=2 THEN 'legacy' END)
             RETURNING operation_id",
            params![self.principal, self.key.key, vm, self.action, activity_id, self.valid_through(),
                lease_end, kind, subject, request, recipe, digest],
            |row| row.get(0),
        )?;
        Ok(OperationRef {
            id,
            receipt: Receipt {
                activity_id,
                status: Status::Pending,
            },
        })
    }
}

#[allow(dead_code, reason = "foreground MCP projection is wired in 4d")]
pub(crate) fn operation_status(
    db: &Connection,
    principal: &str,
    operation: Id,
    now: i64,
) -> Result<Option<OperationStatus>> {
    let now = retry_time(db, now)?;
    Ok(db
        .query_row(
            // Only immutable ledger evidence names a template. A legacy clone
            // bound to a reusable VM ID has none, so its receipt omits it.
            "SELECT o.action,o.status,o.vm,o.recipe_id,
                CASE WHEN o.action=1 AND o.binding_kind='template' THEN o.subject_id
                    WHEN o.action=11 AND o.status=1 THEN (SELECT b.template_id FROM vm_build_intents b
                        WHERE b.state='sealed' AND b.recipe_id=o.recipe_id
                            AND b.recipe_digest=o.recipe_digest) END,
                o.lease_end_ms,o.script_result,o.script_exit_code,o.cleanup_confirmed
         FROM vm_operations o
         WHERE o.principal=?1 AND o.operation_id=?2 AND (o.status=0 OR o.valid_through_ms>=?3)",
            params![principal, operation, now],
            |row| {
                let action: i64 = row.get(0)?;
                let state = Status::from_code(row.get(1)?)?;
                let (action, stage) = match action {
                    1 => ("create", Some("cloning")),
                    2 => ("run_script", Some("executing")),
                    3 => ("start", Some("booting")),
                    4 => ("stop", Some("stopping")),
                    5 => ("snapshot", Some("queued")),
                    6 => ("restore", Some("queued")),
                    8 => ("extend", None),
                    11 => ("template_build", Some("staging")),
                    _ => return Err(rusqlite::Error::InvalidQuery),
                };
                let script = match row.get::<_, Option<String>>(6)?.as_deref() {
                    Some("exited") => Some(ScriptResult::Exited(row.get(7)?)),
                    Some("unavailable") => Some(ScriptResult::Unavailable),
                    Some("timed_out") => Some(ScriptResult::TimedOut),
                    Some("launch_failed") => Some(ScriptResult::LaunchFailed),
                    Some("terminated") => Some(ScriptResult::Terminated),
                    _ => None,
                };
                // Only the proof persisted with the terminal ledger update counts.
                let cleanup = row
                    .get::<_, Option<bool>>(8)?
                    .map(|confirmed| if confirmed { "confirmed" } else { "unconfirmed" });
                Ok(OperationStatus {
                    id: operation,
                    action,
                    state,
                    stage: stage.filter(|_| state == Status::Pending),
                    cleanup,
                    vm: row.get(2)?,
                    recipe: row.get(3)?,
                    template: if matches!(action, "create" | "template_build") {
                        row.get(4)?
                    } else {
                        None
                    },
                    expires_at_ms: row.get(5)?,
                    script,
                })
            },
        )
        .optional()?)
}

#[allow(dead_code, reason = "foreground MCP projection is wired in 4d")]
pub(crate) fn vm_status_rows(
    db: &Connection,
    principal: &str,
    exact: Option<Id>,
    after: Option<Id>,
    limit: u32,
    now: i64,
) -> Result<Vec<VmStatus>> {
    let mut stmt = db.prepare(
        "SELECT v.id,v.template,t.guest,v.state,v.expires_ms,a.cleanup_confirmed
         FROM vm_instances v JOIN vm_templates t ON t.id=v.template
         LEFT JOIN activity a ON a.id=v.cleanup_id
         WHERE v.principal=?1 AND (?2 IS NULL OR v.id=?2) AND (?3 IS NULL OR v.id>?3)
         ORDER BY v.id LIMIT ?4",
    )?;
    let items = stmt.query_map(params![principal, exact, after, limit], |row| {
        let expires: i64 = row.get(4)?;
        let state = match (row.get::<_, i64>(3)?, expires <= now) {
            (0..=4, true) | (4, _) => "teardown",
            (0, _) => "creating",
            (1, _) => "stopped",
            (2, _) => "running",
            (3, _) => "recovering",
            _ => return Err(rusqlite::Error::InvalidQuery),
        };
        let cleanup = match (row.get::<_, Option<bool>>(5)?, state) {
            (Some(false), _) => Some("unconfirmed"),
            (_, "teardown" | "recovering") => Some("pending"),
            _ => None,
        };
        Ok(VmStatus {
            id: row.get(0)?,
            template: Some(row.get(1)?),
            guest: Some(match row.get::<_, i64>(2)? {
                1 => Guest::UbuntuX86_64,
                2 => Guest::WindowsX86_64,
                _ => return Err(rusqlite::Error::InvalidQuery),
            }),
            state,
            ready: false,
            expires_at_ms: Some(expires),
            expired_at_ms: None,
            cleanup,
            snapshots: [],
        })
    })?;
    Ok(items.collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn disk_bytes(db: &Connection) -> Result<Option<u64>> {
    let mut stmt = db.prepare(
        "SELECT bytes FROM vm_templates
        UNION ALL SELECT persistent FROM vm_disk_claims
        UNION ALL SELECT staging FROM vm_disk_claims
        UNION ALL SELECT staging_bytes FROM vm_build_intents",
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

pub(crate) fn new_id() -> Result<Id> {
    let mut id = [0; 16];
    getrandom::fill(&mut id).map_err(|_| anyhow::anyhow!("VM identity unavailable"))?;
    Ok(id)
}

fn new_generation(db: &Connection, build: Option<Id>, vm: Option<Id>, ordinal: i64) -> Result<Id> {
    let id = new_id()?;
    db.execute(
        "INSERT INTO vm_generations(id,build_id,vm_id,generation) VALUES(?1,?2,?3,?4)",
        params![id, build, vm, ordinal],
    )?;
    Ok(id)
}

fn plan(db: &Connection, generation: Id, role: &'static str) -> Result<ArtifactSlot> {
    let key = new_id()?;
    db.execute(
        "INSERT INTO vm_artifacts(generation_id,role,object_key,state) VALUES(?1,?2,?3,'planned')",
        params![generation, role, key],
    )?;
    Ok(ArtifactSlot {
        generation,
        role,
        key,
    })
}

fn plan_pair(db: &Connection, generation: Id, disk_role: &'static str) -> Result<PairSlots> {
    Ok(PairSlots {
        disk: plan(db, generation, disk_role)?,
        vars: plan(db, generation, "vars")?,
    })
}

pub(crate) fn reserve_build(
    db: &Connection,
    request: BuildRequest<'_>,
    operation: OperationRef,
    peak: u64,
) -> Result<BuildSlots> {
    let id = new_id()?;
    ensure!(
        db.execute(
            "INSERT INTO vm_build_intents(id,operation_id,recipe_id,recipe_digest,staging_bytes,state)
             SELECT ?1,operation_id,recipe_id,recipe_digest,?3,'pending' FROM vm_operations
             WHERE operation_id=?2 AND principal=?4 AND request_key=?5 AND action=11 AND status=0
               AND activity_id=?6 AND recipe_id=?7 AND recipe_digest=?8",
            params![id, operation.id, peak, request.principal, request.key.key,
                operation.receipt.activity_id, request.recipe, request.digest],
        )? == 1,
        "invalid build reservation binding"
    );
    let golden = new_generation(db, Some(id), None, 1)?;
    let verification = new_generation(db, Some(id), None, 2)?;
    Ok(BuildSlots {
        id,
        pair: plan_pair(db, golden, "disk")?,
        seed: plan(db, golden, "seed")?,
        verification: plan_pair(db, verification, "verification_overlay")?,
    })
}

pub(crate) fn plan_clone(db: &Connection, vm: Id) -> Result<PairSlots> {
    plan_pair(db, new_generation(db, None, Some(vm), 1)?, "disk")
}

pub(crate) fn created(db: &Connection, slot: ArtifactSlot) -> Result<()> {
    ensure!(
        db.execute(
            "UPDATE vm_artifacts SET state='created'
             WHERE generation_id=?1 AND role=?2 AND object_key=?3 AND state IN ('planned','created')",
            params![slot.generation, slot.role, slot.key],
        )? == 1,
        "invalid created artifact"
    );
    Ok(())
}

pub(crate) fn identify(
    db: &Connection,
    slot: ArtifactSlot,
    evidence: ArtifactEvidence,
) -> Result<()> {
    let volume = evidence.identity.volume.to_be_bytes();
    ensure!(
        db.execute(
            "UPDATE vm_artifacts SET state='identified',volume_id=?4,directory_id=?5,file_id=?6,
                content_digest=?7,bytes=?8
             WHERE generation_id=?1 AND role=?2 AND object_key=?3
               AND (state='created' OR (state='identified' AND volume_id=?4 AND directory_id=?5
                    AND file_id=?6 AND content_digest=?7 AND bytes=?8))",
            params![
                slot.generation,
                slot.role,
                slot.key,
                volume,
                evidence.directory,
                evidence.identity.file,
                evidence.pin.sha256,
                evidence.pin.size
            ],
        )? == 1,
        "invalid artifact identity"
    );
    Ok(())
}

pub(crate) fn sealed_pair(
    db: &Connection,
    recipe: &str,
    digest: [u8; 32],
) -> Result<Option<(Id, PairSlots)>> {
    Ok(db.query_row(
        "SELECT t.id,g.id,d.object_key,v.object_key FROM vm_templates t
         JOIN vm_build_intents b ON b.template_id=t.id
         JOIN vm_generations g ON g.build_id=b.id AND g.active=1
         JOIN vm_artifacts d ON d.generation_id=g.id AND d.role='disk' AND d.state='sealed'
         JOIN vm_artifacts v ON v.generation_id=g.id AND v.role='vars' AND v.state='sealed'
         WHERE b.state='sealed' AND b.recipe_id=?1 AND b.recipe_digest=?2
           AND t.recipe_digest=?2 AND t.disk_digest=d.content_digest AND t.vars_digest=v.content_digest",
        params![recipe, digest],
        |row| {
            let generation = row.get(1)?;
            Ok((row.get(0)?, PairSlots {
                disk: ArtifactSlot { generation, role: "disk", key: row.get(2)? },
                vars: ArtifactSlot { generation, role: "vars", key: row.get(3)? },
            }))
        },
    ).optional()?)
}

pub(crate) fn evidence_matches(
    db: &Connection,
    slot: ArtifactSlot,
    proof: ArtifactEvidence,
    state: &str,
) -> Result<bool> {
    Ok(count(db,
        "SELECT EXISTS(SELECT 1 FROM vm_artifacts WHERE generation_id=?1 AND role=?2 AND object_key=?3
         AND volume_id=?4 AND directory_id=?5 AND file_id=?6 AND content_digest=?7 AND bytes=?8 AND state=?9)",
        params![slot.generation, slot.role, slot.key, proof.identity.volume.to_be_bytes(),
            proof.directory, proof.identity.file, proof.pin.sha256, proof.pin.size, state],
    )? != 0)
}

pub(crate) fn publish_build(
    db: &Connection,
    activity_id: i64,
    slots: BuildSlots,
    request: BuildRequest<'_>,
    proof: [ArtifactEvidence; 2],
    now: i64,
) -> Result<()> {
    let [disk, vars] = proof;
    let retained = request
        .retained
        .persistent()
        .ok_or_else(|| anyhow::anyhow!("invalid retained charge"))?;
    let peak = retained
        .checked_add(
            request
                .transient
                .total()
                .ok_or_else(|| anyhow::anyhow!("invalid transient charge"))?,
        )
        .ok_or_else(|| anyhow::anyhow!("invalid build peak"))?;
    ensure!(
        slots.pair.disk.role == "disk"
            && slots.pair.vars.role == "vars"
            && slots.pair.disk.generation == slots.pair.vars.generation
            && disk.identity != vars.identity
            && disk
                .pin
                .size
                .checked_add(vars.pin.size)
                .is_some_and(|bytes| bytes <= request.retained.images)
            && evidence_matches(db, slots.pair.disk, disk, "identified")?
            && evidence_matches(db, slots.pair.vars, vars, "identified")?,
        "invalid sealed artifact evidence"
    );
    let template = slots.id;
    ensure!(db.execute(
        "INSERT INTO vm_templates
         SELECT ?1,?2,b.recipe_digest,?3,?4,?5 FROM vm_build_intents b
         JOIN vm_operations o USING(operation_id)
         JOIN vm_generations g ON g.build_id=b.id AND g.generation=1
         WHERE b.id=?6 AND g.id=?7 AND b.state='pending' AND b.staging_bytes=?13
           AND o.activity_id=?8 AND o.action=11 AND o.status=0 AND b.recipe_id=?9 AND b.recipe_digest=?10
           AND o.recipe_id=b.recipe_id AND o.recipe_digest=b.recipe_digest
           AND o.principal=?11 AND o.request_key=?12",
        params![template, Guest::UbuntuX86_64 as i64, disk.pin.sha256, vars.pin.sha256,
            retained, slots.id, slots.pair.disk.generation, activity_id, request.recipe,
            request.digest, request.principal, request.key.key, peak],
    )? == 1, "invalid build publication binding");
    // The caller has proved release and removed the transient files.
    ensure!(db.execute(
        "DELETE FROM vm_artifacts WHERE generation_id=?1 AND role='seed' AND object_key=?2 AND state='identified'",
        params![slots.pair.disk.generation, slots.seed.key],
    )? == 1, "invalid released seed");
    ensure!(
        count(
            db,
            "SELECT count(*) FROM vm_generations g JOIN vm_artifacts a ON a.generation_id=g.id
         WHERE g.id=?1 AND g.build_id=?2 AND g.generation=2 AND g.active=0
           AND a.object_key IN (?3,?4) AND a.state='identified'",
            params![
                slots.verification.disk.generation,
                slots.id,
                slots.verification.disk.key,
                slots.verification.vars.key
            ],
        )? == 2,
        "invalid released verification pair"
    );
    forget_generation(db, slots.verification.disk.generation)?;
    activate(db, slots.pair, "sealed")?;
    db.execute(
        "UPDATE vm_build_intents SET state='sealed',template_id=?2,staging_bytes=0,health_verified=1 WHERE id=?1",
        params![slots.id, template],
    )?;
    finish(db, activity_id, Status::Succeeded, now, Some(true))?;
    require_headroom(db)
}

fn activate(db: &Connection, pair: PairSlots, state: &str) -> Result<()> {
    ensure!(
        pair.disk.generation == pair.vars.generation
            && db.execute(
                "UPDATE vm_artifacts SET state=?4 WHERE generation_id=?1 AND state='identified'
                 AND object_key IN (?2,?3)",
                params![pair.disk.generation, pair.disk.key, pair.vars.key, state],
            )? == 2,
        "invalid generation pair"
    );
    ensure!(
        db.execute(
            "UPDATE vm_generations SET active=1 WHERE id=?1 AND active=0",
            [pair.disk.generation],
        )? == 1,
        "invalid generation activation"
    );
    Ok(())
}

fn forget_generation(db: &Connection, generation: Id) -> Result<()> {
    db.execute(
        "DELETE FROM vm_artifacts WHERE generation_id=?1",
        [generation],
    )?;
    db.execute("DELETE FROM vm_generations WHERE id=?1", [generation])?;
    Ok(())
}

pub(crate) fn cancel_untouched_build(
    db: &Connection,
    slots: BuildSlots,
    activity_id: i64,
    now: i64,
) -> Result<()> {
    ensure!(
        count(
            db,
            "SELECT count(*) FROM vm_build_intents b JOIN vm_operations o USING(operation_id)
         WHERE b.id=?1 AND b.state='pending' AND o.activity_id=?2
           AND NOT EXISTS(SELECT 1 FROM vm_artifacts a JOIN vm_generations g ON g.id=a.generation_id
               WHERE g.build_id=b.id AND a.state<>'planned')",
            params![slots.id, activity_id],
        )? == 1,
        "invalid untouched build"
    );
    let mut stmt = db.prepare("SELECT id FROM vm_generations WHERE build_id=?1")?;
    for generation in stmt
        .query_map([slots.id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?
    {
        forget_generation(db, generation)?;
    }
    db.execute("DELETE FROM vm_build_intents WHERE id=?1", [slots.id])?;
    finish(db, activity_id, Status::Failed, now, Some(true))
}

pub(crate) fn abandon_build(db: &Connection, id: Id, activity_id: i64, now: i64) -> Result<()> {
    ensure!(count(db,
        "SELECT count(*) FROM vm_build_intents WHERE id=?1 AND state='pending'
         AND operation_id=(SELECT operation_id FROM vm_operations WHERE activity_id=?2 AND status=0)",
        params![id, activity_id],
    )? == 1, "invalid interrupted build");
    let mut stmt = db.prepare("SELECT id FROM vm_generations WHERE build_id=?1")?;
    for generation in stmt.query_map([id], |row| row.get(0))? {
        abandon_inactive(db, Some(generation?))?;
    }
    classify_abandoned_builds(db, Some(id))?;
    finish(db, activity_id, Status::Interrupted, now, Some(false))
}

pub(crate) fn settle_clone(
    db: &Connection,
    activity_id: i64,
    pair: PairSlots,
    proof: Option<[ArtifactEvidence; 2]>,
    now: i64,
) -> Result<Status> {
    ensure!(count(db,
        "SELECT count(*) FROM vm_operations o JOIN vm_instances v ON v.id=o.vm
         JOIN vm_generations g ON g.vm_id=v.id
         WHERE o.activity_id=?1 AND o.action=1 AND o.status=0 AND o.binding_kind='template'
           AND o.subject_id=v.template AND o.principal=v.principal AND g.id=?2 AND g.active=0 AND v.state=0",
        params![activity_id, pair.disk.generation],
    )? == 1, "invalid clone settlement binding");
    let status = if let Some([disk, vars]) = proof {
        ensure!(disk.identity != vars.identity, "shared clone artifact");
        for (slot, evidence) in [(pair.disk, disk), (pair.vars, vars)] {
            created(db, slot)?;
            identify(db, slot, evidence)?;
        }
        activate(db, pair, "active")?;
        settle_operation(db, activity_id, now, Completion::Succeeded)?
    } else {
        abandon_inactive(db, Some(pair.disk.generation))?;
        settle_operation(db, activity_id, now, Completion::CleanupUnconfirmed)?
    };
    require_headroom(db)?;
    Ok(status)
}

fn abandon_inactive(db: &Connection, generation: Option<Id>) -> Result<()> {
    // Identity is custody evidence, never evidence that a process exited.
    // Keep every charge and never publish an incomplete generation.
    db.execute(
        "UPDATE vm_artifacts SET state=CASE WHEN state='identified' THEN 'pending_disposal' ELSE 'quarantined' END
         WHERE state IN ('planned','created','identified') AND generation_id IN
            (SELECT id FROM vm_generations WHERE active=0 AND (?1 IS NULL OR id=?1))",
        [generation],
    )?;
    Ok(())
}

/// Preserve all charges on uncertainty, including on restart.
pub(crate) fn recover(db: &Connection, now: i64) -> Result<()> {
    purge_tombstones(db, now)?;
    abandon_inactive(db, None)?;
    classify_abandoned_builds(db, None)?;
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
    let mut stmt = db.prepare(
        "SELECT id FROM activity WHERE status=0 AND (action<=6 OR action=11) ORDER BY id LIMIT ?1",
    )?;
    let pending = stmt
        .query_map([MAX_CALLER_PENDING], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for id in pending {
        finish(db, id, Status::Interrupted, now, Some(false))?;
    }
    Ok(())
}

fn classify_abandoned_builds(db: &Connection, id: Option<Id>) -> Result<()> {
    db.execute(
        "UPDATE vm_build_intents SET state=CASE WHEN EXISTS(
            SELECT 1 FROM vm_artifacts a JOIN vm_generations g ON g.id=a.generation_id
            WHERE g.build_id=vm_build_intents.id AND a.state='quarantined')
         THEN 'quarantined' ELSE 'pending_disposal' END
         WHERE state='pending' AND (?1 IS NULL OR id=?1)",
        [id],
    )?;
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
    settle_with_script(db, id, now, completion, None)
}

pub(crate) fn settle_with_script(
    db: &Connection,
    id: i64,
    now: i64,
    completion: Completion,
    script: Option<ScriptResult>,
) -> Result<Status> {
    let (vm, action, state): (Id, i64, i64) = db.query_row(
        "SELECT a.vm,a.action,v.state FROM activity a JOIN vm_instances v ON a.vm=v.id
         WHERE a.id=?1 AND a.status=0 AND a.action<=6",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    ensure!(
        script.is_none() || action == Action::Script as i64,
        "invalid script worker"
    );
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
    finish_with_script(db, id, status, now, Some(true), script)?;
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
        "SELECT id,cleanup_id FROM vm_instances v WHERE state<>4 AND expires_ms<=?1
        AND (state<>0 OR NOT EXISTS(SELECT 1 FROM vm_generations WHERE vm_id=v.id))
        ORDER BY expires_ms,id LIMIT 8",
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
                    "UPDATE vm_instances SET state=4,cleanup_route='lease',cleanup_id=NULL WHERE id=?1",
                    [vm],
                )?;
                finish(db, previous, Status::Interrupted, now, Some(false))?;
            }
            let activity_id = activity(db, "arxd", vm, Action::Expire, now)?;
            db.execute(
                "UPDATE vm_instances SET state=4,cleanup_route='lease',cleanup_id=?2 WHERE id=?1",
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
    forget_generations(db, claim.vm, false)?;
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
        "SELECT id,cleanup_id FROM vm_instances WHERE cleanup_route='lease' AND cleanup_id IS NOT NULL
         ORDER BY id LIMIT 8",
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

pub(crate) fn claim_staging(db: &Connection, now: i64) -> Result<()> {
    let mut stmt = db.prepare(
        "SELECT v.id,v.cleanup_id,coalesce(a.status=0,0)
         FROM vm_instances v LEFT JOIN activity a ON a.id=v.cleanup_id
         WHERE v.cleanup_route='staging' AND (v.cleanup_id IS NULL OR a.action=7)
         ORDER BY v.id LIMIT 8",
    )?;
    let staging = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, Id>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, bool>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (vm, old_expiry, pending) in staging {
        let id = activity(db, "arxd", vm, Action::Recover, now)?;
        db.execute(
            "UPDATE vm_instances SET cleanup_id=?2 WHERE id=?1",
            params![vm, id],
        )?;
        if let Some(old) = old_expiry.filter(|_| pending) {
            finish(db, old, Status::Interrupted, now, Some(false))?;
        }
    }
    Ok(())
}

pub(crate) fn cleanup_kinds(db: &Connection) -> Result<Vec<CleanupKind>> {
    let mut builds = db.prepare(
        "SELECT id FROM vm_build_intents WHERE state IN ('pending_disposal','quarantined')
         ORDER BY id LIMIT 4",
    )?;
    let mut items = builds
        .query_map([], |row| Ok(CleanupKind::Build(row.get(0)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut staging = db.prepare(
        "SELECT id,cleanup_id FROM vm_instances WHERE cleanup_route='staging' AND cleanup_id IS NOT NULL
         ORDER BY id LIMIT 8",
    )?;
    items.extend(
        staging
            .query_map([], |row| {
                Ok(CleanupKind::Staging {
                    vm: row.get(0)?,
                    activity_id: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    );
    items.extend(expiry_intents(db)?.into_iter().map(CleanupKind::from));
    items.extend(recovery_intents(db)?.into_iter().map(CleanupKind::from));
    Ok(items)
}

pub(crate) fn legacy_cleanup_uncertain(db: &Connection) -> Result<bool> {
    Ok(count(
        db,
        "SELECT count(*) FROM vm_instances WHERE cleanup_route='legacy_unknown'",
        [],
    )? > 0)
}

pub(crate) fn cleanup_work(db: &Connection, kind: CleanupKind) -> Result<Option<CleanupWork>> {
    let id = kind.subject();
    let build = matches!(kind, CleanupKind::Build(_));
    let retain = matches!(kind, CleanupKind::Recovery { .. });
    let (activity_id, state, route) = match kind {
        CleanupKind::Build(_) => (None, None, None),
        CleanupKind::Staging { activity_id, .. } => (Some(activity_id), Some(4), Some("staging")),
        CleanupKind::Expiry { activity_id, .. } => (Some(activity_id), Some(4), Some("lease")),
        CleanupKind::Recovery { activity_id, .. } => (Some(activity_id), Some(3), None),
    };
    let owned = if build {
        count(
            db,
            "SELECT count(*) FROM vm_build_intents WHERE id=?1 AND state IN
            ('pending_disposal','quarantined') AND template_id IS NULL
            AND NOT EXISTS(SELECT 1 FROM vm_templates WHERE id=?1)",
            [id],
        )?
    } else {
        count(
            db,
            "SELECT count(*) FROM vm_instances WHERE id=?1 AND cleanup_id=?2
            AND state=?3 AND cleanup_route IS ?4",
            params![id, activity_id, state, route],
        )?
    };
    if owned != 1 {
        return Ok(None);
    }
    let mut stmt = db.prepare(
        "SELECT g.id,g.generation,g.active,a.role,a.object_key,a.state,a.volume_id,a.directory_id,a.file_id
         FROM vm_generations g LEFT JOIN vm_artifacts a ON a.generation_id=g.id
         WHERE (?2 AND g.build_id=?1) OR (NOT ?2 AND g.vm_id=?1)
         ORDER BY g.generation,a.role LIMIT 6",
    )?;
    let mut rows = stmt.query(params![id, build])?;
    let mut layout = Vec::new();
    let mut artifacts = Vec::new();
    let mut complete = true;
    while let Some(row) = rows.next()? {
        let ordinal: i64 = row.get(1)?;
        let active: bool = row.get(2)?;
        let (Some(role), Some(state)): (Option<String>, Option<String>) =
            (row.get(3)?, row.get(5)?)
        else {
            complete = false;
            break;
        };
        let directory = row.get::<_, Option<Vec<u8>>>(7)?.and_then(identity_bytes);
        let file = row.get::<_, Option<Vec<u8>>>(8)?.and_then(identity_bytes);
        let volume = row.get::<_, Option<[u8; 8]>>(6)?.map(u64::from_be_bytes);
        layout.push((ordinal, active, role.clone(), state));
        artifacts.push(CleanupArtifact {
            generation: row.get(0)?,
            role,
            key: row.get(4)?,
            directory,
            identity: volume
                .zip(file)
                .map(|(volume, file)| FileIdentity { volume, file }),
            retain: active && retain,
        });
    }
    let expected: &[(i64, &str)] = if build {
        &[
            (1, "disk"),
            (1, "seed"),
            (1, "vars"),
            (2, "vars"),
            (2, "verification_overlay"),
        ]
    } else {
        &[(1, "disk"), (1, "vars")]
    };
    let active = !build && route != Some("staging");
    let accounted = complete
        && layout.len() == expected.len()
        && layout
            .iter()
            .zip(expected)
            .all(|((ordinal, live, role, state), expected)| {
                (*ordinal, role.as_str()) == *expected
                    && *live == active
                    && if active {
                        state == "active"
                    } else {
                        matches!(state.as_str(), "pending_disposal" | "quarantined")
                    }
            })
        && (build
            || count(
                db,
                "SELECT count(*) FROM vm_disk_claims WHERE vm=?1 AND snapshot IS NULL
            AND persistent>0 AND (?2 OR staging=0)",
                params![id, !active],
            )? == 1
                && count(db, "SELECT count(*) FROM vm_disk_claims WHERE vm=?1", [id])? == 1
                && count(db, "SELECT count(*) FROM vm_snapshots WHERE vm=?1", [id])? == 0);
    let pending_worker = if build {
        None
    } else {
        db.query_row(
            "SELECT id FROM activity WHERE vm=?1 AND status=0 AND action<=6",
            [id],
            |row| row.get(0),
        )
        .optional()?
    };
    Ok(Some(CleanupWork {
        kind,
        pending_worker,
        artifacts,
        accounted,
    }))
}

fn identity_bytes(bytes: Vec<u8>) -> Option<Id> {
    match bytes.len() {
        16 => bytes.try_into().ok(),
        8 => {
            let mut id = [0; 16];
            id[8..].copy_from_slice(&bytes);
            Some(id)
        }
        _ => None,
    }
}

pub(crate) fn settle_cleanup(
    db: &Connection,
    work: &CleanupWork,
    now: i64,
    confirmed: bool,
) -> Result<bool> {
    let Some(current) = cleanup_work(db, work.kind)? else {
        return Ok(true);
    };
    let confirmed = confirmed && current.accounted && current.artifacts == work.artifacts;
    match work.kind {
        CleanupKind::Build(id) if confirmed => {
            forget_generations(db, id, true)?;
            db.execute("DELETE FROM vm_build_intents WHERE id=?1", [id])?;
        }
        CleanupKind::Build(id) => {
            db.execute(
                "UPDATE vm_build_intents SET state='quarantined' WHERE id=?1",
                [id],
            )?;
        }
        CleanupKind::Staging { vm, activity_id } if confirmed => {
            interrupt_pending(db, vm, now, true)?;
            forget_generations(db, vm, false)?;
            db.execute("DELETE FROM vm_instances WHERE id=?1", [vm])?;
            finish(db, activity_id, Status::Succeeded, now, Some(true))?;
        }
        CleanupKind::Expiry { vm, activity_id } if confirmed => {
            interrupt_pending(db, vm, now, true)?;
            record_expired(db, ExpiryClaim { vm, activity_id }, now)?;
        }
        CleanupKind::Recovery { vm, activity_id } if confirmed => {
            interrupt_pending(db, vm, now, true)?;
            record_recovered(db, RecoveryClaim { vm, activity_id }, now)?;
        }
        CleanupKind::Staging { activity_id, .. }
        | CleanupKind::Expiry { activity_id, .. }
        | CleanupKind::Recovery { activity_id, .. } => {
            db.execute(
                "UPDATE activity SET cleanup_confirmed=0 WHERE id=?1",
                [activity_id],
            )?;
        }
    }
    Ok(confirmed)
}

pub(crate) fn interrupt_pending(db: &Connection, vm: Id, now: i64, released: bool) -> Result<()> {
    let pending = db
        .query_row(
            "SELECT id FROM activity WHERE vm=?1 AND status=0 AND action<=6",
            [vm],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(id) = pending {
        finish(db, id, Status::Interrupted, now, Some(released))?;
    }
    Ok(())
}

fn forget_generations(db: &Connection, id: Id, build: bool) -> Result<()> {
    let mut stmt = db.prepare(
        "SELECT id FROM vm_generations
        WHERE (?2 AND build_id=?1) OR (NOT ?2 AND vm_id=?1)",
    )?;
    let generations = stmt
        .query_map(params![id, build], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for generation in generations {
        forget_generation(db, generation)?;
    }
    Ok(())
}

pub(crate) fn record_recovered(db: &Connection, claim: RecoveryClaim, now: i64) -> Result<()> {
    let vm = claim.vm;
    ensure!(db.execute(
        "UPDATE vm_instances SET state=1,cleanup_id=NULL WHERE id=?1 AND state=3 AND cleanup_id=?2",
        params![vm, claim.activity_id],
    )? == 1, "invalid recovery intent");
    db.execute("DELETE FROM vm_snapshots WHERE vm=?1 AND ready=0", [vm])?;
    db.execute(
        "DELETE FROM vm_disk_claims WHERE vm=?1 AND persistent=0",
        [vm],
    )?;
    db.execute("UPDATE vm_disk_claims SET staging=0 WHERE vm=?1", [vm])?;
    finish(db, claim.activity_id, Status::Succeeded, now, Some(true))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_binding(nonce: u64, vm: Id, action: Action) -> Binding<'static> {
        Binding {
            principal: "alice",
            key: RequestId::parse(&format!("{:016x}{nonce:016x}", 0_u64)).unwrap(),
            subject: Subject::Vm(vm),
            action: action as i64,
        }
    }

    #[test]
    fn legacy_clone_receipt_survives_vm_id_reuse() {
        let dir = private_temp();
        let store = VmStore::open(dir.path(), NOW).unwrap();
        store.transact(|db| {
            assert_eq!(count(db, "PRAGMA user_version", [])?, i64::from(VM_SCHEMA_VERSION));
            let vm = [1; 16];
            let [original_template, replacement_template]: [Id; 2] = [[2; 16], [3; 16]];
            for template in [original_template, replacement_template] {
                db.execute(
                    "INSERT INTO vm_templates VALUES(?1,1,?2,zeroblob(32),zeroblob(32),40)",
                    params![template, [template[0]; 32]],
                )?;
            }
            let create = |nonce, template| -> Result<OperationRef> {
                let operation = legacy_binding(nonce, vm, Action::Clone).record(db, NOW, Some(vm), None)?;
                db.execute(
                    "INSERT INTO vm_instances(id,template,principal,expires_ms) VALUES(?1,?2,'alice',?3)",
                    params![vm, template, NOW],
                )?;
                settle_operation(db, operation.receipt.activity_id, NOW, Completion::Succeeded)?;
                Ok(operation)
            };
            let original = create(1, original_template)?;
            let status = |id| -> Result<OperationStatus> {
                Ok(operation_status(db, "alice", id, NOW)?.unwrap())
            };
            let before = status(original.id)?;
            for id in 1..=MAX_TOMBSTONES + 1 {
                let instance = [id as u8; 16];
                if instance != vm {
                    db.execute(
                        "INSERT INTO vm_instances(id,template,principal,expires_ms,state) VALUES(?1,?2,'alice',?3,1)",
                        params![instance, original_template, NOW],
                    )?;
                }
                let [claim] = expiry_claims(db, NOW)?.try_into().unwrap();
                assert_eq!(claim.vm, instance);
                record_expired(db, claim, NOW)?;
            }
            assert!(expired_status(db, "alice", vm)?.is_none());
            let replacement = create(2, replacement_template)?;
            assert_eq!(status(original.id)?, before);
            assert_eq!(before.template, None);
            let replacement = status(replacement.id)?;
            assert_ne!(replacement.id, original.id);
            assert_eq!(replacement.vm, Some(vm));
            assert_eq!(replacement.template, None);
            assert_eq!(replacement.state, Status::Succeeded);
            Ok(())
        }).unwrap();
    }

    #[test]
    fn interrupted_operation_keeps_cleanup_proof_after_audit_pruning() {
        let dir = private_temp();
        let store = VmStore::open(dir.path(), NOW).unwrap();
        let settled = store
            .transact(|db| {
                let vm = [1; 16];
                let operation =
                    legacy_binding(1, vm, Action::Start).record(db, NOW, Some(vm), None)?;
                finish(
                    db,
                    operation.receipt.activity_id,
                    Status::Interrupted,
                    NOW,
                    Some(true),
                )?;
                let before = operation_status(db, "alice", operation.id, NOW)?.unwrap();
                assert_eq!(before.cleanup, Some("confirmed"));
                for _ in 0..MAX_RECEIPTS {
                    refusal(db, "alice", vm, Action::Start, NOW, Refusal::Busy)?;
                }
                assert_eq!(
                    count(
                        db,
                        "SELECT count(*) FROM activity WHERE id=?1",
                        [operation.receipt.activity_id]
                    )?,
                    0
                );
                assert_eq!(
                    operation_status(db, "alice", operation.id, NOW)?,
                    Some(before)
                );
                Ok(operation)
            })
            .unwrap();
        drop(store);
        VmStore::open(dir.path(), NOW)
            .unwrap()
            .transact(|db| {
                let after = operation_status(db, "alice", settled.id, NOW)?.unwrap();
                assert_eq!(after.state, Status::Interrupted);
                assert_eq!(after.cleanup, Some("confirmed"));
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn reservation_refuses_an_operation_bound_to_another_recipe() {
        let dir = crate::service::application::tests::private_temp();
        let db = VmStore::open(dir.path(), 0).unwrap();
        db.transact(|db| {
            let request = BuildRequest {
                principal: "fixture",
                key: RequestId::parse("00000000000000000000000000000001").unwrap(),
                recipe: "fece98c2030f405fb6402fc92a79935d",
                digest: [9; 32],
                retained: crate::vm::DiskClaim::default(),
                transient: crate::vm::DiskClaim::default(),
            };
            for subject in [
                Subject::Recipe("e03a3012c18d20b8691490b580ab41e9", request.digest),
                Subject::Recipe(request.recipe, [8; 32]),
            ] {
                let binding = Binding {
                    principal: request.principal,
                    key: request.key,
                    subject,
                    action: 11,
                };
                let operation = binding.record(db, 0, None, None)?;
                assert!(reserve_build(db, request, operation, 8).is_err());
                assert_eq!(count(db, "SELECT count(*) FROM vm_build_intents", [])?, 0);
                db.execute("DELETE FROM vm_operations", [])?;
            }
            Ok(())
        })
        .unwrap();
    }
    use crate::service::application::tests::private_temp;

    const NOW: i64 = 100;

    fn fixture_db(dir: &Path) -> Connection {
        let db = Connection::open(dir.join("activity.sqlite3")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                dir.join("activity.sqlite3"),
                fs::Permissions::from_mode(0o600),
            )
            .unwrap();
        }
        db
    }

    /// Exact v6 state as the merged v6 opener wrote it. Proof for 20 and 21
    /// is retained, 22 has none and 23 was already pruned. Twenty is the oldest
    /// of a full audit window, so settling the pending 300 at startup prunes it.
    fn v6_fixture(dir: &Path) -> Connection {
        let db = fixture_db(dir);
        db.execute_batch("PRAGMA foreign_keys=OFF; BEGIN IMMEDIATE;")
            .unwrap();
        db.execute_batch(SCHEMA_V5).unwrap();
        db.execute_batch(MIGRATE_V6).unwrap();
        db.execute_batch(
            "PRAGMA user_version=6; COMMIT; PRAGMA foreign_keys=ON; BEGIN IMMEDIATE;
             INSERT INTO activity(id,principal,actor,vm,action,started_ms,status,reason,
                 finished_ms,terminal_order,cleanup_confirmed) VALUES
                 (20,'alice','agent',unhex(printf('%032x',1)),3,10,3,NULL,11,1,1),
                 (21,'alice','agent',unhex(printf('%032x',2)),3,12,3,NULL,13,2,0),
                 (22,'alice','agent',unhex(printf('%032x',1)),8,14,1,NULL,15,3,NULL),
                 (300,'alice','agent',unhex(printf('%032x',4)),3,20,0,NULL,NULL,NULL,NULL);
             WITH RECURSIVE n(i) AS (SELECT 0 UNION ALL SELECT i+1 FROM n WHERE i<124)
             INSERT INTO activity(id,principal,actor,vm,action,started_ms,status,reason,
                 finished_ms,terminal_order)
                 SELECT 100+i,'alice','agent',unhex(printf('%032x',5)),3,16,4,1,16,4+i FROM n;
             INSERT INTO vm_operations(principal,request_key,vm,action,activity_id,
                 valid_through_ms,status,lease_end_ms) VALUES
                 ('alice',unhex(printf('%032x',20)),unhex(printf('%032x',1)),3,20,86400000,3,NULL),
                 ('alice',unhex(printf('%032x',21)),unhex(printf('%032x',2)),3,21,86400000,3,NULL),
                 ('alice',unhex(printf('%032x',22)),unhex(printf('%032x',1)),8,22,86400000,1,400),
                 ('alice',unhex(printf('%032x',23)),unhex(printf('%032x',3)),3,23,86400000,1,NULL),
                 ('alice',unhex(printf('%032x',300)),unhex(printf('%032x',4)),3,300,86400000,0,NULL);
             COMMIT;",
        )
        .unwrap();
        assert_eq!(
            count(&db, "SELECT count(*) FROM activity WHERE status<>0", []).unwrap(),
            MAX_RECEIPTS
        );
        db
    }

    fn ledger_state(db: &Connection) -> Result<Vec<(i64, Id, i64)>> {
        let mut stmt = db.prepare(
            "SELECT activity_id,operation_id,status FROM vm_operations ORDER BY activity_id",
        )?;
        Ok(stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn receipts(db: &Connection, ids: &[(i64, Id, i64)]) -> Result<Vec<OperationStatus>> {
        ids.iter()
            .map(|(_, id, _)| Ok(operation_status(db, "alice", *id, NOW)?.unwrap()))
            .collect()
    }

    fn proof_column(db: &Connection) -> i64 {
        count(
            db,
            "SELECT count(*) FROM pragma_table_info('vm_operations') WHERE name='cleanup_confirmed'",
            [],
        )
        .unwrap()
    }

    #[test]
    fn v6_upgrade_backfills_cleanup_proof_before_recovery_prunes_it() {
        let dir = private_temp();
        let before = ledger_state(&v6_fixture(dir.path())).unwrap();
        let store = VmStore::open(dir.path(), NOW).unwrap();
        let settled = store
            .transact(|db| {
                assert_eq!(
                    count(db, "PRAGMA user_version", [])?,
                    i64::from(VM_SCHEMA_VERSION)
                );
                assert_eq!(count(db, "PRAGMA foreign_keys", [])?, 1);
                assert_eq!(
                    count(db, "SELECT count(*) FROM pragma_foreign_key_check", [])?,
                    0
                );
                // Startup recovery settled 300 and pruned the oldest proof row.
                assert_eq!(
                    count(db, "SELECT count(*) FROM activity WHERE id=20", [])?,
                    0
                );
                assert_eq!(
                    count(db, "SELECT count(*) FROM activity WHERE id IN (21,22)", [])?,
                    2
                );
                let after = ledger_state(db)?;
                let expected: Vec<_> = before
                    .iter()
                    .map(|&(activity, id, status)| {
                        (activity, id, if activity == 300 { 3 } else { status })
                    })
                    .collect();
                assert_eq!(after, expected);
                let receipts = receipts(db, &after)?;
                let summary: Vec<_> = receipts
                    .iter()
                    .map(|r| (r.action, r.state, r.cleanup))
                    .collect();
                assert_eq!(
                    summary,
                    [
                        ("start", Status::Interrupted, Some("confirmed")),
                        ("start", Status::Interrupted, Some("unconfirmed")),
                        ("extend", Status::Succeeded, None),
                        ("start", Status::Succeeded, None),
                        ("start", Status::Interrupted, Some("unconfirmed")),
                    ]
                );
                assert_eq!(receipts[2].expires_at_ms, Some(400));
                Ok(receipts)
            })
            .unwrap();
        drop(store);

        let reopened = VmStore::open(dir.path(), NOW).unwrap();
        reopened
            .transact(|db| {
                assert_eq!(
                    count(db, "PRAGMA user_version", [])?,
                    i64::from(VM_SCHEMA_VERSION)
                );
                let state = ledger_state(db)?;
                assert_eq!(receipts(db, &state)?, settled);
                // Proof is limited to 0 and 1 on terminal ledger rows.
                assert!(
                    db.execute(
                        "UPDATE vm_operations SET cleanup_confirmed=2 WHERE activity_id=23",
                        []
                    )
                    .is_err()
                );
                let pending = legacy_binding(9, [9; 16], Action::Start).record(
                    db,
                    NOW,
                    Some([9; 16]),
                    None,
                )?;
                assert!(
                    db.execute(
                        "UPDATE vm_operations SET cleanup_confirmed=1 WHERE activity_id=?1",
                        [pending.receipt.activity_id]
                    )
                    .is_err()
                );
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn v6_upgrade_rolls_back_proof_column_on_recovery_error_and_can_retry() {
        let dir = private_temp();
        let db = v6_fixture(dir.path());
        db.execute_batch(
            "CREATE TRIGGER fixture_error BEFORE UPDATE OF status ON activity
             BEGIN SELECT raise(ABORT,'fixture migration fault'); END;",
        )
        .unwrap();
        drop(db);
        let path = dir.path().join("activity.sqlite3");
        let before = fs::read(&path).unwrap();
        let error = VmStore::open(dir.path(), NOW).err().unwrap();
        assert!(error.to_string().contains("fixture migration fault"));
        assert_eq!(fs::read(&path).unwrap(), before);
        let db = open_sqlite(&path).unwrap();
        assert_eq!(count(&db, "PRAGMA user_version", []).unwrap(), 6);
        assert_eq!(proof_column(&db), 0);
        db.execute_batch("DROP TRIGGER fixture_error").unwrap();
        drop(db);
        VmStore::open(dir.path(), NOW)
            .unwrap()
            .transact(|db| {
                assert_eq!(proof_column(db), 1);
                assert_eq!(
                    count(
                        db,
                        "SELECT cleanup_confirmed FROM vm_operations WHERE activity_id=20",
                        []
                    )?,
                    1
                );
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn template_clone_receipt_keeps_its_bound_template_after_vm_id_reuse() {
        let dir = private_temp();
        let store = VmStore::open(dir.path(), NOW).unwrap();
        store.transact(|db| {
            let vm = [1; 16];
            let [bound, other]: [Id; 2] = [[2; 16], [3; 16]];
            for template in [bound, other] {
                db.execute(
                    "INSERT INTO vm_templates VALUES(?1,1,?2,zeroblob(32),zeroblob(32),40)",
                    params![template, [template[0]; 32]],
                )?;
            }
            let operation = Binding {
                principal: "alice",
                key: RequestId::parse("00000000000000000000000000000001").unwrap(),
                subject: Subject::Template(bound, [7; 32]),
                action: Action::Clone as i64,
            }
            .record(db, NOW, Some(vm), None)?;
            db.execute(
                "INSERT INTO vm_instances(id,template,principal,expires_ms) VALUES(?1,?2,'alice',?3)",
                params![vm, bound, NOW + 1],
            )?;
            settle_operation(db, operation.receipt.activity_id, NOW, Completion::Succeeded)?;
            let receipt = operation_status(db, "alice", operation.id, NOW)?.unwrap();
            assert_eq!(receipt.template, Some(bound));
            assert_eq!(receipt.cleanup, Some("confirmed"));
            db.execute("DELETE FROM vm_disk_claims", [])?;
            db.execute("DELETE FROM vm_instances", [])?;
            db.execute(
                "INSERT INTO vm_instances(id,template,principal,expires_ms) VALUES(?1,?2,'alice',?3)",
                params![vm, other, NOW + 1],
            )?;
            assert_eq!(operation_status(db, "alice", operation.id, NOW)?, Some(receipt));
            Ok(())
        }).unwrap();
    }

    fn v5_fixture(dir: &Path) -> Connection {
        let db = fixture_db(dir);
        db.execute_batch("PRAGMA foreign_keys=ON; BEGIN IMMEDIATE;")
            .unwrap();
        db.execute_batch(SCHEMA_V5).unwrap();
        db.execute_batch(
            "INSERT INTO activity(id,principal,actor,vm_owner,vm,action,started_ms,status,
                  finished_ms,terminal_order,cleanup_confirmed) VALUES
                  (10,'alice','agent',NULL,unhex(printf('%032x',1)),1,10,1,11,1,1),
                  (11,'alice','agent',NULL,unhex(printf('%032x',1)),2,21,0,NULL,NULL,NULL),
                  (12,'bob','agent',NULL,unhex(printf('%032x',2)),1,12,2,13,2,1),
                  (13,'arxd','daemon','bob',unhex(printf('%032x',2)),7,15,0,NULL,NULL,NULL),
                  (14,'arxd','daemon','bob',unhex(printf('%032x',3)),7,16,1,17,3,1),
                  (15,'alice','agent',NULL,unhex(printf('%032x',1)),8,17,1,17,4,NULL),
                  (16,'bob','agent',NULL,unhex(printf('%032x',4)),1,18,0,NULL,NULL,NULL),
                  (17,'bob','agent',NULL,unhex(printf('%032x',5)),1,19,1,20,5,1),
                  (18,'arxd','daemon','bob',unhex(printf('%032x',5)),7,99,0,NULL,NULL,NULL),
                  (19,'arxd','daemon','bob',unhex(printf('%032x',6)),7,99,0,NULL,NULL,NULL);
             INSERT INTO vm_operations(principal,request_key,vm,action,activity_id,valid_through_ms,
                 status,lease_end_ms)
                 SELECT principal,unhex(printf('%032x',id)),vm,action,id,86400000+started_ms,status,
                     CASE WHEN action=8 THEN 400 END FROM activity WHERE actor='agent';
             INSERT INTO vm_operations VALUES
                 ('bob',unhex(printf('%032x',11)),unhex(printf('%032x',3)),2,9,86400009,3,NULL);
             INSERT INTO vm_templates VALUES(zeroblob(16),1,zeroblob(32),zeroblob(32),zeroblob(32),40);
             INSERT INTO vm_instances(id,template,principal,expires_ms,state,cleanup_id) VALUES
                 (unhex(printf('%032x',1)),zeroblob(16),'alice',400,2,NULL),
                 (unhex(printf('%032x',2)),zeroblob(16),'bob',200,4,13),
                 (unhex(printf('%032x',4)),zeroblob(16),'bob',300,0,NULL),
                 (unhex(printf('%032x',5)),zeroblob(16),'bob',99,4,18),
                 (unhex(printf('%032x',6)),zeroblob(16),'bob',99,4,19);
             INSERT INTO vm_tombstones VALUES(unhex(printf('%032x',3)),'bob',14,17);
             INSERT INTO vm_snapshots VALUES(unhex(printf('%032x',1)),zeroblob(16),1);
             INSERT INTO vm_disk_claims VALUES
                 (8,unhex(printf('%032x',6)),NULL,17,0),
                 (10,unhex(printf('%032x',1)),NULL,5,0),
                 (12,unhex(printf('%032x',2)),NULL,7,2),
                 (16,unhex(printf('%032x',4)),NULL,11,3),
                 (17,unhex(printf('%032x',5)),NULL,13,0);
             UPDATE sqlite_sequence SET seq=80 WHERE name='activity';
             UPDATE vm_retry_clock SET high_water_ms=90;
             COMMIT;",
        )
        .unwrap();
        db
    }

    fn ledger_ids(db: &Connection) -> Result<Vec<(i64, Id)>> {
        let mut stmt =
            db.prepare("SELECT activity_id,operation_id FROM vm_operations ORDER BY activity_id")?;
        Ok(stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }

    #[test]
    fn v5_upgrade_preserves_rows_and_recovers_with_stable_operation_ids() {
        let dir = private_temp();
        drop(v5_fixture(dir.path()));
        let store = VmStore::open(dir.path(), NOW).unwrap();
        let ids = store.transact(|db| {
            assert_eq!(count(db, "PRAGMA user_version", [])?, i64::from(VM_SCHEMA_VERSION));
            // The v5 and v6 VM openers' gates were exactly 0 | 5 and 0 | 5 | 6.
            assert!(!matches!(count(db, "PRAGMA user_version", [])?, 0 | 5 | 6));
            assert_eq!(count(db, "PRAGMA foreign_keys", [])?, 1);
            assert_eq!(count(db, "SELECT count(*) FROM pragma_foreign_key_check", [])?, 0);
            assert_eq!(count(db, "SELECT seq FROM sqlite_sequence WHERE name='activity'", [])?, 81);
            assert_eq!(count(db, "SELECT high_water_ms FROM vm_retry_clock", [])?, 90);
            assert_eq!(disk_bytes(db)?, Some(98));
            assert_eq!(count(db, "SELECT count(*) FROM vm_snapshots WHERE ready=1", [])?, 1);
            assert_eq!(count(db, "SELECT activity_id FROM vm_tombstones WHERE principal='bob' AND expired_at_ms=17", [])?, 14);
            assert_eq!(count(db, "SELECT count(*) FROM vm_instances WHERE (principal='alice' AND
                 expires_ms=400 AND state=3 AND cleanup_id=81) OR (principal='bob' AND state=4 AND
                 cleanup_route='staging' AND ((expires_ms=200 AND cleanup_id=13) OR
                 (expires_ms=300 AND cleanup_id IS NULL)))", [])?, 3);
            assert_eq!(count(db, "SELECT count(*) FROM vm_instances WHERE expires_ms=99 AND state=4
                 AND ((cleanup_id=18 AND cleanup_route='lease')
                 OR (cleanup_id=19 AND cleanup_route='legacy_unknown'))", [])?, 2);
            assert_eq!(count(db, "SELECT count(*) FROM activity WHERE id IN (11,16) AND
                 status=3 AND finished_ms=?1 AND cleanup_confirmed=0", [NOW])?, 2);
            assert_eq!(count(db, "SELECT count(*) FROM vm_operations WHERE action=2 AND
                 script_result='legacy' AND script_exit_code IS NULL AND status=3", [])?, 2);
            assert_eq!(count(db, "SELECT count(*) FROM vm_operations WHERE binding_kind='legacy_vm'
                 AND subject_id IS NULL AND request_digest IS NULL", [])?, 7);
            assert_eq!(count(db, "SELECT count(*) FROM vm_operations WHERE
                 (activity_id IN (10,12,17) AND cleanup_confirmed=1)
                 OR (activity_id IN (11,16) AND cleanup_confirmed=0)
                 OR (activity_id IN (9,15) AND cleanup_confirmed IS NULL)", [])?, 7);

            let key = RequestId::parse("0000000000000000000000000000000b").unwrap();
            for (principal, vm, id) in [("alice", 1, 11), ("bob", 3, 9)] {
                let mut subject = [0; 16];
                subject[15] = vm;
                let replay = Binding { principal, key, subject: Subject::Vm(subject), action: Action::Script as i64 }
                    .replay(db, NOW, false)?.unwrap().unwrap();
                assert_eq!(replay.operation.receipt, Receipt { activity_id: id, status: Status::Interrupted });
            }
            let mut vm = [0; 16];
            vm[15] = 1;
            let extend = Binding {
                principal: "alice", subject: Subject::Vm(vm), action: Action::Extend as i64,
                key: RequestId::parse("0000000000000000000000000000000f").unwrap(),
            }.replay(db, NOW, false)?.unwrap().unwrap();
            assert_eq!(extend.operation.receipt, Receipt { activity_id: 15, status: Status::Succeeded });
            assert_eq!(extend.lease_end, Some(400));

            assert_eq!(count(db, "SELECT count(DISTINCT operation_id) FROM vm_operations", [])?, 7);
            assert!(headroom(db)?);
            ledger_ids(db)
        }).unwrap();
        assert_eq!(
            ids.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            [9, 10, 11, 12, 15, 16, 17]
        );
        drop(store);

        let reopened = VmStore::open(dir.path(), NOW).unwrap();
        reopened
            .transact(|db| {
                assert_eq!(ledger_ids(db)?, ids);
                assert_eq!(
                    count(
                        db,
                        "SELECT seq FROM sqlite_sequence WHERE name='activity'",
                        []
                    )?,
                    81
                );
                assert_eq!(
                    count(db, "SELECT count(*) FROM activity WHERE id>80", [])?,
                    1
                );
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn v5_upgrade_rolls_back_on_recovery_error_and_can_retry() {
        let dir = private_temp();
        let db = v5_fixture(dir.path());
        db.execute_batch(
            "CREATE TRIGGER fixture_error BEFORE UPDATE OF state ON vm_instances
             BEGIN SELECT raise(ABORT,'fixture migration fault'); END;",
        )
        .unwrap();
        drop(db);
        let path = dir.path().join("activity.sqlite3");
        let before = fs::read(&path).unwrap();
        let error = VmStore::open(dir.path(), NOW).err().unwrap();
        assert!(error.to_string().contains("fixture migration fault"));
        assert_eq!(fs::read(&path).unwrap(), before);
        let db = open_sqlite(&path).unwrap();
        assert_eq!(count(&db, "PRAGMA user_version", []).unwrap(), 5);
        db.execute_batch("DROP TRIGGER fixture_error").unwrap();
        drop(db);
        assert!(VmStore::open(dir.path(), NOW).is_ok());
    }

    #[test]
    fn v5_upgrade_rolls_back_when_state_has_no_headroom() {
        let dir = private_temp();
        let db = v5_fixture(dir.path());
        db.execute_batch(
            "BEGIN IMMEDIATE;
             CREATE TABLE fixture_padding(bytes BLOB);
             INSERT INTO fixture_padding VALUES(zeroblob(3940000));
             COMMIT;",
        )
        .unwrap();
        check_size(&db).unwrap();
        drop(db);
        let path = dir.path().join("activity.sqlite3");
        let before = fs::read(&path).unwrap();
        let error = VmStore::open(dir.path(), NOW).err().unwrap();
        assert!(full(&error), "{error}");
        assert!(fs::read(&path).unwrap() == before);
        let db = open_sqlite(&path).unwrap();
        assert_eq!(count(&db, "PRAGMA user_version", []).unwrap(), 5);
        db.execute_batch("DROP TABLE fixture_padding").unwrap();
        drop(db);
        assert!(VmStore::open(dir.path(), NOW).is_ok());
    }

    #[test]
    fn future_vm_version_leaves_state_unchanged() {
        let dir = private_temp();
        let db = v5_fixture(dir.path());
        db.pragma_update(None, "user_version", VM_SCHEMA_VERSION + 1)
            .unwrap();
        drop(db);
        let path = dir.path().join("activity.sqlite3");
        let before = fs::read(&path).unwrap();
        let error = VmStore::open(dir.path(), NOW).err().unwrap();
        assert_eq!(error.to_string(), "unsupported VM state version");
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn general_and_service_profiles_keep_v4_schema() {
        for open in [
            super::super::Activity::open,
            super::super::Activity::open_service_fixture,
        ] {
            let dir = private_temp();
            drop(open(dir.path()).unwrap());
            let path = dir.path().join("activity.sqlite3");
            let before = fs::read(&path).unwrap();
            assert!(VmStore::open(dir.path(), NOW).is_err());
            assert_eq!(fs::read(&path).unwrap(), before);
            drop(open(dir.path()).unwrap());
            let db = open_sqlite(&path).unwrap();
            assert_eq!(count(&db, "PRAGMA user_version", []).unwrap(), 4);
            assert_eq!(
                count(
                    &db,
                    "SELECT count(*) FROM sqlite_schema WHERE name GLOB 'vm_*'",
                    []
                )
                .unwrap(),
                0
            );
        }
    }

    #[test]
    fn v6_checks_bindings_evidence_and_script_result_in_db_and_journal() {
        let dir = private_temp();
        let path = dir.path().join("activity.sqlite3");
        let journal = dir.path().join("activity.sqlite3-journal");
        let sentinels = [
            "fixture-script-contents-9cc20e7f",
            "fixture-token-23deea7d903170f8",
            "C:/fixture-absolute-path-48fd070a",
            "S-1-5-21-11112222-33334444-55556666-7777",
        ];
        let scan = |path: &Path| {
            let bytes = fs::read(path).unwrap();
            for sentinel in sentinels {
                assert!(
                    !bytes
                        .windows(sentinel.len())
                        .any(|w| w == sentinel.as_bytes())
                );
            }
        };
        let old = v5_fixture(dir.path());
        old.execute_batch("BEGIN IMMEDIATE; UPDATE vm_retry_clock SET high_water_ms=91;")
            .unwrap();
        scan(&path);
        scan(&journal);
        old.execute_batch("ROLLBACK;").unwrap();
        drop(old);
        let store = VmStore::open(dir.path(), NOW).unwrap();
        scan(&path);
        store.transact(|db| {
            for sentinel in sentinels {
                assert!(db.execute("UPDATE vm_operations SET script_result=?1 WHERE activity_id=9", [sentinel]).is_err());
            }
            assert!(db.execute("UPDATE vm_operations SET script_result='exited' WHERE activity_id=9", []).is_err());
            assert!(db.execute("UPDATE vm_operations SET script_result='timed_out',script_exit_code=7 WHERE activity_id=9", []).is_err());
            db.execute("UPDATE vm_operations SET script_result='exited',script_exit_code=7 WHERE activity_id=9", [])?;
            assert!(db.execute("UPDATE vm_operations SET binding_kind='template',subject_id=zeroblob(16) WHERE activity_id=10", []).is_err());
            db.execute("UPDATE vm_operations SET binding_kind='template',subject_id=zeroblob(16),
                request_digest=zeroblob(32) WHERE activity_id=10", [])?;

            db.execute_batch(
                "INSERT INTO activity(principal,actor,action,started_ms) VALUES('alice','agent',11,100);
                 INSERT INTO vm_operations(principal,request_key,operation_id,action,activity_id,
                    valid_through_ms,binding_kind,recipe_id,recipe_digest)
                    VALUES('alice',zeroblob(16),zeroblob(16),11,last_insert_rowid(),86400100,
                        'recipe','ubuntu-fixture',zeroblob(32));
                 INSERT INTO vm_build_intents(id,operation_id,recipe_id,recipe_digest,staging_bytes,state)
                    VALUES(zeroblob(16),zeroblob(16),'ubuntu-fixture',zeroblob(32),100,'pending');
                 INSERT INTO vm_generations(id,build_id,generation) VALUES(zeroblob(16),zeroblob(16),1);
                 INSERT INTO vm_artifacts(generation_id,role,object_key,state)
                    VALUES(zeroblob(16),'disk',zeroblob(16),'planned');",
            )?;
            for sentinel in sentinels {
                assert!(db.execute("UPDATE vm_artifacts SET object_key=CAST(?1 AS BLOB)", [sentinel]).is_err());
            }
            assert!(db.execute("UPDATE vm_artifacts SET state='identified'", []).is_err());
            assert!(db.execute("UPDATE vm_build_intents SET staging_bytes=-1", []).is_err());
            assert!(db.execute("UPDATE vm_generations SET build_id=randomblob(16)", []).is_err());
            assert!(db.execute("INSERT INTO vm_generations(id,build_id,generation) VALUES(randomblob(16),zeroblob(16),1)", []).is_err());
            db.execute("UPDATE vm_artifacts SET state='identified',volume_id=zeroblob(8),
                directory_id=zeroblob(8),file_id=zeroblob(8)", [])?;
            db.execute("INSERT INTO vm_operations(principal,request_key,operation_id,action,
                activity_id,valid_through_ms,binding_kind,recipe_id,recipe_digest)
                VALUES('bob',zeroblob(16),randomblob(16),11,200,86400100,'recipe',
                    'ubuntu-fixture',zeroblob(32))", [])?;
            assert!(db.execute("INSERT INTO vm_build_intents(id,operation_id,recipe_id,recipe_digest,staging_bytes,state)
                SELECT randomblob(16),operation_id,'ubuntu-fixture',zeroblob(32),100,'pending'
                FROM vm_operations WHERE activity_id=200", []).is_err());
            scan(&journal);
            Ok(())
        }).unwrap();
        scan(&path);
    }

    #[test]
    fn substituted_sqlite_main_refuses_before_reconciliation() {
        let original = private_temp();
        let replacement = private_temp();
        let paths = [&original, &replacement].map(|dir| {
            drop(v5_fixture(dir.path()));
            let path = dir.path().join("activity.sqlite3");
            let before = fs::read(&path).unwrap();
            (path, before)
        });

        let other_path = replacement.path().join("activity.sqlite3");
        let parked = original.path().join("parked.sqlite3");
        let result = VmStore::open_inner(original.path(), NOW, |path| {
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
        for (path, before) in paths {
            assert_eq!(fs::read(&path).unwrap(), before);
            let raw = open_sqlite(&path).unwrap();
            assert_eq!(count(&raw, "PRAGMA user_version", []).unwrap(), 5);
            assert_eq!(
                count(
                    &raw,
                    "SELECT status FROM vm_operations WHERE activity_id=11",
                    []
                )
                .unwrap(),
                0
            );
        }
    }
}
