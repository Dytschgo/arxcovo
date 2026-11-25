//! Protected host-side SQLite backup custody and serial streaming driver
//! for static Ubuntu Docker Compose VMs (ADR 0064 §6, ADR 0070, leg E4).
//!
//! Host custody guarantees:
//! - Guest never provides any host path or filename; host derives exclusive
//!   staging and settled leaves strictly from opaque host-generated IDs under a
//!   separate managed backup root disjoint from VM state and secret storage.
//! - Host pre-reserves staging capacity before guest/host I/O.
//! - Chunks are read sequentially in 16 KiB decoded units up to the enrolled
//!   size cap (512 MiB default), bounded by explicit idle and absolute deadlines
//!   and cancellation.
//! - Streaming SHA-256, exact size, SQLite header magic and `PRAGMA quick_check`
//!   are verified before exclusive settled publication, flush, and durable
//!   attribution in SQLite; only after durable commit is an opaque backup ID
//!   returned.
//! - Uncertain partial writes stay charged (`partial_uncertain`) until
//!   attributed staging cleanup confirms removal of the exact staging leaf.
//! - Settled backups have NO agent/guest/reaper/pressure delete API.
//! - At most two retained copies per project are allowed until owner-only
//!   maintenance frees capacity.
//!
//! Real guest/Docker/NTFS hardware paths are **unverified on real hardware**.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Result, ensure};
use arx_guest::compose::{
    ComposeRequest as Req, ComposeResponse as Resp, OperationKind, OperationStage, OperationState,
    OperationView, RejectCode, decode_base64_bounded,
};
use arx_guest::compose_backup::{
    BACKUP_CHUNK_BYTES, BackupRestoreReport, DEFAULT_BACKUP_DEADLINE_S, MAX_BACKUP_BYTES,
    SQLITE_HEADER_MAGIC, chunk_count, expected_chunk_len, restore_sqlite_offline,
    verify_portable_sqlite_file,
};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::{Instant, sleep};

use super::compose::Target;
use super::guest::ComposeChannel;
use super::model::{Clock, Id};
use super::paths::{Artifact, ArtifactRole, FileIdentity, LocalPath};
use super::policy::Capacity;
use crate::service::Shutdown;
use crate::store::vm::compose as store;

#[path = "compose_backup_owner.rs"]
mod owner;
pub use owner::backup_retire_cli;
pub(crate) use owner::{OwnerBackup, OwnerRetirementProof};

/// Maximum settled backups retained per project before owner-only maintenance.
pub const MAX_RETAINED_BACKUPS_PER_PROJECT: i64 = 2;
/// Default idle deadline per guest backup frame / chunk exchange.
pub const DEFAULT_IDLE_DEADLINE: Duration = Duration::from_secs(15);
/// Default absolute deadline for the complete guest backup + serial transfer.
pub const DEFAULT_ABSOLUTE_DEADLINE: Duration = Duration::from_secs(600);
/// Default poll interval while the guest runs the SQLite Online Backup helper.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(100);
/// Minimum remaining wall-clock budget required to start a new serial exchange,
/// so an almost-expired absolute deadline settles `deadline_exceeded` cleanly
/// without timing out and closing a shared Compose channel.
const MIN_EXCHANGE_BUDGET: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BackupOpStatus {
    Pending,
    Succeeded,
    Failed,
    Interrupted,
}

impl BackupOpStatus {
    pub fn parse(raw: &str) -> rusqlite::Result<Self> {
        match raw {
            "pending" => Ok(Self::Pending),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "interrupted" => Ok(Self::Interrupted),
            _ => Err(rusqlite::Error::InvalidQuery),
        }
    }

    pub fn wire(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BackupRecordState {
    Staging,
    Settled,
    PartialUncertain,
    Cleaned,
}

impl BackupRecordState {
    pub fn parse(raw: &str) -> rusqlite::Result<Self> {
        match raw {
            "staging" => Ok(Self::Staging),
            "settled" => Ok(Self::Settled),
            "partial_uncertain" => Ok(Self::PartialUncertain),
            "cleaned" => Ok(Self::Cleaned),
            _ => Err(rusqlite::Error::InvalidQuery),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BackupFault {
    NotFound,
    NotReady,
    Busy,
    BackupLimit,
    Disk,
    RecoveryRequired,
    ReplayFull,
    StaleKey,
    KeyCollision,
}

#[derive(Clone, Debug)]
pub(crate) struct BackupIntent {
    pub id: Id,
    pub backup_id: Id,
    pub target: Target,
    pub generation: Id,
    pub boot: u64,
    pub revision: u64,
    pub volume: String,
    pub database: String,
    pub max_bytes: u64,
    pub sent: bool,
    pub has_staging_witness: bool,
    pub status: BackupOpStatus,
    pub failure: Option<String>,
    pub size_bytes: Option<u64>,
    pub sha256: Option<[u8; 32]>,
    #[allow(dead_code)]
    pub backup_state: BackupRecordState,
    #[allow(dead_code)]
    pub host_staging_charge: u64,
    #[allow(dead_code)]
    pub retained_charge: u64,
}

impl BackupIntent {
    fn accepts(&self, view: &OperationView) -> bool {
        view.operation_id == hex::encode(self.id)
            && view.project_id == self.target.project
            && view.kind == OperationKind::Backup
            && view.revision == self.revision
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct BackupRecord {
    pub id: Id,
    pub operation_id: Id,
    pub vm: Id,
    pub project: String,
    pub generation: Id,
    pub boot: u64,
    pub revision: u64,
    pub volume: String,
    pub database: String,
    pub state: BackupRecordState,
    pub host_staging_charge: u64,
    pub guest_staging_charge: u64,
    pub retained_charge: u64,
    pub size_bytes: Option<u64>,
    pub sha256: Option<[u8; 32]>,
    pub volume_id: Option<u64>,
    pub directory_id: Option<Id>,
    pub file_id: Option<Id>,
    pub created_ms: i64,
    pub settled_ms: Option<i64>,
}

/// Timeouts governing a host backup transfer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BackupTimeouts {
    pub idle: Duration,
    pub absolute: Duration,
    pub poll: Duration,
    pub guest_deadline_s: u32,
}

impl Default for BackupTimeouts {
    fn default() -> Self {
        Self {
            idle: DEFAULT_IDLE_DEADLINE,
            absolute: DEFAULT_ABSOLUTE_DEADLINE,
            poll: DEFAULT_POLL_INTERVAL,
            guest_deadline_s: DEFAULT_BACKUP_DEADLINE_S,
        }
    }
}

/// Verified identity of a created staging or settled host backup file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HostFileWitness {
    pub identity: FileIdentity,
    pub directory_id: Id,
}

/// One `partial_uncertain` backup row returned for attributed custody cleanup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PartialBackupRecord {
    pub backup_id: Id,
    pub operation_id: Id,
    pub witness: Option<HostFileWitness>,
}

/// Proof token constructed ONLY inside `vm::compose_backup` after attributed
/// custody confirms removal of both the staging leaf and any never-settled
/// leaf for a `partial_uncertain` backup row.
#[derive(Debug)]
pub(crate) struct PartialCleanupProof {
    backup_id: Id,
    witness: HostFileWitness,
}

impl PartialCleanupProof {
    pub(crate) fn witness(&self) -> HostFileWitness {
        self.witness
    }

    pub(crate) fn backup_id(&self) -> Id {
        self.backup_id
    }
}

/// Failure classification for `StagingWriter::finish_and_publish`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PublishError {
    /// Failed before touching the settled directory (digest, header, size, or
    /// `quick_check` failure). Staging leaf may still be cleaned by identity.
    PrePublish,
    /// Publication found an occupied settled leaf or failed after touching the
    /// settled directory. Caller MUST retain charge (`partial_uncertain`) until
    /// attributed reconciliation confirms removal.
    PostPublishUncertain,
    HardLinkUnavailable,
}

/// Held handle keeping the published backup protected (deny write/delete, allow
/// read on Windows) through the durable SQLite commit, after which transient
/// staging cleanup runs.
pub(crate) trait BackupHold: Send {
    fn finish_committed(self: Box<Self>);
}

pub(crate) struct PublishedBackup {
    pub witness: HostFileWitness,
    pub hold: Box<dyn BackupHold>,
}

/// Exclusive handle to an in-progress host staging file.
pub(crate) trait StagingWriter: Send {
    fn witness(&self) -> HostFileWitness;
    fn write_chunk(&mut self, bytes: &[u8]) -> Result<()>;
    /// Flushes staging, verifies exact size/digest through the held handle and
    /// SQLite `quick_check`, promotes into an exclusive settled leaf in the
    /// managed backup role, verifies identity, flushes post-link, and returns
    /// the settled witness plus the held handle for the store commit.
    fn finish_and_publish(
        self: Box<Self>,
        backup_id: Id,
        expected_size: u64,
        expected_sha256: [u8; 32],
    ) -> std::result::Result<PublishedBackup, PublishError>;
    /// Aborts staging and attempts attributed removal of this exact staging leaf.
    /// Returns `Ok(true)` ONLY when removal is confirmed; `Ok(false)` / `Err`
    /// keeps the partial charged (`partial_uncertain`).
    fn abort_and_cleanup(self: Box<Self>) -> Result<bool>;
}

/// Separate managed host backup custody.
///
/// Intentionally exposes NO delete API for settled backups. Only transient
/// staging leaves and never-settled `partial_uncertain` leaves may be removed
/// after attributed identity verification.
pub(crate) trait BackupCustody: Send + Sync {
    /// Namespace recorded before any guest or host backup I/O.
    fn namespace_id(&self) -> Option<Id> {
        None
    }

    /// Creates an exclusive staging leaf for `operation_id` inside the managed
    /// staging role. Never uses any guest-supplied path.
    fn begin_staging(&self, operation_id: Id) -> Result<Box<dyn StagingWriter>>;

    /// Attributed cleanup of a `partial_uncertain` backup's staging leaf and
    /// any never-settled leaf for `backup_id` whose identity matches `expected`.
    /// Never called for settled backups.
    fn cleanup_partial_staging(
        &self,
        operation_id: Id,
        backup_id: Id,
        expected: Option<HostFileWitness>,
    ) -> Result<bool>;
}

/// Builds a typed `Artifact` under a managed backup root strictly from a
/// host-generated opaque `Id` and `ArtifactRole`.
pub(crate) fn backup_artifact(root: &LocalPath, role: ArtifactRole, id: Id) -> Result<Artifact> {
    let (subdir, ext) = match role {
        ArtifactRole::ComposeBackup => ("settled", "sqlite3"),
        ArtifactRole::ComposeBackupStaging => ("staging", "part"),
        _ => anyhow::bail!("invalid Compose backup artifact role"),
    };
    let leaf_name = format!("{}.{ext}", hex::encode(id));
    let path = root
        .join(subdir)
        .and_then(|dir| dir.join(&leaf_name))
        .map_err(|_| anyhow::anyhow!("invalid Compose backup path"))?;
    ensure!(root.contains(&path), "Compose backup path escaped root");
    Artifact::new(role, path).map_err(|_| anyhow::anyhow!("invalid Compose backup artifact"))
}

/// Real filesystem-backed managed backup directory (separate managed D: backup
/// role on Windows; private 0700 directory on Unix fixtures).
///
/// Enforces:
/// - disjointness from VM state root and secret root
/// - no symlinks or reparse points on root, subdirs, or leaves
/// - exclusive creation (`create_new(true)`) for both staging and settled leaves
/// - held handle (deny write and delete, allow read on Windows) through commit
/// - `sync_all()` (`FlushFileBuffers`) + parent directory flush before commit
/// - NO settled backup deletion method
#[allow(
    dead_code,
    reason = "reserved for qualified backup custody activation and E4 fixtures"
)]
pub(crate) struct ManagedBackupDir {
    root: PathBuf,
    local_root: Option<LocalPath>,
    staging_dir: PathBuf,
    settled_dir: PathBuf,
    pin: crate::config::RootPin,
    namespace: Arc<BackupNamespace>,
    publish_link: fn(&BackupNamespace, &Path, &Path) -> std::io::Result<()>,
    flush_published: fn(&File) -> std::io::Result<()>,
}

#[allow(
    dead_code,
    reason = "reserved for qualified backup custody activation and E4 fixtures"
)]
impl ManagedBackupDir {
    pub(crate) fn new(
        backup_root: &Path,
        state_root: &Path,
        secret_root: Option<&Path>,
    ) -> Result<Self> {
        Self::open(backup_root, state_root, secret_root, true)
    }

