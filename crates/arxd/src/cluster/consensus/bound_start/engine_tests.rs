//! The test-only capability for the shared bound Engine path.
use super::*;
use crate::cluster::{enrollment::NodeId, startup_plan::tests::Layout};
use std::{
    fs::{File, OpenOptions, TryLockError},
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
use tokio::sync::oneshot;

const LOCKS: [&str; 3] = [
    "cluster-owner.lock",
    "cluster-state.lock",
    "cluster-log.lock",
];
type FileId = (u64, u64);
type GuardIds = [FileId; 3];

#[cfg(unix)]
fn file_id(file: &File) -> Option<FileId> {
    use std::os::unix::fs::MetadataExt;

    let meta = file.metadata().ok()?;
    (meta.is_file() && meta.nlink() == 1).then(|| (meta.dev(), meta.ino()))
}

#[cfg(windows)]
fn file_id(file: &File) -> Option<FileId> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    (unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } != 0
        && info.dwFileAttributes & 0x410 == 0
        && info.nNumberOfLinks == 1)
        .then_some((
            u64::from(info.dwVolumeSerialNumber),
            u64::from(info.nFileIndexHigh) << 32 | u64::from(info.nFileIndexLow),
        ))
}

fn original_guards(owner: &Owner) -> Option<GuardIds> {
    // Check all original guards and capture the exact three lock identities.
    owner.publication_originals().ok()?;
    let id = |name| file_id(&owner.bound_guard(name).ok()?);
    Some([id(LOCKS[0])?, id(LOCKS[1])?, id(LOCKS[2])?])
}

struct GuardProof {
    state_dir: PathBuf,
    ids: Mutex<Option<GuardIds>>,
}

impl GuardProof {
    fn new(state_dir: PathBuf) -> Self {
        Self {
            state_dir,
            ids: Mutex::default(),
        }
    }

    fn capture(&self, owner: &Owner) -> Result<(), &'static str> {
        let ids = original_guards(owner).ok_or("original owner guards")?;
        *self.ids.lock().unwrap() = Some(ids);
        Ok(())
    }

    fn same_owner(&self, state: &State) -> Result<(), &'static str> {
        let actual = state.owner.as_ref().and_then(original_guards);
        if actual.is_some() && actual == *self.ids.lock().unwrap() {
            Ok(())
        } else {
            Err("original owner guards")
        }
    }

    fn probe(&self, index: usize, held: bool) -> bool {
        let expected = self.ids.lock().unwrap().map(|ids| ids[index]);
        let mut open = OpenOptions::new();
        open.read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            open.custom_flags(libc::O_NOFOLLOW);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::{
                FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
            };
            open.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        }
        let Ok(file) = open.open(self.state_dir.join(LOCKS[index])) else {
            return false;
        };
        if expected.is_none() || file_id(&file) != expected {
            return false;
        }
        match file.try_lock() {
            Ok(()) => file.unlock().is_ok() && !held,
            Err(TryLockError::WouldBlock) => held,
            Err(TryLockError::Error(_)) => false,
        }
    }

    fn locks(&self, state: &State, held: [bool; 3]) -> Result<(), &'static str> {
        self.same_owner(state)?;
        for (index, held) in held.into_iter().enumerate() {
            if !self.probe(index, held) {
                return Err(LOCKS[index]);
            }
        }
        Ok(())
    }

    fn finish_close(&self, job: &Job, actual: bool) -> Result<(), &'static str> {
        let proof = self.locks(&job.state(), [true, false, false]);
        job.finish_close(None, proof.is_ok(), actual);
        proof
    }
}

pub(super) struct Witness {
    guards: GuardProof,
    failure: Mutex<Option<&'static str>>,
    startup_release: Mutex<Option<oneshot::Receiver<()>>>,
}

impl Witness {
    fn new(state_dir: PathBuf, startup_release: oneshot::Receiver<()>) -> Self {
        Self {
            guards: GuardProof::new(state_dir),
            failure: Mutex::default(),
            startup_release: Mutex::new(Some(startup_release)),
        }
    }

    pub(super) fn take_startup_release(&self) -> oneshot::Receiver<()> {
        self.startup_release.lock().unwrap().take().unwrap()
    }

    pub(super) fn fail(&self, reason: &'static str) {
        self.failure.lock().unwrap().get_or_insert(reason);
    }

    pub(super) fn prepared(&self, state: &State) -> Result<(usize, usize), &'static str> {
        // Operational preparation publishes the untouched genesis it created.
        if !matches!(
            state.observation,
            Some(Observation::UntouchedGenesis { .. })
        ) {
            return Err("fresh bound preparation");
        }
        let owner = state.owner.as_ref().ok_or("prepared owner")?;
        let stores = state.stores.as_ref().ok_or("prepared stores")?;
        if stores.io_counts().1 != 2 {
            return Err("fresh SQLite opens");
        }
        self.guards.capture(owner)?;
        Ok(stores.pointers())
    }

    pub(super) fn transferred(
        &self,
        state: &State,
        (log_at, sm_at): (usize, usize),
    ) -> Result<(), &'static str> {
        let (log, sm) = state.adapters.as_ref().ok_or("transferred adapters")?;
        if log_at == 0
            || sm_at == 0
            || log_at == sm_at
            || log_at != log.0.lock().unwrap().bound_pointer()
            || sm_at != sm.0.lock().unwrap().bound_pointer()
        {
            return Err("transferred SQLite identities");
        }
        self.guards.same_owner(state)
    }

    pub(super) fn finish_close(&self, job: &Job, actual: bool) {
        if let Err(reason) = self.guards.finish_close(job, actual) {
            self.fail(reason);
        }
    }
}

