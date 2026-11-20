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
    Closing,
    Clean,
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
}
pub(super) struct Job {
    state: Mutex<State>,
    changed: watch::Sender<Status>,
    close: Notify,
    #[cfg(test)]
    gate: Mutex<Option<(Arc<std::sync::Barrier>, Arc<std::sync::Barrier>)>>,
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
            }),
            changed,
            close: Notify::new(),
            #[cfg(test)]
            gate: Mutex::new(None),
        }))
    }
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        // Poison is uncertainty, never disposal authority.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn set(&self, status: Status) {
        let mut s = self.state();
        if matches!(s.status, Status::Unconfirmed | Status::Clean) {
            return;
        }
        if status == Status::EngineMayExist {
            s.engine_may_exist = true;
        }
        s.status = status;
        self.changed.send_replace(status);
    }
    pub(super) fn observe(&self) -> Observer {
        Observer(self.changed.subscribe())
    }
    pub(super) fn request_close(&self) -> Observer {
        let mut s = self.state();
        if !s.close_requested {
            s.close_requested = true;
            self.close.notify_one();
        }
        self.observe()
    }
    fn prepare(&self, plan: Plan, intent: &StartIntent, expected: Option<&ExpectedIdentity>) {
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
        if let Some((entered, release)) = self.gate.lock().unwrap().as_ref() {
            entered.wait();
            release.wait();
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
            s.owner = None;
            s.status = Status::Clean;
        }
        self.changed.send_replace(s.status);
    }
    // Module-private composition loan, never a generic caller capability.
    // Future publication composition must live here and retain this phase/lock
    // guard; this slice performs no reservation or publication.
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
    let job = Job::anchor();
    let observer = job.observe();
    let driver = tokio::spawn(async move {
        let node_id = intent.node.get();
        let prepared = tokio::task::spawn_blocking(move || {
            job.prepare(plan, &intent, expected.as_ref());
        })
        .await;
        if prepared.is_err() {
            job.set(Status::Unconfirmed);
            return;
        }
        if job.state().status == Status::Prepared {
            let config = match params.config() {
                Ok(c) => Arc::new(c),
                Err(_) => {
                    job.set(Status::Refused);
                    job.close.notified().await;
                    if job.begin_close() {
                        job.finish_close(None, true, false);
                    }
                    return;
                }
            };
            if job.transfer().is_err() {
                job.set(Status::Unconfirmed);
                return;
            }
            // Test execution is structurally stopped before Engine; releasing a
            // test barrier cannot cause a background Engine continuation.
            #[cfg(test)]
            {
                let _ = (&config, &network, node_id);
                job.set(Status::Prepared);
                return;
            }
            #[cfg(not(test))]
            {
                let (ls, sm) = job.state().adapters.as_ref().unwrap().clone();
                let log_released = ls.0.lock().unwrap().released();
                let sm_released = sm.0.lock().unwrap().released();
                let log_weak = Arc::downgrade(&ls.0);
                let sm_weak = Arc::downgrade(&sm.0);
                job.set(Status::EngineMayExist);
                let startup = tokio::spawn(async move {
                    let raft = Engine::new(node_id, config, network, ls, sm.clone())
                        .await
                        .map_err(|_| ())?;
                    let node = ConsensusNode {
                        id: node_id,
                        issue_permits: Arc::new(Semaphore::new(issue::MAX_ISSUING)),
                        read_permits: Arc::new(Semaphore::new(READ_PERMITS)),
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
                    job.set(Status::Unconfirmed);
                    return;
                }
                job.state().adapters = None; // release startup clones before shutdown
                job.set(Status::Running);
            }
        }
        job.close.notified().await;
        if !job.begin_close() {
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
                (result, actual, released)
            })
            .await;
            match closed {
                Ok((result, actual, released)) => {
                    job.finish_close(result.err().map(|e| e.0), released, actual);
                }
                Err(_) => job.finish_close(None, false, false),
            }
        } else {
            // Only a proven pre-Engine refusal is disposable. No absent-node
            // reconstruction can convert EngineMayExist into a clean close.
            job.finish_close(None, true, false);
        }
    });
    tokio::spawn(async move {
        if driver.await.is_err() {
            job.set(Status::Unconfirmed);
        }
    });
    (job, observer)
}

#[cfg(test)]
mod tests;