    fn open(
        backup_root: &Path,
        state_root: &Path,
        secret_root: Option<&Path>,
        create: bool,
    ) -> Result<Self> {
        ensure_disjoint_roots(backup_root, state_root)?;
        if let Some(secret) = secret_root {
            ensure_disjoint_roots(backup_root, secret)?;
        }
        crate::config::checked_path(backup_root)?;
        let pin = crate::config::RootPin::acquire(backup_root)?;
        ensure!(
            backup_root.is_dir(),
            "Compose backup root must be a directory"
        );
        crate::security::private(backup_root)?;
        crate::local::require_owner(backup_root)?;
        pin.verify(backup_root)?;

        let staging_dir = private_subdir(backup_root, "staging", create)?;
        let settled_dir = private_subdir(backup_root, "settled", create)?;
        let local_root = backup_root
            .to_str()
            .and_then(|raw| LocalPath::parse(raw).ok());
        Ok(Self {
            namespace: Arc::new(BackupNamespace::open(
                backup_root,
                &staging_dir,
                &settled_dir,
            )?),
            publish_link: BackupNamespace::link,
            flush_published: File::sync_all,
            root: backup_root.to_path_buf(),
            local_root,
            staging_dir,
            settled_dir,
            pin,
        })
    }

    #[cfg(test)]
    pub(crate) fn fail_published_flush(&mut self) {
        self.flush_published = |_| Err(std::io::Error::other("injected post-link flush failure"));
    }

    /// Resolves the settled leaf path for owner-only offline restore verification.
    pub(crate) fn settled_path(&self, backup_id: Id) -> Result<PathBuf> {
        self.pin.verify(&self.root)?;
        settled_leaf_path(&self.settled_dir, self.local_root.as_ref(), backup_id)
    }

    /// Verifies an offline restore of a settled backup into a disposable
    /// owner-controlled target directory, without mutating the settled leaf.
    pub(crate) fn verify_offline_restore(
        &self,
        backup_id: Id,
        target_dir: &Path,
        target_basename: &str,
        expected_sha256: [u8; 32],
        max_bytes: u64,
    ) -> Result<BackupRestoreReport> {
        ensure_disjoint_roots(&self.root, target_dir)?;
        let settled = self.settled_path(backup_id)?;
        let expected_hex = hex::encode(expected_sha256);
        restore_sqlite_offline(
            &settled,
            target_dir,
            target_basename,
            &expected_hex,
            max_bytes,
        )
        .map_err(|e| anyhow::anyhow!("offline backup restore verification failed: {e:?}"))
    }

    fn staging_path(&self, operation_id: Id) -> Result<PathBuf> {
        self.pin.verify(&self.root)?;
        staging_leaf_path(&self.staging_dir, self.local_root.as_ref(), operation_id)
    }
}

fn staging_leaf_path(
    staging_dir: &Path,
    local_root: Option<&LocalPath>,
    operation_id: Id,
) -> Result<PathBuf> {
    let path = if let Some(root) = local_root {
        let art = backup_artifact(root, ArtifactRole::ComposeBackupStaging, operation_id)?;
        PathBuf::from(
            art.path_for(ArtifactRole::ComposeBackupStaging)
                .map_err(|_| anyhow::anyhow!("invalid staging artifact role"))?
                .as_str(),
        )
    } else {
        staging_dir.join(format!("{}.part", hex::encode(operation_id)))
    };
    ensure!(
        path.parent() == Some(staging_dir),
        "staging backup path escaped root"
    );
    Ok(path)
}

fn settled_leaf_path(
    settled_dir: &Path,
    local_root: Option<&LocalPath>,
    backup_id: Id,
) -> Result<PathBuf> {
    let path = if let Some(root) = local_root {
        let art = backup_artifact(root, ArtifactRole::ComposeBackup, backup_id)?;
        PathBuf::from(
            art.path_for(ArtifactRole::ComposeBackup)
                .map_err(|_| anyhow::anyhow!("invalid settled artifact role"))?
                .as_str(),
        )
    } else {
        settled_dir.join(format!("{}.sqlite3", hex::encode(backup_id)))
    };
    ensure!(
        path.parent() == Some(settled_dir),
        "settled backup path escaped root"
    );
    Ok(path)
}

struct BackupNamespace {
    staging: File,
    settled: File,
    id: Id,
}

fn directory_handle(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::*;
        options
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_dir() && !is_reparse(&metadata),
        "invalid backup directory"
    );
    Ok(file)
}

