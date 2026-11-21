//! Process-retained bound startup. No service caller is activated by this slice.
#![allow(dead_code)] // Private future composition entry; no activation in step 5a.
use super::*;
use crate::cluster::{
    bound_open::OwnedStores,
    join_receipt::ExpectedIdentity,
    startup_plan::{Owner, Plan, Refusal},
};
use startup_recovery::{Observation, StartIntent};
use tokio::sync::{Notify, watch};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Status {
    Preparing,
    Prepared,
    Refused,
    EngineMayExist,
    Running,
    #[cfg(target_os = "linux")]
    Founding,
    #[cfg(target_os = "linux")]
    FounderReady,
    #[cfg(target_os = "linux")]
    FounderRefused,
    #[cfg(any(test, target_os = "linux"))]
    LearnerReady,
    #[cfg(any(test, target_os = "linux"))]
    LearnerRefused,
    Closing,
    Clean,
    ClosedAfterLearnerRefusal,
    #[cfg(target_os = "linux")]
    ClosedAfterFounderRefusal,
    Unconfirmed,
}
struct State {
    owner: Option<Owner>,
    stores: Option<OwnedStores>,
    observation: Option<Observation>,
    refusal: Option<Refusal>,
    adapters: Option<(LogAdapter, SmAdapter)>,
    node: Option<Arc<tokio::sync::Mutex<ConsensusNode>>>,
    status: Status,
    close_requested: bool,
    close_dispatched: bool,
    close_failure: Option<ShutdownReport>,
    engine_may_exist: bool,
    strict_learner_recovery: bool,
    #[cfg(target_os = "linux")]
    founder: Option<founder::Operation>,
    #[cfg(target_os = "linux")]
    voter_transfers: [Option<crate::cluster::enrollment::NodeId>; 2],
    #[cfg(target_os = "linux")]
    founder_failure: Option<founder::Error>,
    #[cfg(target_os = "linux")]
    issuer: Option<Arc<enrollment_ingress::ReadyIssuer>>,
    #[cfg(target_os = "linux")]
    ingress: Option<crate::cluster::enrollment::session::ListenerOwner>,
    #[cfg(target_os = "linux")]
    admission: Option<Arc<learner_admission::Controller>>,
    #[cfg(target_os = "linux")]
    learner: Option<learner::Operation>,
}
#[cfg(test)]
struct PreparationPause {
    entered: tokio::sync::oneshot::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
}
pub(super) struct Job {
    state: Mutex<State>,
    changed: watch::Sender<Status>,
    close: Notify,
    #[cfg(test)]
    gate: Mutex<Option<PreparationPause>>,
    #[cfg(test)]
    startup_finished: Notify,
}
pub(super) struct Observer(watch::Receiver<Status>);
impl Observer {
    pub(super) fn status(&self) -> Status {
        *self.0.borrow()
    }
    pub(super) async fn changed(&mut self) -> Option<Status> {
        self.0.changed().await.ok()?;
        Some(self.status())
    }
}

