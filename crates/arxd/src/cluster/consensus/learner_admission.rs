//! One retained membership intent, with the existing Owners and proposal unit.
use super::{membership_observation::AdmissionObservation, *};
use crate::cluster::{c13_diagnostics::record, pki::verify::AuthenticatedPeer};
use openraft::{
    ChangeMembers, errors::PreconditionFailed, raft::Precondition,
    type_config::async_runtime::watch::WatchReceiver,
};
use std::{
    future::{Future, poll_fn},
    net::SocketAddr,
    pin::pin,
    sync::atomic::{AtomicBool, Ordering},
    task::Poll,
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

impl Reply {
    pub(super) fn refusal_category(self) -> Option<&'static str> {
        Some(match self {
            Self::Applied(..) => return None,
            Self::Revoked(..) => "revoked",
            Self::Pending => "busy",
            Self::AddressConflict => "address",
            Self::StaleTail => "stale-tail",
            Self::Refused => "refused",
            Self::Unknown => "unconfirmed",
        })
    }
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

fn finish_registration<N, R, G>(
    node: tokio::sync::MutexGuard<'_, N>,
    extra_guard: G,
    register: impl FnOnce(&N, PromotionStart) -> R,
) -> R {
    let (ready, start) = oneshot::channel();
    let registered = register(&node, PromotionStart(start));
    // Owners may poll the worker before registration returns. Release all
    // registration locks before its single fresh observation can touch the node.
    drop(node);
    drop(extra_guard);
    let _ = ready.send(());
    registered
}

pub(super) fn register_with_node<N, R>(
    node: &tokio::sync::Mutex<N>,
    register: impl FnOnce(&N, PromotionStart) -> R,
) -> Result<R, Reply> {
    let node = node.try_lock().map_err(|_| Reply::Refused)?;
    Ok(finish_registration(node, (), register))
}

struct StopGate {
    closed: Mutex<bool>,
    stopped: tokio::sync::Notify,
}

impl StopGate {
    fn new() -> Self {
        Self {
            closed: Mutex::new(false),
            stopped: tokio::sync::Notify::new(),
        }
    }

    fn stop(&self) {
        if let Ok(mut closed) = self.closed.lock() {
            *closed = true;
        }
        self.stopped.notify_waiters();
    }

    fn is_open(&self) -> bool {
        self.closed.lock().is_ok_and(|closed| !*closed)
    }

    fn lock_open(&self) -> Option<std::sync::MutexGuard<'_, bool>> {
        let closed = self.closed.lock().ok()?;
        (!*closed).then_some(closed)
    }

    async fn observe<T>(&self, read: impl Future<Output = T>, on_stop: Reply) -> Result<T, Reply> {
        let mut read = pin!(read);
        let stopped = self.stopped.notified();
        tokio::pin!(stopped);
        stopped.as_mut().enable();
        tokio::select! {
            biased;
            () = stopped.as_mut() => Err(on_stop),
            result = poll_fn(|cx| {
                // The first poll can register an owned read. Order it
                // against stop, and recheck the gate on each later poll.
                let Some(_open) = self.lock_open() else {
                    return Poll::Ready(Err(on_stop));
                };
                read.as_mut().poll(cx).map(Ok)
            }) => result,
        }
    }

    fn report_if_open(&self, report: impl FnOnce()) {
        if let Some(_open) = self.lock_open() {
            report();
        }
    }
}