#[cfg(unix)]
fn leaf_name(path: &Path) -> std::io::Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;
    let leaf = path.file_name().ok_or(std::io::ErrorKind::InvalidInput)?;
    Ok(std::ffi::CString::new(leaf.as_bytes())?)
}

impl BackupNamespace {
    #[cfg(unix)]
    fn link(&self, staging: &Path, settled: &Path) -> std::io::Result<()> {
        use std::os::fd::AsRawFd;
        let source = leaf_name(staging)?;
        let destination = leaf_name(settled)?;
        if unsafe {
            libc::linkat(
                self.staging.as_raw_fd(),
                source.as_ptr(),
                self.settled.as_raw_fd(),
                destination.as_ptr(),
                0,
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    #[cfg(windows)]
    fn link(&self, staging: &Path, settled: &Path) -> std::io::Result<()> {
        fs::hard_link(staging, settled)
    }

    fn open(root: &Path, staging_path: &Path, settled_path: &Path) -> Result<Self> {
        let root = directory_handle(root)?;
        let staging = directory_handle(staging_path)?;
        let settled = directory_handle(settled_path)?;
        let mut digest = Sha256::new();
        for directory in [&root, &staging, &settled] {
            let identity = file_identity(directory)?;
            digest.update(identity.volume.to_be_bytes());
            digest.update(identity.file);
        }
        let mut id = [0; 16];
        id.copy_from_slice(&digest.finalize()[..16]);
        Ok(Self {
            staging,
            settled,
            id,
        })
    }
}

struct LocalBackupHold {
    namespace: Arc<BackupNamespace>,
    witness: HostFileWitness,
    file: Option<File>,
    settled_file: Option<File>,
    staging_path: PathBuf,
}

impl BackupHold for LocalBackupHold {
    fn finish_committed(mut self: Box<Self>) {
        // Release both held handles first so Windows permits unlinking the
        // transient staging hard-link after the settled leaf is durable in DB.
        drop(self.settled_file.take());
        drop(self.file.take());
        let _ = remove_attributed_file(&self.namespace.staging, &self.staging_path, self.witness);
    }
}

#[allow(
    dead_code,
    reason = "reserved for qualified backup custody activation and E4 fixtures"
)]
struct LocalStagingWriter {
    namespace: Arc<BackupNamespace>,
    publish_link: fn(&BackupNamespace, &Path, &Path) -> std::io::Result<()>,
    flush_published: fn(&File) -> std::io::Result<()>,
    root: PathBuf,
    local_root: Option<LocalPath>,
    staging_path: PathBuf,
    settled_dir: PathBuf,
    file: Option<File>,
    witness: HostFileWitness,
    written: u64,
    hasher: Sha256,
    header: Vec<u8>,
}

impl BackupCustody for ManagedBackupDir {
    fn namespace_id(&self) -> Option<Id> {
        Some(self.namespace.id)
    }

    fn begin_staging(&self, operation_id: Id) -> Result<Box<dyn StagingWriter>> {
        self.pin.verify(&self.root)?;
        check_no_reparse(&self.staging_dir)?;
        let staging_path = self.staging_path(operation_id)?;
        let file = open_exclusive_private(&self.namespace.staging, &staging_path)?;
        let identity = file_identity(&file)?;
        let directory_id = self.namespace.id;
        let witness = HostFileWitness {
            identity,
            directory_id,
        };
        Ok(Box::new(LocalStagingWriter {
            namespace: self.namespace.clone(),
            publish_link: self.publish_link,
            flush_published: self.flush_published,
            root: self.root.clone(),
            local_root: self.local_root.clone(),

            staging_path,
            settled_dir: self.settled_dir.clone(),
            file: Some(file),
            witness,
            written: 0,
            hasher: Sha256::new(),
            header: Vec::with_capacity(SQLITE_HEADER_MAGIC.len()),
        }))
    }

    fn cleanup_partial_staging(
        &self,
        operation_id: Id,
        backup_id: Id,
        expected: Option<HostFileWitness>,
    ) -> Result<bool> {
        self.pin.verify(&self.root)?;
        check_no_reparse(&self.staging_dir)?;
        check_no_reparse(&self.settled_dir)?;
        let staging = self.staging_path(operation_id)?;
        let settled = self.settled_path(backup_id)?;
        let Some(expected) = expected.filter(|w| w.directory_id == self.namespace.id) else {
            return Ok(false);
        };
        let staging_ok = remove_attributed_file(&self.namespace.staging, &staging, expected)?;
        let settled_ok = remove_attributed_file(&self.namespace.settled, &settled, expected)?;
        Ok(staging_ok && settled_ok)
    }
}

impl StagingWriter for LocalStagingWriter {
    fn witness(&self) -> HostFileWitness {
        self.witness
    }

    fn write_chunk(&mut self, bytes: &[u8]) -> Result<()> {
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("staging file closed"))?;
        let next = self
            .written
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| anyhow::anyhow!("staging size overflow"))?;
        ensure!(next <= MAX_BACKUP_BYTES, "backup exceeds maximum size cap");
        if self.header.len() < SQLITE_HEADER_MAGIC.len() {
            let need = (SQLITE_HEADER_MAGIC.len() - self.header.len()).min(bytes.len());
            self.header.extend_from_slice(&bytes[..need]);
        }
        file.write_all(bytes)?;
        self.hasher.update(bytes);
        self.written = next;
        Ok(())
    }

    fn finish_and_publish(
        mut self: Box<Self>,
        backup_id: Id,
        expected_size: u64,
        expected_sha256: [u8; 32],
    ) -> std::result::Result<PublishedBackup, PublishError> {
        // Phase 1: pre-publication verification while holding the exclusive
        // writable handle (deny write and delete, allow read on Windows).
        let (file, settled_path) = (|| -> Result<(File, PathBuf)> {
            ensure!(
                self.written == expected_size
                    && (1..=MAX_BACKUP_BYTES).contains(&expected_size)
                    && self.header.as_slice() == SQLITE_HEADER_MAGIC,
                "backup size or SQLite header mismatch"
            );
            let actual: [u8; 32] = self.hasher.clone().finalize().into();
            ensure!(actual == expected_sha256, "backup SHA-256 digest mismatch");

            let mut file = self
                .file
                .take()
                .ok_or_else(|| anyhow::anyhow!("staging file already closed"))?;
            ensure!(
                file_identity(&file)? == self.witness.identity,
                "staging file identity drifted"
            );
            file.sync_all()?;

            // Verify exact size, SQLite magic, and SHA-256 directly through the
            // held handle so no TOCTOU gap exists between write and digest check.
            file.seek(SeekFrom::Start(0))?;
            let mut verify_hasher = Sha256::new();
            let mut verify_total = 0u64;
            let mut verify_header = [0u8; 16];
            file.read_exact(&mut verify_header)?;
            ensure!(
                &verify_header == SQLITE_HEADER_MAGIC,
                "held staging header mismatch"
            );
            verify_hasher.update(verify_header);
            verify_total += verify_header.len() as u64;
            let mut buf = [0u8; 16 * 1024];
            loop {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                verify_total = verify_total
                    .checked_add(n as u64)
                    .ok_or_else(|| anyhow::anyhow!("held staging size overflow"))?;
                ensure!(
                    verify_total <= expected_size,
                    "held staging exceeds expected size"
                );
                verify_hasher.update(&buf[..n]);
            }
            ensure!(
                verify_total == expected_size,
                "held staging length mismatch"
            );
            let verified_digest: [u8; 32] = verify_hasher.finalize().into();
            ensure!(
                verified_digest == expected_sha256,
                "held staging SHA-256 mismatch"
            );

            // Run PRAGMA quick_check while the handle remains held (FILE_SHARE_READ
            // permits SQLite read-only open while blocking external writers/deleters).
            let verified = verify_portable_sqlite_file(&self.staging_path, expected_size)
                .map_err(|e| anyhow::anyhow!("host backup quick_check failed: {e:?}"))?;
            ensure!(
                verified.total_bytes == expected_size
                    && verified.digest == hex::encode(expected_sha256),
                "host backup post-flush verification mismatch"
            );

            check_no_reparse(&self.root)?;
            check_no_reparse(&self.settled_dir)?;
            let settled_path =
                settled_leaf_path(&self.settled_dir, self.local_root.as_ref(), backup_id)?;
            Ok((file, settled_path))
        })()
        .map_err(|_| PublishError::PrePublish)?;

        // Exclusive settled leaf check: an already-occupied settled leaf is a
        // post-publish-class conflict and must stay charged (partial_uncertain).
        if fs::symlink_metadata(&settled_path).is_ok() {
            drop(file);
            return Err(PublishError::PostPublishUncertain);
        }

        // Phase 2: publication. Any failure from this point on leaves the row
        // `partial_uncertain` (charged) until attributed reconciliation proves
        // removal of both staging and any unpublished settled leaf.
        if let Err(error) = (self.publish_link)(&self.namespace, &self.staging_path, &settled_path)
        {
            return Err(if error.kind() == std::io::ErrorKind::AlreadyExists {
                PublishError::PostPublishUncertain
            } else {
                PublishError::HardLinkUnavailable
            });
        }
        (|| -> Result<PublishedBackup> {
            let held = file;
            let expected_identity = self.witness.identity;
            let settled_ro = open_readonly_no_reparse(&settled_path)?;
            let settled_identity = file_identity(&settled_ro)?;
            ensure!(
                settled_identity == expected_identity,
                "settled backup file identity mismatch"
            );

            // Flush through the held writable handle after publication
            // (FlushFileBuffers on Windows, fsync on Unix) and sync settled dir.
            (self.flush_published)(&held)?;
            #[cfg(unix)]
            self.namespace.settled.sync_all()?;

            let directory_id = self.namespace.id;
            Ok(PublishedBackup {
                witness: HostFileWitness {
                    identity: settled_identity,
                    directory_id,
                },
                hold: Box::new(LocalBackupHold {
                    namespace: self.namespace.clone(),
                    witness: self.witness,
                    file: Some(held),
                    settled_file: Some(settled_ro),
                    staging_path: self.staging_path.clone(),
                }),
            })
        })()
        .map_err(|_| PublishError::PostPublishUncertain)
    }

    fn abort_and_cleanup(mut self: Box<Self>) -> Result<bool> {
        drop(self.file.take());
        remove_attributed_file(&self.namespace.staging, &self.staging_path, self.witness)
    }
}

