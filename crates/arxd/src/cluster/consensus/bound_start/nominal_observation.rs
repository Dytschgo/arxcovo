//! The D6 one-shot observation. Its capability is constructed only in the ignored case.
use super::*;
use crate::cluster::{bound_open::BoundAttempt, enrollment::NodeId, startup_plan::Mode};
use std::{
    future::{Future, poll_fn},
    io::Write,
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    task::Poll,
};
use tokio::sync::oneshot;
use windows_sys::Win32::{
    Foundation::{HANDLE, INVALID_HANDLE_VALUE},
    Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        FILE_SHARE_WRITE, FILE_TYPE_PIPE, GetFileInformationByHandle, GetFileType, ReadFile,
    },
    System::{
        Console::{GetStdHandle, STD_INPUT_HANDLE},
        Pipes::PeekNamedPipe,
    },
};

const CASE: &str =
    "cluster::consensus::bound_start::nominal_observation::nominal_real_engine_start_close";
const LOCKS: [&str; 3] = [
    "cluster-owner.lock",
    "cluster-state.lock",
    "cluster-log.lock",
];
type FileId = (u32, u64);
type GuardIds = [FileId; 3];

fn file_id(file: &std::fs::File) -> Option<FileId> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    (unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } != 0
        && info.dwFileAttributes & 0x410 == 0
        && info.nNumberOfLinks == 1)
        .then_some((
            info.dwVolumeSerialNumber,
            u64::from(info.nFileIndexHigh) << 32 | u64::from(info.nFileIndexLow),
        ))
}

fn original_guards(owner: &Owner) -> Option<GuardIds> {
    // Recheck every original guard, then get the exact three lock identities
    // from fallible duplicates of their retained handles.
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
        let guards = original_guards(owner).ok_or("owner_identity")?;
        *self.ids.lock().unwrap() = Some(guards);
        Ok(())
    }

    fn same_owner(&self, state: &State) -> Result<(), &'static str> {
        let checked = state.owner.as_ref().and_then(original_guards);
        if checked.is_some() && checked == *self.ids.lock().unwrap() {
            Ok(())
        } else {
            Err("owner_identity")
        }
    }

    fn probe(&self, index: usize, held: bool) -> bool {
        let path = self.state_dir.join(LOCKS[index]);
        let expected = self.ids.lock().unwrap().map(|ids| ids[index]);
        let Ok(file) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
        else {
            return false;
        };
        if expected.is_none() || file_id(&file) != expected {
            return false;
        }
        match file.try_lock() {
            Ok(()) => file.unlock().is_ok() && !held,
            Err(std::fs::TryLockError::WouldBlock) => held,
            Err(std::fs::TryLockError::Error(_)) => false,
        }
    }

    fn locks(&self, state: &State, held: [bool; 3]) -> Result<[bool; 3], &'static str> {
        self.same_owner(state)?;
        Ok(std::array::from_fn(|i| self.probe(i, held[i])))
    }

    fn finish_close(
        &self,
        job: &Job,
        actual: bool,
        record: impl FnOnce(Result<[bool; 3], &'static str>),
    ) {
        let locks = self.locks(&job.state(), [true, false, false]);
        record(locks);
        job.finish_close(None, locks.is_ok_and(|locks| locks == [true; 3]), actual);
    }
}

type Handshake = (oneshot::Sender<()>, oneshot::Receiver<()>);

