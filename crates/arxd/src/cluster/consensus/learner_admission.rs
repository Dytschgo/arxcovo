//! One retained membership intent, with the existing Owners and proposal unit.
use super::{membership_observation::AdmissionObservation, *};
use crate::cluster::pki::verify::AuthenticatedPeer;
use openraft::{
    ChangeMembers, errors::PreconditionFailed, raft::Precondition,
    type_config::async_runtime::watch::WatchReceiver,
};
use std::{
    net::SocketAddr,
    sync::atomic::{AtomicBool, Ordering},
};
#[derive(Default)]
pub(super) struct IntentSlot {
    occupied: Arc<AtomicBool>,
    released: Arc<tokio::sync::Notify>,
}

impl IntentSlot {
    pub(super) async fn notified(&self) {
        self.released.notified().await;
    }

    #[cfg(test)]
    fn load(&self, order: Ordering) -> bool {
        self.occupied.load(order)
    }
}

pub(super) struct Intent {
    occupied: Arc<AtomicBool>,
    released: Option<Arc<tokio::sync::Notify>>,
}
impl Intent {
    fn reserve(slot: &Arc<AtomicBool>) -> Result<Self, Reply> {
        slot.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| Reply::Pending)?;
        Ok(Self {
            occupied: slot.clone(),
            released: None,
        })
    }

    pub(super) fn waking(slot: &IntentSlot) -> Result<Self, Reply> {
        let mut intent = Self::reserve(&slot.occupied)?;
        intent.released = Some(slot.released.clone());
        Ok(intent)
    }
}

impl Drop for Intent {
    fn drop(&mut self) {
        self.occupied.store(false, Ordering::Release);
        if let Some(released) = &self.released {
            released.notify_one();
        }
    }
}

pub(super) const BUDGET: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Reply {
    Applied(LogPos, SocketAddr),
    Revoked(LogPos, SocketAddr),
    Pending,
    AddressConflict,
    StaleTail,
    Refused,
    Unknown,
}

pub(super) fn covers(progress: LogPos, target: LogPos) -> bool {
    progress.index > target.index || progress == target
}

type RetainedNode = tokio::sync::Mutex<ConsensusNode>;

pub(super) struct PromotionStart(oneshot::Receiver<()>);

impl PromotionStart {
    async fn after_release<T>(self, work: impl std::future::Future<Output = T>, refused: T) -> T {
        if self.0.await.is_ok() {
            work.await
        } else {
            refused
        }
    }

    pub(super) async fn run(self, work: impl std::future::Future<Output = ()>) {
        self.after_release(work, ()).await;
    }
}

pub(super) fn register_with_node<N, R>(
    node: &tokio::sync::Mutex<N>,
    register: impl FnOnce(&N, PromotionStart) -> R,
) -> Result<R, Reply> {
    let node = node.try_lock().map_err(|_| Reply::Refused)?;
    let (ready, start) = oneshot::channel();
    let registered = register(&node, PromotionStart(start));
    // Owners may poll the worker before registration returns. Release this
    // borrow before its single fresh observation can touch the node.
    drop(node);
    let _ = ready.send(());
    Ok(registered)
}

pub(super) struct Controller {
    node: Weak<RetainedNode>,
    closed: Mutex<bool>,
    intent: Arc<IntentSlot>,
    #[cfg(test)]
    pub(super) hooks: Hooks,
}

impl Controller {
    pub(super) fn new(node: &Arc<RetainedNode>) -> Arc<Self> {
        Arc::new(Self {
            node: Arc::downgrade(node),
            closed: Mutex::new(false),
            intent: Arc::default(),
            #[cfg(test)]
            hooks: Hooks::default(),
        })
    }

    pub(super) fn stop(&self) {
        if let Ok(mut closed) = self.closed.lock() {
            *closed = true;
        }
    }

    pub(super) fn open(&self) -> bool {
        self.closed.lock().is_ok_and(|closed| !*closed)
    }

    pub(super) async fn intent_released(&self) {
        self.intent.notified().await;
    }