#[allow(
    dead_code,
    reason = "reserved for qualified backup custody activation and E4 fixtures"
)]
fn ensure_disjoint_roots(a: &Path, b: &Path) -> Result<()> {
    let norm = |p: &Path| -> Result<PathBuf> {
        if p.exists() {
            Ok(fs::canonicalize(p)?)
        } else {
            Ok(p.to_path_buf())
        }
    };
    let ca = norm(a)?;
    let cb = norm(b)?;
    ensure!(
        !ca.starts_with(&cb) && !cb.starts_with(&ca),
        "Compose backup root overlaps protected directory"
    );
    #[cfg(windows)]
    {
        if let (Some(sa), Some(sb)) = (a.to_str(), b.to_str())
            && let (Ok(la), Ok(lb)) = (LocalPath::parse(sa), LocalPath::parse(sb))
        {
            ensure!(!la.overlaps(&lb), "Compose backup root overlaps local path");
        }
    }
    Ok(())
}

#[allow(
    dead_code,
    reason = "reserved for qualified backup custody activation and E4 fixtures"
)]
fn private_subdir(root: &Path, name: &str, create: bool) -> Result<PathBuf> {
    let dir = root.join(name);
    match fs::symlink_metadata(&dir) {
        Ok(meta) => {
            ensure!(
                !is_reparse(&meta) && meta.is_dir(),
                "Compose backup subdir is not a real directory"
            );
        }
        Err(e) if create && e.kind() == std::io::ErrorKind::NotFound => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                fs::DirBuilder::new().mode(0o700).create(&dir)?;
            }
            #[cfg(not(unix))]
            {
                fs::create_dir(&dir)?;
            }
        }
        Err(e) => return Err(e.into()),
    }
    crate::security::private(&dir)?;
    crate::local::require_owner(&dir)?;
    Ok(dir)
}

#[allow(
    dead_code,
    reason = "reserved for qualified backup custody activation and E4 fixtures"
)]
fn check_no_reparse(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    ensure!(!is_reparse(&meta), "reparse point or symlink refused");
    Ok(())
}

#[allow(
    dead_code,
    reason = "reserved for qualified backup custody activation and E4 fixtures"
)]
fn is_reparse(meta: &fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    false
}

#[allow(
    dead_code,
    reason = "reserved for qualified backup custody activation and E4 fixtures"
)]
#[cfg(unix)]
fn open_exclusive_private(dir: &File, path: &Path) -> Result<File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let name = leaf_name(path)?;
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            name.as_ptr(),
            libc::O_CREAT | libc::O_EXCL | libc::O_RDWR | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600 as libc::c_int,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(windows)]
fn open_exclusive_private(_dir: &File, path: &Path) -> Result<File> {
    let mut opts = OpenOptions::new();
    opts.read(true).write(true).create_new(true);
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        };
        // Deny write and delete while allowing read-only verification opens
        // (such as SQLite quick_check) through the same held handle lifecycle.
        opts.share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = opts.open(path)?;
    let meta = file.metadata()?;
    ensure!(
        meta.is_file() && !is_reparse(&meta),
        "exclusive backup file is not a regular file"
    );
    Ok(file)
}

#[allow(
    dead_code,
    reason = "reserved for qualified backup custody activation and E4 fixtures"
)]
fn open_readonly_no_reparse(path: &Path) -> Result<File> {
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
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        // Allow reading while our own held writable handle (share_mode = FILE_SHARE_READ)
        // is still open on the same file.
        opts.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = opts.open(path)?;
    let meta = file.metadata()?;
    ensure!(
        meta.is_file() && !is_reparse(&meta),
        "backup file is not a regular file"
    );
    Ok(file)
}

#[allow(
    dead_code,
    reason = "reserved for qualified backup custody activation and E4 fixtures"
)]
fn remove_attributed_file(dir: &File, path: &Path, expected: HostFileWitness) -> Result<bool> {
    remove_verified(dir, path, expected, || {})
}

#[cfg(windows)]
fn remove_verified(
    _dir: &File,
    path: &Path,
    expected: HostFileWitness,
    after_verify: impl FnOnce(),
) -> Result<bool> {
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::*;
    let file = match OpenOptions::new()
        .access_mode(DELETE | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
    {
        Ok(file) => file,
        Err(error) => return Ok(error.kind() == std::io::ErrorKind::NotFound),
    };
    let meta = file.metadata()?;
    if !meta.is_file() || is_reparse(&meta) || file_identity(&file)? != expected.identity {
        return Ok(false);
    }
    after_verify();
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    Ok(unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfo,
            (&disposition as *const FILE_DISPOSITION_INFO).cast(),
            size_of_val(&disposition) as u32,
        ) != 0
    })
}

