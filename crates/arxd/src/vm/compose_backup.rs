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
}

impl PartialCleanupProof {
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

        let staging_dir = ensure_private_subdir(backup_root, "staging")?;
        let settled_dir = ensure_private_subdir(backup_root, "settled")?;
        let local_root = backup_root
            .to_str()
            .and_then(|raw| LocalPath::parse(raw).ok());
        Ok(Self {
            root: backup_root.to_path_buf(),
            local_root,
            staging_dir,
            settled_dir,
            pin,
        })
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

struct LocalBackupHold {
    file: Option<File>,
    settled_file: Option<File>,
    staging_path: PathBuf,
    staging_dir: PathBuf,
}

impl BackupHold for LocalBackupHold {
    fn finish_committed(mut self: Box<Self>) {
        // Release both held handles first so Windows permits unlinking the
        // transient staging hard-link after the settled leaf is durable in DB.
        drop(self.settled_file.take());
        drop(self.file.take());
        let _ = fs::remove_file(&self.staging_path);
        let _ = sync_directory(&self.staging_dir);
    }
}

#[allow(
    dead_code,
    reason = "reserved for qualified backup custody activation and E4 fixtures"
)]
struct LocalStagingWriter {
    root: PathBuf,
    local_root: Option<LocalPath>,
    staging_dir: PathBuf,
    staging_path: PathBuf,
    settled_dir: PathBuf,
    file: Option<File>,
    witness: HostFileWitness,
    written: u64,
    hasher: Sha256,
    header: Vec<u8>,
}