struct QuietNetwork {
    factory: mem_net::Factory,
    clients: Arc<AtomicUsize>,
}

impl RaftNetworkFactory<Arx> for QuietNetwork {
    type Network = mem_net::MemNetwork;

    async fn new_client(&mut self, target: u64, node: &BasicNode) -> Self::Network {
        self.clients.fetch_add(1, Ordering::Relaxed);
        self.factory.new_client(target, node).await
    }
}

fn fresh(layout: &Layout) -> (Plan, StartIntent) {
    let plan = layout.plan("create-founder");
    let intent = StartIntent {
        mode: plan.mode,
        cluster: [1; 16],
        node: NodeId::new(1).unwrap(),
        peer: plan.peer.parse().unwrap(),
        enrollment: plan.enrollment.parse().unwrap(),
    };
    (plan, intent)
}

#[tokio::test(flavor = "current_thread")]
async fn bound_start_real_engine_start_close() {
    let layout = Layout::new();
    let (plan, intent) = fresh(&layout);
    let (release_startup, startup_release) = oneshot::channel();
    let witness = Arc::new(Witness::new(plan.state.clone(), startup_release));
    let clients = Arc::new(AtomicUsize::new(0));
    let network = QuietNetwork {
        factory: mem_net::Factory {
            source: 1,
            router: Arc::default(),
        },
        clients: clients.clone(),
    };
    let (job, mut observer, supervisor) = dispatch(
        plan,
        intent,
        None,
        Params::default(),
        network,
        Some(witness.clone()),
    );

    let outcome = tokio::time::timeout(Duration::from_secs(10), async {
        let live = async {
            loop {
                match observer.status() {
                    Status::EngineMayExist => break,
                    Status::Preparing | Status::Prepared => {}
                    _ => return Err("startup did not reach the Engine boundary"),
                }
                observer
                    .changed()
                    .await
                    .ok_or("startup observation closed")?;
            }
            drop(observer);
            if job.changed.receiver_count() != 0 {
                return Err("observer remained attached at the Engine boundary");
            }
            release_startup.send(()).map_err(|_| "startup release")?;
            // The job signals completion after publishing its state and joining the worker.
            job.startup_finished.notified().await;
            let node = {
                let state = job.state();
                if state.status != Status::Running {
                    return Err("startup worker did not join successfully");
                }
                if state.adapters.is_some() || job.changed.receiver_count() != 0 {
                    return Err("startup retained an adapter clone or observer");
                }
                witness.guards.locks(&state, [true; 3])?;
                state.node.clone().ok_or("retained node")?
            };
            node.lock()
                .await
                .raft
                .is_some()
                .then_some(())
                .ok_or("retained Engine")
        }
        .await;

        drop(job.request_close());
        supervisor.await.map_err(|_| "supervisor join")?;
        live?;
        Ok::<_, &'static str>(())
    })
    .await
    .expect("bound start/close exceeded its 10-second cooperative wait");

    let state = job.state();
    assert_eq!(
        (outcome, state.status),
        (Ok(()), Status::Clean),
        "bound start/close failed: {:?}; shutdown: {:?}",
        *witness.failure.lock().unwrap(),
        state.close_failure,
    );
    assert!(state.close_requested && state.close_dispatched);
    assert!(state.owner.is_none() && state.stores.is_none());
    assert!(state.adapters.is_none() && state.node.is_none());
    assert!(witness.guards.probe(0, false), "outer lock after disposal");
    assert_eq!(clients.load(Ordering::Relaxed), 0, "unexpected peer client");
}

#[test]
fn original_guard_refusal_keeps_close_unconfirmed() {
    let layout = Layout::new();
    let (plan, intent) = fresh(&layout);
    let config = plan.config.clone();
    let proof = GuardProof::new(plan.state.clone());
    let job = Job::anchor();
    job.prepare(plan, &intent, None);
    assert_eq!(job.state().status, Status::Prepared);
    proof.capture(job.state().owner.as_ref().unwrap()).unwrap();

    drop(job.request_close());
    assert!(job.begin_close());
    let alias = layout.root.path().join("config-alias");
    std::fs::hard_link(config, &alias).unwrap();
    assert_eq!(proof.finish_close(job, true), Err("original owner guards"));
    assert_eq!(job.state().status, Status::Unconfirmed);
    assert!(job.state().owner.is_some());
    assert!(job.state().stores.is_some());
    std::fs::remove_file(alias).unwrap();
    job.finish_close(None, true, true);
    assert_eq!(job.state().status, Status::Unconfirmed);

    // This fixture never started an Engine. Release only its test resources.
    let mut state = job.state();
    drop(state.stores.take());
    drop(state.owner.take());
}
