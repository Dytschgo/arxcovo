use super::{
    ExpiryClaim, Id, RecoveryClaim,
    paths::FileIdentity,
    runtime::{Quiesced, ReaperRuntime, StopDisposition},
};
use std::{
    collections::HashMap,
    future::Future,
    time::{Duration, Instant},
};

const PERIOD: Duration = Duration::from_secs(5);
const WORKER_BUDGET: Duration = Duration::from_secs(1);
const STOP_BUDGET: Duration = Duration::from_secs(2);
const ITEM_BUDGET: Duration = Duration::from_secs(4);

/// A false tick ends the runner. The foreground owns startup and cancellation.
pub trait ReaperTick: Send {
    fn next_tick(&mut self) -> impl Future<Output = bool> + Send;
}

pub struct MonotonicReaperTicks(tokio::time::Interval);

impl Default for MonotonicReaperTicks {
    fn default() -> Self {
        use tokio::time::{Instant, MissedTickBehavior, interval_at};
        let mut interval = interval_at(Instant::now() + PERIOD, PERIOD);
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        Self(interval)
    }
}

impl ReaperTick for MonotonicReaperTicks {
    async fn next_tick(&mut self) -> bool {
        self.0.tick().await;
        true
    }
}

#[cfg(test)]
#[tokio::test(start_paused = true)]
async fn monotonic_tick_starts_after_five_seconds_and_skips_missed_turns() {
    let mut ticks = MonotonicReaperTicks::default();
    assert!(
        tokio::time::timeout(Duration::from_secs(4), ticks.next_tick())
            .await
            .is_err()
    );
    tokio::time::advance(Duration::from_secs(12)).await;
    assert!(ticks.next_tick().await);
    assert!(
        tokio::time::timeout(Duration::from_millis(1), ticks.next_tick())
            .await
            .is_err()
    );
    assert!(ticks.next_tick().await);
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum CleanupKind {
    Build(Id),
    Staging { vm: Id, activity_id: i64 },
    Expiry { vm: Id, activity_id: i64 },
    Recovery { vm: Id, activity_id: i64 },
    Snapshot { vm: Id, generation: Id },
}

impl From<ExpiryClaim> for CleanupKind {
    fn from(claim: ExpiryClaim) -> Self {
        Self::Expiry {
            vm: claim.vm,
            activity_id: claim.activity_id,
        }
    }
}

impl From<RecoveryClaim> for CleanupKind {
    fn from(claim: RecoveryClaim) -> Self {
        Self::Recovery {
            vm: claim.vm,
            activity_id: claim.activity_id,
        }
    }
}

impl CleanupKind {
    pub(crate) fn subject(self) -> Id {
        match self {
            Self::Build(id)
            | Self::Staging { vm: id, .. }
            | Self::Expiry { vm: id, .. }
            | Self::Recovery { vm: id, .. }
            | Self::Snapshot { vm: id, .. } => id,
        }
    }

    pub(crate) fn vm(self) -> Option<Id> {
        (!matches!(self, Self::Build(_))).then(|| self.subject())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CleanupArtifact {
    pub generation: Id,
    pub role: String,
    pub key: Id,
    pub directory: Option<Id>,
    pub identity: Option<FileIdentity>,
    pub retain: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct CleanupWork {
    pub kind: CleanupKind,
    pub pending_worker: Option<i64>,
    pub snapshot_phase: Option<String>,
    pub artifacts: Vec<CleanupArtifact>,
    pub accounted: bool,
    pub release_only: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ReconcileOutcome {
    pub released: bool,
    pub disposed: bool,
}

/// Requires the exact generated slots, including each recorded directory and
/// file identity. A receipt from a fixture is never native custody authority.
pub(crate) trait ArtifactCustody {
    /// A read-only takeover following an exact, fenced no-owner observation.
    fn release_orphan(
        &self,
        _work: &CleanupWork,
        _deadline: Instant,
        _clock: &dyn Fn() -> super::ClockReading,
    ) -> bool {
        false
    }

    fn reconcile(&self, work: &CleanupWork, deadline: Instant) -> bool;
}

pub(super) struct ClosedCustody;

impl ArtifactCustody for ClosedCustody {
    fn reconcile(&self, _: &CleanupWork, _: Instant) -> bool {
        false
    }
}

#[derive(Default)]
pub(super) struct Sweeps {
    retries: HashMap<CleanupKind, (u32, Instant)>,
    workers: HashMap<Id, i64>,
}

impl Sweeps {
    pub fn prepare(
        &mut self,
        db: &rusqlite::Connection,
        items: &mut [CleanupWork],
    ) -> anyhow::Result<()> {
        use crate::store::vm as store;

        // Selection is capped. Remove only retired claims, including routes
        // superseded since the last attempt.
        for kind in self.retries.keys().copied().collect::<Vec<_>>() {
            if !items.iter().any(|work| work.kind == kind)
                && store::cleanup_work(db, kind)?.is_none()
            {
                self.clear_retry(kind);
            }
        }
        for vm in self.workers.keys().copied().collect::<Vec<_>>() {
            if !items.iter().any(|work| work.kind.vm() == Some(vm))
                && store::count(db, "SELECT count(*) FROM vm_instances WHERE id=?1", [vm])? == 0
            {
                self.workers.remove(&vm);
            }
        }
        for work in items {
            self.observe_worker(work);
        }
        Ok(())
    }

    pub fn observe_worker(&mut self, work: &mut CleanupWork) {
        if let Some(vm) = work.kind.vm() {
            if let Some(worker) = work.pending_worker {
                self.workers.insert(vm, worker);
            } else {
                // Caller settlement does not establish worker release.
                work.pending_worker = self.workers.get(&vm).copied();
            }
        }
    }

    pub fn due(&self, kind: CleanupKind, now: Instant) -> bool {
        self.retry_at(kind).is_none_or(|next| now >= next)
    }

    pub fn retry_at(&self, kind: CleanupKind) -> Option<Instant> {
        self.retries.get(&kind).map(|(_, next)| *next)
    }

    pub fn clear_retry(&mut self, kind: CleanupKind) {
        self.retries.remove(&kind);
    }

    pub fn settle(&mut self, work: &CleanupWork, now: Instant, confirmed: bool) {
        let kind = work.kind;
        if confirmed {
            self.retries.remove(&kind);
            if let Some(vm) = kind.vm() {
                self.workers.remove(&vm);
            }
        } else {
            let attempts = self.retries.get(&kind).map_or(0, |(n, _)| (*n + 1).min(4));
            let delay = Duration::from_secs((5 * (1 << attempts)).min(60));
            if let Some(next) = now.checked_add(delay) {
                self.retries.insert(kind, (attempts, next));
            }
        }
    }
}

pub(super) fn release(
    work: &CleanupWork,
    runtime: &impl ReaperRuntime,
    cutoff: Option<Instant>,
    pending_release: bool,
    mut orphan: impl FnMut(Instant) -> bool,
    now: impl Fn() -> Instant,
) -> (bool, Instant) {
    let start = now();
    let (Some(join_by), Some(stop_by), Some(finish_by)) = (
        start.checked_add(WORKER_BUDGET),
        start.checked_add(STOP_BUDGET),
        start.checked_add(ITEM_BUDGET),
    ) else {
        return (false, start);
    };
    let finish_by = cutoff.map_or(finish_by, |limit| limit.min(finish_by));
    let join_by = join_by.min(finish_by);
    let stop_by = stop_by.min(finish_by);
    if start >= finish_by {
        return (false, finish_by);
    }
    let released = match runtime.quiesce(work, join_by) {
        Quiesced::Offline => !pending_release,
        Quiesced::Orphaned => {
            pending_release
                && runtime.with_orphan_fence(work, &mut || now() < finish_by && orphan(finish_by))
        }
        Quiesced::Unconfirmed => false,
        Quiesced::OwnedTestJob
            if matches!(
                work.kind,
                CleanupKind::Expiry { .. } | CleanupKind::Recovery { .. }
            ) && now() < stop_by =>
        {
            let stopped =
                runtime.graceful_stop(work, stop_by) != StopDisposition::ChargedQuarantine;
            stopped
                || ((work.accounted || work.release_only)
                    && matches!(work.kind, CleanupKind::Expiry { .. })
                    && now() < finish_by
                    && runtime.force_reap(work, finish_by) != StopDisposition::ChargedQuarantine)
        }
        Quiesced::OwnedTestJob => false,
    };
    (released && now() < finish_by, finish_by)
}

pub(super) fn dispose(
    work: &CleanupWork,
    custody: &impl ArtifactCustody,
    released: bool,
    finish_by: Instant,
    now: impl Fn() -> Instant,
) -> ReconcileOutcome {
    let disposed = released
        && !work.release_only
        && work.accounted
        && now() < finish_by
        && custody.reconcile(work, finish_by);
    ReconcileOutcome { released, disposed }
}

#[cfg(test)]
pub(crate) mod fixture {
    use super::*;
    use crate::vm::{ArtifactEvidence, ArtifactSlot, paths::ArtifactPin};
    use anyhow::{Result, ensure};
    use sha2::{Digest, Sha256};
    use std::{
        fs, io,
        path::{Path, PathBuf},
    };

    /// A private fixture directory whose writers are controlled and joined by
    /// the test. Pathname deletion here does not qualify native handle custody.
    pub(crate) struct FixtureCustody {
        root: PathBuf,
        handle: fs::File,
        directory: FileIdentity,
    }

    impl FixtureCustody {
        pub(crate) fn open(root: &Path) -> Result<Self> {
            crate::config::checked_path(root)?;
            let handle = open(root, true, false)?;
            let directory = identity(&handle, true)?;
            Ok(Self {
                root: root.to_path_buf(),
                handle,
                directory,
            })
        }

        pub(crate) fn path(&self, role: &str, key: Id) -> PathBuf {
            self.root.join(format!("{role}-{}", hex::encode(key)))
        }

        pub(crate) fn stage(&self, slot: ArtifactSlot, bytes: &[u8]) -> Result<ArtifactEvidence> {
            use std::io::Write;
            let path = self.path(slot.role, slot.key);
            fs::File::create_new(&path)?.write_all(bytes)?;
            let file = open(&path, false, true)?;
            Ok(ArtifactEvidence {
                identity: identity(&file, false)?,
                directory: self.directory.file,
                pin: ArtifactPin {
                    size: bytes.len() as u64,
                    sha256: Sha256::digest(bytes).into(),
                },
            })
        }

        fn check(&self, slot: &CleanupArtifact) -> Result<(FileIdentity, bool)> {
            ensure!(
                matches!(
                    slot.role.as_str(),
                    "disk"
                        | "vars"
                        | "seed"
                        | "verification_overlay"
                        | "static_disk"
                        | "static_vars"
                ),
                "unknown fixture role"
            );
            let recorded = slot
                .identity
                .ok_or_else(|| anyhow::anyhow!("unidentified fixture slot"))?;
            crate::config::checked_path(&self.root)?;
            ensure!(
                slot.directory == Some(self.directory.file)
                    && recorded.volume == self.directory.volume
                    && identity(&self.handle, true)? == self.directory
                    && identity(&open(&self.root, true, false)?, true)? == self.directory,
                "fixture directory changed"
            );
            let path = self.path(&slot.role, slot.key);
            match fs::symlink_metadata(&path) {
                Ok(_) => {
                    crate::config::checked_path(&path)?;
                    ensure!(
                        identity(&open(&path, false, true)?, false)? == recorded,
                        "fixture file changed"
                    );
                    Ok((recorded, true))
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound && !slot.retain => {
                    Ok((recorded, false))
                }
                Err(error) => Err(error.into()),
            }
        }

        fn prove_absent(&self, identities: &[FileIdentity], deadline: Instant) -> Result<()> {
            if identities.is_empty() {
                return Ok(());
            }
            // This walk is evidence only. Deletion uses the exact role paths.
            // Refuse on an incomplete scan, including links or volume changes.
            let mut directories = vec![self.root.clone()];
            let mut remaining = 256usize;
            while let Some(directory) = directories.pop() {
                ensure!(
                    Instant::now() < deadline,
                    "fixture absence deadline exceeded"
                );
                crate::config::checked_path(&directory)?;
                let handle = open(&directory, true, false)?;
                let directory_id = identity(&handle, true)?;
                ensure!(
                    directory_id.volume == self.directory.volume
                        && (directory != self.root || directory_id == self.directory),
                    "fixture directory changed"
                );
                for entry in fs::read_dir(&directory)? {
                    ensure!(
                        remaining > 0 && Instant::now() < deadline,
                        "fixture absence scan exceeded its budget"
                    );
                    remaining -= 1;
                    let entry = entry?;
                    let path = entry.path();
                    let kind = entry.file_type()?;
                    crate::config::checked_path(&path)?;
                    if kind.is_dir() {
                        directories.push(path);
                    } else if kind.is_file() {
                        let actual = identity(&open(&path, false, false)?, false)?;
                        ensure!(
                            !identities.contains(&actual),
                            "fixture artifact identity remains reachable"
                        );
                    }
                }
            }
            Ok(())
        }

        fn remove(&self, work: &CleanupWork, deadline: Instant) -> Result<()> {
            let mut missing = Vec::new();
            let mut disposed = Vec::new();
            for slot in &work.artifacts {
                ensure!(Instant::now() < deadline, "fixture proof deadline exceeded");
                let (recorded, present) = self.check(slot)?;
                if !slot.retain {
                    disposed.push(recorded);
                    if !present {
                        missing.push(recorded);
                    }
                }
            }
            self.prove_absent(&missing, deadline)?;
            for slot in work.artifacts.iter().filter(|slot| !slot.retain) {
                ensure!(
                    Instant::now() < deadline,
                    "fixture removal deadline exceeded"
                );
                if self.check(slot)?.1 {
                    fs::remove_file(self.path(&slot.role, slot.key))?;
                }
                ensure!(!self.check(slot)?.1, "fixture file still present");
            }
            self.prove_absent(&disposed, deadline)?;
            #[cfg(unix)]
            self.handle.sync_all()?;
            Ok(())
        }
    }

    impl ArtifactCustody for FixtureCustody {
        fn reconcile(&self, work: &CleanupWork, deadline: Instant) -> bool {
            self.remove(work, deadline).is_ok()
        }
    }

    fn open(path: &Path, directory: bool, exclusive: bool) -> io::Result<fs::File> {
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let _ = exclusive;
            let flags = libc::O_NOFOLLOW | libc::O_NONBLOCK;
            options.custom_flags(if directory {
                flags | libc::O_DIRECTORY
            } else {
                flags
            });
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::*;
            let directory_flag = if directory {
                FILE_FLAG_BACKUP_SEMANTICS
            } else {
                0
            };
            options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | directory_flag);
            options.share_mode(if exclusive {
                0
            } else {
                FILE_SHARE_READ | FILE_SHARE_WRITE
            });
        }
        options.open(path)
    }

    fn id(value: u64) -> Id {
        let mut id = [0; 16];
        id[8..].copy_from_slice(&value.to_be_bytes());
        id
    }

    #[cfg(unix)]
    fn identity(file: &fs::File, directory: bool) -> Result<FileIdentity> {
        use std::os::unix::fs::MetadataExt;
        let metadata = file.metadata()?;
        ensure!(
            if directory {
                metadata.is_dir()
            } else {
                metadata.is_file() && metadata.nlink() == 1
            },
            "invalid fixture object"
        );
        Ok(FileIdentity {
            volume: metadata.dev(),
            file: id(metadata.ino()),
        })
    }

    #[cfg(windows)]
    fn identity(file: &fs::File, directory: bool) -> Result<FileIdentity> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::*;
        let handle = file.as_raw_handle();
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // The borrowed handle remains open for the synchronous query.
        ensure!(
            unsafe {
                GetFileType(handle) == FILE_TYPE_DISK
                    && GetFileInformationByHandle(handle, &mut info) != 0
            },
            "invalid fixture handle"
        );
        ensure!(
            info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0
                && (info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0) == directory
                && (directory || info.nNumberOfLinks == 1),
            "invalid fixture object"
        );
        let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
        Ok(FileIdentity {
            volume: u64::from(info.dwVolumeSerialNumber),
            file: id(index),
        })
    }
}

#[cfg(test)]
mod reaper_tests {
    use super::{fixture::FixtureCustody, *};
    use crate::{
        service::{Shutdown, application::tests::private_temp},
        store::vm as store,
        vm::*,
    };
    use std::{
        fs,
        sync::{Arc, Barrier, Mutex},
    };

    const NOW: i64 = 1_800_000_000_000;
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * MIB;
    const OWNER: &str = "alice";

    #[derive(Clone)]
    struct TestClock {
        reading: Arc<Mutex<ClockReading>>,
        next_sample: Arc<Mutex<Option<std::sync::mpsc::Sender<()>>>>,
    }

    impl TestClock {
        fn set(&self, utc_ms: i64, seconds: u64) {
            let mut clock = self.reading.lock().unwrap();
            clock.utc_ms = utc_ms;
            clock.monotonic += Duration::from_secs(seconds);
        }
    }

    impl Clock for TestClock {
        fn now(&self) -> ClockReading {
            let reading = *self.reading.lock().unwrap();
            if let Some(sampled) = self.next_sample.lock().unwrap().take() {
                let _ = sampled.send(());
            }
            reading
        }
    }

    struct Fixture {
        ctl: VmController<TestClock>,
        clock: TestClock,
        custody: FixtureCustody,
        root: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            let root = private_temp();
            let clock = TestClock {
                reading: Arc::new(Mutex::new(ClockReading {
                    utc_ms: NOW,
                    monotonic: Instant::now(),
                })),
                next_sample: Default::default(),
            };
            let ctl = VmController::open(root.path(), VmPolicy::default(), clock.clone()).unwrap();
            let custody = FixtureCustody::open(root.path()).unwrap();
            Self {
                ctl,
                clock,
                custody,
                root,
            }
        }

        fn reopen(self) -> Self {
            let Self {
                ctl,
                clock,
                custody,
                root,
            } = self;
            drop(ctl);
            let ctl = VmController::open(root.path(), VmPolicy::default(), clock.clone()).unwrap();
            Self {
                ctl,
                clock,
                custody,
                root,
            }
        }

        fn build(&self, number: u8, publish: bool) -> (Id, PairSlots, [ArtifactEvidence; 2]) {
            let spec = BuildRequest {
                principal: OWNER,
                key: key(NOW, number),
                recipe: "e03a3012c18d20b8691490b580ab41e9",
                digest: [number; 32],
                retained: disk(),
                transient: disk(),
            };
            let BuildAdmission::Fresh(slots, worker, _) = self
                .ctl
                .admit_build(spec, host(), resources(), |_, _| unreachable!())
                .unwrap()
                .unwrap()
            else {
                panic!("fresh build required");
            };
            let entries = [
                slots.pair.disk,
                slots.pair.vars,
                slots.seed,
                slots.verification.disk,
                slots.verification.vars,
            ];
            let staged = entries.map(|slot| self.custody.stage(slot, b"staged").unwrap());
            let evidence: Vec<_> = entries
                .into_iter()
                .zip(staged)
                .flat_map(|(slot, proof)| [(slot, None), (slot, Some(proof))])
                .collect();
            self.ctl
                .build_evidence(&worker, slots.id, &evidence)
                .unwrap();
            let pair = [staged[0], staged[1]];
            if publish {
                for slot in &entries[2..] {
                    fs::remove_file(self.custody.path(slot.role, slot.key)).unwrap();
                }
                assert_eq!(
                    self.ctl.finish_build(&worker, *slots, spec, pair).unwrap(),
                    Status::Succeeded
                );
            }
            (slots.id, slots.pair, pair)
        }

        fn clone(
            &self,
            golden: (Id, PairSlots, [ArtifactEvidence; 2]),
            number: u8,
            publish: bool,
        ) -> (Id, PairSlots) {
            let (vm, slots, worker) = self.reserve_clone(golden, number);
            let roles = [slots.disk, slots.vars];
            let files = self.stage_clone(slots);
            if publish {
                assert_eq!(
                    self.ctl
                        .finish_linked_clone(&worker, slots, Some(files))
                        .unwrap(),
                    Status::Succeeded
                );
            } else {
                // A crash can leave the role evidence committed before the
                // generation pointer, as in the restart recovery contract.
                self.ctl
                    .reaper_test_store()
                    .transact(|db| {
                        for (slot, evidence) in roles.into_iter().zip(files) {
                            store::created(db, slot)?;
                            store::identify(db, slot, evidence)?;
                        }
                        Ok(())
                    })
                    .unwrap();
            }
            (vm, slots)
        }

        fn admit_clone(
            &self,
            golden: (Id, PairSlots, [ArtifactEvidence; 2]),
            number: u8,
        ) -> Result<CloneAdmission, Refusal> {
            let (template, pair, proof) = golden;
            let request = Request {
                principal: OWNER,
                request_id: key(NOW, number),
                vm: [0; 16],
                disk: disk(),
                operation: Operation::Clone {
                    template,
                    lease_seconds: Some(60),
                },
            };
            self.ctl
                .admit_linked_clone(request, host(), resources(), pair, proof)
                .unwrap()
        }

        fn reserve_clone(
            &self,
            golden: (Id, PairSlots, [ArtifactEvidence; 2]),
            number: u8,
        ) -> (Id, PairSlots, PendingOperation) {
            let CloneAdmission::Fresh(vm, slots, worker, _) =
                self.admit_clone(golden, number).unwrap()
            else {
                panic!("fresh clone required");
            };
            (vm, slots, worker)
        }

        fn stage_clone(&self, slots: PairSlots) -> [ArtifactEvidence; 2] {
            [slots.disk, slots.vars].map(|slot| self.custody.stage(slot, b"clone").unwrap())
        }

        fn stage_snapshot(
            &self,
            work: &store::snapshot::Work,
            worker: &PendingOperation,
        ) -> [ArtifactEvidence; 2] {
            let slots = work.candidate.unwrap();
            [slots.disk, slots.vars].map(|slot| {
                self.ctl.snapshot_leaf_begin(worker, work, slot).unwrap();
                let evidence = self.custody.stage(slot, b"clone").unwrap();
                self.ctl
                    .snapshot_leaf_identity(
                        worker,
                        work,
                        slot,
                        evidence.identity,
                        evidence.directory,
                    )
                    .unwrap();
                self.ctl
                    .snapshot_leaf_complete(worker, work, slot, evidence)
                    .unwrap();
                evidence
            })
        }

        fn sweep(&self, runtime: &impl ReaperRuntime) {
            assert!(self.ctl.sweep_with(runtime, &self.custody).unwrap());
        }

        fn count(&self, sql: &str) -> i64 {
            self.ctl
                .reaper_test_store()
                .transact(|db| store::count(db, sql, []))
                .unwrap()
        }

        fn assert_pair(&self, slots: PairSlots, present: bool) {
            for slot in [slots.disk, slots.vars] {
                assert_eq!(self.custody.path(slot.role, slot.key).exists(), present);
            }
        }

        fn worker(&self, vm: Id, number: u8, operation: Operation<'_>) -> PendingOperation {
            match self
                .ctl
                .admit(
                    Request {
                        principal: OWNER,
                        request_id: key(self.clock.now().utc_ms, number),
                        vm,
                        disk: DiskClaim::default(),
                        operation,
                    },
                    host(),
                    resources(),
                )
                .unwrap()
            {
                Admission::Admitted(worker) => worker,
                other => panic!("expected worker, got {other:?}"),
            }
        }
    }

    fn key(utc: i64, number: u8) -> RequestId {
        RequestId::parse(&format!("{utc:016x}{number:016x}")).unwrap()
    }

    fn disk() -> DiskClaim {
        DiskClaim {
            images: MIB,
            metadata: 4096,
            staging: 0,
        }
    }

    fn host() -> HostObservation {
        HostObservation {
            owner_idle: Some(true),
            logical_cpus: Some(8),
            available_ram: Some(32 * GIB),
            capacity: Some(Capacity {
                free_bytes: 400 * GIB,
                managed_bytes: 0,
            }),
        }
    }

    fn resources() -> Resources {
        Resources {
            vcpus: 2,
            guest_ram: 4 * GIB,
            overhead_ram: 256 * MIB,
        }
    }

    struct Runtime<F>(F, Mutex<Vec<&'static str>>);

    impl<F: Fn(&CleanupWork) -> Quiesced> Runtime<F> {
        fn new(quiesce: F) -> Self {
            Self(quiesce, Mutex::default())
        }
    }

    impl<F: Fn(&CleanupWork) -> Quiesced> ReaperRuntime for Runtime<F> {
        fn quiesce(&self, work: &CleanupWork, _: Instant) -> Quiesced {
            self.1.lock().unwrap().push("join");
            (self.0)(work)
        }

        fn graceful_stop(&self, _: &CleanupWork, _: Instant) -> StopDisposition {
            self.1.lock().unwrap().push("graceful");
            StopDisposition::ChargedQuarantine
        }

        fn force_reap(&self, work: &CleanupWork, _: Instant) -> StopDisposition {
            assert!(matches!(work.kind, CleanupKind::Expiry { .. }));
            self.1.lock().unwrap().push("force");
            StopDisposition::DirtyRelease
        }
    }

    fn offline(_: &CleanupWork) -> Quiesced {
        Quiesced::Offline
    }

    fn vault_clone(f: &Fixture) -> Id {
        let (template, pair, proof) = f.build(51, true);
        let request = Request {
            principal: "arxd",
            request_id: key(NOW, 52),
            vm: [0; 16],
            disk: disk(),
            operation: Operation::Clone {
                template,
                lease_seconds: Some(60),
            },
        };
        let CloneAdmission::Fresh(vm, slots, worker, _) = f
            .ctl
            .admit_linked_clone(request, host(), resources(), pair, proof)
            .unwrap()
            .unwrap()
        else {
            panic!("fresh vault clone required");
        };
        assert_eq!(
            f.ctl
                .finish_linked_clone(&worker, slots, Some(f.stage_clone(slots)))
                .unwrap(),
            Status::Succeeded,
        );
        f.ctl.expire_vault_test(vm).unwrap();
        vm
    }

    #[test]
    fn vault_custody_budget_starts_only_after_acquisition() {
        let f = Fixture::new();
        let vm = vault_clone(&f);
        let runtime = Runtime::new(offline);
        let fenced = std::sync::atomic::AtomicBool::new(false);
        let mut turn =
            f.ctl
                .prioritize_vault_sweep(vm, Duration::from_secs(20), Duration::from_secs(15));
        f.ctl.hold_sweeps_for_test(|| {
            assert_eq!(
                f.ctl
                    .sweep_vault_test_with(vm, &runtime, &f.custody, &mut turn, &fenced)
                    .unwrap(),
                None
            );
            f.clock.set(NOW, 16);
        });
        assert_eq!(
            f.ctl
                .sweep_vault_test_with(vm, &runtime, &f.custody, &mut turn, &fenced)
                .unwrap(),
            Some(VaultTestCleanup::Confirmed)
        );
        assert!(f.ctl.vault_test_released(vm).unwrap());
    }

    #[test]
    fn vault_proof_requires_both_attribution_and_zero_charge() {
        let f = Fixture::new();
        let vm = vault_clone(&f);
        // A confirmed receipt beside a still-charged VM is insufficient.
        f.ctl.reaper_test_store().transact(|db| {
            db.execute("INSERT INTO activity(principal,vm,action,started_ms,status,finished_ms,terminal_order,
                cleanup_confirmed,actor,vm_owner) VALUES('arxd',?1,7,?2,1,?2,
                (SELECT coalesce(max(terminal_order),0)+1 FROM activity),1,'daemon','arxd')", rusqlite::params![vm,NOW])?;
            db.execute("INSERT INTO vm_tombstones VALUES(?1,'arxd',last_insert_rowid(),?2)", rusqlite::params![vm,NOW])?;
            Ok(())
        }).unwrap();
        assert!(!f.ctl.vault_test_released(vm).unwrap());
        f.ctl
            .reaper_test_store()
            .transact(|db| {
                db.execute("DELETE FROM vm_disk_claims WHERE vm=?1", [vm])?;
                Ok(())
            })
            .unwrap();
        assert!(f.ctl.vault_test_released(vm).unwrap());
        f.ctl
            .reaper_test_store()
            .transact(|db| {
                db.execute(
                    "UPDATE vm_tombstones SET principal='alice' WHERE id=?1",
                    [vm],
                )?;
                Ok(())
            })
            .unwrap();
        assert!(!f.ctl.vault_test_released(vm).unwrap());
    }

    #[test]
    fn vault_lock_timeout_does_not_claim_a_teardown_failure() {
        let f = Fixture::new();
        let vm = vault_clone(&f);
        let runtime = Runtime::new(offline);
        let fenced = std::sync::atomic::AtomicBool::new(false);
        let mut turn =
            f.ctl
                .prioritize_vault_sweep(vm, Duration::from_secs(6), Duration::from_secs(15));
        f.ctl.hold_sweeps_for_test(|| {
            assert_eq!(
                f.ctl
                    .sweep_vault_test_with(vm, &runtime, &f.custody, &mut turn, &fenced)
                    .unwrap(),
                None
            );
            fenced.store(true, std::sync::atomic::Ordering::SeqCst);
            assert_eq!(
                f.ctl
                    .sweep_vault_test_with(vm, &runtime, &f.custody, &mut turn, &fenced)
                    .unwrap(),
                Some(VaultTestCleanup::Inconclusive)
            );
            fenced.store(false, std::sync::atomic::Ordering::SeqCst);
            f.clock.set(NOW, 6);
            assert_eq!(
                f.ctl
                    .sweep_vault_test_with(vm, &runtime, &f.custody, &mut turn, &fenced)
                    .unwrap(),
                Some(VaultTestCleanup::Inconclusive)
            );
        });
        assert!(runtime.1.lock().unwrap().is_empty());
        assert!(!f.ctl.vault_test_released(vm).unwrap());
    }

    #[test]
    fn vault_backoff_retries_when_due_within_custody_budget() {
        let f = Fixture::new();
        let vm = vault_clone(&f);
        f.sweep(&Runtime::new(|_| Quiesced::Unconfirmed));
        let runtime = Runtime::new(offline);
        let fenced = std::sync::atomic::AtomicBool::new(false);
        {
            let mut short =
                f.ctl
                    .prioritize_vault_sweep(vm, Duration::from_secs(6), Duration::from_secs(4));
            assert_eq!(
                f.ctl
                    .sweep_vault_test_with(vm, &runtime, &f.custody, &mut short, &fenced)
                    .unwrap(),
                Some(VaultTestCleanup::Inconclusive)
            );
        }
        let mut turn =
            f.ctl
                .prioritize_vault_sweep(vm, Duration::from_secs(6), Duration::from_secs(15));
        assert_eq!(
            f.ctl
                .sweep_vault_test_with(vm, &runtime, &f.custody, &mut turn, &fenced)
                .unwrap(),
            None
        );
        assert!(runtime.1.lock().unwrap().is_empty());
        f.clock.set(NOW, 5);
        assert_eq!(
            f.ctl
                .sweep_vault_test_with(vm, &runtime, &f.custody, &mut turn, &fenced)
                .unwrap(),
            Some(VaultTestCleanup::Confirmed)
        );
    }

    #[test]
    fn vault_retries_its_own_unconfirmed_attempt_at_eligibility() {
        let f = Fixture::new();
        let vm = vault_clone(&f);
        let fenced = std::sync::atomic::AtomicBool::new(false);
        let mut turn =
            f.ctl
                .prioritize_vault_sweep(vm, Duration::from_secs(6), Duration::from_secs(15));
        let failed = Runtime::new(|_| Quiesced::Unconfirmed);
        let ready = Runtime::new(offline);
        assert_eq!(
            f.ctl
                .sweep_vault_test_with(vm, &failed, &f.custody, &mut turn, &fenced)
                .unwrap(),
            None
        );
        f.clock.set(NOW, 4);
        assert_eq!(
            f.ctl
                .sweep_vault_test_with(vm, &ready, &f.custody, &mut turn, &fenced)
                .unwrap(),
            None
        );
        assert!(ready.1.lock().unwrap().is_empty());
        f.clock.set(NOW, 1);
        assert_eq!(
            f.ctl
                .sweep_vault_test_with(vm, &ready, &f.custody, &mut turn, &fenced)
                .unwrap(),
            Some(VaultTestCleanup::Confirmed)
        );
    }

    #[test]
    fn vault_attempt_still_unconfirmed_at_cutoff_retains_charge() {
        let f = Fixture::new();
        let vm = vault_clone(&f);
        let fenced = std::sync::atomic::AtomicBool::new(false);
        let mut turn =
            f.ctl
                .prioritize_vault_sweep(vm, Duration::from_secs(6), Duration::from_secs(15));
        let slow = Runtime::new(|_| {
            f.clock.set(NOW, 15);
            Quiesced::Unconfirmed
        });
        assert_eq!(
            f.ctl
                .sweep_vault_test_with(vm, &slow, &f.custody, &mut turn, &fenced)
                .unwrap(),
            Some(VaultTestCleanup::Unconfirmed)
        );
        assert!(!f.ctl.vault_test_released(vm).unwrap());
        assert!(f.count("SELECT count(*) FROM vm_disk_claims") > 0);
    }

    #[test]
    fn vault_proof_arriving_at_cutoff_cannot_pass() {
        struct Late<'a>(&'a Fixture);
        impl ArtifactCustody for Late<'_> {
            fn reconcile(&self, work: &CleanupWork, deadline: Instant) -> bool {
                let disposed = self.0.custody.reconcile(work, deadline);
                self.0.clock.set(NOW, 15);
                disposed
            }
        }

        let f = Fixture::new();
        let vm = vault_clone(&f);
        let mut turn =
            f.ctl
                .prioritize_vault_sweep(vm, Duration::from_secs(6), Duration::from_secs(15));
        let result = f
            .ctl
            .sweep_vault_test_with(
                vm,
                &Runtime::new(offline),
                &Late(&f),
                &mut turn,
                &std::sync::atomic::AtomicBool::new(false),
            )
            .unwrap();
        assert!(f.ctl.vault_test_released(vm).unwrap());
        assert_eq!(result, Some(VaultTestCleanup::Unconfirmed));
    }

    #[test]
    fn vault_owed_tick_arriving_during_custody_runs_after_release() {
        let f = Fixture::new();
        let vm = vault_clone(&f);
        let mut turn =
            f.ctl
                .prioritize_vault_sweep(vm, Duration::from_secs(6), Duration::from_secs(15));
        let ordinary = Runtime::new(offline);
        assert!(!f.ctl.sweep_with(&ordinary, &f.custody).unwrap());
        let (entered, in_custody) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let vault_runtime = Runtime::new(move |_: &CleanupWork| {
            entered.send(()).unwrap();
            released.recv_timeout(Duration::from_secs(3)).unwrap();
            Quiesced::Offline
        });
        let fixture = &f;
        std::thread::scope(|scope| {
            let result = scope.spawn(move || {
                fixture
                    .ctl
                    .sweep_vault_test_with(
                        vm,
                        &vault_runtime,
                        &fixture.custody,
                        &mut turn,
                        &std::sync::atomic::AtomicBool::new(false),
                    )
                    .unwrap()
            });
            in_custody.recv_timeout(Duration::from_secs(3)).unwrap();
            let (done, tick) = std::sync::mpsc::channel();
            scope.spawn(move || {
                done.send(fixture.ctl.sweep_with(&ordinary, &fixture.custody).unwrap())
                    .unwrap();
            });
            // The owed tick must remain pending until the active item releases custody.
            let early = tick.recv_timeout(Duration::from_millis(100));
            release.send(()).unwrap();
            assert!(matches!(
                early,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ));
            assert_eq!(result.join().unwrap(), Some(VaultTestCleanup::Confirmed));
            assert!(tick.recv_timeout(Duration::from_secs(3)).unwrap());
        });
    }

    #[test]
    fn vault_backoff_survives_omission_from_ordinary_batch() {
        let f = Fixture::new();
        let vm = vault_clone(&f);
        let work = f
            .ctl
            .reaper_test_store()
            .transact(|db| {
                let claim = store::vault_test_expiry(db, vm, NOW)?.unwrap();
                Ok(store::cleanup_work(db, claim.into())?.unwrap())
            })
            .unwrap();
        let mut sweeps = Sweeps::default();
        let now = f.clock.now().monotonic;
        sweeps.settle(&work, now, false);
        f.ctl
            .reaper_test_store()
            .transact(|db| sweeps.prepare(db, &mut []))
            .unwrap();
        assert_eq!(
            sweeps.retry_at(work.kind),
            Some(now + Duration::from_secs(5))
        );
    }

    #[test]
    fn review_unconfirmed_sweep_settles_the_losing_receipt_once() {
        let f = Fixture::new();
        let (vm, slots) = f.clone(f.build(1, true), 2, true);
        let start = f.worker(vm, 3, Operation::Start);
        f.ctl.finish(&start, Completion::Succeeded).unwrap();
        let stop = f.worker(vm, 4, Operation::Stop);
        let receipt = || {
            f.ctl
                .reaper_test_store()
                .transact(|db| {
                    Ok(db.query_row(
                        "SELECT a.status, a.cleanup_confirmed, a.terminal_order,
                            o.status, o.cleanup_confirmed
                     FROM activity a JOIN vm_operations o ON o.activity_id=a.id WHERE a.id=?1",
                        [stop.activity_id()],
                        |row| {
                            Ok((
                                row.get::<_, i64>(0)?,
                                row.get::<_, Option<bool>>(1)?,
                                row.get::<_, Option<i64>>(2)?,
                                row.get::<_, i64>(3)?,
                                row.get::<_, Option<bool>>(4)?,
                            ))
                        },
                    )?)
                })
                .unwrap()
        };
        f.clock.set(NOW + 60_000, 0);
        f.sweep(&Runtime::new(|_| Quiesced::Unconfirmed));
        let terminal = receipt();
        assert!(
            matches!(terminal, (3, Some(false), Some(_), 3, Some(false))),
            "{terminal:?}"
        );
        f.assert_pair(slots, true);
        assert_eq!(f.count("SELECT count(*) FROM vm_disk_claims"), 1);
        f.clock.set(NOW + 60_000, 5);
        f.sweep(&Runtime::new(|work| {
            assert_eq!(work.pending_worker, Some(stop.activity_id()));
            Quiesced::Offline
        }));
        assert_eq!(receipt(), terminal);
        assert!(f.ctl.expired_status(OWNER, vm).unwrap().is_some());
    }

    #[test]
    fn review_renamed_file_keeps_the_charge_and_survives_retry() {
        let f = Fixture::new();
        let (vm, slots) = f.clone(f.build(1, true), 2, true);
        let original = f.custody.path(slots.disk.role, slots.disk.key);
        let sibling = f.root.path().join("unmanaged");
        fs::create_dir(&sibling).unwrap();
        let renamed = sibling.join("renamed-disk");
        fs::rename(&original, &renamed).unwrap();
        f.clock.set(NOW + 60_000, 0);
        f.sweep(&Runtime::new(offline));
        assert_eq!(
            f.count("SELECT count(*) FROM vm_disk_claims"),
            1,
            "rename released the charge"
        );
        assert!(f.ctl.expired_status(OWNER, vm).unwrap().is_none());
        assert_eq!(fs::read(&renamed).unwrap(), b"clone");
        assert!(f.custody.path(slots.vars.role, slots.vars.key).exists());
        fs::rename(&renamed, &original).unwrap();
        f.clock.set(NOW + 60_000, 5);
        f.sweep(&Runtime::new(offline));
        f.assert_pair(slots, false);
        assert!(f.ctl.expired_status(OWNER, vm).unwrap().is_some());
    }

    #[test]
    fn review_sweep_claims_due_creating_clone_before_worker_returns() {
        let f = Fixture::new();
        let (vm, slots, worker) = f.reserve_clone(f.build(1, true), 2);
        let proof = f.stage_clone(slots);
        f.clock.set(NOW + 61_000, 1);
        assert!(f.clock.now().monotonic < worker.deadline());
        f.sweep(&Runtime::new(|work| {
            assert!(matches!(work.kind, CleanupKind::Staging { vm: claimed, .. } if claimed == vm));
            assert_eq!(work.pending_worker, Some(worker.activity_id()));
            Quiesced::Unconfirmed
        }));
        assert_eq!(
            f.count("SELECT count(*) FROM vm_instances WHERE cleanup_route='staging'"),
            1
        );
        f.clock.set(NOW, 0);
        assert_eq!(
            f.ctl
                .finish_linked_clone(&worker, slots, Some(proof))
                .unwrap(),
            Status::Interrupted
        );
        assert_eq!(
            f.count("SELECT count(*) FROM vm_generations WHERE vm_id IS NOT NULL AND active=1"),
            0
        );
        f.clock.set(NOW, 5);
        f.sweep(&Runtime::new(offline));
        assert_eq!(f.count("SELECT count(*) FROM vm_disk_claims"), 0);
        assert!(f.ctl.expired_status(OWNER, vm).unwrap().is_none());
        f.assert_pair(slots, false);
    }

    #[test]
    fn review_due_clone_cannot_publish_without_a_sweep() {
        let f = Fixture::new();
        let (vm, slots, worker) = f.reserve_clone(f.build(1, true), 2);
        let proof = f.stage_clone(slots);
        f.clock.set(NOW + 60_000, 1);
        assert!(f.clock.now().monotonic < worker.deadline());
        assert_eq!(
            f.ctl
                .finish_linked_clone(&worker, slots, Some(proof))
                .unwrap(),
            Status::Interrupted
        );
        assert_eq!(
            f.count("SELECT count(*) FROM vm_instances WHERE cleanup_route='staging'"),
            1
        );
        f.sweep(&Runtime::new(offline));
        f.assert_pair(slots, false);
        assert!(f.ctl.expired_status(OWNER, vm).unwrap().is_none());
    }

    #[test]
    fn review_failed_clone_fences_fresh_admission_without_a_tick() {
        let f = Fixture::new();
        let golden = f.build(1, true);
        let (_, slots, worker) = f.reserve_clone(golden, 2);
        assert_eq!(
            f.ctl.finish_linked_clone(&worker, slots, None).unwrap(),
            Status::Interrupted
        );
        assert!(matches!(
            f.admit_clone(golden, 2),
            Ok(CloneAdmission::Existing(_))
        ));
        assert!(matches!(f.admit_clone(golden, 3), Err(Refusal::Shutdown)));
        assert_eq!(f.count("SELECT count(*) FROM vm_disk_claims"), 1);
    }

    #[test]
    fn review_sealed_build_reuse_checks_staging_fence_before_verifying() {
        let f = Fixture::new();
        let golden = f.build(1, true);
        let (_, slots, worker) = f.reserve_clone(golden, 2);
        assert_eq!(
            f.ctl.finish_linked_clone(&worker, slots, None).unwrap(),
            Status::Interrupted
        );
        let verifications = std::cell::Cell::new(0);
        let admit = |number| {
            f.ctl
                .admit_build(
                    BuildRequest {
                        principal: OWNER,
                        key: key(NOW, number),
                        recipe: "e03a3012c18d20b8691490b580ab41e9",
                        digest: [1; 32],
                        retained: disk(),
                        transient: disk(),
                    },
                    host(),
                    resources(),
                    |_, _| {
                        verifications.set(verifications.get() + 1);
                        Some(golden.2)
                    },
                )
                .unwrap()
        };
        assert!(matches!(
            admit(1),
            Ok(BuildAdmission::Existing(receipt)) if receipt.receipt.status == Status::Succeeded
        ));
        let fresh = admit(3);
        assert!(matches!(fresh, Err(Refusal::Shutdown)), "{fresh:?}");
        assert_eq!(verifications.get(), 0);
        assert_eq!(
            f.count("SELECT count(*) FROM vm_operations WHERE action=11"),
            1
        );
    }

    #[test]
    fn review_failed_staging_refusals_distinguish_the_live_lease() {
        let f = Fixture::new();
        let (vm, slots, worker) = f.reserve_clone(f.build(1, true), 2);
        assert_eq!(
            f.ctl.finish_linked_clone(&worker, slots, None).unwrap(),
            Status::Interrupted
        );
        let change = f.ctl.extend_lease(OWNER, vm, key(NOW, 3), 120).unwrap();
        let action = f
            .ctl
            .admit(
                Request {
                    principal: OWNER,
                    request_id: key(NOW, 4),
                    vm,
                    disk: DiskClaim::default(),
                    operation: Operation::Start,
                },
                host(),
                resources(),
            )
            .unwrap();
        assert_eq!(change, Err(Refusal::CleanupUnconfirmed));
        assert!(matches!(
            action,
            Admission::Refused(Refusal::CleanupUnconfirmed)
        ));
    }

    #[test]
    fn restart_sweep_releases_failed_build_and_clone_without_expiring_them() {
        let f = Fixture::new();
        let golden = f.build(1, true);
        let (leased, leased_pair) = f.clone(golden, 2, true);
        let (_, build_pair, _) = f.build(4, false);
        let f = f.reopen();
        let seen = Mutex::new(Vec::new());
        let runtime = Runtime::new(|work| {
            seen.lock().unwrap().push(work.kind);
            offline(work)
        });
        f.sweep(&runtime);
        assert_eq!(
            f.count("SELECT count(*) FROM vm_build_intents WHERE state<>'sealed'"),
            0
        );
        let (failed, failed_pair) = f.clone(golden, 3, false);
        f.clock.set(NOW + 61_000, 0);
        let f = f.reopen();
        f.sweep(&runtime);
        assert!(matches!(seen.lock().unwrap().as_slice(),
            [CleanupKind::Build(_), CleanupKind::Staging { vm: staged, .. },
                CleanupKind::Expiry { vm: expired, .. }] if *staged == failed && *expired == leased));
        assert_eq!(f.count("SELECT count(*) FROM vm_disk_claims"), 0);
        assert_eq!(f.count("SELECT count(*) FROM vm_templates"), 1);
        assert_eq!(f.count("SELECT count(*) FROM vm_tombstones"), 1);
        assert!(f.ctl.expired_status(OWNER, leased).unwrap().is_some());
        assert!(f.ctl.expired_status(OWNER, failed).unwrap().is_none());
        assert!(f.ctl.expired_status("bob", leased).unwrap().is_none());
        assert_eq!(
            f.count(
                "SELECT count(*) FROM activity WHERE principal='arxd' AND
            actor='daemon' AND action IN (7,10) AND cleanup_confirmed=1"
            ),
            2
        );
        f.assert_pair(golden.1, true);
        f.assert_pair(build_pair, false);
        f.assert_pair(failed_pair, false);
        f.assert_pair(leased_pair, false);
        assert_eq!(fs::read_dir(f.root.path()).unwrap().count(), 4);
    }

    #[test]
    fn reaper_proof_failure_keeps_charge_until_monotonic_retry() {
        let f = Fixture::new();
        let golden = f.build(1, true);
        let (vm, pair) = f.clone(golden, 2, true);
        let protected = ["static-disk", "backup", "input.iso", "disk-unmanaged"];
        for name in protected {
            fs::write(f.root.path().join(name), b"protected").unwrap();
        }
        let disk = f.custody.path(pair.disk.role, pair.disk.key);
        let saved = f.root.path().join("held-fixture-original");
        fs::rename(&disk, &saved).unwrap();
        fs::hard_link(f.root.path().join(protected[0]), &disk).unwrap();
        f.clock.set(NOW + 60_000, 0);

        let runtime = Runtime::new(offline);
        f.sweep(&runtime);
        assert_eq!(
            f.count("SELECT count(*) FROM activity WHERE action=7 AND cleanup_confirmed=0"),
            1
        );
        assert_eq!(f.count("SELECT count(*) FROM vm_disk_claims"), 1);
        assert!(f.ctl.expired_status(OWNER, vm).unwrap().is_none());
        f.assert_pair(pair, true);
        let claim = f.ctl.expiry_intents().unwrap()[0];
        assert!(
            f.ctl
                .finish_expiry(claim, CleanupResult::Confirmed)
                .is_err()
        );
        fs::remove_file(&disk).unwrap();
        fs::rename(&saved, &disk).unwrap();
        // An exact role already removed before a metadata commit is safe to retry.
        fs::remove_file(f.custody.path(pair.vars.role, pair.vars.key)).unwrap();
        f.clock.set(NOW + 10_000, 4);
        assert_eq!(
            f.ctl.extend_lease(OWNER, vm, key(NOW, 3), 60).unwrap(),
            Err(Refusal::Expired)
        );
        f.sweep(&runtime);
        assert_eq!(runtime.1.lock().unwrap().len(), 1);
        f.clock.set(NOW + 5_000, 1);
        f.sweep(&runtime);
        assert_eq!(
            f.ctl
                .expired_status(OWNER, vm)
                .unwrap()
                .unwrap()
                .activity_id,
            claim.activity_id
        );
        assert_eq!(f.count("SELECT count(*) FROM vm_disk_claims"), 0);
        f.assert_pair(pair, false);
        f.assert_pair(golden.1, true);
        for name in protected {
            assert_eq!(fs::read(f.root.path().join(name)).unwrap(), b"protected");
        }
    }

    #[test]
    fn expiry_joins_workers_before_removal_and_fences_late_script_and_stop() {
        let f = Fixture::new();
        let golden = f.build(1, true);
        let operations = [
            Operation::Script {
                script: "exit 0",
                timeout_seconds: 30,
            },
            Operation::Stop,
        ];
        for (index, operation) in operations.into_iter().enumerate() {
            let number = 2 + 3 * index as u8;
            let (vm, pair) = f.clone(golden, number, true);
            let start = f.worker(vm, number + 1, Operation::Start);
            f.ctl.finish(&start, Completion::Succeeded).unwrap();
            let is_script = matches!(operation, Operation::Script { .. });
            let worker = f.worker(vm, number + 2, operation);
            f.clock.set(f.clock.now().utc_ms + 60_000, 0);
            let runtime = Runtime::new(|work| {
                f.assert_pair(pair, true);
                assert_eq!(work.pending_worker, Some(worker.activity_id()));
                let outcome = if is_script {
                    f.ctl.finish_script(&worker, ScriptResult::Exited(0))
                } else {
                    f.ctl.finish(&worker, Completion::Succeeded)
                };
                assert_eq!(outcome.unwrap(), Status::Interrupted);
                Quiesced::OwnedTestJob
            });
            f.sweep(&runtime);
            assert_eq!(*runtime.1.lock().unwrap(), ["join", "graceful", "force"]);
            f.assert_pair(pair, false);
        }
        assert_eq!(
            f.count("SELECT count(*) FROM vm_operations WHERE action IN (2,4) AND status=3"),
            2
        );
        assert_eq!(
            f.count(
                "SELECT count(*) FROM vm_operations WHERE script_result='interrupted'
                AND script_exit_code IS NULL AND cleanup_confirmed=0"
            ),
            1
        );
    }

    struct Ticks<'a>(&'a TestClock, std::vec::IntoIter<i64>);

    impl ReaperTick for Ticks<'_> {
        async fn next_tick(&mut self) -> bool {
            self.1.next().is_some_and(|now| {
                self.0.set(now, 5);
                true
            })
        }
    }

    #[tokio::test]
    async fn periodic_ticks_claim_only_after_utc_moves_forward() {
        let f = Fixture::new();
        let golden = f.build(1, true);
        let (vm, pair) = f.clone(golden, 2, true);
        let runtime = Runtime::new(|work| {
            assert_eq!(f.clock.now().utc_ms, NOW + 100_000);
            offline(work)
        });
        f.sweep(&runtime);
        f.ctl
            .run_reaper_with(
                &runtime,
                &f.custody,
                &mut Ticks(
                    &f.clock,
                    vec![NOW + 30_000, NOW - 1_000, NOW + 100_000, NOW].into_iter(),
                ),
                &Shutdown::default(),
            )
            .await
            .unwrap();
        assert_eq!(*runtime.1.lock().unwrap(), ["join"]);
        assert_eq!(
            f.ctl
                .expired_status(OWNER, vm)
                .unwrap()
                .unwrap()
                .expired_at_ms,
            NOW + 100_000
        );
        f.assert_pair(pair, false);
    }

    #[test]
    fn competing_sweep_skips_while_worker_is_joining() {
        let f = Fixture::new();
        let golden = f.build(1, true);
        let (vm, pair) = f.clone(golden, 2, true);
        f.clock.set(NOW + 60_000, 0);
        let entered = Barrier::new(2);
        let resume = Barrier::new(2);
        let runtime = Runtime::new(|_| {
            entered.wait();
            resume.wait();
            Quiesced::Offline
        });
        std::thread::scope(|scope| {
            let sweep = scope.spawn(|| f.sweep(&runtime));
            entered.wait();
            let skipped = f.ctl.sweep_with(&runtime, &f.custody).unwrap();
            let still_present = f.custody.path(pair.disk.role, pair.disk.key).exists();
            resume.wait();
            assert!(!skipped);
            assert!(still_present);
            sweep.join().unwrap();
        });
        assert_eq!(*runtime.1.lock().unwrap(), ["join"]);
        assert!(f.ctl.expired_status(OWNER, vm).unwrap().is_some());
    }

    #[test]
    fn close_allows_pending_worker_settlement_before_final_recovery() {
        let f = Fixture::new();
        let golden = f.build(1, true);
        let (vm, pair) = f.clone(golden, 2, true);
        let start = f.worker(vm, 3, Operation::Start);
        f.ctl.close_admission().unwrap();
        assert!(f.ctl.recovery_intents().unwrap().is_empty());
        assert_eq!(
            f.ctl.finish(&start, Completion::Succeeded).unwrap(),
            Status::Succeeded
        );
        f.ctl.recover_after_drain().unwrap();
        assert_eq!(f.ctl.recovery_intents().unwrap()[0].vm, vm);
        assert_eq!(
            f.count("SELECT count(*) FROM vm_operations WHERE action=3 AND status=1"),
            1
        );
        f.sweep(&Runtime::new(offline));
        assert!(f.ctl.recovery_intents().unwrap().is_empty());
        assert_eq!(
            f.count("SELECT count(*) FROM vm_instances WHERE state=1"),
            1
        );
        assert_eq!(f.count("SELECT count(*) FROM vm_disk_claims"), 1);
        f.assert_pair(pair, true);
    }

    #[test]
    fn crash_before_snapshot_identity_preserves_each_leafs_charge_and_custody() {
        for disk_state in ["planned", "identified", "unknown"] {
            let f = Fixture::new();
            let golden = f.build(1, true);
            let (vm, _) = f.clone(golden, 2, true);
            let bytes = |f: &Fixture| {
                f.ctl
                    .reaper_test_store()
                    .transact(|db| store::disk_bytes(db))
                    .unwrap()
                    .unwrap()
            };
            let original = bytes(&f);
            let (work, worker, op) = f
                .ctl
                .admit_snapshot(
                    OWNER,
                    key(NOW, 10),
                    vm,
                    Action::Snapshot as i64,
                    &snapshot::Request::Create(None),
                    None,
                    host(),
                    resources(),
                )
                .unwrap()
                .unwrap();
            let charged = bytes(&f);
            assert!(charged > original);
            let slot = work.candidate.unwrap().disk;
            let path = f.custody.path(slot.role, slot.key);
            if disk_state != "planned" {
                f.ctl.snapshot_leaf_begin(&worker, &work, slot).unwrap();
                let receipt = f.custody.stage(slot, b"clone").unwrap();
                if disk_state == "identified" {
                    f.ctl
                        .snapshot_leaf_identity(
                            &worker,
                            &work,
                            slot,
                            receipt.identity,
                            receipt.directory,
                        )
                        .unwrap();
                }
            }
            // Drop the writer before any failure handler, shutdown drain or sweep.
            let f = f.reopen();
            assert_eq!(
                f.ctl.operation_status(OWNER, op.id).unwrap().unwrap().state,
                Status::Interrupted
            );
            assert_eq!(bytes(&f), charged);
            f.sweep(&Runtime::new(offline));
            let unknown = disk_state == "unknown";
            assert_eq!(path.exists(), unknown);
            assert_eq!(bytes(&f), if unknown { charged } else { original });
            assert_eq!(
                f.count("SELECT count(*) FROM vm_snapshot_work"),
                i64::from(unknown)
            );
            assert_eq!(
                f.count("SELECT count(*) FROM vm_snapshot_reconciliation"),
                1
            );
            assert_eq!(
                f.count(
                    "SELECT count(*) FROM vm_snapshot_reconciliation WHERE cause='quarantined'"
                ),
                i64::from(unknown)
            );
        }
    }

    #[test]
    fn release_only_expiry_never_calls_artifact_custody() {
        struct Probe;
        impl ArtifactCustody for Probe {
            fn reconcile(&self, _: &CleanupWork, _: Instant) -> bool {
                panic!("release-only expiry called artifact custody")
            }
        }

        let work = CleanupWork {
            kind: CleanupKind::Expiry {
                vm: [8; 16],
                activity_id: 1,
            },
            pending_worker: None,
            snapshot_phase: None,
            artifacts: Vec::new(),
            accounted: false,
            release_only: true,
        };
        let (released, finish_by) = release(
            &work,
            &Runtime::new(offline),
            None,
            false,
            |_| false,
            Instant::now,
        );
        let outcome = dispose(&work, &Probe, released, finish_by, Instant::now);
        assert!(outcome.released);
        assert!(!outcome.disposed);
    }

    #[test]
    fn claimed_orphan_copy_cannot_publish_even_when_disposal_fails() {
        struct RefuseDisposal<F>(F);
        impl<F: Fn()> ArtifactCustody for RefuseDisposal<F> {
            fn reconcile(&self, _: &CleanupWork, _: Instant) -> bool {
                (self.0)();
                false
            }
        }
        let f = Fixture::new();
        let (vm, _) = f.clone(f.build(1, true), 2, true);
        let (work, worker, op) = f
            .ctl
            .admit_snapshot(
                OWNER,
                key(NOW, 10),
                vm,
                Action::Snapshot as i64,
                &snapshot::Request::Create(None),
                None,
                host(),
                resources(),
            )
            .unwrap()
            .unwrap();
        let proof = f.stage_snapshot(&work, &worker);
        let attempted = std::cell::Cell::new(false);
        let custody = RefuseDisposal(|| {
            attempted.set(true);
            let err = f
                .ctl
                .finish_snapshot_copy(&worker, &work, proof, None, host(), resources())
                .unwrap_err();
            assert_eq!(err.to_string(), "snapshot commit fenced");
        });
        assert!(f.ctl.sweep_with(&Runtime::new(offline), &custody).unwrap());
        assert!(attempted.get());
        assert_eq!(
            f.ctl.operation_status(OWNER, op.id).unwrap().unwrap().state,
            Status::Interrupted
        );
        assert_eq!(f.count("SELECT count(*) FROM vm_restore_points"), 0);
        assert_eq!(
            f.count("SELECT count(*) FROM vm_snapshot_work WHERE phase='abandoned'"),
            1
        );
        f.assert_pair(work.candidate.unwrap(), true);
    }

    #[test]
    fn snapshot_restore_waiting_for_sql_writer_cannot_publish_after_deadline() {
        let f = Fixture::new();
        let (vm, active) = f.clone(f.build(1, true), 2, true);
        let admit = |nonce, request| {
            f.ctl
                .admit_snapshot(
                    OWNER,
                    key(NOW, nonce),
                    vm,
                    match request {
                        snapshot::Request::Create(_) => Action::Snapshot,
                        snapshot::Request::Restore(_) => Action::Restore,
                        _ => unreachable!(),
                    } as i64,
                    &request,
                    None,
                    host(),
                    resources(),
                )
                .unwrap()
                .unwrap()
        };
        let (create, worker, _) = admit(10, snapshot::Request::Create(None));
        let proof = f.stage_snapshot(&create, &worker);
        f.ctl
            .finish_snapshot_copy(&worker, &create, proof, None, host(), resources())
            .unwrap();
        let (restore, worker, op) = admit(11, snapshot::Request::Restore(create.target));
        let candidate = restore.candidate.unwrap();
        let proof = f.stage_snapshot(&restore, &worker);
        let store = f.ctl.reaper_test_store();
        let charge = store.transact(|db| store::disk_bytes(db)).unwrap().unwrap();

        let error = std::thread::scope(|scope| {
            let task = store
                .transact(|_| {
                    // Own the SQLite writer. The settlement thread samples a valid
                    // token outside the transaction and cannot acquire it yet.
                    let (sampled, read) = std::sync::mpsc::channel();
                    *f.clock.next_sample.lock().unwrap() = Some(sampled);
                    let task = scope.spawn(|| {
                        f.ctl.finish_snapshot_copy(
                            &worker,
                            &restore,
                            proof,
                            None,
                            host(),
                            resources(),
                        )
                    });
                    read.recv_timeout(Duration::from_secs(5))?;
                    f.clock.set(NOW, 15 * 60 + 1);
                    Ok(task)
                })
                .unwrap();
            task.join().unwrap().unwrap_err()
        });
        assert_eq!(error.to_string(), "snapshot worker token expired");
        assert_eq!(
            f.ctl.operation_status(OWNER, op.id).unwrap().unwrap().state,
            Status::Pending
        );
        store.transact(|db| {
            let generation: Id = db.query_row(
                "SELECT id FROM vm_generations WHERE vm_id=?1 AND active=1",
                [vm], |row| row.get(0),
            )?;
            let identified = store::count(
                db, "SELECT count(*) FROM vm_artifacts WHERE generation_id=?1 AND state='identified'",
                [candidate.disk.generation],
            )?;
            let copying = store::count(
                db, "SELECT count(*) FROM vm_snapshot_work WHERE activity=?1 AND phase='copy'",
                [worker.activity_id()],
            )?;
            assert_eq!(generation, active.disk.generation);
            assert_eq!(identified, 2);
            assert_eq!(copying, 1);
            assert_eq!(store::disk_bytes(db)?, Some(charge));
            Ok(())
        }).unwrap();
        f.assert_pair(active, true);
        f.assert_pair(candidate, true);
    }

    #[test]
    fn snapshot_retire_sql_settlement_failure_keeps_claim_armed_and_retries_after_backoff() {
        let f = Fixture::new();
        let golden = f.build(1, true);
        let (vm, active_slots) = f.clone(golden, 2, true);
        let stage_copy = |work: &store::snapshot::Work, worker: &PendingOperation| {
            let proof = f.stage_snapshot(work, worker);
            f.ctl
                .finish_snapshot_copy(worker, work, proof, None, host(), resources())
                .unwrap();
        };

        let (create_work, create_worker, _) = f
            .ctl
            .admit_snapshot(
                OWNER,
                key(NOW, 10),
                vm,
                Action::Snapshot as i64,
                &snapshot::Request::Create(None),
                None,
                host(),
                resources(),
            )
            .unwrap()
            .unwrap();
        stage_copy(&create_work, &create_worker);
        let point = create_work.target;

        let (restore_work, restore_worker, _) = f
            .ctl
            .admit_snapshot(
                OWNER,
                key(NOW, 11),
                vm,
                Action::Restore as i64,
                &snapshot::Request::Restore(point),
                None,
                host(),
                resources(),
            )
            .unwrap()
            .unwrap();
        stage_copy(&restore_work, &restore_worker);
        assert_eq!(
            f.count("SELECT count(*) FROM vm_snapshot_work WHERE phase='retire'"),
            1
        );
        let bytes_before = f
            .ctl
            .reaper_test_store()
            .transact(|db| store::disk_bytes(db))
            .unwrap()
            .unwrap();

        // Fail the SQL settlement transaction after physical disposal succeeds.
        f.ctl
            .reaper_test_store()
            .transact(|db| {
                db.execute_batch(
                    "CREATE TRIGGER fail_retire_settle BEFORE DELETE ON vm_snapshot_work
                     BEGIN SELECT RAISE(ABORT,'transient sql failure'); END;",
                )?;
                Ok(())
            })
            .unwrap();
        f.sweep(&Runtime::new(offline));
        f.assert_pair(active_slots, false);
        assert_eq!(
            f.count("SELECT count(*) FROM vm_snapshot_work WHERE phase='retire'"),
            1
        );
        assert_eq!(
            f.count("SELECT count(*) FROM vm_snapshot_reconciliation"),
            0
        );
        assert_eq!(
            f.ctl
                .reaper_test_store()
                .transact(|db| store::disk_bytes(db))
                .unwrap()
                .unwrap(),
            bytes_before
        );

        // Remove the fault. A sweep before backoff elapses leaves the claim armed.
        f.ctl
            .reaper_test_store()
            .transact(|db| {
                db.execute_batch("DROP TRIGGER fail_retire_settle;")?;
                Ok(())
            })
            .unwrap();
        f.sweep(&Runtime::new(offline));
        assert_eq!(
            f.count("SELECT count(*) FROM vm_snapshot_work WHERE phase='retire'"),
            1
        );

        // Advance past backoff: the next eligible turn proves disposed files
        // absent and settles the claim and correlated daemon receipt once.
        f.clock.set(NOW + 10_000, 10);
        f.sweep(&Runtime::new(offline));
        assert_eq!(f.count("SELECT count(*) FROM vm_snapshot_work"), 0);
        assert_eq!(
            f.count(
                "SELECT count(*) FROM vm_snapshot_reconciliation r
                 JOIN activity a ON a.id=r.activity_id
                 WHERE r.cause='retired' AND a.action=19 AND a.actor='daemon'
                   AND a.status=1 AND a.cleanup_confirmed=1"
            ),
            1
        );
        assert!(
            f.ctl
                .reaper_test_store()
                .transact(|db| store::disk_bytes(db))
                .unwrap()
                .unwrap()
                < bytes_before
        );
    }
}
