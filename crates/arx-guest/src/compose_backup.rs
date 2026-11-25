//! Protected SQLite online backup helper for static Compose volumes (ADR 0064 §6, leg E4).
//!
//! Uses the workspace-pinned bundled SQLite library and its Online Backup API so
//! WAL transactions are captured consistently into a portable single-file copy,
//! verified with `PRAGMA quick_check`, and bounded by size and deadline.
//!
//! Real guest volume mounts, Linux privilege drops and host custody are
//! **unverified on real hardware**; hosted CI verifies SQLite WAL backup,
//! `quick_check`, symlink/path-isolation refusal, size/deadline bounds and
//! offline restore against local fixtures.

use std::ffi::CStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use rusqlite::{Connection, ErrorCode, OpenFlags, backup::Backup, ffi};
use sha2::{Digest, Sha256};

/// Decoded payload size per typed serial backup chunk (16 KiB).
pub const BACKUP_CHUNK_BYTES: usize = 16 * 1024;
/// Initial maximum size of one protected SQLite backup (512 MiB).
pub const MAX_BACKUP_BYTES: u64 = 512 * 1024 * 1024;
/// Maximum number of chunks in a single backup transfer.
pub const MAX_BACKUP_CHUNKS: u32 = (MAX_BACKUP_BYTES / BACKUP_CHUNK_BYTES as u64) as u32;
/// Fixed root-owned guest staging directory on the persistent disk.
pub const BACKUP_STAGE_ROOT: &str = "/var/lib/arx-compose/backup-staging";
/// Default bounded backup deadline in seconds.
pub const DEFAULT_BACKUP_DEADLINE_S: u32 = 120;
/// Maximum permitted backup deadline in seconds.
pub const MAX_BACKUP_DEADLINE_S: u32 = 300;
/// Pages copied per Online Backup API step so deadlines are enforced promptly.
pub const BACKUP_PAGES_PER_STEP: i32 = 64;
/// Sleep interval when SQLite reports BUSY/LOCKED between backup steps.
pub const BACKUP_STEP_SLEEP: Duration = Duration::from_millis(10);
/// Exact workspace-pinned SQLite version number (3.50.2).
pub const SQLITE_PINNED_VERSION_NUMBER: i32 = 3050002;
/// Exact workspace-pinned SQLite source ID.
pub const SQLITE_PINNED_SOURCE_ID: &str =
    "2025-06-28 14:00:48 2af157d77fb1304a74176eaee7fbc7c7e932d946bf25325e9c26c91db19e3079";
/// Standard 16-byte SQLite 3 database header magic.
pub const SQLITE_HEADER_MAGIC: &[u8; 16] = b"SQLite format 3\0";

const MAX_DB_BASENAME: usize = 64;
const MAX_VOLUME_SLOT: usize = 16;

/// Verified metadata of a staged portable SQLite backup.
/// Carries no filesystem path.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StagedBackupMeta {
    pub total_bytes: u64,
    pub chunks: u32,
    pub digest: String,
}

/// Closed failure classification for the guest SQLite backup helper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupHelperError {
    Unavailable,
    InvalidInput,
    PathViolation,
    NotFound,
    TooLarge,
    TimedOut,
    Corrupt,
    BackupFailed,
    StoreFailed,
}

/// Result of an offline restore into a disposable owner-controlled target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupRestoreReport {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub digest: String,
}

/// Validates an enrolled SQLite database basename.
///
/// Only a single lowercase filename ending in `.db`, `.sqlite`, or `.sqlite3`
/// is accepted; separators, parent traversal, NUL and hidden/dot prefixes are
/// rejected.
pub fn is_db_basename(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > MAX_DB_BASENAME {
        return false;
    }
    if !bytes[0].is_ascii_lowercase() && !bytes[0].is_ascii_digit() {
        return false;
    }
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        return false;
    }
    if !bytes
        .iter()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-'))
    {
        return false;
    }
    name.ends_with(".db") || name.ends_with(".sqlite") || name.ends_with(".sqlite3")
}

/// Validates a Compose volume slot name (matches `manifest::VolumeSpec`).
pub fn is_volume_slot(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= MAX_VOLUME_SLOT
        && bytes[0].is_ascii_lowercase()
        && bytes[bytes.len() - 1] != b'-'
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// Confirms the linked bundled SQLite library matches the exact workspace pin.
pub fn verify_pinned_sqlite() -> Result<(), BackupHelperError> {
    // SAFETY: sqlite3_libversion_number and sqlite3_sourceid return static
    // read-only values from the bundled SQLite library.
    let ok = unsafe {
        ffi::sqlite3_libversion_number() == SQLITE_PINNED_VERSION_NUMBER
            && CStr::from_ptr(ffi::sqlite3_sourceid()).to_bytes()
                == SQLITE_PINNED_SOURCE_ID.as_bytes()
    };
    if ok {
        Ok(())
    } else {
        Err(BackupHelperError::Unavailable)
    }
}

/// Computes the exact number of 16 KiB chunks for a non-zero backup size.
pub fn chunk_count(total_bytes: u64) -> Option<u32> {
    if total_bytes == 0 || total_bytes > MAX_BACKUP_BYTES {
        return None;
    }
    u32::try_from(total_bytes.div_ceil(BACKUP_CHUNK_BYTES as u64))
        .ok()
        .filter(|&n| (1..=MAX_BACKUP_CHUNKS).contains(&n))
}

/// Expected decoded byte length for chunk `index` of a `total_bytes` backup.
pub fn expected_chunk_len(total_bytes: u64, index: u32) -> Option<usize> {
    let chunks = chunk_count(total_bytes)?;
    if index >= chunks {
        return None;
    }
    let offset = u64::from(index).checked_mul(BACKUP_CHUNK_BYTES as u64)?;
    let remaining = total_bytes.checked_sub(offset)?;
    Some(remaining.min(BACKUP_CHUNK_BYTES as u64) as usize)
}

fn is_symlink_or_reparse(meta: &fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    false
}

/// Verifies `dir` exists as a real directory (not a symlink or reparse point).
fn verify_real_dir(dir: &Path) -> Result<(), BackupHelperError> {
    let meta = fs::symlink_metadata(dir).map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            BackupHelperError::NotFound
        } else {
            BackupHelperError::PathViolation
        }
    })?;
    if is_symlink_or_reparse(&meta) || !meta.is_dir() {
        return Err(BackupHelperError::PathViolation);
    }
    Ok(())
}

