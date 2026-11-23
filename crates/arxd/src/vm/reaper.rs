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
            | Self::Recovery { vm: id, .. } => id,
        }
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
    pub artifacts: Vec<CleanupArtifact>,
    pub accounted: bool,
}

/// Requires the exact generated slots, including each recorded directory and
/// file identity. A receipt from a fixture is never native custody authority.
pub(crate) trait ArtifactCustody {
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
}

impl Sweeps {
    pub fn retain(&mut self, items: &[CleanupWork]) {
        self.retries
            .retain(|key, _| items.iter().any(|work| work.kind == *key));
    }

    pub fn due(&self, kind: CleanupKind, now: Instant) -> bool {
        self.retries.get(&kind).is_none_or(|(_, next)| now >= *next)
    }

    pub fn settle(&mut self, kind: CleanupKind, now: Instant, confirmed: bool) {
        if confirmed {
            self.retries.remove(&kind);
        } else {
            let attempts = self.retries.get(&kind).map_or(0, |(n, _)| (*n + 1).min(4));
            let delay = Duration::from_secs((5 * (1 << attempts)).min(60));
            if let Some(next) = now.checked_add(delay) {
                self.retries.insert(kind, (attempts, next));
            }
        }
    }
}

pub(super) fn reconcile(
    work: &CleanupWork,
    runtime: &impl ReaperRuntime,
    custody: &impl ArtifactCustody,
    now: impl Fn() -> Instant,
) -> bool {
    let start = now();
    let (Some(join_by), Some(stop_by), Some(finish_by)) = (
        start.checked_add(WORKER_BUDGET),
        start.checked_add(STOP_BUDGET),
        start.checked_add(ITEM_BUDGET),
    ) else {
        return false;
    };
    let released = match runtime.quiesce(work, join_by) {
        Quiesced::Offline => true,
        Quiesced::Unconfirmed => false,
        Quiesced::OwnedTestJob if now() < stop_by => {
            let stopped =
                runtime.graceful_stop(work, stop_by) != StopDisposition::ChargedQuarantine;
            stopped
                || (work.accounted
                    && matches!(work.kind, CleanupKind::Expiry { .. })
                    && now() < finish_by
                    && runtime.force_reap(work, finish_by) != StopDisposition::ChargedQuarantine)
        }
        Quiesced::OwnedTestJob => false,
    };
    released && work.accounted && now() < finish_by && custody.reconcile(work, finish_by)
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
            let handle = open(root, true)?;
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
            let file = open(&path, false)?;
            Ok(ArtifactEvidence {
                identity: identity(&file, false)?,
                directory: self.directory.file,
                pin: ArtifactPin {
                    size: bytes.len() as u64,
                    sha256: Sha256::digest(bytes).into(),
                },
            })
        }

        fn check(&self, slot: &CleanupArtifact) -> Result<bool> {
            ensure!(
                matches!(
                    slot.role.as_str(),
                    "disk" | "vars" | "seed" | "verification_overlay"
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
                    && identity(&open(&self.root, true)?, true)? == self.directory,
                "fixture directory changed"
            );
            let path = self.path(&slot.role, slot.key);
            match fs::symlink_metadata(&path) {
                Ok(_) => {
                    crate::config::checked_path(&path)?;
                    ensure!(
                        identity(&open(&path, false)?, false)? == recorded,
                        "fixture file changed"
                    );
                    Ok(true)
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound && !slot.retain => Ok(false),
                Err(error) => Err(error.into()),
            }
        }

        fn remove(&self, work: &CleanupWork, deadline: Instant) -> Result<()> {
            for slot in &work.artifacts {
                ensure!(Instant::now() < deadline, "fixture proof deadline exceeded");
                self.check(slot)?;
            }
            for slot in work.artifacts.iter().filter(|slot| !slot.retain) {
                ensure!(
                    Instant::now() < deadline,
                    "fixture removal deadline exceeded"
                );
                if self.check(slot)? {
                    fs::remove_file(self.path(&slot.role, slot.key))?;
                }
                ensure!(!self.check(slot)?, "fixture file still present");
            }
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

    fn open(path: &Path, directory: bool) -> io::Result<fs::File> {
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
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
            options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS);
            options.share_mode(if directory {
                FILE_SHARE_READ | FILE_SHARE_WRITE
            } else {
                0
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
    struct TestClock(Arc<Mutex<ClockReading>>);

    impl TestClock {
        fn set(&self, utc_ms: i64, seconds: u64) {
            let mut clock = self.0.lock().unwrap();
            clock.utc_ms = utc_ms;
            clock.monotonic += Duration::from_secs(seconds);
        }
    }

    impl Clock for TestClock {
        fn now(&self) -> ClockReading {
            *self.0.lock().unwrap()
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
            let clock = TestClock(Arc::new(Mutex::new(ClockReading {
                utc_ms: NOW,
                monotonic: Instant::now(),
            })));
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
            let CloneAdmission::Fresh(vm, slots, worker, _) = self
                .ctl
                .admit_linked_clone(request, host(), resources(), pair, proof)
                .unwrap()
                .unwrap()
            else {
                panic!("fresh clone required");
            };
            let roles = [slots.disk, slots.vars];
            let files = roles.map(|slot| self.custody.stage(slot, b"clone").unwrap());
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
        assert_eq!(f.count("SELECT count(*) FROM vm_operations WHERE script_result='interrupted' AND script_exit_code IS NULL"), 1);
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
}