#[cfg(unix)]
fn remove_verified(
    dir: &File,
    path: &Path,
    expected: HostFileWitness,
    after_verify: impl FnOnce(),
) -> Result<bool> {
    use std::ffi::CString;
    use std::os::unix::{
        ffi::OsStrExt,
        io::{AsRawFd, FromRawFd},
    };
    let name = CString::new(
        path.file_name()
            .ok_or_else(|| anyhow::anyhow!("missing leaf"))?
            .as_bytes(),
    )?;
    let directory = dir.as_raw_fd();
    let open = |name: &CString| -> std::io::Result<File> {
        let fd = unsafe {
            libc::openat(
                directory,
                name.as_ptr(),
                libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    };
    // A deterministic private directory preserves custody across a crash between
    // rename and unlink. Reconciliation inspects that slot even if the source is absent.
    let private = CString::new(format!(".cleanup-{}", name.to_str()?))?;
    if unsafe { libc::mkdirat(directory, private.as_ptr(), 0o700) } != 0
        && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
    {
        return Ok(false);
    }
    let quarantine = open(&private)?;
    if unsafe { libc::flock(quarantine.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Ok(false);
    }
    ensure!(quarantine.metadata()?.is_dir(), "invalid cleanup directory");
    let q = quarantine.as_raw_fd();
    let inspect = || -> Result<Option<File>> {
        let fd = unsafe {
            libc::openat(
                q,
                c"leaf".as_ptr(),
                libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
            )
        };
        if fd >= 0 {
            return Ok(Some(unsafe { File::from_raw_fd(fd) }));
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(error.into())
        }
    };
    let remove_isolated = |file: File| -> Result<bool> {
        Ok(file.metadata()?.is_file()
            && file_identity(&file)? == expected.identity
            && unsafe { libc::unlinkat(q, c"leaf".as_ptr(), 0) } == 0)
    };
    if let Some(previous) = inspect()?
        && !remove_isolated(previous)?
    {
        return Ok(false);
    }
    match open(&name) {
        Ok(file) => {
            if !file.metadata()?.is_file() || file_identity(&file)? != expected.identity {
                return Ok(false);
            }
            after_verify();
            if unsafe { libc::renameat(directory, name.as_ptr(), q, c"leaf".as_ptr()) } != 0 {
                return Ok(false);
            }
            if !remove_isolated(inspect()?.ok_or_else(|| anyhow::anyhow!("cleanup slot missing"))?)?
            {
                // Preserve the replacement and its original name where possible.
                if unsafe { libc::linkat(q, c"leaf".as_ptr(), directory, name.as_ptr(), 0) } == 0 {
                    unsafe { libc::unlinkat(q, c"leaf".as_ptr(), 0) };
                }
                return Ok(false);
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(_) => return Ok(false),
    }
    quarantine.sync_all()?;
    ensure!(
        unsafe { libc::unlinkat(directory, private.as_ptr(), libc::AT_REMOVEDIR) } == 0,
        "cleanup directory retained"
    );
    dir.sync_all()?;
    Ok(true)
}

#[allow(
    dead_code,
    reason = "reserved for qualified backup custody activation and E4 fixtures"
)]
#[cfg(unix)]
fn file_identity(file: &File) -> Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let meta = file.metadata()?;
    ensure!(
        (meta.is_file() || meta.is_dir()) && !meta.file_type().is_symlink(),
        "regular file required"
    );
    let mut file_id = [0u8; 16];
    file_id[..8].copy_from_slice(&meta.ino().to_be_bytes());
    Ok(FileIdentity {
        volume: meta.dev(),
        file: file_id,
    })
}

#[allow(
    dead_code,
    reason = "reserved for qualified backup custody activation and E4 fixtures"
)]
#[cfg(windows)]
fn file_identity(file: &File) -> Result<FileIdentity> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::*;
    let mut info = FILE_ID_INFO::default();
    ensure!(
        unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileIdInfo,
                (&mut info as *mut FILE_ID_INFO).cast(),
                size_of_val(&info) as u32,
            )
        } != 0,
        "backup identity unavailable"
    );
    Ok(FileIdentity {
        volume: info.VolumeSerialNumber,
        file: info.FileId.Identifier,
    })
}

/// Verified serial backup session bound to one active VM generation and boot.
pub(crate) struct BackupSession {
    pub generation: Id,
    pub wire_boot: u64,
    channel: AsyncMutex<ComposeChannel>,
}

impl BackupSession {
    #[allow(
        dead_code,
        reason = "reserved for qualified activation and E4 fake peer fixtures"
    )]
    pub(crate) fn new(generation: Id, channel: ComposeChannel) -> Self {
        Self {
            generation,
            wire_boot: channel.generation(),
            channel: AsyncMutex::new(channel),
        }
    }
}

/// Host backup coordinator owned by `Supervisor`.
pub(crate) struct BackupHost {
    pub custody: Mutex<Option<Arc<dyn BackupCustody>>>,
    pub capacity: Mutex<Option<Capacity>>,
    pub timeouts: Mutex<BackupTimeouts>,
    sessions: Mutex<HashMap<Id, Arc<BackupSession>>>,
}

impl Default for BackupHost {
    fn default() -> Self {
        Self {
            custody: Mutex::new(None),
            capacity: Mutex::new(None),
            timeouts: Mutex::new(BackupTimeouts::default()),
            sessions: Mutex::new(HashMap::new()),
        }
    }
}