/// Returns a path suitable for passing to SQLite with `SQLITE_OPEN_NOFOLLOW`.
///
/// On Unix, once `parent` has been verified as a real non-symlink directory,
/// we canonicalize `parent` and re-join the unfollowed leaf basename so OS
/// symlinked temp roots (such as `/var` -> `/private/var` on macOS) do not
/// trigger `SQLITE_CANTOPEN_SYMLINK`, while the leaf itself and its `-wal`/
/// `-shm`/`-journal` companions remain subject to `SQLITE_OPEN_NOFOLLOW`.
fn sqlite_nofollow_path(path: &Path) -> Result<PathBuf, BackupHelperError> {
    #[cfg(unix)]
    {
        let parent = path.parent().ok_or(BackupHelperError::PathViolation)?;
        let leaf = path.file_name().ok_or(BackupHelperError::PathViolation)?;
        verify_real_dir(parent)?;
        let canonical_parent =
            fs::canonicalize(parent).map_err(|_| BackupHelperError::PathViolation)?;
        Ok(canonical_parent.join(leaf))
    }
    #[cfg(not(unix))]
    {
        Ok(path.to_path_buf())
    }
}

/// Verifies `path` is a regular file (not a symlink or reparse point).
/// When `required` is false, `NotFound` is accepted.
fn verify_regular_no_follow(path: &Path, required: bool) -> Result<Option<u64>, BackupHelperError> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if is_symlink_or_reparse(&meta) || !meta.is_file() {
                return Err(BackupHelperError::PathViolation);
            }
            Ok(Some(meta.len()))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound && !required => Ok(None),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Err(BackupHelperError::NotFound),
        Err(_) => Err(BackupHelperError::PathViolation),
    }
}

/// Checks that `candidate` does not live inside `forbidden` and vice versa.
fn verify_disjoint_dirs(a: &Path, b: &Path) -> Result<(), BackupHelperError> {
    let ca = fs::canonicalize(a).map_err(|_| BackupHelperError::PathViolation)?;
    let cb = fs::canonicalize(b).map_err(|_| BackupHelperError::PathViolation)?;
    if ca.starts_with(&cb) || cb.starts_with(&ca) {
        return Err(BackupHelperError::PathViolation);
    }
    Ok(())
}

fn create_exclusive_private(path: &Path) -> Result<File, BackupHelperError> {
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        opts.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    opts.open(path).map_err(|_| BackupHelperError::StoreFailed)
}

fn remove_sqlite_artifacts(path: &Path) {
    let _ = fs::remove_file(path);
    if let Some(s) = path.to_str() {
        for suffix in ["-wal", "-shm", "-journal"] {
            let _ = fs::remove_file(format!("{s}{suffix}"));
        }
    }
}

/// Runs `PRAGMA quick_check` on an opened read-only SQLite connection and
/// requires the exact single-row response `"ok"`.
pub fn verify_sqlite_quick_check(conn: &Connection) -> Result<(), BackupHelperError> {
    let mut stmt = conn
        .prepare("PRAGMA quick_check;")
        .map_err(|_| BackupHelperError::Corrupt)?;
    let rows: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|_| BackupHelperError::Corrupt)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| BackupHelperError::Corrupt)?;
    if rows.len() == 1 && rows[0] == "ok" {
        Ok(())
    } else {
        Err(BackupHelperError::Corrupt)
    }
}

/// Verifies a staged portable SQLite file on disk: regular non-symlink, size in
/// `1..=max_bytes`, SQLite header magic, `PRAGMA quick_check` == `"ok"`, and
/// returns its verified `StagedBackupMeta` (size, chunks, SHA-256).
pub fn verify_portable_sqlite_file(
    path: &Path,
    max_bytes: u64,
) -> Result<StagedBackupMeta, BackupHelperError> {
    verify_pinned_sqlite()?;
    if max_bytes == 0 || max_bytes > MAX_BACKUP_BYTES {
        return Err(BackupHelperError::InvalidInput);
    }
    let Some(size) = verify_regular_no_follow(path, true)? else {
        return Err(BackupHelperError::NotFound);
    };
    if size < SQLITE_HEADER_MAGIC.len() as u64 {
        return Err(BackupHelperError::Corrupt);
    }
    if size > max_bytes {
        return Err(BackupHelperError::TooLarge);
    }
    // Check header magic + compute exact SHA-256 in one bounded streaming read.
    let mut file = open_readonly_no_follow(path)?;
    let mut header = [0u8; 16];
    file.read_exact(&mut header)
        .map_err(|_| BackupHelperError::Corrupt)?;
    if &header != SQLITE_HEADER_MAGIC {
        return Err(BackupHelperError::Corrupt);
    }
    let mut hasher = Sha256::new();
    hasher.update(header);
    let mut total = header.len() as u64;
    let mut buf = [0u8; 16 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|_| BackupHelperError::StoreFailed)?;
        if n == 0 {
            break;
        }
        total = total
            .checked_add(n as u64)
            .ok_or(BackupHelperError::TooLarge)?;
        if total > max_bytes {
            return Err(BackupHelperError::TooLarge);
        }
        hasher.update(&buf[..n]);
    }
    if total != size {
        return Err(BackupHelperError::Corrupt);
    }
    let digest = format!("{:x}", hasher.finalize());
    drop(file);

    // Open read-only with NOFOLLOW and run PRAGMA quick_check.
    let sqlite_path = sqlite_nofollow_path(path)?;
    let conn = Connection::open_with_flags(
        &sqlite_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(|_| BackupHelperError::Corrupt)?;
    verify_sqlite_quick_check(&conn)?;

    let chunks = chunk_count(total).ok_or(BackupHelperError::TooLarge)?;
    Ok(StagedBackupMeta {
        total_bytes: total,
        chunks,
        digest,
    })
}