impl BackupCustody for ManagedBackupDir {
    fn begin_staging(&self, operation_id: Id) -> Result<Box<dyn StagingWriter>> {
        self.pin.verify(&self.root)?;
        check_no_reparse(&self.staging_dir)?;
        let staging_path = self.staging_path(operation_id)?;
        let file = open_exclusive_private(&staging_path)?;
        let identity = file_identity(&file)?;
        let directory_id = dir_id_hash(&self.staging_dir)?;
        let witness = HostFileWitness {
            identity,
            directory_id,
        };
        Ok(Box::new(LocalStagingWriter {
            root: self.root.clone(),
            local_root: self.local_root.clone(),
            staging_dir: self.staging_dir.clone(),
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
        let staging_ok = remove_attributed_file(&staging, expected)?;
        let settled_ok = remove_attributed_file(&settled, expected)?;
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
        let (mut file, settled_path) = (|| -> Result<(File, PathBuf)> {
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
        (|| -> Result<PublishedBackup> {
            let (held, expected_identity) =
                if fs::hard_link(&self.staging_path, &settled_path).is_ok() {
                    (file, self.witness.identity)
                } else {
                    let mut dst = open_exclusive_private(&settled_path)?;
                    file.seek(SeekFrom::Start(0))?;
                    std::io::copy(&mut file, &mut dst)?;
                    dst.sync_all()?;
                    let dst_id = file_identity(&dst)?;
                    drop(file);
                    (dst, dst_id)
                };

            let settled_ro = open_readonly_no_reparse(&settled_path)?;
            let settled_identity = file_identity(&settled_ro)?;
            ensure!(
                settled_identity == expected_identity,
                "settled backup file identity mismatch"
            );

            // Flush through the held writable handle after publication
            // (FlushFileBuffers on Windows, fsync on Unix) and sync settled dir.
            held.sync_all()?;
            sync_directory(&self.settled_dir)?;

            let directory_id = dir_id_hash(&self.settled_dir)?;
            Ok(PublishedBackup {
                witness: HostFileWitness {
                    identity: settled_identity,
                    directory_id,
                },
                hold: Box::new(LocalBackupHold {
                    file: Some(held),
                    settled_file: Some(settled_ro),
                    staging_path: self.staging_path.clone(),
                    staging_dir: self.staging_dir.clone(),
                }),
            })
        })()
        .map_err(|_| PublishError::PostPublishUncertain)
    }

    fn abort_and_cleanup(mut self: Box<Self>) -> Result<bool> {
        drop(self.file.take());
        remove_attributed_file(&self.staging_path, Some(self.witness))
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
fn ensure_private_subdir(root: &Path, name: &str) -> Result<PathBuf> {
    let dir = root.join(name);
    match fs::symlink_metadata(&dir) {
        Ok(meta) => {
            ensure!(
                !is_reparse(&meta) && meta.is_dir(),
                "Compose backup subdir is not a real directory"
            );
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
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
fn open_exclusive_private(path: &Path) -> Result<File> {
    let mut opts = OpenOptions::new();
    opts.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
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
fn remove_attributed_file(path: &Path, expected: Option<HostFileWitness>) -> Result<bool> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(_) => return Ok(false),
    };
    if is_reparse(&meta) || !meta.is_file() {
        return Ok(false);
    }
    // Never remove an on-disk file without a matching recorded file identity.
    let Some(expected) = expected else {
        return Ok(false);
    };
    let Ok(file) = open_readonly_no_reparse(path) else {
        return Ok(false);
    };
    let Ok(actual) = file_identity(&file) else {
        return Ok(false);
    };
    drop(file);
    if actual != expected.identity {
        return Ok(false);
    }
    if fs::remove_file(path).is_err() {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        let _ = sync_directory(parent);
    }
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
        meta.is_file() && !meta.file_type().is_symlink(),
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
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_TYPE_DISK, GetFileInformationByHandle, GetFileType,
    };
    let handle = file.as_raw_handle();
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    ensure!(
        unsafe {
            GetFileType(handle) == FILE_TYPE_DISK
                && GetFileInformationByHandle(handle, &mut info) != 0
        },
        "unable to read backup file identity"
    );
    ensure!(
        info.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) == 0,
        "regular backup file required"
    );
    let mut file_id = [0u8; 16];
    file_id[..4].copy_from_slice(&info.nFileIndexHigh.to_be_bytes());
    file_id[4..8].copy_from_slice(&info.nFileIndexLow.to_be_bytes());
    Ok(FileIdentity {
        volume: u64::from(info.dwVolumeSerialNumber),
        file: file_id,
    })
}

#[allow(
    dead_code,
    reason = "reserved for qualified backup custody activation and E4 fixtures"
)]
fn dir_id_hash(dir: &Path) -> Result<Id> {
    let canonical = fs::canonicalize(dir)?;
    let digest = Sha256::digest(canonical.to_string_lossy().as_bytes());
    let mut id = [0u8; 16];
    id.copy_from_slice(&digest[..16]);
    Ok(id)
}

#[allow(
    dead_code,
    reason = "reserved for qualified backup custody activation and E4 fixtures"
)]
fn sync_directory(dir: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        if let Ok(d) = File::open(dir) {
            d.sync_all()?;
        }
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
    }
    Ok(())
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
            if custody
                .cleanup_partial_staging(row.operation_id, row.backup_id, row.witness)
                .unwrap_or(false)
            {
                let proof = PartialCleanupProof {
                    backup_id: row.backup_id,
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
    /// Never returns early after admission without settling the operation and
    /// releasing the project lease.
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
        let Ok(Some(intent)) = db.transact(|tx| store::backup_replay(tx, principal, key)) else {
            return Ok(());
        };
        if intent.status != BackupOpStatus::Pending
            || intent.target != *target
            || intent.generation != session.generation
            || intent.boot != session.wire_boot
        {
            return Ok(());
        }

        let settle_fail = |reason: &str, interrupted: bool, cleaned: bool| -> Result<()> {
            let now = controller.compose_now().unwrap_or(0);
            let _ = db.transact(|tx| {
                store::settle_backup_failed(tx, &intent, reason, interrupted, cleaned, now)
            });
            Ok(())
        };

        let Some(custody) = self.custody() else {
            return settle_fail("backup_custody_unavailable", false, true);
        };
        let boot_live = db
            .transact(|tx| {
                Ok(
                    store::live_boot(tx, target.vm, intent.generation, intent.boot)?
                        && store::boot(tx, principal, target)? == Some(intent.generation),
                )
            })
            .unwrap_or(false);
        if !boot_live {
            return settle_fail("retired_boot", true, true);
        }

        let timeouts = self.timeouts();
        let min_budget = timeouts.idle.min(MIN_EXCHANGE_BUDGET);
        let abs_deadline = Instant::now() + timeouts.absolute;
        let op_hex = hex::encode(intent.id);
        let mut channel = session.channel.lock().await;

        let next_timeout = || -> Option<Duration> {
            let left = abs_deadline.saturating_duration_since(Instant::now());
            if left < min_budget || left.is_zero() {
                None
            } else {
                Some(left.min(timeouts.idle))
            }
        };

        // 1. Query before submit: never blind-retry an operation after a lost reply.
        let Some(t) = next_timeout() else {
            return settle_fail("deadline_exceeded", true, true);
        };
        let query = match channel
            .exchange_with_timeout(cancel, t, |id| Req::ComposeOperation {
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
                        .exchange_with_timeout(cancel, t, |id| Req::ComposeBackupBegin {
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
                        Ok(Resp::ComposeRejected {
                            code: RejectCode::BackupStagingOccupied,
                            ..
                        }) if !retried_stale_staging => {
                            // Clear stale staging left by an earlier terminal
                            // backup on this VM and boot, then retry Begin once.
                            retried_stale_staging = true;
                            let stale_op = db
                                .transact(|tx| {
                                    store::latest_terminal_backup_op_on_boot(
                                        tx,
                                        target.vm,
                                        intent.generation,
                                        intent.boot,
                                        intent.id,
                                    )
                                })
                                .ok()
                                .flatten();
                            let Some(stale_id) = stale_op else {
                                return settle_fail(
                                    reject_label(RejectCode::BackupStagingOccupied),
                                    false,
                                    true,
                                );
                            };
                            let Some(rel_t) = next_timeout() else {
                                return settle_fail("deadline_exceeded", true, true);
                            };
                            let stale_hex = hex::encode(stale_id);
                            match channel
                                .exchange_with_timeout(cancel, rel_t, |id| {
                                    Req::ComposeBackupRelease {
                                        id,
                                        operation_id: stale_hex,
                                    }
                                })
                                .await
                            {
                                Ok(Resp::ComposeBackupReleased { .. }) => continue,
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
                        .exchange_with_timeout(cancel, t, |id| Req::ComposeReconcile {
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
                        .exchange_with_timeout(cancel, t, |id| Req::ComposeOperation {
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
            .exchange_with_timeout(cancel, t, |id| Req::ComposeBackupMeta {
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
                .exchange_with_timeout(cancel, t, |id| Req::ComposeBackupChunk {
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
            Err(PublishError::PrePublish) => {
                // Publication never touched the settled directory; attributed
                // cleanup of the staging leaf may safely release the charge.
                let cleaned = custody
                    .cleanup_partial_staging(intent.id, intent.backup_id, Some(staging_witness))
                    .unwrap_or(false);
                let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
                return settle_fail("verification_or_flush_failed", false, cleaned);
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
        if committed.is_err() {
            // Keep the settled file and charge conservative (partial_uncertain)
            // and record its settled identity so the reconciler can attribute it.
            drop(published.hold);
            let _ = db.transact(|tx| {
                store::record_backup_staging_identity(
                    tx,
                    intent.backup_id,
                    settled_witness.identity.volume,
                    settled_witness.directory_id,
                    settled_witness.identity.file,
                )
            });
            let _ = settle_fail("attribution_commit_failed", true, false);
            let _ = best_effort_release(&mut channel, &op_hex, timeouts.idle).await;
            return Ok(());
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
        .exchange_with_timeout(&local, timeout.min(Duration::from_secs(2)), |id| {
            Req::ComposeBackupRelease {
                id,
                operation_id: operation_id.to_owned(),
            }
        })
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
    fn managed_backup_dir_enforces_size_exclusive_leaf_held_handle_and_restore_disjointness() {
        let state_root = tempfile::tempdir().unwrap();
        let backup_root = crate::service::application::tests::private_temp();
        let src_dir = tempfile::tempdir().unwrap();
        let restore_dir = tempfile::tempdir().unwrap();

        let (bytes, sha) = make_valid_portable_sqlite(src_dir.path());
        let managed = ManagedBackupDir::new(backup_root.path(), state_root.path(), None).unwrap();

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
    }
}