impl BackupHost {
    #[allow(
        dead_code,
        reason = "reserved for qualified activation and E4 fake peer fixtures"
    )]
    pub(crate) fn attach(&self, vm: Id, session: BackupSession) -> Result<()> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| anyhow::anyhow!("Compose backup sessions unavailable"))?;
        sessions.insert(vm, Arc::new(session));
        Ok(())
    }

    pub(crate) fn session(&self, vm: Id) -> Option<Arc<BackupSession>> {
        self.sessions.lock().ok()?.get(&vm).cloned()
    }

    pub(crate) fn capacity(&self) -> Option<Capacity> {
        self.capacity.lock().ok().and_then(|c| *c)
    }

    pub(crate) fn custody(&self) -> Option<Arc<dyn BackupCustody>> {
        self.custody.lock().ok().and_then(|c| c.clone())
    }

    pub(crate) fn timeouts(&self) -> BackupTimeouts {
        self.timeouts.lock().map(|t| *t).unwrap_or_default()
    }

    /// Attaches backup custody and immediately reconciles any `partial_uncertain`
    /// rows whose on-disk staging/never-settled leaves can be attributed and removed.
    #[allow(
        dead_code,
        reason = "reserved for qualified backup custody activation and E4 fixtures"
    )]
    pub(crate) fn set_custody(
        &self,
        db: &crate::store::vm::VmStore,
        custody: Arc<dyn BackupCustody>,
    ) -> Result<usize> {
        {
            let mut slot = self
                .custody
                .lock()
                .map_err(|_| anyhow::anyhow!("Compose backup custody unavailable"))?;
            *slot = Some(custody);
        }
        self.reconcile_partials(db)
    }

    /// Reconciles `partial_uncertain` backup rows by confirming attributed
    /// removal of both the staging leaf and any never-settled leaf before
    /// constructing `PartialCleanupProof` to release the retained charge.
    pub(crate) fn reconcile_partials(&self, db: &crate::store::vm::VmStore) -> Result<usize> {
        let Some(custody) = self.custody() else {
            return Ok(0);
        };
        let partials = db.transact(|tx| store::list_partial_uncertain_backups(tx))?;
        let mut reconciled = 0usize;
        for row in partials {
            let Some(witness) = row.witness else { continue };
            if custody
                .cleanup_partial_staging(row.operation_id, row.backup_id, row.witness)
                .unwrap_or(false)
            {
                let proof = PartialCleanupProof {
                    backup_id: row.backup_id,
                    witness,
                };
                if db.transact(|tx| store::reconcile_partial_backup(tx, proof))? {
                    reconciled += 1;
                }
            }
        }
        Ok(reconciled)
    }

    /// Drives one admitted `compose_backup` operation over the verified Compose
    /// serial channel to durable host settlement (or attributed failure/partial).
    ///
    /// Store errors propagate to dispatch. A pending lease can be retried with the same key.
    pub(crate) async fn drive<C: Clock>(
        &self,
        controller: &super::VmController<C>,
        principal: &str,
        key: Id,
        target: &Target,
        session: Arc<BackupSession>,
        cancel: &Shutdown,
    ) -> Result<()> {
        let db = controller.compose_store();
        let intent = db
            .transact(|tx| store::backup_replay(tx, principal, key))?
            .ok_or_else(|| anyhow::anyhow!("admitted backup unavailable"))?;
        if intent.status != BackupOpStatus::Pending
            || intent.target != *target
            || intent.generation != session.generation
            || intent.boot != session.wire_boot
        {
            return Ok(());
        }

        let settle_fail = |reason: &str, interrupted: bool, cleaned: bool| -> Result<()> {
            let now = controller.compose_now()?;
            db.transact(|tx| {
                store::settle_backup_failed(tx, &intent, reason, interrupted, cleaned, now)
            })
        };

        if intent.has_staging_witness {
            return settle_fail("attribution_commit_failed", true, false);
        }
        let Some(custody) = self.custody() else {
            return settle_fail("backup_custody_unavailable", false, true);
        };
        let boot_live = db.transact(|tx| {
            Ok(
                store::live_boot(tx, target.vm, intent.generation, intent.boot)?
                    && store::boot(tx, principal, target)? == Some(intent.generation),
            )
        })?;
        if !boot_live {
            return settle_fail("retired_boot", true, true);
        }

        if let Some(directory_id) = custody.namespace_id()
            && !db
                .transact(|tx| store::bind_backup_namespace(tx, intent.backup_id, directory_id))?
        {
            return settle_fail("backup_custody_unavailable", true, false);
        }

        let timeouts = self.timeouts();
        let min_budget = timeouts.idle.min(MIN_EXCHANGE_BUDGET);
        let abs_deadline = Instant::now() + timeouts.absolute;
        let op_hex = hex::encode(intent.id);
        let mut channel = match super::qmp::bounded(cancel, abs_deadline, async {
            Ok(session.channel.lock().await)
        })
        .await
        {
            Ok(channel) => channel,
            Err(_) => return settle_fail("guest_channel_error", true, true),
        };

        let next_timeout = || -> Option<Instant> {
            let left = abs_deadline.saturating_duration_since(Instant::now());
            if left < min_budget || left.is_zero() {
                None
            } else {
                Some(abs_deadline.min(Instant::now() + timeouts.idle))
            }
        };

        // 1. Query before submit: never blind-retry an operation after a lost reply.
        let Some(t) = next_timeout() else {
            return settle_fail("deadline_exceeded", true, true);
        };
        let query = match channel
            .exchange_until(cancel, t, |id| Req::ComposeOperation {
                id,
                operation_id: op_hex.clone(),
            })
            .await
        {
            Ok(resp) => resp,
            Err(err) => {
                let interrupted = matches!(
                    err,
                    super::qmp::ChannelError::Cancelled | super::qmp::ChannelError::Timeout
                );
                return settle_fail("guest_channel_error", interrupted, true);
            }
        };

        let mut view = match query {
            Resp::ComposeOperationState { operation, .. } if intent.sent => operation,
            Resp::ComposeRejected {
                code: RejectCode::UnknownOperation,
                ..
            } if !intent.sent => {
                // Persist submission boundary BEFORE transmitting ComposeBackupBegin.
                if db
                    .transact(|tx| {
                        ensure!(
                            store::live_boot(tx, target.vm, intent.generation, intent.boot)?,
                            "Compose backup boot changed"
                        );
                        store::mark_backup_sent(tx, intent.id)
                    })
                    .is_err()
                {
                    return settle_fail("retired_boot", true, true);
                }
                let mut retried_stale_staging = false;
                loop {
                    let Some(t) = next_timeout() else {
                        return settle_fail("deadline_exceeded", true, true);
                    };
                    match channel
                        .exchange_until(cancel, t, |id| Req::ComposeBackupBegin {
                            id,
                            operation_id: op_hex.clone(),
                            generation: intent.boot,
                            project_id: intent.target.project.clone(),
                            volume: intent.volume.clone(),
                            database: intent.database.clone(),
                            max_bytes: intent.max_bytes,
                            deadline_s: timeouts.guest_deadline_s,
                        })
                        .await
                    {
                        Ok(Resp::ComposeOperationState { operation, .. }) => break operation,
                        Ok(Resp::ComposeBackupOccupied {
                            operation_id: stale_hex,
                            ..
                        }) if !retried_stale_staging => {
                            retried_stale_staging = true;
                            let mut stale_id = [0; 16];
                            if hex::decode_to_slice(&stale_hex, &mut stale_id).is_err()
                                || !db.transact(|tx| {
                                    store::terminal_backup_on_boot(
                                        tx,
                                        target,
                                        intent.generation,
                                        intent.boot,
                                        stale_id,
                                    )
                                })?
                            {
                                return settle_fail("backup_staging_occupied", false, true);
                            }
                            let Some(rel_t) = next_timeout() else {
                                return settle_fail("deadline_exceeded", true, true);
                            };

                            match channel
                                .exchange_until(cancel, rel_t, |id| Req::ComposeBackupRelease {
                                    id,
                                    operation_id: stale_hex.clone(),
                                })
                                .await
                            {
                                Ok(Resp::ComposeBackupReleased {
                                    operation_id,
                                    removed: true,
                                    ..
                                }) if operation_id == stale_hex => continue,
                                _ => {
                                    return settle_fail(
                                        reject_label(RejectCode::BackupStagingOccupied),
                                        false,
                                        true,
                                    );
                                }
                            }
                        }
                        Ok(Resp::ComposeRejected { code, .. }) => {
                            return settle_fail(reject_label(code), false, true);
                        }
                        Ok(_) => return settle_fail("unexpected_guest_reply", false, true),
                        Err(err) => {
                            let interrupted = matches!(
                                err,
                                super::qmp::ChannelError::Cancelled
                                    | super::qmp::ChannelError::Timeout
                            );
                            // Submission crossed wire; guest staging may exist, host staging untouched.
                            return settle_fail("guest_channel_error", interrupted, true);
                        }
                    }
                }
            }
            Resp::ComposeRejected {
                code: RejectCode::UnknownOperation,
                ..
            } => {
                // Already marked sent, yet guest reports UnknownOperation: do not blindly resubmit.
                return settle_fail("lost_operation_unknown", true, true);
            }
            _ => return settle_fail("unexpected_guest_reply", false, true),
        };

        // 2. Poll until the guest SQLite Online Backup helper settles.
        let mut reconciled = false;
        loop {
            if cancel.is_cancelled() {
                let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
                return settle_fail("cancelled", true, true);
            }
            if !intent.accepts(&view) {
                let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
                return settle_fail("attribution_mismatch", false, true);
            }
            match view.state {
                OperationState::Succeeded | OperationState::Failed => break,
                OperationState::Ambiguous if !reconciled => {
                    reconciled = true;
                    let Some(t) = next_timeout() else {
                        let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
                        return settle_fail("deadline_exceeded", true, true);
                    };
                    match channel
                        .exchange_until(cancel, t, |id| Req::ComposeReconcile {
                            id,
                            operation_id: op_hex.clone(),
                        })
                        .await
                    {
                        Ok(Resp::ComposeOperationState { operation, .. }) => view = operation,
                        _ => {
                            let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
                            return settle_fail("reconcile_failed", true, true);
                        }
                    }
                }
                OperationState::Running => {
                    let left = abs_deadline.saturating_duration_since(Instant::now());
                    if left < min_budget || left.is_zero() {
                        let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
                        return settle_fail("deadline_exceeded", true, true);
                    }
                    tokio::select! {
                        _ = cancel.cancelled() => {
                            let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
                            return settle_fail("cancelled", true, true);
                        }
                        _ = sleep(left.min(timeouts.poll)) => {}
                    }
                    let Some(t) = next_timeout() else {
                        let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
                        return settle_fail("deadline_exceeded", true, true);
                    };
                    match channel
                        .exchange_until(cancel, t, |id| Req::ComposeOperation {
                            id,
                            operation_id: op_hex.clone(),
                        })
                        .await
                    {
                        Ok(Resp::ComposeOperationState { operation, .. }) => view = operation,
                        _ => {
                            let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
                            return settle_fail("poll_failed", true, true);
                        }
                    }
                }
                _ => {
                    let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
                    return settle_fail("ambiguous_backup", true, true);
                }
            }
        }

        if view.stage != OperationStage::Settled || view.state != OperationState::Succeeded {
            let reason = view.failure.map_or("guest_backup_failed", failure_label);
            let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
            return settle_fail(reason, false, true);
        }

        // 3. Fetch and strictly validate backup metadata (size, chunk count, digest).
        let Some(t) = next_timeout() else {
            let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
            return settle_fail("deadline_exceeded", true, true);
        };
        let (total_bytes, chunks, digest_hex) = match channel
            .exchange_until(cancel, t, |id| Req::ComposeBackupMeta {
                id,
                operation_id: op_hex.clone(),
            })
            .await
        {
            Ok(Resp::ComposeBackupReady {
                operation_id,
                project_id,
                revision,
                total_bytes,
                chunks,
                digest,
                ..
            }) => {
                if operation_id != op_hex
                    || project_id != intent.target.project
                    || revision != intent.revision
                    || total_bytes == 0
                    || total_bytes > intent.max_bytes
                    || total_bytes > MAX_BACKUP_BYTES
                    || chunk_count(total_bytes) != Some(chunks)
                    || digest.len() != 64
                    || !digest
                        .bytes()
                        .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
                {
                    let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
                    return settle_fail("invalid_backup_metadata", false, true);
                }
                (total_bytes, chunks, digest)
            }
            Ok(Resp::ComposeRejected { code, .. }) => {
                let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
                return settle_fail(reject_label(code), false, true);
            }
            _ => {
                let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
                return settle_fail("backup_meta_failed", true, true);
            }
        };
        let mut expected_sha = [0u8; 32];
        if hex::decode_to_slice(&digest_hex, &mut expected_sha).is_err() {
            let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
            return settle_fail("invalid_backup_digest", false, true);
        }

        // 4. Create exclusive host staging leaf and record its file identity
        // BEFORE streaming any chunk bytes.
        let mut writer = match custody.begin_staging(intent.id) {
            Ok(w) => w,
            Err(_) => {
                let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
                return settle_fail("host_staging_create_failed", false, false);
            }
        };
        let staging_witness = writer.witness();
        if db
            .transact(|tx| {
                store::record_backup_staging_identity(
                    tx,
                    intent.backup_id,
                    staging_witness.identity.volume,
                    staging_witness.directory_id,
                    staging_witness.identity.file,
                )
            })
            .is_err()
        {
            let cleaned = writer.abort_and_cleanup().unwrap_or(false);
            let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
            return settle_fail("store_identity_failed", false, cleaned);
        }

        // 5. Stream exact-sequence 16 KiB chunks under idle + absolute deadlines.
        let mut received_bytes = 0u64;
        for index in 0..chunks {
            if cancel.is_cancelled() {
                let cleaned = writer.abort_and_cleanup().unwrap_or(false);
                let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
                return settle_fail("cancelled", true, cleaned);
            }
            let Some(t) = next_timeout() else {
                let cleaned = writer.abort_and_cleanup().unwrap_or(false);
                let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
                return settle_fail("deadline_exceeded", true, cleaned);
            };
            let Some(want_len) = expected_chunk_len(total_bytes, index) else {
                let cleaned = writer.abort_and_cleanup().unwrap_or(false);
                let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
                return settle_fail("chunk_bounds_error", false, cleaned);
            };
            let reply = channel
                .exchange_until(cancel, t, |id| Req::ComposeBackupChunk {
                    id,
                    operation_id: op_hex.clone(),
                    index,
                })
                .await;
            let data_b64 = match reply {
                Ok(Resp::ComposeBackupData {
                    operation_id,
                    index: got_idx,
                    data,
                    ..
                }) if operation_id == op_hex && got_idx == index => data,
                Ok(Resp::ComposeBackupData { .. }) => {
                    let cleaned = writer.abort_and_cleanup().unwrap_or(false);
                    let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
                    return settle_fail("chunk_sequence_mismatch", false, cleaned);
                }
                Ok(Resp::ComposeRejected { code, .. }) => {
                    let cleaned = writer.abort_and_cleanup().unwrap_or(false);
                    let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
                    return settle_fail(reject_label(code), false, cleaned);
                }
                Err(super::qmp::ChannelError::Cancelled) => {
                    let cleaned = writer.abort_and_cleanup().unwrap_or(false);
                    let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
                    return settle_fail("cancelled", true, cleaned);
                }
                Err(super::qmp::ChannelError::Timeout) => {
                    let cleaned = writer.abort_and_cleanup().unwrap_or(false);
                    let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
                    return settle_fail("idle_deadline_exceeded", true, cleaned);
                }
                _ => {
                    let cleaned = writer.abort_and_cleanup().unwrap_or(false);
                    let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
                    return settle_fail("chunk_transfer_failed", true, cleaned);
                }
            };
            let Some(decoded) = decode_base64_bounded(&data_b64, BACKUP_CHUNK_BYTES) else {
                let cleaned = writer.abort_and_cleanup().unwrap_or(false);
                let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
                return settle_fail("chunk_decode_failed", false, cleaned);
            };
            if decoded.len() != want_len {
                let cleaned = writer.abort_and_cleanup().unwrap_or(false);
                let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
                return settle_fail("chunk_size_mismatch", false, cleaned);
            }
            let Some(next_total) = received_bytes.checked_add(decoded.len() as u64) else {
                let cleaned = writer.abort_and_cleanup().unwrap_or(false);
                let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
                return settle_fail("backup_size_overflow", false, cleaned);
            };
            if next_total > total_bytes || next_total > intent.max_bytes {
                let cleaned = writer.abort_and_cleanup().unwrap_or(false);
                let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
                return settle_fail("backup_size_exceeded", false, cleaned);
            }
            if writer.write_chunk(&decoded).is_err() {
                let cleaned = writer.abort_and_cleanup().unwrap_or(false);
                let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
                return settle_fail("host_write_failed", false, cleaned);
            }
            received_bytes = next_total;
        }

        if received_bytes != total_bytes {
            let cleaned = writer.abort_and_cleanup().unwrap_or(false);
            let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
            return settle_fail("backup_size_mismatch", false, cleaned);
        }

        // 6. Verify digest, SQLite quick_check, flush, and publish exclusive
        // settled leaf in the separate managed D: backup role while holding
        // the handle open through the durable store commit.
        let published = match writer.finish_and_publish(intent.backup_id, total_bytes, expected_sha)
        {
            Ok(p) => p,
            Err(error @ (PublishError::PrePublish | PublishError::HardLinkUnavailable)) => {
                // Publication never touched the settled directory; attributed
                // cleanup of the staging leaf may safely release the charge.
                let cleaned = custody
                    .cleanup_partial_staging(intent.id, intent.backup_id, Some(staging_witness))
                    .unwrap_or(false);
                let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
                return settle_fail(
                    if error == PublishError::HardLinkUnavailable {
                        "hard_link_unavailable"
                    } else {
                        "verification_or_flush_failed"
                    },
                    false,
                    cleaned,
                );
            }
            Err(PublishError::PostPublishUncertain) => {
                // Publication started or hit an occupied settled leaf: ALWAYS
                // stay partial_uncertain (charged) until attributed reconciler runs.
                let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
                return settle_fail("verification_or_flush_failed", false, false);
            }
        };

        // 7. Commit durable attribution (VM generation, project, revision,
        // digest, size, settled file identity and charge) BEFORE dropping the
        // held handle, releasing guest staging, or exposing backup_id.
        let settled_witness = published.witness;
        let committed = controller.compose_now().and_then(|now| {
            db.transact(|tx| {
                ensure!(
                    store::live_boot(tx, target.vm, intent.generation, intent.boot)?
                        && store::boot(tx, principal, target)? == Some(intent.generation),
                    "Compose backup boot retired before commit"
                );
                store::settle_backup_succeeded(
                    tx,
                    &intent,
                    total_bytes,
                    expected_sha,
                    settled_witness.identity.volume,
                    settled_witness.directory_id,
                    settled_witness.identity.file,
                    now,
                )
            })
        });
        if let Err(error) = committed {
            // Keep the settled file and charge conservative (partial_uncertain)
            // and record its settled identity so the reconciler can attribute it.
            drop(published.hold);
            settle_fail("attribution_commit_failed", true, false)?;
            let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
            return Err(error);
        }

        // Durable store commit succeeded: release the held handle and remove
        // the transient staging leaf, then best-effort release guest staging.
        published.hold.finish_committed();
        let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
        Ok(())
    }
}