fn open_readonly_no_follow(path: &Path) -> Result<File, BackupHelperError> {
    let mut opts = OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        opts.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = opts.open(path).map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            BackupHelperError::NotFound
        } else {
            BackupHelperError::PathViolation
        }
    })?;
    let meta = file
        .metadata()
        .map_err(|_| BackupHelperError::PathViolation)?;
    if is_symlink_or_reparse(&meta) || !meta.is_file() {
        return Err(BackupHelperError::PathViolation);
    }
    Ok(file)
}

/// Runs a consistent SQLite Online Backup from `volume_data_dir/<db_basename>`
/// into the exclusive staging leaf `staging_file`, enforcing:
/// - pinned SQLite library version and source ID
/// - strict database basename and disjoint volume/staging directories
/// - no-follow symlink/reparse rejection on the directory, DB, and `-wal`/`-shm`/`-journal`
/// - bounded wall-clock `deadline` checked between Online Backup steps
/// - bounded `max_bytes` ceiling (<= 512 MiB)
/// - portable `DELETE` journal mode (self-contained single file) and `sync_all`
/// - `PRAGMA quick_check` == `"ok"` on the portable output
///
/// Any failure cleans up `staging_file` before returning.
pub fn backup_sqlite_online(
    volume_data_dir: &Path,
    db_basename: &str,
    staging_file: &Path,
    max_bytes: u64,
    deadline: Instant,
) -> Result<StagedBackupMeta, BackupHelperError> {
    verify_pinned_sqlite()?;
    if !is_db_basename(db_basename) || max_bytes == 0 || max_bytes > MAX_BACKUP_BYTES {
        return Err(BackupHelperError::InvalidInput);
    }
    verify_real_dir(volume_data_dir)?;
    let staging_parent = staging_file
        .parent()
        .ok_or(BackupHelperError::PathViolation)?;
    verify_real_dir(staging_parent)?;
    verify_disjoint_dirs(volume_data_dir, staging_parent)?;

    let src_path = volume_data_dir.join(db_basename);
    verify_regular_no_follow(&src_path, true)?;
    for suffix in ["-wal", "-shm", "-journal"] {
        let companion = volume_data_dir.join(format!("{db_basename}{suffix}"));
        verify_regular_no_follow(&companion, false)?;
    }

    if Instant::now() >= deadline {
        return Err(BackupHelperError::TimedOut);
    }

    // Reserve staging path exclusively before opening SQLite on it.
    let reserved = create_exclusive_private(staging_file)?;
    drop(reserved);

    let result = (|| -> Result<StagedBackupMeta, BackupHelperError> {
        let sqlite_src_path = sqlite_nofollow_path(&src_path)?;
        let sqlite_dst_path = sqlite_nofollow_path(staging_file)?;
        let src = Connection::open_with_flags(
            &sqlite_src_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(|_| BackupHelperError::BackupFailed)?;
        let _ = src.busy_timeout(Duration::from_millis(250));

        // Re-verify companions after opening source to narrow TOCTOU windows.
        for suffix in ["-wal", "-shm", "-journal"] {
            let companion = volume_data_dir.join(format!("{db_basename}{suffix}"));
            verify_regular_no_follow(&companion, false)?;
        }

        let mut dst = Connection::open_with_flags(
            &sqlite_dst_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(|_| BackupHelperError::StoreFailed)?;
        dst.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;")
            .map_err(|_| BackupHelperError::StoreFailed)?;

        {
            let backup =
                Backup::new(&src, &mut dst).map_err(|_| BackupHelperError::BackupFailed)?;
            loop {
                if Instant::now() >= deadline {
                    return Err(BackupHelperError::TimedOut);
                }
                match backup.step(BACKUP_PAGES_PER_STEP) {
                    Ok(rusqlite::backup::StepResult::Done) => break,
                    Ok(rusqlite::backup::StepResult::More) => {}
                    Ok(rusqlite::backup::StepResult::Busy)
                    | Ok(rusqlite::backup::StepResult::Locked) => {
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        if remaining.is_zero() {
                            return Err(BackupHelperError::TimedOut);
                        }
                        thread::sleep(remaining.min(BACKUP_STEP_SLEEP));
                    }
                    Ok(_) => return Err(BackupHelperError::BackupFailed),
                    Err(rusqlite::Error::SqliteFailure(err, _))
                        if matches!(
                            err.code,
                            ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
                        ) =>
                    {
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        if remaining.is_zero() {
                            return Err(BackupHelperError::TimedOut);
                        }
                        thread::sleep(remaining.min(BACKUP_STEP_SLEEP));
                    }
                    Err(_) => return Err(BackupHelperError::BackupFailed),
                }
                // Check size ceiling mid-copy so oversized databases stop promptly.
                if let Ok(meta) = fs::symlink_metadata(staging_file)
                    && meta.len() > max_bytes
                {
                    return Err(BackupHelperError::TooLarge);
                }
            }
        }

        // Ensure portable single-file journal mode and flush.
        dst.execute_batch("PRAGMA journal_mode=DELETE;")
            .map_err(|_| BackupHelperError::StoreFailed)?;
        drop(dst);
        drop(src);

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(staging_file)
            .map_err(|_| BackupHelperError::StoreFailed)?;
        file.sync_all()
            .map_err(|_| BackupHelperError::StoreFailed)?;
        drop(file);

        verify_portable_sqlite_file(staging_file, max_bytes)
    })();

    if result.is_err() {
        remove_sqlite_artifacts(staging_file);
    }
    result
}

/// Verifies and restores a settled SQLite backup offline into a disposable
/// owner-controlled target directory, checking path isolation, SHA-256 digest,
/// size ceiling, exclusive creation, flush, and `PRAGMA quick_check`.
pub fn restore_sqlite_offline(
    backup_file: &Path,
    target_dir: &Path,
    target_basename: &str,
    expected_sha256: &str,
    max_bytes: u64,
) -> Result<BackupRestoreReport, BackupHelperError> {
    verify_pinned_sqlite()?;
    if !is_db_basename(target_basename)
        || expected_sha256.len() != 64
        || !expected_sha256
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        || max_bytes == 0
        || max_bytes > MAX_BACKUP_BYTES
    {
        return Err(BackupHelperError::InvalidInput);
    }
    verify_real_dir(target_dir)?;
    let backup_parent = backup_file
        .parent()
        .ok_or(BackupHelperError::PathViolation)?;
    verify_real_dir(backup_parent)?;
    verify_disjoint_dirs(backup_parent, target_dir)?;

    let meta = verify_portable_sqlite_file(backup_file, max_bytes)?;
    if meta.digest != expected_sha256 {
        return Err(BackupHelperError::Corrupt);
    }

    let dest = target_dir.join(target_basename);
    let mut src = open_readonly_no_follow(backup_file)?;
    let mut out = create_exclusive_private(&dest)?;

    let copy_result = (|| -> Result<String, BackupHelperError> {
        let mut hasher = Sha256::new();
        let mut copied = 0u64;
        let mut buf = [0u8; 16 * 1024];
        loop {
            let n = src
                .read(&mut buf)
                .map_err(|_| BackupHelperError::StoreFailed)?;
            if n == 0 {
                break;
            }
            copied = copied
                .checked_add(n as u64)
                .ok_or(BackupHelperError::TooLarge)?;
            if copied > meta.total_bytes {
                return Err(BackupHelperError::Corrupt);
            }
            out.write_all(&buf[..n])
                .map_err(|_| BackupHelperError::StoreFailed)?;
            hasher.update(&buf[..n]);
        }
        out.sync_all().map_err(|_| BackupHelperError::StoreFailed)?;
        if copied != meta.total_bytes {
            return Err(BackupHelperError::Corrupt);
        }
        Ok(format!("{:x}", hasher.finalize()))
    })();

    drop(out);
    let digest = match copy_result {
        Ok(d) if d == expected_sha256 => d,
        Ok(_) => {
            remove_sqlite_artifacts(&dest);
            return Err(BackupHelperError::Corrupt);
        }
        Err(e) => {
            remove_sqlite_artifacts(&dest);
            return Err(e);
        }
    };

    let verify = verify_portable_sqlite_file(&dest, max_bytes);
    match verify {
        Ok(restored)
            if restored.digest == expected_sha256 && restored.total_bytes == meta.total_bytes =>
        {
            Ok(BackupRestoreReport {
                path: dest,
                size_bytes: restored.total_bytes,
                digest,
            })
        }
        _ => {
            remove_sqlite_artifacts(&dest);
            Err(BackupHelperError::Corrupt)
        }
    }
}

#[cfg(target_os = "linux")]
pub const LINUX_HELPER_VOL_FD: i32 = 3;
#[cfg(target_os = "linux")]
pub const LINUX_HELPER_DST_STAT_FD: i32 = 4;
#[cfg(target_os = "linux")]
pub const LINUX_HELPER_CONFIG_FD: i32 = 5;
#[cfg(target_os = "linux")]
pub const LINUX_HELPER_RESULT_FD: i32 = 6;

/// Bounded request sent over `LINUX_HELPER_CONFIG_FD` to the internal
/// `arx-guest backup-sqlite-helper` subprocess.
#[cfg(target_os = "linux")]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinuxBackupHelperConfig {
    pub database: String,
    pub dst_path: String,
    /// Verified symlink-free volume `_data` path, used only when `isolate_root`
    /// is false (non-root Linux tests). When `isolate_root` is true the helper
    /// chroots into `LINUX_HELPER_VOL_FD` and opens `/<database>` directly.
    pub vol_path: Option<String>,
    pub max_bytes: u64,
    pub deadline_ms: u64,
    pub uid: u32,
    pub gid: u32,
    pub isolate_root: bool,
}

/// Bounded reply written to `LINUX_HELPER_RESULT_FD` by the internal
/// `arx-guest backup-sqlite-helper` subprocess.
#[cfg(target_os = "linux")]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinuxBackupHelperReply {
    pub ok: bool,
    pub error: Option<String>,
}