impl Job {
    // Deliberately process-owned, even if every observer/driver disappears.
    // Uncertain jobs have no disposal operation. Successful explicit close can
    // release custody; the small anchor and result remain for repeated observers.
    fn anchor() -> &'static Self {
        let (changed, _) = watch::channel(Status::Preparing);
        Box::leak(Box::new(Self {
            state: Mutex::new(State {
                owner: None,
                stores: None,
                observation: None,
                refusal: None,
                adapters: None,
                node: None,
                status: Status::Preparing,
                close_requested: false,
                close_dispatched: false,
                close_failure: None,
                engine_may_exist: false,
                strict_learner_recovery: false,
                #[cfg(target_os = "linux")]
                founder: None,
                #[cfg(target_os = "linux")]
                voter_transfers: [None; 2],
                #[cfg(target_os = "linux")]
                founder_failure: None,
                #[cfg(target_os = "linux")]
                issuer: None,
                #[cfg(target_os = "linux")]
                ingress: None,
                #[cfg(target_os = "linux")]
                admission: None,
                #[cfg(target_os = "linux")]
                learner: None,
            }),
            changed,
            close: Notify::new(),
            #[cfg(test)]
            gate: Mutex::new(None),
            #[cfg(test)]
            startup_finished: Notify::new(),
        }))
    }
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        // Poison is uncertainty, never disposal authority.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn set(&self, status: Status) {
        let mut s = self.state();
        if matches!(
            s.status,
            Status::Unconfirmed | Status::Clean | Status::ClosedAfterLearnerRefusal
        ) {
            return;
        }
        if status == Status::Refused && s.strict_learner_recovery {
            s.refusal.get_or_insert(Refusal::Plan);
        }
        if status == Status::EngineMayExist {
            s.engine_may_exist = true;
        }
        s.status = status;
        self.changed.send_replace(status);
        #[cfg(test)]
        if matches!(status, Status::Running | Status::Unconfirmed) {
            self.startup_finished.notify_one();
        }
    }
    pub(super) fn observe(&self) -> Observer {
        Observer(self.changed.subscribe())
    }
    pub(super) fn request_close(&self) -> Observer {
        let mut s = self.state();
        if !s.close_requested {
            s.close_requested = true;
            #[cfg(target_os = "linux")]
            if let Some(ingress) = &s.ingress {
                ingress.stop();
            }
            #[cfg(target_os = "linux")]
            if let Some(admission) = &s.admission {
                admission.stop();
            }
            self.close.notify_one();
        }
        self.observe()
    }
    #[cfg(target_os = "linux")]
    fn install_learner_admission(&self, network: &peer_transport::PeerFactory) {
        let mut state = self.state();
        if state.close_requested || state.status != Status::Founding {
            return;
        }
        if let Some(node) = &state.node {
            let controller = learner_admission::Controller::new(node);
            if network.install_admission(controller.clone()).is_ok() {
                state.admission = Some(controller);
            }
        }
    }

    #[cfg(target_os = "linux")]
    async fn observe_promotion(&self, cluster: crate::cluster::enrollment::ClusterId) {
        let (node, controller) = {
            let mut state = self.state();
            if state.close_requested {
                return;
            }
            let Some(node) = state.node.clone() else {
                return;
            };
            if state.status == Status::LearnerReady && state.admission.is_none() {
                state.admission = Some(learner_admission::Controller::new(&node));
            }
            let Some(controller) = state.admission.clone() else {
                return;
            };
            (node, controller)
        };
        initial_promotion::observe(cluster, node, controller).await;
    }

    fn prepare(&self, plan: Plan, intent: &StartIntent, expected: Option<&ExpectedIdentity>) {
        #[cfg(test)]
        let gate = self.gate.lock().unwrap().take();
        let mut s = self.state();
        if s.status != Status::Preparing || s.owner.is_some() {
            return;
        }
        match Owner::acquire(plan) {
            Ok(owner) => s.owner = Some(owner),
            Err(r) => {
                s.refusal = Some(r);
                s.status = Status::Refused;
                self.changed.send_replace(Status::Refused);
                return;
            }
        }
        let (stores, observation) =
            startup_recovery::prepare_bound(s.owner.as_mut().unwrap(), intent, expected);
        let good = observation.is_some();
        s.stores = Some(stores);
        s.observation = observation;
        s.status = if good {
            Status::Prepared
        } else {
            Status::Refused
        };
        self.changed.send_replace(s.status);
        drop(s);
        #[cfg(test)]
        if let Some(gate) = gate
            && gate.entered.send(()).is_ok()
        {
            // Dropping the last sender also releases a paused worker.
            let _ = gate.release.recv();
        }
    }
    fn transfer(&self) -> Result<(), ()> {
        let mut s = self.state();
        if s.status != Status::Prepared || s.adapters.is_some() {
            return Err(());
        }
        let (sm, log) = s.stores.as_mut().ok_or(())?.take_pair().map_err(|_| ())?;
        s.adapters = Some((
            LogAdapter(Arc::new(Mutex::new(log))),
            SmAdapter(Arc::new(Mutex::new(sm))),
        ));
        Ok(())
    }
    fn begin_close(&self) -> bool {
        let mut s = self.state();
        if s.close_dispatched || matches!(s.status, Status::Unconfirmed | Status::Clean) {
            return false;
        }
        s.close_dispatched = true;
        s.status = Status::Closing;
        self.changed.send_replace(Status::Closing);
        true
    }
    #[cfg(any(test, target_os = "linux"))]
    fn learner_attach_finished(&self, result: Result<(), Refusal>) {
        match result {
            Ok(()) => self.set(Status::LearnerReady),
            Err(reason) => {
                self.state().refusal.get_or_insert(reason);
                self.set(Status::LearnerRefused);
                drop(self.request_close());
            }
        }
    }

    fn finish_close(&self, failure: Option<ShutdownReport>, proven: bool, actual: bool) {
        let mut s = self.state();
        if s.status == Status::Unconfirmed {
            return;
        }
        if failure.is_some() {
            s.close_failure = failure;
        }
        if !proven || (s.engine_may_exist && !actual) || s.close_failure.is_some() {
            s.status = Status::Unconfirmed;
        } else {
            s.adapters = None;
            s.stores = None;
            s.node = None;
            if s.strict_learner_recovery && s.refusal.is_some() && s.owner.is_some() {
                s.status = Status::ClosedAfterLearnerRefusal;
            } else {
                s.owner = None;
                s.status = Status::Clean;
            }
            #[cfg(target_os = "linux")]
            {
                s.learner = None;
                s.issuer = None;
                s.admission = None;
                s.founder = None;
                if s.founder_failure.is_some() {
                    s.status = Status::ClosedAfterFounderRefusal;
                }
            }
        }
        self.changed.send_replace(s.status);
    }
    // Module-private loan, never a generic caller capability.
    fn with_retained_owner<T>(
        &self,
        compose: impl FnOnce(&mut Owner) -> Result<T, Refusal>,
    ) -> Result<T, Refusal> {
        let mut s = self.state();
        if s.status != Status::Running || s.close_requested || s.node.is_none() {
            return Err(Refusal::Ownership);
        }
        let owner = s.owner.as_mut().ok_or(Refusal::Ownership)?;
        owner.publication_originals()?;
        compose(owner)
    }
}
/// The caller gets observation/control of one retained operation, never a task
/// abort handle. Both startup and close are supervised by a process-owned driver.
pub(super) fn start<N>(
    plan: Plan,
    intent: StartIntent,
    expected: Option<ExpectedIdentity>,
    params: Params,
    network: N,
) -> (&'static Job, Observer)
where
    N: RaftNetworkFactory<Arx> + Send + 'static,
    N::Network: NetSnapshot<Arx, SnapshotData = Vec<u8>>,
{
    let (job, observer, _) = dispatch(
        plan,
        intent,
        expected,
        params,
        network,
        #[cfg(test)]
        None,
    );
    (job, observer)
}

/// Starts a fresh founder under the original retained job and its close gate.
#[cfg(all(not(test), target_os = "linux"))]
pub(super) fn start_founder(
    plan: Plan,
    intent: StartIntent,
    params: Params,
    wrap: std::path::PathBuf,
    store: std::path::PathBuf,
) -> Result<(&'static Job, Observer), founder::Error> {
    let (job, observer, _) = founder::linux::dispatch_founder(plan, intent, params, wrap, store)?;
    Ok((job, observer))
}

fn dispatch<N>(
    plan: Plan,
    intent: StartIntent,
    expected: Option<ExpectedIdentity>,
    params: Params,
    network: N,
    #[cfg(test)] witness: Option<Arc<engine_tests::Witness>>,
) -> (&'static Job, Observer, tokio::task::JoinHandle<()>)
where
    N: RaftNetworkFactory<Arx> + Send + 'static,
    N::Network: NetSnapshot<Arx, SnapshotData = Vec<u8>>,
{
    drive(
        Job::anchor(),
        Some(plan),
        intent,
        expected,
        params,
        network,
        #[cfg(target_os = "linux")]
        None,
        #[cfg(test)]
        witness,
    )
}

#[cfg(target_os = "linux")]
fn dispatch_on(
    job: &'static Job,
    plan: Plan,
    intent: StartIntent,
    expected: Option<ExpectedIdentity>,
    params: Params,
    network: peer_transport::PeerFactory,
    #[cfg(test)] witness: Option<Arc<engine_tests::Witness>>,
) -> (&'static Job, Observer, tokio::task::JoinHandle<()>) {
    drive(
        job,
        Some(plan),
        intent,
        expected,
        params,
        network.clone(),
        Some(network),
        #[cfg(test)]
        witness,
    )
}

#[allow(clippy::too_many_arguments)] // Linux-only retained peer admission binding.
fn drive<N>(
    job: &'static Job,
    plan: Option<Plan>,
    intent: StartIntent,
    expected: Option<ExpectedIdentity>,
    params: Params,
    network: N,
    #[cfg(target_os = "linux")] issuer_network: Option<peer_transport::PeerFactory>,
    #[cfg(test)] witness: Option<Arc<engine_tests::Witness>>,
) -> (&'static Job, Observer, tokio::task::JoinHandle<()>)
where
    N: RaftNetworkFactory<Arx> + Send + 'static,
    N::Network: NetSnapshot<Arx, SnapshotData = Vec<u8>>,
{
    let observer = job.observe();
    #[cfg(test)]
    let supervisor_witness = witness.clone();
    let driver = tokio::spawn(async move {
        let node_id = intent.node.get();
        #[cfg(target_os = "linux")]
        let intent_after_start = intent.peer;
        #[cfg(target_os = "linux")]
        let intent_after_start_cluster = intent.cluster;
        let prepared = tokio::task::spawn_blocking(move || {
            if let Some(plan) = plan {
                job.prepare(plan, &intent, expected.as_ref());
            } else {
                #[cfg(target_os = "linux")]
                learner::prepare(job, &intent);
            }
            #[cfg(target_os = "linux")]
            founder::prepare(job, &intent)
        })
        .await;
        let Ok(_prepared) = prepared else {
            #[cfg(test)]
            if let Some(w) = &witness {
                w.fail("preparation_join");
            }
            job.set(Status::Unconfirmed);
            return;
        };
        #[cfg(test)]
        let original = if let Some(w) = &witness {
            let proof = w.prepared(&job.state());
            match proof {
                Ok(pointers) => Some(pointers),
                Err(reason) => {
                    w.fail(reason);
                    job.set(Status::Unconfirmed);
                    return;
                }
            }
        } else {
            None
        };
        if job.state().status == Status::Prepared {
            let config = match params.config() {
                Ok(c) => Arc::new(c),
                Err(_) => {
                    #[cfg(test)]
                    if let Some(w) = &witness {
                        w.fail("config");
                        job.set(Status::Unconfirmed);
                        return;
                    }
                    job.set(Status::Refused);
                    job.close.notified().await;
                    if job.begin_close() {
                        job.finish_close(None, true, false);
                    }
                    return;
                }
            };
            if job.transfer().is_err() {
                #[cfg(test)]
                if let Some(w) = &witness {
                    w.fail("transfer");
                }
                job.set(Status::Unconfirmed);
                return;
            }
            // Only the test-owned witness can pass the test-build stop.
            #[cfg(test)]
            let startup_release = if let Some((w, original)) = witness.as_ref().zip(original) {
                let proof = w.transferred(&job.state(), original);
                if let Err(reason) = proof {
                    w.fail(reason);
                    job.set(Status::Unconfirmed);
                    return;
                }
                Some(w.take_startup_release())
            } else {
                // The Linux founder entry requires its own fixture capability.
                #[cfg(target_os = "linux")]
                let allowed = founder::linux::hosted_fixture(job) || learner::hosted_fixture(job);
                #[cfg(not(target_os = "linux"))]
                let allowed = false;
                if !allowed {
                    job.set(Status::Prepared);
                    return;
                }
                None
            };
            {
                let (ls, sm) = job.state().adapters.as_ref().unwrap().clone();
                let log_released = ls.0.lock().unwrap().released();
                let sm_released = sm.0.lock().unwrap().released();
                let log_weak = Arc::downgrade(&ls.0);
                let sm_weak = Arc::downgrade(&sm.0);
                job.set(Status::EngineMayExist);
                let startup = tokio::spawn(async move {
                    // Hold the published boundary until the test drops its observer.
                    #[cfg(test)]
                    if let Some(release) = startup_release {
                        release.await.map_err(|_| ())?;
                    }
                    let raft = Engine::new(node_id, config, network, ls, sm.clone())
                        .await
                        .map_err(|_| ())?;
                    let node = ConsensusNode {
                        id: node_id,
                        issue_permits: Arc::new(Semaphore::new(issue::MAX_ISSUING)),
                        read_permits: Arc::new(Semaphore::new(READ_PERMITS)),
                        #[cfg(test)]
                        read_blocking: Arc::default(),
                        raft: Some(raft),
                        sm: Some(sm),
                        log_released,
                        sm_released,
                        log_weak,
                        sm_weak,
                        permits: Arc::new(Semaphore::new(64)),
                        owners: Mutex::default(),
                    };
                    // Install before notification, even without an observer.
                    job.state().node = Some(Arc::new(tokio::sync::Mutex::new(node)));
                    Ok::<_, ()>(())
                })
                .await;
                if !matches!(startup, Ok(Ok(()))) {
                    #[cfg(test)]
                    if let Some(w) = &witness {
                        w.fail("startup_join");
                    }
                    job.set(Status::Unconfirmed);
                    return;
                }
                job.state().adapters = None; // release startup clones before shutdown
                job.set(Status::Running);
                #[cfg(target_os = "linux")]
                if let Some(prepared) = _prepared {
                    match prepared.run(job).await {
                        Ok(()) => {
                            if let Some(network) = issuer_network {
                                job.install_learner_admission(&network);
                            }
                            job.set(Status::FounderReady);
                        }
                        Err(reason) => founder::refuse(job, reason),
                    }
                }
                #[cfg(target_os = "linux")]
                learner::attach(job, &intent_after_start).await;
            }
        }
        #[cfg(target_os = "linux")]
        tokio::select! {
            biased;
            () = job.close.notified() => {}
            () = job.observe_promotion(intent_after_start_cluster) => job.close.notified().await,
        }
        #[cfg(not(target_os = "linux"))]
        job.close.notified().await;
        if !job.begin_close() {
            return;
        }
        #[cfg(target_os = "linux")]
        if !enrollment_ingress::close(job).await
            || !founder::close_transport(job).await
            || !learner::close_transport(job).await
        {
            job.finish_close(None, false, false);
            return;
        }
        let node = job.state().node.clone();
        if let Some(node) = node {
            // The async mutex protects a node which remains in the process
            // anchor even if the close task unwinds or execution is lost.
            let closed = tokio::spawn(async move {
                let mut node = node.lock().await;
                let actual = node.raft.is_some();
                let result = node.shutdown().await;
                let released = node.log_released.is_set()
                    && node.sm_released.is_set()
                    && node.log_weak.upgrade().is_none()
                    && node.sm_weak.upgrade().is_none();
                #[cfg(test)]
                let owners_drained = node
                    .owners
                    .lock()
                    .is_ok_and(|owners| owners.closed && owners.set.is_empty());
                (
                    result,
                    actual,
                    released,
                    #[cfg(test)]
                    owners_drained,
                )
            })
            .await;
            match closed {
                Ok(closed) => {
                    #[cfg(test)]
                    let (result, actual, released, owners_drained) = closed;
                    #[cfg(not(test))]
                    let (result, actual, released) = closed;
                    #[cfg(test)]
                    if let Some(w) = &witness
                        && result.is_ok()
                    {
                        if actual && released && owners_drained {
                            w.finish_close(job, actual);
                        } else {
                            w.fail("Engine join or store/owner release proof");
                            job.finish_close(None, false, actual);
                        }
                        return;
                    }
                    job.finish_close(result.err().map(|e| e.0), released, actual);
                }
                Err(_) => {
                    #[cfg(test)]
                    if let Some(w) = &witness {
                        w.fail("close_join");
                    }
                    job.finish_close(None, false, false);
                }
            }
        } else {
            // Only a proven pre-Engine refusal is disposable. No absent-node
            // reconstruction can convert EngineMayExist into a clean close.
            job.finish_close(None, true, false);
        }
    });
    let supervisor = tokio::spawn(async move {
        if driver.await.is_err() {
            #[cfg(test)]
            if let Some(w) = &supervisor_witness {
                w.fail("driver_join");
            }
            job.set(Status::Unconfirmed);
        }
    });
    (job, observer, supervisor)
}

#[cfg(test)]
mod engine_tests;

#[cfg(target_os = "linux")]
mod enrollment_ingress;

#[cfg(any(test, target_os = "linux"))]
pub(super) mod founder;

#[cfg(target_os = "linux")]
mod learner;

#[cfg(test)]
mod learner_tests;

#[cfg(test)]
mod tests;