async fn best_effort_release(
    channel: &mut ComposeChannel,
    operation_id: &str,
    timeout: Duration,
) -> Result<()> {
    if !channel.is_open() {
        return Ok(());
    }
    // Use a fresh local shutdown token with a short bound so release is still
    // attempted when the caller's shutdown token was cancelled mid-transfer.
    let local = Shutdown::default();
    let _ = channel
        .exchange_until(
            &local,
            Instant::now() + timeout.min(Duration::from_secs(2)),
            |id| Req::ComposeBackupRelease {
                id,
                operation_id: operation_id.to_owned(),
            },
        )
        .await;
    Ok(())
}

fn reject_label(code: RejectCode) -> &'static str {
    match code {
        RejectCode::Busy => "guest_busy",
        RejectCode::ReconcileRequired => "compose_recovery_required",
        RejectCode::UnknownProject => "unknown_project",
        RejectCode::NotCommitted => "not_committed",
        RejectCode::UnknownVolume => "unknown_volume",
        RejectCode::InvalidDatabase => "invalid_database",
        RejectCode::BackupNotReady => "backup_not_ready",
        RejectCode::BackupChunkOutOfRange => "backup_chunk_out_of_range",
        RejectCode::BackupStagingOccupied => "backup_staging_occupied",
        RejectCode::GenerationMismatch => "generation_mismatch",
        RejectCode::OperationConflict => "operation_conflict",
        RejectCode::StoreFailure => "guest_store_failure",
        _ => "guest_rejected",
    }
}