    pub(super) fn register_promotion(
        &self,
        work: impl std::future::Future<Output = ()> + Send + 'static,
    ) -> Result<oneshot::Receiver<()>, Reply> {
        let closed = self.closed.lock().map_err(|_| Reply::Refused)?;
        if *closed {
            return Err(Reply::Refused);
        }
        let intent = Intent::waking(&self.intent)?;
        let node = self.node.upgrade().ok_or(Reply::Refused)?;
        register_with_node(&node, |node, start| {
            node.register_admission(intent, start.run(work))
        })?
    }

    pub(super) fn progress(&self, learner: u64) -> Option<(LogPos, Option<LogPos>)> {
        let node = self.node.upgrade()?;
        let node = node.try_lock().ok()?;
        let metrics = node.raft.as_ref()?.metrics();
        let metrics = metrics.borrow_watched();
        if metrics.running_state.is_err()
            || metrics.current_leader != Some(node.id)
            || !metrics.vote.committed
            || metrics.vote.leader_id.node_id != node.id
        {
            return None;
        }
        let matched = metrics.replication.as_ref()?.get(&learner)?.map(pos);
        Some((pos(metrics.cluster_committed?), matched))
    }

    pub(super) async fn admit(
        self: &Arc<Self>,
        peer: AuthenticatedPeer,
        address: SocketAddr,
        deadline: Instant,
    ) -> Reply {
        #[cfg(test)]
        self.hooks.requests.fetch_add(1, Ordering::Relaxed);
        let submitted = Arc::new(AtomicBool::new(false));
        let result = self.register(peer, address, deadline, submitted.clone());
        let rx = match result {
            Ok(rx) => rx,
            Err(answer) => return answer,
        };
        match timeout_at(deadline, rx).await {
            Ok(Ok(reply)) => reply,
            _ if submitted.load(Ordering::SeqCst) => Reply::Unknown,
            _ => Reply::Refused,
        }
    }

    fn register(
        self: &Arc<Self>,
        peer: AuthenticatedPeer,
        address: SocketAddr,
        deadline: Instant,
        submitted: Arc<AtomicBool>,
    ) -> Result<oneshot::Receiver<Reply>, Reply> {
        peer.authenticated_identity().map_err(|_| Reply::Refused)?;
        let closed = self.closed.lock().map_err(|_| Reply::Refused)?;
        if *closed || Instant::now() >= deadline {
            return Err(Reply::Refused);
        }
        let intent = Intent::waking(&self.intent)?;
        let node = self.node.upgrade().ok_or(Reply::Refused)?;
        let controller = self.clone();
        register_with_node(&node, |node, start| {
            node.register_admission(
                intent,
                start.after_release(
                    async move { controller.run(peer, address, deadline, &submitted).await },
                    Reply::Refused,
                ),
            )
        })?
    }