async fn wait_for_node_bounded<'a, N, R>(
    node: &'a tokio::sync::Mutex<N>,
    gate: &StopGate,
    deadline: Instant,
    on_locked: impl FnOnce(tokio::sync::MutexGuard<'a, N>, std::sync::MutexGuard<'_, bool>) -> R,
    on_busy: impl FnOnce(),
) -> Option<R> {
    let retry_until = peer_registry::store_wait_deadline(Some(deadline));
    let mut on_locked = Some(on_locked);
    loop {
        let stopped = gate.stopped.notified();
        tokio::pin!(stopped);
        // Register the stop waiter before checking `closed` so a concurrent
        // `stop()` cannot miss the wakeup between the check and `select!`.
        stopped.as_mut().enable();
        enum Attempt<R> {
            Done(R),
            Busy,
            StopOrExpired,
        }
        let attempt = {
            let open = gate.lock_open()?;
            if Instant::now() >= retry_until {
                Attempt::StopOrExpired
            } else if let Ok(guard) = node.try_lock() {
                if Instant::now() >= retry_until {
                    drop(guard);
                    Attempt::StopOrExpired
                } else {
                    Attempt::Done(on_locked.take().expect("single lock acquisition")(
                        guard, open,
                    ))
                }
            } else {
                Attempt::Busy
            }
        };
        match attempt {
            Attempt::Done(value) => return Some(value),
            Attempt::StopOrExpired => return None,
            Attempt::Busy => {
                let keep_waiting = tokio::select! {
                    biased;
                    () = stopped.as_mut() => return None,
                    keep = peer_registry::poll_store_until(retry_until) => keep,
                };
                if !keep_waiting {
                    gate.report_if_open(on_busy);
                    return None;
                }
            }
        }
    }
}

async fn register_with_node_bounded<N, R>(
    node: &tokio::sync::Mutex<N>,
    gate: &StopGate,
    deadline: Instant,
    register: impl FnOnce(&N, PromotionStart) -> R,
    on_busy: impl FnOnce(),
) -> Result<R, Reply> {
    wait_for_node_bounded(
        node,
        gate,
        deadline,
        |node, open| finish_registration(node, open, register),
        on_busy,
    )
    .await
    .ok_or(Reply::Refused)
}

pub(super) struct Controller {
    node: Weak<RetainedNode>,
    gate: StopGate,
    intent: Arc<IntentSlot>,
    #[cfg(test)]
    pub(super) hooks: Hooks,
}

impl Controller {
    pub(super) fn new(node: &Arc<RetainedNode>) -> Arc<Self> {
        Arc::new(Self {
            node: Arc::downgrade(node),
            gate: StopGate::new(),
            intent: Arc::default(),
            #[cfg(test)]
            hooks: Hooks::default(),
        })
    }

    pub(super) fn stop(&self) {
        self.gate.stop();
    }

    pub(super) fn open(&self) -> bool {
        self.gate.is_open()
    }

    pub(super) async fn intent_released(&self) {
        self.intent.notified().await;
    }

    pub(super) fn register_promotion(
        &self,
        work: impl std::future::Future<Output = ()> + Send + 'static,
    ) -> Result<oneshot::Receiver<()>, Reply> {
        let _open = self.gate.lock_open().ok_or(Reply::Refused)?;
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

    fn record_diagnostic(&self, stage: &'static str, category: &'static str) {
        #[cfg(test)]
        if let Ok(mut recorded) = self.hooks.diagnostics.lock() {
            recorded.push((stage, category));
        }
        record(stage, category);
    }

    async fn lock_node_for_stage<'a>(
        &self,
        node: &'a RetainedNode,
        deadline: Instant,
        stage: &'static str,
    ) -> Option<tokio::sync::MutexGuard<'a, ConsensusNode>> {
        wait_for_node_bounded(
            node,
            &self.gate,
            deadline,
            |guard, open| {
                drop(open);
                guard
            },
            || self.record_diagnostic(stage, "node-busy"),
        )
        .await
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
        let result = self
            .register(peer, address, deadline, submitted.clone())
            .await;
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

