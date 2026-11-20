//! The D6 one-shot observation. Its capability is constructed only in the ignored case.
use super::*;
use crate::cluster::{
    bound_open::BoundAttempt,
    enrollment::NodeId,
    startup_plan::{Mode, tests},
};
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
type GuardIds = Vec<(PathBuf, u64, u64)>;
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
    state_dir: PathBuf,
    guards: Mutex<GuardIds>,
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
            state_dir,
            guards: Mutex::default(),
            handshake: Mutex::new(Some((polled, detached))),
            first_failure: Mutex::new(false),
            running: Notify::new(),
        }
    }

    pub(super) fn stage(&self, name: &str, fields: impl std::fmt::Display) {
        let mut stdout = std::io::stdout().lock();
        let _ = writeln!(
            stdout,
            "ARX5B {} {name} {fields}",
            self.start.elapsed().as_micros()
        );
        let _ = stdout.flush();
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
        *self.guards.lock().unwrap() = tests::guards(owner);
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
        self.same_owner(state)?;
        self.stage("transferred", "1 1 1");
        Ok(())
    }

    fn same_owner(&self, state: &State) -> Result<(), ()> {
        self.check(
            state
                .owner
                .as_ref()
                .is_some_and(|o| tests::guards(o) == *self.guards.lock().unwrap()),
            "owner_identity",
        )
    }

    fn probe(&self, name: &str, held: bool) -> bool {
        let path = self.state_dir.join(name);
        let guards = self.guards.lock().unwrap();
        let Some((_, volume, index)) = guards.iter().find(|(p, _, _)| p == &path) else {
            return false;
        };
        let Ok(file) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
        else {
            return false;
        };
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0
            || info.dwFileAttributes & 0x410 != 0
            || info.nNumberOfLinks != 1
            || u64::from(info.dwVolumeSerialNumber) != *volume
            || (u64::from(info.nFileIndexHigh) << 32 | u64::from(info.nFileIndexLow)) != *index
        {
            return false;
        }
        match file.try_lock() {
            Ok(()) => file.unlock().is_ok() && !held,
            Err(std::fs::TryLockError::WouldBlock) => held,
            Err(std::fs::TryLockError::Error(_)) => false,
        }
    }

    fn locks(&self, state: &State, stage: &str, held: [bool; 3]) -> Result<(), ()> {
        self.same_owner(state)?;
        let [outer, sm, log] = std::array::from_fn(|i| self.probe(LOCKS[i], held[i]));
        self.check(outer && sm && log, stage)?;
        self.stage(stage, "1 1 1");
        Ok(())
    }

    pub(super) fn driver_done(&self) {
        // Refusal before startup must also release the waiting test.
        drop(self.handshake.lock().unwrap().take());
        self.running.notify_one();
    }

    pub(super) fn before_disposal(&self, state: &State) -> bool {
        self.locks(state, "before_disposal", [true, false, false])
            .is_ok()
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

#[tokio::test(flavor = "current_thread")]
#[ignore = "D6: separate owner authorization for one Windows observation on the reviewed head"]
async fn nominal_real_engine_start_close() {
    let start = Instant::now();
    assert!(go(), "D6 observation admission refused");
    let retained = crate::service::application::tests::private_temp().keep();
    for leaf in ["state", "credentials", "workspace"] {
        std::fs::create_dir(retained.join(leaf)).unwrap();
    }
    let config = retained.join("config");
    std::fs::write(&config, b"{}").unwrap();
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
        .unwrap(),
    )
    .unwrap();
    let intent = StartIntent {
        mode: Mode::CreateFounder,
        cluster: [1; 16],
        node: NodeId::new(1).unwrap(),
        peer: plan.peer.parse().unwrap(),
        enrollment: plan.enrollment.parse().unwrap(),
    };
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
    witness
        .check(witness.probe(LOCKS[0], false), "outer_release")
        .unwrap();
    witness.stage("outer_released", "1");
    witness
        .check(clients.load(Ordering::SeqCst) == 0, "peer_client")
        .unwrap();
    witness.stage("finished", "1 1 0");
}