fn failure_label(code: arx_guest::compose::FailureCode) -> &'static str {
    use arx_guest::compose::FailureCode as F;
    match code {
        F::BackupNotFound => "backup_not_found",
        F::BackupTimedOut => "backup_timed_out",
        F::BackupTooLarge => "backup_too_large",
        F::BackupCorrupt => "backup_corrupt",
        F::StoreFailed => "guest_store_failed",
        _ => "guest_backup_failed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_valid_portable_sqlite(dir: &Path) -> (Vec<u8>, [u8; 32]) {
        let vol = dir.join("vol");
        let stage = dir.join("stage");
        fs::create_dir_all(&vol).unwrap();
        fs::create_dir_all(&stage).unwrap();
        let db = vol.join("app.sqlite3");
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA wal_autocheckpoint=0;
             CREATE TABLE kv(k TEXT PRIMARY KEY, v TEXT NOT NULL);
             INSERT INTO kv VALUES ('a', 'wal_value');",
        )
        .unwrap();
        let out = stage.join("out.sqlite3");
        let meta = arx_guest::compose_backup::backup_sqlite_online(
            &vol,
            "app.sqlite3",
            &out,
            MAX_BACKUP_BYTES,
            std::time::Instant::now() + Duration::from_secs(15),
        )
        .unwrap();
        drop(conn);
        let bytes = fs::read(&out).unwrap();
        assert_eq!(&bytes[18..20], &[1, 1]);
        let mut sha = [0u8; 32];
        hex::decode_to_slice(&meta.digest, &mut sha).unwrap();
        (bytes, sha)
    }

    #[test]
    fn attributed_cleanup_preserves_concurrent_replacement() {
        let root = crate::mcp::vm::tests::backup_fixture_root();
        let state = tempfile::tempdir().unwrap();
        let managed = ManagedBackupDir::new(root.path(), state.path(), None).unwrap();
        let op = [19; 16];
        let writer = managed.begin_staging(op).unwrap();
        let witness = writer.witness();
        drop(writer);
        let leaf = managed.staging_path(op).unwrap();
        let saved = leaf.with_extension("saved");
        let removed = remove_verified(&managed.namespace.staging, &leaf, witness, || {
            let rename = fs::rename(&leaf, &saved);
            #[cfg(windows)]
            assert!(rename.is_err(), "delete handle must exclude rename");
            #[cfg(unix)]
            assert!(rename.is_ok());
            if rename.is_ok() {
                fs::write(&leaf, b"replacement").unwrap();
            }
        })
        .unwrap();
        #[cfg(windows)]
        assert!(removed && !leaf.exists());
        #[cfg(unix)]
        {
            assert!(!removed);
            assert_eq!(fs::read(leaf).unwrap(), b"replacement");
        }
    }

    #[test]
    fn managed_backup_dir_enforces_size_exclusive_leaf_held_handle_and_restore_disjointness() {
        let state_root = tempfile::tempdir().unwrap();
        let backup_root = crate::mcp::vm::tests::backup_fixture_root();
        let src_dir = tempfile::tempdir().unwrap();
        let restore_dir = tempfile::tempdir().unwrap();

        let (bytes, sha) = make_valid_portable_sqlite(src_dir.path());
        let mut managed =
            ManagedBackupDir::new(backup_root.path(), state_root.path(), None).unwrap();

        // 1. Mismatched expected_size fails finish_and_publish even when the
        // written bytes are a valid portable SQLite file whose digest matches
        // expected_sha256 (kills size-check bypasses independently of quick_check).
        let op_short = [0x11; 16];
        let b_short = [0x12; 16];
        let mut w_short = managed.begin_staging(op_short).unwrap();
        let short_witness = w_short.witness();
        w_short.write_chunk(&bytes).unwrap();
        let err = w_short
            .finish_and_publish(b_short, bytes.len() as u64 + 4096, sha)
            .err()
            .expect("size mismatch must fail");
        assert_eq!(err, PublishError::PrePublish);
        assert!(
            managed
                .cleanup_partial_staging(op_short, b_short, Some(short_witness))
                .unwrap()
        );
        assert!(!managed.staging_path(op_short).unwrap().exists());
        assert!(!managed.settled_path(b_short).unwrap().exists());

        // 2. Pre-existing settled leaf refuses publish with PostPublishUncertain,
        // preserves the foreign file, and attributed cleanup refuses to delete
        // the foreign-identity settled leaf.
        let op_occ = [0x21; 16];
        let b_occ = [0x22; 16];
        let foreign_settled = managed.settled_path(b_occ).unwrap();
        fs::write(&foreign_settled, b"foreign-settled-bytes").unwrap();
        let mut w_occ = managed.begin_staging(op_occ).unwrap();
        let occ_witness = w_occ.witness();
        w_occ.write_chunk(&bytes).unwrap();
        let occ_err = w_occ
            .finish_and_publish(b_occ, bytes.len() as u64, sha)
            .err()
            .expect("occupied settled leaf must fail");
        assert_eq!(occ_err, PublishError::PostPublishUncertain);
        assert_eq!(
            fs::read(&foreign_settled).unwrap(),
            b"foreign-settled-bytes"
        );
        // Reconciler removes the attributed staging leaf, refuses to delete the
        // foreign settled leaf (identity mismatch), and returns false (stays charged).
        assert!(
            !managed
                .cleanup_partial_staging(op_occ, b_occ, Some(occ_witness))
                .unwrap()
        );
        assert_eq!(
            fs::read(&foreign_settled).unwrap(),
            b"foreign-settled-bytes"
        );

        // 3. Normal publish holds the file open through commit, verifies identity
        // equality, and removes the staging leaf only when finish_committed runs.
        let op_ok = [0x31; 16];
        let b_ok = [0x32; 16];
        let mut w_ok = managed.begin_staging(op_ok).unwrap();
        let ok_staging_witness = w_ok.witness();
        w_ok.write_chunk(&bytes).unwrap();
        let published = w_ok
            .finish_and_publish(b_ok, bytes.len() as u64, sha)
            .expect("publish should succeed");
        assert_eq!(published.witness.identity, ok_staging_witness.identity);
        let settled_path = managed.settled_path(b_ok).unwrap();
        assert!(settled_path.exists());
        #[cfg(windows)]
        {
            // While the handle is held, external write and delete are denied
            // while read-only inspection succeeds.
            assert!(OpenOptions::new().write(true).open(&settled_path).is_err());
            assert!(fs::remove_file(&settled_path).is_err());
            assert!(open_readonly_no_reparse(&settled_path).is_ok());
        }
        published.hold.finish_committed();
        assert!(!managed.staging_path(op_ok).unwrap().exists());
        assert!(settled_path.exists());

        // 4. Offline restore refuses target inside backup root and existing target
        // leaf, and leaves settled leaf bytes and identity unchanged.
        let nested = backup_root.path().join("nested-restore");
        fs::create_dir_all(&nested).unwrap();
        assert!(
            managed
                .verify_offline_restore(b_ok, &nested, "bad.sqlite3", sha, MAX_BACKUP_BYTES)
                .is_err()
        );
        let settled_id_before =
            file_identity(&open_readonly_no_reparse(&settled_path).unwrap()).unwrap();
        let report = managed
            .verify_offline_restore(
                b_ok,
                restore_dir.path(),
                "ok.sqlite3",
                sha,
                MAX_BACKUP_BYTES,
            )
            .unwrap();
        assert_eq!(report.size_bytes, bytes.len() as u64);
        assert!(
            managed
                .verify_offline_restore(
                    b_ok,
                    restore_dir.path(),
                    "ok.sqlite3",
                    sha,
                    MAX_BACKUP_BYTES
                )
                .is_err()
        );
        let settled_id_after =
            file_identity(&open_readonly_no_reparse(&settled_path).unwrap()).unwrap();
        assert_eq!(settled_id_before, settled_id_after);
        assert_eq!(fs::read(&settled_path).unwrap(), bytes);

        // 5. Unpublished settled leaf whose identity matches recorded witness IS
        // removed by attributed cleanup (simulating commit failure after link).
        let op_crash = [0x41; 16];
        let b_crash = [0x42; 16];
        let mut w_crash = managed.begin_staging(op_crash).unwrap();
        w_crash.write_chunk(&bytes).unwrap();
        let crashed = w_crash
            .finish_and_publish(b_crash, bytes.len() as u64, sha)
            .unwrap();
        let crashed_witness = crashed.witness;
        drop(crashed.hold); // Drop without finish_committed (commit failed).
        assert!(managed.settled_path(b_crash).unwrap().exists());
        assert!(managed.staging_path(op_crash).unwrap().exists());
        assert!(
            managed
                .cleanup_partial_staging(op_crash, b_crash, Some(crashed_witness))
                .unwrap()
        );
        assert!(!managed.settled_path(b_crash).unwrap().exists());
        assert!(!managed.staging_path(op_crash).unwrap().exists());
        managed.publish_link = |_, _, _| Err(std::io::ErrorKind::Unsupported.into());
        let mut writer = managed.begin_staging(op_crash).unwrap();
        let witness = writer.witness();
        writer.write_chunk(&bytes).unwrap();
        assert_eq!(
            writer
                .finish_and_publish(b_crash, bytes.len() as u64, sha)
                .err(),
            Some(PublishError::HardLinkUnavailable)
        );
        assert!(!managed.settled_path(b_crash).unwrap().exists());
        assert!(
            managed
                .cleanup_partial_staging(op_crash, b_crash, Some(witness))
                .unwrap()
        );
    }
}