    async fn register(
        self: &Arc<Self>,
        peer: AuthenticatedPeer,
        address: SocketAddr,
        deadline: Instant,
        submitted: Arc<AtomicBool>,
    ) -> Result<oneshot::Receiver<Reply>, Reply> {
        peer.authenticated_identity().map_err(|_| Reply::Refused)?;
        {
            let _open = self.gate.lock_open().ok_or(Reply::Refused)?;
            if Instant::now() >= deadline {
                return Err(Reply::Refused);
            }
        }
        let intent = Intent::waking(&self.intent)?;
        let node = self.node.upgrade().ok_or(Reply::Refused)?;
        let controller = self.clone();
        register_with_node_bounded(
            &node,
            &self.gate,
            deadline,
            |node, start| {
                node.register_admission(
                    intent,
                    start.after_release(
                        async move { controller.run(peer, address, deadline, &submitted).await },
                        Reply::Refused,
                    ),
                )
            },
            || self.record_diagnostic("admission-register", "node-busy"),
        )
        .await?
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
            let Some(node) = self
                .lock_node_for_stage(&node, deadline, "admission-worker")
                .await
            else {
                return Reply::Refused;
            };
            let Some(raft) = node.raft.clone() else {
                return Reply::Refused;
            };
            let observed = self
                .gate
                .observe(
                    node.admission_observation(peer, address, None),
                    Reply::Refused,
                )
                .await;
            (raft, observed)
        };
        let observed = match observed {
            Ok(observed) => observed,
            Err(reply) => return reply,
        };
        let (leader, membership, tail) = match observed {
            Ok(AdmissionObservation::Answer(reply)) => return reply,
            Ok(AdmissionObservation::Add {
                leader,
                membership,
                tail,
            }) => (leader, membership, tail),
            Err(error) => {
                use super::membership_observation::MembershipReadError;
                record(
                    "admission-membership",
                    match error {
                        MembershipReadError::Busy => "busy",
                        MembershipReadError::NotLeader => "not-leader",
                        MembershipReadError::Unavailable => "unavailable",
                        MembershipReadError::RetryLater => "view-changed",
                        MembershipReadError::PeerRefused => "identity",
                    },
                );
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
        let result = self
            .submit_membership(deadline, submitted, || {
                raft.change_membership_if(add, true, guards)
            })
            .await;
        let result = match result {
            Ok(result) => result,
            Err(reply) => return reply,
        };
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
        self.confirm_admission(&node, peer, address, position, deadline)
            .await
    }

    async fn submit_membership<F: Future>(
        &self,
        deadline: Instant,
        submitted: &AtomicBool,
        dispatch: impl FnOnce() -> F,
    ) -> Result<F::Output, Reply> {
        let mut proposal = pin!(async { dispatch().await });
        let mut started = false;
        poll_fn(|cx| {
            if started {
                return proposal.as_mut().poll(cx).map(Ok);
            }
            let Some(_open) = self.gate.lock_open() else {
                return Poll::Ready(Err(Reply::Refused));
            };
            // Publish before the final deadline check so the caller cannot
            // time out with Refused and then have a proposal start.
            submitted.store(true, Ordering::SeqCst);
            if Instant::now() >= deadline {
                submitted.store(false, Ordering::SeqCst);
                return Poll::Ready(Err(Reply::Refused));
            }
            // The Raft API starts on poll. Serialize that handoff with stop
            // and let the owner await any pending result outside the gate.
            started = true;
            #[cfg(test)]
            self.hooks.calls.fetch_add(1, Ordering::Relaxed);
            proposal.as_mut().poll(cx).map(Ok)
        })
        .await
    }

    async fn confirm_admission(
        &self,
        node: &RetainedNode,
        peer: AuthenticatedPeer,
        address: SocketAddr,
        position: LogPos,
        deadline: Instant,
    ) -> Reply {
        let Some(node) = self
            .lock_node_for_stage(node, deadline, "admission-confirm")
            .await
        else {
            return Reply::Unknown;
        };
        match self
            .gate
            .observe(
                node.admission_observation(peer, address, Some(position)),
                Reply::Unknown,
            )
            .await
        {
            Ok(Ok(AdmissionObservation::Answer(reply))) | Err(reply) => reply,
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
    pub(super) diagnostics: Mutex<Vec<(&'static str, &'static str)>>,
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