    async fn run(
        &self,
        peer: AuthenticatedPeer,
        address: SocketAddr,
        deadline: Instant,
        submitted: &AtomicBool,
    ) -> Reply {
        let Some(node) = self.node.upgrade() else {
            return Reply::Refused;
        };
        let (raft, observed) = {
            let Ok(node) = node.try_lock() else {
                return Reply::Refused;
            };
            let Some(raft) = node.raft.clone() else {
                return Reply::Refused;
            };
            (raft, node.admission_observation(peer, address, None).await)
        };
        let (leader, membership, tail) = match observed {
            Ok(AdmissionObservation::Answer(reply)) => return reply,
            Ok(AdmissionObservation::Add {
                leader,
                membership,
                tail,
            }) => (leader, membership, tail),
            Err(error) => {
                #[cfg(test)]
                if self.hooks.active.load(Ordering::Relaxed) {
                    eprintln!("C10 fresh membership read refused: {error:?}");
                }
                let _ = error;
                return Reply::Refused;
            }
        };
        #[cfg(test)]
        if !self.hooks.pause(deadline).await {
            return Reply::Refused;
        }

        submitted.store(true, Ordering::SeqCst);
        if Instant::now() >= deadline {
            return Reply::Refused;
        }
        let add = ChangeMembers::AddNodes(BTreeMap::from([(
            peer.node().get(),
            BasicNode::new(address.to_string()),
        )]));
        let guards = [
            Precondition::LastMembershipLogId {
                last_membership_log_id: Some(membership),
            },
            Precondition::CommittedLeaderId {
                committed_leader_id: leader,
            },
            Precondition::LastLogId {
                last_log_id: Some(tail),
            },
        ];
        #[cfg(test)]
        self.hooks.calls.fetch_add(1, Ordering::Relaxed);
        let result = raft.change_membership_if(add, true, guards).await;
        let response = match result {
            Ok(response) => response,
            Err(RaftError::APIError(ClientWriteError::PreconditionFailed(
                PreconditionFailed::LastLogIdMismatch { expected, actual },
            ))) if expected == Some(tail) => {
                #[cfg(test)]
                if let Some(actual) = actual {
                    *self.hooks.stale.lock().unwrap() = Some((pos(tail), pos(actual)));
                }
                let _ = actual;
                return Reply::StaleTail;
            }
            Err(RaftError::APIError(ClientWriteError::PreconditionFailed(_))) => {
                #[cfg(test)]
                if self.hooks.active.load(Ordering::Relaxed) {
                    eprintln!("C10 membership API refused a leader or membership guard");
                }
                return Reply::Refused;
            }
            Err(error) => {
                #[cfg(test)]
                if self.hooks.active.load(Ordering::Relaxed) {
                    eprintln!("C10 membership API refused: {error}");
                }
                let _ = error;
                return Reply::Unknown;
            }
        };
        let position = pos(response.log_id);
        let Ok(node) = node.try_lock() else {
            return Reply::Unknown;
        };
        match node
            .admission_observation(peer, address, Some(position))
            .await
        {
            Ok(AdmissionObservation::Answer(reply)) => reply,
            _ => {
                #[cfg(test)]
                if self.hooks.active.load(Ordering::Relaxed) {
                    eprintln!("C10 post-apply membership confirmation unavailable at {position:?}");
                }
                Reply::Unknown
            }
        }
    }
}

impl ConsensusNode {
    fn register_admission<T: Send + 'static>(
        &self,
        intent: Intent,
        run: impl std::future::Future<Output = T> + Send + 'static,
    ) -> Result<oneshot::Receiver<T>, Reply> {
        let permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| Reply::Refused)?;
        let mut owners = self.owners.lock().map_err(|_| Reply::Refused)?;
        if owners.closed || self.raft.is_none() {
            return Err(Reply::Refused);
        }
        while owners.set.try_join_next().is_some() {}
        let (tx, rx) = oneshot::channel();
        owners.set.spawn(async move {
            let reply = run.await;
            drop(permit);
            drop(intent);
            let _ = tx.send(reply);
        });
        Ok(rx)
    }
}

#[cfg(test)]
type Barrier = (oneshot::Sender<()>, oneshot::Receiver<()>);

#[cfg(test)]
#[derive(Default)]
pub(super) struct Hooks {
    pub(super) active: AtomicBool,
    pub(super) requests: std::sync::atomic::AtomicUsize,
    pub(super) calls: std::sync::atomic::AtomicUsize,
    pub(super) stale: Mutex<Option<(LogPos, LogPos)>>,
    pub(super) suppress_success: AtomicBool,
    pub(super) lost_reply: Mutex<Option<Reply>>,
    pub(super) barrier: Mutex<Option<Barrier>>,
    pub(super) promotion: super::initial_promotion::Hooks,
}

#[cfg(test)]
impl Hooks {
    async fn pause(&self, deadline: Instant) -> bool {
        let barrier = self.barrier.lock().unwrap().take();
        if let Some((entered, release)) = barrier {
            entered.send(()).is_ok() && matches!(timeout_at(deadline, release).await, Ok(Ok(())))
        } else {
            true
        }
    }
}

#[cfg(test)]
mod tests;