fn go() -> bool {
    if !std::env::args_os().skip(1).eq([
        "--ignored",
        "--exact",
        CASE,
        "--nocapture",
        "--test-threads=1",
    ]) {
        return false;
    }
    let end = Instant::now() + Duration::from_secs(2);
    unsafe {
        let pipe = GetStdHandle(STD_INPUT_HANDLE);
        if pipe.is_null() || pipe == INVALID_HANDLE_VALUE || GetFileType(pipe) != FILE_TYPE_PIPE {
            return false;
        }
        while Instant::now() < end {
            let mut available = 0;
            if PeekNamedPipe(
                pipe,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            ) == 0
                || available > 1
            {
                return false;
            }
            if available == 1 {
                return read_go(pipe);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    false
}

unsafe fn read_go(pipe: HANDLE) -> bool {
    let mut byte = 0;
    let mut n = 0;
    // The private pipe has one reader. PeekNamedPipe reported a queued byte.
    let read = unsafe { ReadFile(pipe, &mut byte, 1, &mut n, std::ptr::null_mut()) };
    read != 0 && n == 1 && byte == b'G'
}

pub(super) struct Witness {
    start: Instant,
    guards: GuardProof,
    handshake: Mutex<Option<Handshake>>,
    first_failure: Mutex<bool>,
    pub(super) running: Notify,
}

impl Witness {
    fn new(
        start: Instant,
        state_dir: PathBuf,
        polled: oneshot::Sender<()>,
        detached: oneshot::Receiver<()>,
    ) -> Self {
        Self {
            start,
            guards: GuardProof::new(state_dir),
            handshake: Mutex::new(Some((polled, detached))),
            first_failure: Mutex::new(false),
            running: Notify::new(),
        }
    }

    pub(super) fn stage(&self, name: &str, fields: impl std::fmt::Display) {
        stage(self.start, name, fields);
    }

    pub(super) fn fail(&self, code: &str) {
        let mut failed = self.first_failure.lock().unwrap();
        if !*failed {
            self.stage("failure", code);
            *failed = true;
        }
    }

    pub(super) fn orphaned(&self, r: ShutdownReport) {
        let mut failed = self.first_failure.lock().unwrap();
        self.stage(
            "orphaned",
            format_args!(
                "{} {} {} {} {} {} {} {} {} {} {} {} {}",
                r.first as u8,
                r.engine as u8,
                r.engine_ms,
                r.owners_done_at_start,
                r.owners_waited,
                r.owners_left,
                r.owners_panicked,
                r.last_owner_join_ms,
                r.propose_held,
                r.read_held,
                r.log_strong,
                r.sm_strong,
                r.total_ms,
            ),
        );
        *failed = true;
    }

    pub(super) fn check(&self, good: bool, code: &str) -> Result<(), ()> {
        if good {
            Ok(())
        } else {
            self.fail(code);
            Err(())
        }
    }

    pub(super) fn prepared(&self, state: &State) -> Result<(usize, usize), ()> {
        if let Some(r) = state.refusal {
            self.stage("owner_refusal", r as u8);
            return Err(());
        }
        let owner = state
            .owner
            .as_ref()
            .ok_or_else(|| self.fail("preparation"))?;
        if let BoundAttempt::Failed(failure) = owner.bound_attempt {
            use crate::cluster::bound_open::BoundFailure::*;
            let code = match failure {
                Intent => "bound_intent",
                Layout => "bound_layout",
                Inspection(_) => "bound_inspection",
                Unavailable => "bound_unavailable",
                Identity => "bound_identity",
                Open(_) => "bound_open",
                Init(_) => "bound_init",
                Unwind => "bound_unwind",
            };
            self.fail(code);
            return Err(());
        }
        self.check(
            matches!(state.observation, Some(Observation::Fresh)),
            "preparation",
        )?;
        let stores = state
            .stores
            .as_ref()
            .ok_or_else(|| self.fail("preparation"))?;
        let (_, opens) = stores.io_counts();
        self.check(opens == 2, "open_count")?;
        self.guards.capture(owner).map_err(|code| self.fail(code))?;
        self.stage("prepared", "2");
        Ok(stores.pointers())
    }

    pub(super) fn transferred(
        &self,
        state: &State,
        (log_at, sm_at): (usize, usize),
    ) -> Result<(), ()> {
        let (log, sm) = state.adapters.as_ref().unwrap();
        self.check(
            log_at != 0
                && sm_at != 0
                && log_at != sm_at
                && log_at == log.0.lock().unwrap().bound_pointer()
                && sm_at == sm.0.lock().unwrap().bound_pointer(),
            "transfer_identity",
        )?;
        self.guards
            .same_owner(state)
            .map_err(|code| self.fail(code))?;
        self.stage("transferred", "1 1 1");
        Ok(())
    }

    fn record_locks(&self, stage: &str, locks: Result<[bool; 3], &'static str>) -> Result<(), ()> {
        let [outer, sm, log] = locks.map_err(|code| self.fail(code))?;
        self.stage(
            stage,
            format_args!("{} {} {}", u8::from(outer), u8::from(sm), u8::from(log)),
        );
        self.check(outer && sm && log, stage)
    }

    fn locks(&self, state: &State, stage: &str, held: [bool; 3]) -> Result<(), ()> {
        self.record_locks(stage, self.guards.locks(state, held))
    }

    pub(super) fn driver_done(&self) {
        // Refusal before startup must also release the waiting test.
        drop(self.handshake.lock().unwrap().take());
        self.running.notify_one();
    }

    pub(super) fn finish_close(&self, job: &Job, actual: bool) {
        self.guards.finish_close(job, actual, |locks| {
            let _ = self.record_locks("before_disposal", locks);
        });
    }

    pub(super) async fn first_poll<T, E>(
        &self,
        future: impl Future<Output = Result<T, E>>,
    ) -> Result<T, ()> {
        let (polled, detached) = self.handshake.lock().unwrap().take().unwrap();
        let mut future = Box::pin(future);
        let first = poll_fn(|cx| Poll::Ready(future.as_mut().poll(cx))).await;
        self.stage(
            "engine_poll",
            if first.is_pending() {
                "pending"
            } else {
                "ready"
            },
        );
        if matches!(&first, Poll::Ready(Err(_))) {
            self.fail("engine_start");
        }
        if polled.send(()).is_err() || detached.await.is_err() {
            self.fail("handshake");
            // Custody is uncertain. Keep the exact resources until process exit.
            std::mem::forget(first);
            std::mem::forget(future);
            return Err(());
        }
        match first {
            Poll::Ready(result) => result,
            Poll::Pending => future.await,
        }
        .map_err(|_| self.fail("engine_start"))
    }
}

struct QuietNetwork {
    factory: mem_net::Factory,
    clients: Arc<AtomicUsize>,
}

impl RaftNetworkFactory<Arx> for QuietNetwork {
    type Network = mem_net::MemNetwork;

    async fn new_client(&mut self, target: u64, node: &BasicNode) -> Self::Network {
        self.clients.fetch_add(1, Ordering::SeqCst);
        self.factory.new_client(target, node).await
    }
}

fn stage(start: Instant, name: &str, fields: impl std::fmt::Display) {
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(
        stdout,
        "ARX5B {} {name} {fields}",
        start.elapsed().as_micros()
    );
    let _ = stdout.flush();
}

fn setup() -> Result<(PathBuf, Plan, StartIntent), &'static str> {
    let parent = std::env::temp_dir();
    crate::security::private(&parent).map_err(|_| "fixture_privacy")?;
    // The parent ACL is already qualified and inheritable. Keep the fresh
    // directory before any fallible validation or setup below it.
    let retained = tempfile::tempdir_in(parent)
        .map_err(|_| "fixture_create")?
        .keep();
    crate::security::private(&retained).map_err(|_| "fixture_privacy")?;
    for leaf in ["state", "credentials", "workspace"] {
        std::fs::create_dir(retained.join(leaf)).map_err(|_| "fixture_layout")?;
    }
    let config = retained.join("config");
    std::fs::write(&config, b"{}").map_err(|_| "fixture_config")?;
    let plan = Plan::parse(
        &serde_json::to_vec(&serde_json::json!({
            "mode": "create-founder",
            "config": config,
            "state": retained.join("state"),
            "credentials": retained.join("credentials"),
            "workspace": retained.join("workspace"),
            "peer": "127.0.0.1:7001",
            "enrollment": "127.0.0.1:7002",
        }))
        .map_err(|_| "fixture_plan")?,
    )
    .map_err(|_| "fixture_plan")?;
    let intent = StartIntent {
        mode: Mode::CreateFounder,
        cluster: [1; 16],
        node: NodeId::new(1).ok_or("fixture_plan")?,
        peer: plan.peer.parse().map_err(|_| "fixture_plan")?,
        enrollment: plan.enrollment.parse().map_err(|_| "fixture_plan")?,
    };
    Ok((retained, plan, intent))
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "D6: separate owner authorization for one Windows observation on the reviewed head"]
async fn nominal_real_engine_start_close() {
    let start = Instant::now();
    if !go() {
        stage(start, "failure", "test_admission");
        panic!("D6 observation admission refused");
    }
    let (retained, plan, intent) = setup().unwrap_or_else(|code| {
        stage(start, "failure", code);
        panic!("D6 fixture setup refused");
    });
    let (polled, first_poll) = oneshot::channel();
    let (detach, detached) = oneshot::channel();
    let witness = Arc::new(Witness::new(
        start,
        retained.join("state"),
        polled,
        detached,
    ));
    let clients = Arc::new(AtomicUsize::new(0));
    let network = QuietNetwork {
        factory: mem_net::Factory {
            source: 1,
            router: Arc::default(),
        },
        clients: clients.clone(),
    };
    witness.stage("admitted", "1");
    let (job, observer, supervisor) = dispatch(
        plan,
        intent,
        None,
        Params::default(),
        network,
        Some(witness.clone()),
    );
    let observe = async {
        first_poll
            .await
            .map_err(|_| witness.fail("first_poll_missing"))?;
        drop(observer);
        witness.check(job.changed.receiver_count() == 0, "observer_count")?;
        witness.locks(&job.state(), "detached", [true; 3])?;
        // A Notify observes driver completion of startup without creating a watch receiver.
        let running = witness.running.notified();
        detach.send(()).map_err(|_| witness.fail("handshake"))?;
        running.await;
        let node = {
            let state = job.state();
            witness.check(
                state.status == Status::Running
                    && state.adapters.is_none()
                    && job.changed.receiver_count() == 0,
                "running",
            )?;
            state
                .node
                .clone()
                .ok_or_else(|| witness.fail("retained_node"))?
        };
        witness.check(node.lock().await.raft.is_some(), "retained_node")?;
        drop(node);
        witness.locks(&job.state(), "live", [true; 3])?;
        witness.check(clients.load(Ordering::SeqCst) == 0, "peer_client")?;
        witness.stage("close_requested", "1");
        drop(job.request_close());
        Ok(())
    };
    let observed: Result<(), ()> = observe.await;
    if observed.is_err() {
        job.set(Status::Unconfirmed);
        job.close.notify_one();
    }
    supervisor
        .await
        .map_err(|_| witness.fail("supervisor_join"))
        .unwrap();
    assert_eq!(observed, Ok(()), "D6 observation failed");
    let state = job.state();
    witness
        .check(
            state.status == Status::Clean
                && state.owner.is_none()
                && state.stores.is_none()
                && state.adapters.is_none()
                && state.node.is_none()
                && state.close_requested
                && state.close_dispatched
                && job.changed.receiver_count() == 0,
            "final_state",
        )
        .unwrap();
    let outer_free = witness.guards.probe(0, false);
    witness.stage("outer_released", u8::from(outer_free));
    witness.check(outer_free, "outer_release").unwrap();
    witness
        .check(clients.load(Ordering::SeqCst) == 0, "peer_client")
        .unwrap();
    witness.stage("finished", "1 1 0");
}

#[test]
fn original_guard_refusal_keeps_close_unconfirmed() {
    use crate::cluster::startup_plan::tests::Layout;

    let layout = Layout::new();
    let plan = layout.plan("create-founder");
    let intent = StartIntent {
        mode: plan.mode,
        cluster: [1; 16],
        node: NodeId::new(1).unwrap(),
        peer: plan.peer.parse().unwrap(),
        enrollment: plan.enrollment.parse().unwrap(),
    };
    let config = plan.config.clone();
    let proof = GuardProof::new(plan.state.clone());
    let job = Job::anchor();
    job.prepare(plan, &intent, None);
    assert_eq!(job.state().status, Status::Prepared);
    proof.capture(job.state().owner.as_ref().unwrap()).unwrap();

    drop(job.request_close());
    assert!(job.begin_close());
    // Adding a link makes rechecking the already-held original config refuse.
    let alias = layout.root.path().join("config-alias");
    std::fs::hard_link(config, &alias).unwrap();
    let mut recorded = None;
    proof.finish_close(job, true, |locks| recorded = Some(locks));

    assert_eq!(recorded, Some(Err("owner_identity")));
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