#[cfg(target_os = "linux")]
impl BackupHelperError {
    pub fn wire_code(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::InvalidInput => "invalid_input",
            Self::PathViolation => "path_violation",
            Self::NotFound => "not_found",
            Self::TooLarge => "too_large",
            Self::TimedOut => "timed_out",
            Self::Corrupt => "corrupt",
            Self::BackupFailed => "backup_failed",
            Self::StoreFailed => "store_failed",
        }
    }

    pub fn from_wire_code(code: &str) -> Self {
        match code {
            "unavailable" => Self::Unavailable,
            "invalid_input" => Self::InvalidInput,
            "path_violation" => Self::PathViolation,
            "not_found" => Self::NotFound,
            "too_large" => Self::TooLarge,
            "timed_out" => Self::TimedOut,
            "corrupt" => Self::Corrupt,
            "backup_failed" => Self::BackupFailed,
            "store_failed" => Self::StoreFailed,
            _ => Self::BackupFailed,
        }
    }
}

/// Non-blocking, no-follow metadata check of one file inside an opened Linux
/// directory descriptor using `fstatat(..., AT_SYMLINK_NOFOLLOW)`.
///
/// Rejects symlinks, FIFOs, sockets, device nodes, directories, and files whose
/// `(uid, gid)` does not match `owner`, without ever opening the entry (so a
/// hostile FIFO companion can never hang the privileged broker).
#[cfg(target_os = "linux")]
pub fn verify_dir_file_no_follow_fstatat(
    dir_fd: std::os::fd::BorrowedFd<'_>,
    name: &str,
    owner: (u32, u32),
    required: bool,
) -> Result<(), BackupHelperError> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;

    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        return Err(BackupHelperError::InvalidInput);
    }
    let cname = CString::new(name).map_err(|_| BackupHelperError::InvalidInput)?;
    let mut st = std::mem::MaybeUninit::<libc::stat>::zeroed();
    // SAFETY: dir_fd is a live directory descriptor, cname is NUL-terminated,
    // and st points to writable storage for libc::stat.
    let rc = unsafe {
        libc::fstatat(
            dir_fd.as_raw_fd(),
            cname.as_ptr(),
            st.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if rc != 0 {
        let err = io::Error::last_os_error();
        if err.kind() == io::ErrorKind::NotFound {
            return if required {
                Err(BackupHelperError::NotFound)
            } else {
                Ok(())
            };
        }
        return Err(BackupHelperError::PathViolation);
    }
    // SAFETY: fstatat succeeded and initialized st.
    let st = unsafe { st.assume_init() };
    if (st.st_mode & libc::S_IFMT) != libc::S_IFREG {
        return Err(BackupHelperError::PathViolation);
    }
    if (st.st_uid, st.st_gid) != owner {
        return Err(BackupHelperError::PathViolation);
    }
    Ok(())
}

/// Entry point for the internal Linux `arx-guest backup-sqlite-helper`
/// subprocess.
///
/// Protocol on inherited descriptors:
/// - fd 3 (`LINUX_HELPER_VOL_FD`): `O_NOFOLLOW | O_DIRECTORY` fd of volume `_data`
/// - fd 4 (`LINUX_HELPER_DST_STAT_FD`): fd of parent-created exclusive 0600 staging leaf
/// - fd 5 (`LINUX_HELPER_CONFIG_FD`): pipe carrying bounded JSON `LinuxBackupHelperConfig`
/// - fd 6 (`LINUX_HELPER_RESULT_FD`): pipe receiving bounded JSON `LinuxBackupHelperReply`
///
/// Isolation order:
/// 1. Verify pinned SQLite and open the root-owned staging leaf (`PRAGMA journal_mode=OFF`)
///    so SQLite holds an open descriptor and never touches the staging path again.
/// 2. When `isolate_root` is true (root production), `fchdir(vol_fd)` -> `chroot(".")`
///    -> `chdir("/")`, drop supplementary groups and `(gid, uid)` to the workload
///    owner, and enable `PR_SET_NO_NEW_PRIVS` + non-dumpable.
/// 3. Re-check DB and `-wal`/`-shm`/`-journal` via `fstatat(AT_SYMLINK_NOFOLLOW)`
///    (non-blocking), open the plain symlink-free path `/<database>` with
///    `SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_NOFOLLOW`, and run the stepped
///    SQLite Online Backup API checking deadline and `fstat(dst_stat_fd)` size cap.
#[cfg(target_os = "linux")]
fn fd_is_open(fd: i32) -> bool {
    // SAFETY: F_GETFD is side-effect-free on any integer descriptor number.
    unsafe { libc::fcntl(fd, libc::F_GETFD) != -1 }
}

#[cfg(target_os = "linux")]
pub fn run_linux_backup_helper_main() -> std::process::ExitCode {
    use std::os::fd::FromRawFd;

    let res = run_linux_backup_helper_inner();
    let reply = match &res {
        Ok(()) => LinuxBackupHelperReply {
            ok: true,
            error: None,
        },
        Err(err) => LinuxBackupHelperReply {
            ok: false,
            error: Some(err.wire_code().to_owned()),
        },
    };
    if fd_is_open(LINUX_HELPER_RESULT_FD) {
        // SAFETY: verified open descriptor handed by parent on LINUX_HELPER_RESULT_FD.
        let mut result_file = unsafe { File::from_raw_fd(LINUX_HELPER_RESULT_FD) };
        if let Ok(bytes) = serde_json::to_vec(&reply) {
            let _ = result_file.write_all(&bytes);
            let _ = result_file.flush();
        }
        drop(result_file);
    }
    if res.is_ok() {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}

#[cfg(target_os = "linux")]
fn run_linux_backup_helper_inner() -> Result<(), BackupHelperError> {
    use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::fs::MetadataExt;

    verify_pinned_sqlite()?;
    if !fd_is_open(LINUX_HELPER_VOL_FD)
        || !fd_is_open(LINUX_HELPER_DST_STAT_FD)
        || !fd_is_open(LINUX_HELPER_CONFIG_FD)
    {
        return Err(BackupHelperError::InvalidInput);
    }

    // SAFETY: verified live descriptors passed by parent on fds 3, 4, 5.
    let vol_fd = unsafe { OwnedFd::from_raw_fd(LINUX_HELPER_VOL_FD) };
    let dst_stat_fd = unsafe { OwnedFd::from_raw_fd(LINUX_HELPER_DST_STAT_FD) };
    let config_file = unsafe { File::from_raw_fd(LINUX_HELPER_CONFIG_FD) };

    // Read bounded JSON config from pipe fd 5.
    let mut cfg_bytes = Vec::new();
    config_file
        .take(4097)
        .read_to_end(&mut cfg_bytes)
        .map_err(|_| BackupHelperError::InvalidInput)?;
    if cfg_bytes.is_empty() || cfg_bytes.len() > 4096 {
        return Err(BackupHelperError::InvalidInput);
    }
    let cfg: LinuxBackupHelperConfig =
        serde_json::from_slice(&cfg_bytes).map_err(|_| BackupHelperError::InvalidInput)?;
    if !is_db_basename(&cfg.database)
        || cfg.max_bytes == 0
        || cfg.max_bytes > MAX_BACKUP_BYTES
        || cfg.deadline_ms == 0
    {
        return Err(BackupHelperError::InvalidInput);
    }
    let deadline = Instant::now() + Duration::from_millis(cfg.deadline_ms);

    // Verify destination staging leaf identity matches the fd handed by parent
    // before opening SQLite on it.
    let dst_path = PathBuf::from(&cfg.dst_path);
    let dst_path_meta =
        fs::symlink_metadata(&dst_path).map_err(|_| BackupHelperError::PathViolation)?;
    if is_symlink_or_reparse(&dst_path_meta) || !dst_path_meta.is_file() {
        return Err(BackupHelperError::PathViolation);
    }
    let dst_fd_meta = File::from(
        dst_stat_fd
            .try_clone()
            .map_err(|_| BackupHelperError::StoreFailed)?,
    )
    .metadata()
    .map_err(|_| BackupHelperError::StoreFailed)?;
    if (dst_path_meta.dev(), dst_path_meta.ino()) != (dst_fd_meta.dev(), dst_fd_meta.ino()) {
        return Err(BackupHelperError::PathViolation);
    }

    // Open destination SQLite connection BEFORE chroot/privilege drop and set
    // journal_mode=OFF so SQLite writes strictly through its held descriptor
    // without touching any staging directory paths afterwards.
    let sqlite_dst_path = sqlite_nofollow_path(&dst_path)?;
    let mut dst = Connection::open_with_flags(
        &sqlite_dst_path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(|_| BackupHelperError::StoreFailed)?;
    dst.execute_batch("PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF;")
        .map_err(|_| BackupHelperError::StoreFailed)?;

    // Enter restricted root and drop credentials before opening any workload DB.
    if cfg.isolate_root {
        // SAFETY: vol_fd is a live pinned O_NOFOLLOW|O_DIRECTORY descriptor of _data.
        unsafe {
            if libc::fchdir(vol_fd.as_raw_fd()) != 0
                || libc::chroot(c".".as_ptr()) != 0
                || libc::chdir(c"/".as_ptr()) != 0
                || libc::setgroups(0, std::ptr::null()) != 0
                || libc::setresgid(cfg.gid, cfg.gid, cfg.gid) != 0
                || libc::setresuid(cfg.uid, cfg.uid, cfg.uid) != 0
            {
                return Err(BackupHelperError::Unavailable);
            }
        }
    }
    // Always lock down helper process privileges and disable core dumps.
    // SAFETY: constant scalar arguments to prctl.
    unsafe {
        if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1 as libc::c_ulong, 0, 0, 0) != 0 {
            return Err(BackupHelperError::Unavailable);
        }
        let _ = libc::prctl(libc::PR_SET_DUMPABLE, 0 as libc::c_ulong, 0, 0, 0);
    }

    // Re-verify source DB and companions non-blockingly via fstatat on vol_fd.
    let owner = (cfg.uid, cfg.gid);
    verify_dir_file_no_follow_fstatat(vol_fd.as_fd(), &cfg.database, owner, true)?;
    for suffix in ["-wal", "-shm", "-journal"] {
        let companion = format!("{}{suffix}", cfg.database);
        verify_dir_file_no_follow_fstatat(vol_fd.as_fd(), &companion, owner, false)?;
    }

    // Resolve plain symlink-free source path accepted by SQLite with NOFOLLOW.
    let src_path = if cfg.isolate_root {
        PathBuf::from("/").join(&cfg.database)
    } else {
        let raw_vol = cfg
            .vol_path
            .as_deref()
            .ok_or(BackupHelperError::InvalidInput)?;
        let vol_dir = Path::new(raw_vol);
        verify_real_dir(vol_dir)?;
        let staging_parent = dst_path.parent().ok_or(BackupHelperError::PathViolation)?;
        verify_disjoint_dirs(vol_dir, staging_parent)?;
        sqlite_nofollow_path(&vol_dir.join(&cfg.database))?
    };

    if Instant::now() >= deadline {
        return Err(BackupHelperError::TimedOut);
    }

    let src = Connection::open_with_flags(
        &src_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(|_| BackupHelperError::BackupFailed)?;
    let _ = src.busy_timeout(Duration::from_millis(250));

    // Re-check companions after opening source to narrow TOCTOU windows.
    for suffix in ["-wal", "-shm", "-journal"] {
        let companion = format!("{}{suffix}", cfg.database);
        verify_dir_file_no_follow_fstatat(vol_fd.as_fd(), &companion, owner, false)?;
    }
    drop(vol_fd);

    {
        let backup = Backup::new(&src, &mut dst).map_err(|_| BackupHelperError::BackupFailed)?;
        loop {
            if Instant::now() >= deadline {
                return Err(BackupHelperError::TimedOut);
            }
            match backup.step(BACKUP_PAGES_PER_STEP) {
                Ok(rusqlite::backup::StepResult::Done) => break,
                Ok(rusqlite::backup::StepResult::More) => {}
                Ok(rusqlite::backup::StepResult::Busy)
                | Ok(rusqlite::backup::StepResult::Locked) => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Err(BackupHelperError::TimedOut);
                    }
                    thread::sleep(remaining.min(BACKUP_STEP_SLEEP));
                }
                Ok(_) => return Err(BackupHelperError::BackupFailed),
                Err(rusqlite::Error::SqliteFailure(err, _))
                    if matches!(
                        err.code,
                        ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
                    ) =>
                {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Err(BackupHelperError::TimedOut);
                    }
                    thread::sleep(remaining.min(BACKUP_STEP_SLEEP));
                }
                Err(_) => return Err(BackupHelperError::BackupFailed),
            }
            // Check size ceiling mid-copy via fstat on the open staging descriptor
            // (works even inside chroot where the staging path is invisible).
            let mut st = std::mem::MaybeUninit::<libc::stat>::zeroed();
            // SAFETY: dst_stat_fd is a live file descriptor and st is writable.
            if unsafe { libc::fstat(dst_stat_fd.as_raw_fd(), st.as_mut_ptr()) } == 0 {
                // SAFETY: fstat succeeded and initialized st.
                let st = unsafe { st.assume_init() };
                if st.st_size < 0 || (st.st_size as u64) > cfg.max_bytes {
                    return Err(BackupHelperError::TooLarge);
                }
            }
        }
    }

    drop(dst);
    drop(src);
    drop(dst_stat_fd);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn scratch_dir(label: &str) -> PathBuf {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "arx-guest-backup-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        #[cfg(unix)]
        {
            fs::canonicalize(&dir).unwrap()
        }
        #[cfg(not(unix))]
        {
            dir
        }
    }

    #[test]
    fn validates_db_basename_and_volume_slot_strictly() {
        for good in ["app.db", "calm-life.sqlite3", "events_1.sqlite", "0.db"] {
            assert!(is_db_basename(good), "expected valid: {good}");
        }
        for bad in [
            "",
            ".hidden.db",
            "../app.db",
            "sub/app.db",
            "sub\\app.db",
            "app..db",
            "APP.db",
            "app.txt",
            "app.db-wal",
            "app.db\0",
            &format!("{}.db", "a".repeat(62)),
        ] {
            assert!(!is_db_basename(bad), "expected invalid: {bad}");
        }
        assert!(is_volume_slot("data"));
        assert!(is_volume_slot("db-1"));
        assert!(!is_volume_slot(""));
        assert!(!is_volume_slot("-data"));
        assert!(!is_volume_slot("data-"));
        assert!(!is_volume_slot("DATA"));
        assert!(!is_volume_slot("a_b"));
    }

    #[test]
    fn online_backup_captures_uncheckpointed_wal_and_restores_offline() {
        let root = scratch_dir("wal");
        let volume = root.join("vol_data");
        let staging = root.join("staging");
        let restore = root.join("restore");
        fs::create_dir_all(&volume).unwrap();
        fs::create_dir_all(&staging).unwrap();
        fs::create_dir_all(&restore).unwrap();

        let db_path = volume.join("calm-life.sqlite3");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA wal_autocheckpoint=0;
             CREATE TABLE events (id INTEGER PRIMARY KEY, kind TEXT NOT NULL, score INTEGER NOT NULL);
             INSERT INTO events(kind, score) VALUES ('boot', 1), ('wal_only_tx', 42);",
        )
        .unwrap();
        // Verify WAL sidecar exists and source header reports WAL (2, 2).
        assert!(volume.join("calm-life.sqlite3-wal").exists());
        let src_header = fs::read(&db_path).unwrap();
        assert_eq!(&src_header[18..20], &[2, 2]);

        let staged_path = staging.join("op1.sqlite3");
        let meta = backup_sqlite_online(
            &volume,
            "calm-life.sqlite3",
            &staged_path,
            MAX_BACKUP_BYTES,
            Instant::now() + Duration::from_secs(30),
        )
        .unwrap();
        assert!(meta.total_bytes >= 4096);
        assert_eq!(meta.chunks, chunk_count(meta.total_bytes).unwrap());
        assert_eq!(meta.digest.len(), 64);
        // Portable staged output has DELETE journal header bytes (1, 1) and no sidecars.
        let staged_bytes = fs::read(&staged_path).unwrap();
        assert_eq!(&staged_bytes[18..20], &[1, 1]);
        for suffix in ["-wal", "-shm", "-journal"] {
            assert!(!staging.join(format!("op1.sqlite3{suffix}")).exists());
        }

        // Keep writing AFTER backup to prove snapshot consistency.
        conn.execute(
            "INSERT INTO events(kind, score) VALUES ('post_backup', 99)",
            [],
        )
        .unwrap();
        drop(conn);

        // Restore target inside backup directory is refused (disjointness).
        let nested_restore = staging.join("nested-restore");
        fs::create_dir_all(&nested_restore).unwrap();
        assert_eq!(
            restore_sqlite_offline(
                &staged_path,
                &nested_restore,
                "escaped.sqlite3",
                &meta.digest,
                MAX_BACKUP_BYTES,
            ),
            Err(BackupHelperError::PathViolation)
        );
        assert_eq!(
            restore_sqlite_offline(
                &staged_path,
                &staging,
                "same_dir.sqlite3",
                &meta.digest,
                MAX_BACKUP_BYTES,
            ),
            Err(BackupHelperError::PathViolation)
        );

        // Restore offline into a disposable owner-controlled target and verify rows.
        let report = restore_sqlite_offline(
            &staged_path,
            &restore,
            "restored.sqlite3",
            &meta.digest,
            MAX_BACKUP_BYTES,
        )
        .unwrap();
        assert_eq!(report.size_bytes, meta.total_bytes);
        assert_eq!(report.digest, meta.digest);
        let restored_bytes = fs::read(&report.path).unwrap();
        assert_eq!(&restored_bytes[18..20], &[1, 1]);
        for suffix in ["-wal", "-shm", "-journal"] {
            assert!(!restore.join(format!("restored.sqlite3{suffix}")).exists());
        }
        // Existing target leaf is refused (exclusive create).
        assert_eq!(
            restore_sqlite_offline(
                &staged_path,
                &restore,
                "restored.sqlite3",
                &meta.digest,
                MAX_BACKUP_BYTES,
            ),
            Err(BackupHelperError::StoreFailed)
        );
        // Source backup bytes remain unchanged after restore.
        assert_eq!(fs::read(&staged_path).unwrap(), staged_bytes);

        let restored = Connection::open_with_flags(
            &report.path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .unwrap();
        verify_sqlite_quick_check(&restored).unwrap();
        let rows: Vec<(String, i64)> = restored
            .prepare("SELECT kind, score FROM events ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![("boot".to_owned(), 1), ("wal_only_tx".to_owned(), 42)]
        );

        // Wrong digest refuses restore and leaves no file in target.
        let wrong_digest = "0".repeat(64);
        assert_eq!(
            restore_sqlite_offline(
                &staged_path,
                &restore,
                "bad.sqlite3",
                &wrong_digest,
                MAX_BACKUP_BYTES,
            ),
            Err(BackupHelperError::Corrupt)
        );
        assert!(!restore.join("bad.sqlite3").exists());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn online_backup_enforces_size_cap_deadline_corrupt_and_disjoint_paths() {
        let root = scratch_dir("bounds");
        let volume = root.join("vol_data");
        let staging = root.join("staging");
        fs::create_dir_all(&volume).unwrap();
        fs::create_dir_all(&staging).unwrap();

        let db_path = volume.join("app.db");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE blobs (id INTEGER PRIMARY KEY, data BLOB);
             INSERT INTO blobs(data) VALUES (zeroblob(32768));",
        )
        .unwrap();
        drop(conn);

        // Staging inside volume directory is refused (path isolation).
        assert_eq!(
            backup_sqlite_online(
                &volume,
                "app.db",
                &volume.join("escaped.sqlite3"),
                MAX_BACKUP_BYTES,
                Instant::now() + Duration::from_secs(10),
            ),
            Err(BackupHelperError::PathViolation)
        );

        // Expired deadline refuses before backup writes.
        assert_eq!(
            backup_sqlite_online(
                &volume,
                "app.db",
                &staging.join("timed_out.sqlite3"),
                MAX_BACKUP_BYTES,
                Instant::now(),
            ),
            Err(BackupHelperError::TimedOut)
        );
        assert!(!staging.join("timed_out.sqlite3").exists());

        // Size cap smaller than database refuses and cleans staging.
        assert_eq!(
            backup_sqlite_online(
                &volume,
                "app.db",
                &staging.join("too_big.sqlite3"),
                4096,
                Instant::now() + Duration::from_secs(10),
            ),
            Err(BackupHelperError::TooLarge)
        );
        assert!(!staging.join("too_big.sqlite3").exists());

        // Corrupt database fails quick_check/backup and leaves no staging file.
        let corrupt = volume.join("corrupt.db");
        fs::write(&corrupt, b"not a sqlite database at all").unwrap();
        let err = backup_sqlite_online(
            &volume,
            "corrupt.db",
            &staging.join("corrupt.sqlite3"),
            MAX_BACKUP_BYTES,
            Instant::now() + Duration::from_secs(10),
        )
        .unwrap_err();
        assert!(matches!(
            err,
            BackupHelperError::BackupFailed | BackupHelperError::Corrupt
        ));
        assert!(!staging.join("corrupt.sqlite3").exists());

        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn online_backup_refuses_symlinked_db_and_wal() {
        use std::os::unix::fs::symlink;

        let root = scratch_dir("symlink");
        let volume = root.join("vol_data");
        let staging = root.join("staging");
        let outside = root.join("outside.db");
        fs::create_dir_all(&volume).unwrap();
        fs::create_dir_all(&staging).unwrap();
        let conn = Connection::open(&outside).unwrap();
        conn.execute_batch("CREATE TABLE secret(x TEXT); INSERT INTO secret VALUES ('topsecret');")
            .unwrap();
        drop(conn);

        // Symlinked main DB refused.
        symlink(&outside, volume.join("linked.db")).unwrap();
        assert_eq!(
            backup_sqlite_online(
                &volume,
                "linked.db",
                &staging.join("linked.sqlite3"),
                MAX_BACKUP_BYTES,
                Instant::now() + Duration::from_secs(10),
            ),
            Err(BackupHelperError::PathViolation)
        );

        // Real DB with symlinked -wal refused.
        let real_db = volume.join("real.db");
        let conn = Connection::open(&real_db).unwrap();
        conn.execute_batch("CREATE TABLE t(x INT); INSERT INTO t VALUES (1);")
            .unwrap();
        drop(conn);
        symlink(&outside, volume.join("real.db-wal")).unwrap();
        assert_eq!(
            backup_sqlite_online(
                &volume,
                "real.db",
                &staging.join("real.sqlite3"),
                MAX_BACKUP_BYTES,
                Instant::now() + Duration::from_secs(10),
            ),
            Err(BackupHelperError::PathViolation)
        );

        let _ = fs::remove_dir_all(&root);
    }
}
