//! Local applied authorization for owned peer transport.
//! Temporary ownership can run store destructors; this is not TLS task ownership.
use super::adapter::SmAdapter;
use crate::cluster::{
    enrollment::{ClusterId, KeyDigest, NodeId, PeerRefusal},
    pki::verify::VerifyRefusal,
};
use std::{
    sync::{Arc, Mutex, TryLockError, Weak},
    time::Duration,
};
use tokio::time::{Instant, sleep_until};

// Each transport check can wait 4 ms. The caller's 75 ms absolute
// AppendEntries deadline still applies across the entire exchange.
const STORE_WAIT: Duration = Duration::from_millis(4);
const STORE_POLL: Duration = Duration::from_micros(250);

type AppliedStore = Weak<Mutex<super::StateStore>>;

pub(super) enum ObservationRefusal {
    StoreBusy,
    Refused(VerifyRefusal),
}

impl From<VerifyRefusal> for ObservationRefusal {
    fn from(reason: VerifyRefusal) -> Self {
        Self::Refused(reason)
    }
}

impl From<ObservationRefusal> for VerifyRefusal {
    fn from(reason: ObservationRefusal) -> Self {
        match reason {
            ObservationRefusal::StoreBusy => Self::Unavailable,
            ObservationRefusal::Refused(reason) => reason,
        }
    }
}

fn observe_store<T>(
    store: &AppliedStore,
    acquisition_deadline: Option<Instant>,
    observe: impl FnOnce(&super::StateStore) -> Result<T, ObservationRefusal>,
) -> Result<T, ObservationRefusal> {
    let store = store.upgrade().ok_or(VerifyRefusal::Unavailable)?;
    let guard = match store.try_lock() {
        Ok(guard) => guard,
        Err(TryLockError::WouldBlock) if !store.is_poisoned() => {
            return Err(ObservationRefusal::StoreBusy);
        }
        Err(_) => return Err(VerifyRefusal::Unavailable.into()),
    };
    if acquisition_deadline.is_some_and(|deadline| Instant::now() >= deadline)
        || guard.is_poisoned()
    {
        return Err(VerifyRefusal::Unavailable.into());
    }
    observe(&guard)
}

pub(super) async fn wait_for_store<T>(
    mut observe: impl FnMut(Option<Instant>) -> Result<T, ObservationRefusal>,
) -> Result<T, VerifyRefusal> {
    let deadline = Instant::now() + STORE_WAIT;
    let mut acquisition_deadline = None;
    loop {
        match observe(acquisition_deadline) {
            Err(ObservationRefusal::StoreBusy) => {}
            result => return result.map_err(Into::into),
        }
        // The synchronous observation returned with neither lock held.
        let now = Instant::now();
        if now >= deadline {
            return Err(VerifyRefusal::Unavailable);
        }
        sleep_until((now + STORE_POLL).min(deadline)).await;
        if Instant::now() >= deadline {
            return Err(VerifyRefusal::Unavailable);
        }
        acquisition_deadline = Some(deadline);
    }
}

#[derive(Clone)]
pub(super) struct AppliedRegistry(AppliedStore);

impl AppliedRegistry {
    pub(super) fn new(sm: &SmAdapter) -> Self {
        Self(Arc::downgrade(&sm.0))
    }

    pub(super) async fn authorize(
        &self,
        cluster: ClusterId,
        node: NodeId,
        key: KeyDigest,
    ) -> Result<(), VerifyRefusal> {
        wait_for_store(|deadline| self.observe(cluster, node, key, deadline)).await
    }

    fn observe(
        &self,
        cluster: ClusterId,
        node: NodeId,
        key: KeyDigest,
        deadline: Option<Instant>,
    ) -> Result<(), ObservationRefusal> {
        observe_store(&self.0, deadline, |store| {
            store
                .state()
                .authorize_peer(cluster, node, key)
                .map_err(|reason| {
                    match reason {
                        PeerRefusal::WrongCluster => VerifyRefusal::WrongCluster,
                        PeerRefusal::NotMember => VerifyRefusal::NotMember,
                        PeerRefusal::KeyMismatch => VerifyRefusal::KeyMismatch,
                    }
                    .into()
                })
        })
    }

    #[cfg(test)]
    pub(super) fn immediate(&self) -> crate::cluster::pki::verify::Registry {
        let registry = self.clone();
        Arc::new(move |cluster, node, key| {
            registry
                .observe(cluster, node, key, None)
                .map_err(Into::into)
        })
    }
}

#[cfg(test)]
pub(super) fn registry(sm: &SmAdapter) -> crate::cluster::pki::verify::Registry {
    AppliedRegistry::new(sm).immediate()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Lane {
    Strict,
    Receipt,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Phase {
    Bootstrap,
    Strict,
    Refused,
}

pub(super) struct LearnerGate {
    store: AppliedStore,
    identity: Arc<crate::cluster::join_receipt::ExpectedIdentity>,
    address: std::net::SocketAddr,
    clock: Arc<dyn super::checked_clock::CheckedClock>,
    phase: std::sync::Mutex<Phase>,
    receipt: bool,
}

impl LearnerGate {
    pub(super) fn new(
        sm: &SmAdapter,
        identity: Arc<crate::cluster::join_receipt::ExpectedIdentity>,
        address: std::net::SocketAddr,
        clock: Arc<dyn super::checked_clock::CheckedClock>,
        receipt: bool,
    ) -> Arc<Self> {
        Arc::new(Self {
            store: Arc::downgrade(&sm.0),
            identity,
            address,
            clock,
            phase: std::sync::Mutex::new(if receipt {
                Phase::Bootstrap
            } else {
                Phase::Strict
            }),
            receipt,
        })
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn has_receipt(&self) -> bool {
        self.receipt
    }

    pub(super) fn phase(&self) -> Result<Phase, VerifyRefusal> {
        self.observe(None, |_, phase| Ok(phase)).map_err(Into::into)
    }

    fn observe<T>(
        &self,
        acquisition_deadline: Option<Instant>,
        f: impl FnOnce(&super::StateStore, Phase) -> Result<T, VerifyRefusal>,
    ) -> Result<T, ObservationRefusal> {
        observe_store(&self.store, acquisition_deadline, |store| {
            if self.clock.sample_ms().is_err() {
                return Err(VerifyRefusal::Unavailable.into());
            }
            let mut phase = self
                .phase
                .try_lock()
                .map_err(|_| VerifyRefusal::Unavailable)?;
            let original = self.identity.origin.position();
            if *phase != Phase::Refused {
                if let Some(applied) = store.applied().filter(|p| p.index >= original.index) {
                    *phase = if crossed(
                        &self.identity,
                        self.address,
                        applied,
                        store.membership(),
                        store.state(),
                    ) {
                        Phase::Strict
                    } else {
                        Phase::Refused
                    };
                } else if *phase == Phase::Strict
                    || store.membership().is_some_and(|(_, membership)| {
                        !matches_address(&self.identity, self.address, membership)
                    })
                {
                    *phase = Phase::Refused;
                }
            }
            f(store, *phase).map_err(Into::into)
        })
    }

    #[cfg(any(test, target_os = "linux"))]
    pub(super) fn inspect_strict<T>(
        &self,
        deadline: Option<Instant>,
        inspect: impl FnOnce(&super::StateStore) -> T,
    ) -> Result<T, ObservationRefusal> {
        self.observe(deadline, |store, phase| {
            if phase != Phase::Strict {
                return Err(VerifyRefusal::NotMember);
            }
            Ok(inspect(store))
        })
    }

    pub(super) fn admission_progress(
        &self,
    ) -> Result<(Phase, Option<super::LogPos>), VerifyRefusal> {
        self.observe(None, |store, phase| Ok((phase, store.applied())))
            .map_err(Into::into)
    }

    pub(super) fn admission_anchor(
        &self,
    ) -> Result<(crate::cluster::enrollment::NodeId, &[u8; 32]), VerifyRefusal> {
        use crate::cluster::{enrollment::NodeId, join_receipt::Origin};
        let Origin::Redeem { anchors, .. } = &self.identity.origin else {
            return Err(VerifyRefusal::NotMember);
        };
        let leader = self.identity.origin.position().leader;
        let node = NodeId::new(leader).ok_or(VerifyRefusal::NotMember)?;
        let i = anchors
            .binary_search_by_key(&leader, |(id, _)| *id)
            .map_err(|_| VerifyRefusal::NotMember)?;
        Ok((node, &anchors[i].1))
    }

    pub(super) fn authorize_admission_anchor(
        &self,
        cluster: crate::cluster::enrollment::ClusterId,
        node: crate::cluster::enrollment::NodeId,
        key: crate::cluster::enrollment::KeyDigest,
    ) -> Result<(), VerifyRefusal> {
        if cluster != self.identity.cluster || self.admission_anchor()? != (node, key.as_bytes()) {
            return Err(VerifyRefusal::NotMember);
        }
        self.try_authorize(cluster, node, key, true, None)
            .map(|_| ())
            .map_err(Into::into)
    }

    /// Applied history may pass E between observations. Check any copy of
    /// that row on the live Append path before it can reach the state machine.
    pub(super) fn matches_original_append(
        &self,
        append: &openraft::raft::AppendEntriesRequest<super::Arx>,
    ) -> bool {
        let original = self.identity.origin.position();
        append
            .prev_log_id
            .into_iter()
            .chain(append.entries.iter().map(|entry| entry.log_id))
            .map(super::pos)
            .all(|position| position.index != original.index || position == original)
    }

    pub(super) async fn authorize(
        &self,
        cluster: crate::cluster::enrollment::ClusterId,
        node: crate::cluster::enrollment::NodeId,
        key: crate::cluster::enrollment::KeyDigest,
        bootstrap_allowed: bool,
    ) -> Result<Lane, VerifyRefusal> {
        wait_for_store(|deadline| {
            self.try_authorize(cluster, node, key, bootstrap_allowed, deadline)
        })
        .await
    }

    fn try_authorize(
        &self,
        cluster: crate::cluster::enrollment::ClusterId,
        node: crate::cluster::enrollment::NodeId,
        key: crate::cluster::enrollment::KeyDigest,
        bootstrap_allowed: bool,
        acquisition_deadline: Option<Instant>,
    ) -> Result<Lane, ObservationRefusal> {
        self.observe(acquisition_deadline, |store, phase| {
            if phase == Phase::Refused || (phase == Phase::Bootstrap && !bootstrap_allowed) {
                return Err(VerifyRefusal::Unavailable);
            }
            match store.state().authorize_peer(cluster, node, key) {
                Ok(()) => Ok(Lane::Strict),
                Err(PeerRefusal::WrongCluster) => Err(VerifyRefusal::WrongCluster),
                Err(PeerRefusal::KeyMismatch) => Err(VerifyRefusal::KeyMismatch),
                Err(PeerRefusal::NotMember) => {
                    use crate::cluster::join_receipt::Origin;
                    if self.receipt
                        && phase == Phase::Bootstrap
                        && let Origin::Redeem { anchors, .. } = &self.identity.origin
                        && anchors
                            .binary_search_by_key(&node.get(), |(id, _)| *id)
                            .is_ok_and(|i| &anchors[i].1 == key.as_bytes())
                    {
                        return Ok(Lane::Receipt);
                    }
                    Err(VerifyRefusal::NotMember)
                }
            }
        })
    }
}

pub(super) fn crossed(
    expected: &crate::cluster::join_receipt::ExpectedIdentity,
    address: std::net::SocketAddr,
    applied: super::LogPos,
    membership: Option<&(super::LogPos, super::MembershipBlob)>,
    state: &crate::cluster::enrollment::EnrollmentState,
) -> bool {
    let original = expected.origin.position();
    if applied.index < original.index
        || (applied.index == original.index && applied != original)
        || applied < original
        || state
            .authorize_peer(expected.cluster, expected.node, expected.key)
            .is_err()
    {
        return false;
    }
    membership
        .filter(|(p, _)| p.term <= applied.term && (p.index < applied.index || *p == applied))
        .is_some_and(|(_, membership)| matches_address(expected, address, membership))
}

fn matches_address(
    expected: &crate::cluster::join_receipt::ExpectedIdentity,
    address: std::net::SocketAddr,
    membership: &super::MembershipBlob,
) -> bool {
    super::decode(membership).is_ok_and(|membership| {
        membership
            .get_node(&expected.node.get())
            .is_none_or(|node| node.addr == address.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::{
        enrollment::{Command, FoundMembers, KeyDigest, NodeId, Outcome},
        state_store::{Entry, Init, LogPos, StateStore},
    };
    use crate::service::application::tests::private_temp;
    use std::{
        future::Future,
        sync::mpsc,
        task::{Context, Poll, Waker},
        thread,
    };

    fn node(n: u64) -> NodeId {
        NodeId::new(n).unwrap()
    }
    fn key(n: u8) -> KeyDigest {
        KeyDigest::of_spki(&[n]).unwrap()
    }
    fn position(index: u64) -> LogPos {
        LogPos {
            term: 1,
            leader: 1,
            index,
        }
    }
    fn fixture() -> (tempfile::TempDir, SmAdapter) {
        let dir = private_temp();
        let mut store = StateStore::open(
            dir.path(),
            Init {
                cluster: [1; 16],
                first_free: node(3),
            },
        )
        .unwrap();
        let result = store
            .apply(
                position(0),
                &Entry::Enrollment(Command::Found {
                    cluster: [1; 16],
                    members: FoundMembers::new([(node(1), key(1)), (node(2), key(2))]).unwrap(),
                    proposed_at_ms: 100,
                }),
            )
            .unwrap();
        assert!(matches!(result, Some(Outcome::Founded { replay: false })));
        (dir, SmAdapter(Arc::new(Mutex::new(store))))
    }

    fn learner_gate(sm: &SmAdapter) -> Arc<LearnerGate> {
        use crate::cluster::join_receipt::{ExpectedIdentity, Origin};
        LearnerGate::new(
            sm,
            Arc::new(ExpectedIdentity {
                cluster: [1; 16],
                node: node(2),
                key: key(2),
                ca: [0; 32],
                origin: Origin::Redeem {
                    term: 1,
                    leader: 1,
                    index: 1,
                    anchors: vec![(1, *key(1).as_bytes())],
                },
            }),
            "192.168.0.2:7001".parse().unwrap(),
            Arc::new(super::super::checked_clock::HostClock::new(0)),
            true,
        )
    }

    #[tokio::test(start_paused = true)]
    async fn learner_inbound_authorizes_when_applied_store_is_released_within_budget() {
        let (_dir, sm) = fixture();
        let gate = learner_gate(&sm);
        let mut authorization = std::pin::pin!(gate.authorize([1; 16], node(1), key(1), true));
        {
            let _held = sm.0.lock().unwrap();
            assert!(
                authorization
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop()))
                    .is_pending()
            );
        }
        assert_eq!(authorization.await, Ok(Lane::Strict));
    }

    #[tokio::test(start_paused = true)]
    async fn learner_inbound_refuses_when_applied_store_outlasts_budget() {
        let (_dir, sm) = fixture();
        let gate = learner_gate(&sm);
        let (entered, ready) = tokio::sync::oneshot::channel();
        let (release, wait) = mpsc::channel::<()>();
        let holder = tokio::task::spawn_blocking(move || {
            let _held = sm.0.lock().unwrap();
            entered.send(()).unwrap();
            let _ = wait.recv();
        });
        ready.await.unwrap();
        let mut authorization = std::pin::pin!(gate.authorize([1; 16], node(1), key(1), true));
        assert!(
            authorization
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        tokio::time::advance(Duration::from_millis(4)).await;
        assert_eq!(
            authorization
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Err(VerifyRefusal::Unavailable))
        );
        drop(release);
        holder.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn strict_observation_waits_for_store_and_guards_the_operation() {
        use super::super::{BasicNode, Members, encode};
        let (_dir, sm) = fixture();
        let membership = Members::new(
            vec![[1, 2].into()],
            std::collections::BTreeMap::from([
                (1, BasicNode::new("192.168.0.1:7001")),
                (2, BasicNode::new("192.168.0.2:7001")),
            ]),
        )
        .unwrap();
        sm.0.lock()
            .unwrap()
            .apply(
                position(1),
                &Entry::Membership(encode(&membership).unwrap()),
            )
            .unwrap();
        let gate = learner_gate(&sm);
        let mut observation = std::pin::pin!(wait_for_store(|deadline| {
            gate.inspect_strict(deadline, |store| {
                assert!(matches!(sm.0.try_lock(), Err(TryLockError::WouldBlock)));
                store.applied()
            })
        }));
        {
            let _held = sm.0.lock().unwrap();
            assert!(
                observation
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop()))
                    .is_pending()
            );
        }
        assert_eq!(observation.await, Ok(Some(position(1))));
    }

    #[test]
    fn actual_bindings_remove_and_observation_immutability() {
        let (_dir, sm) = fixture();
        let callback = registry(&sm);
        let before = sm.0.lock().unwrap().snapshot();
        assert!(callback([1; 16], node(2), key(2)).is_ok());
        assert_eq!(
            callback([2; 16], node(2), key(2)),
            Err(VerifyRefusal::WrongCluster)
        );
        assert_eq!(
            callback([1; 16], node(3), key(2)),
            Err(VerifyRefusal::NotMember)
        );
        assert_eq!(
            callback([1; 16], node(2), key(3)),
            Err(VerifyRefusal::KeyMismatch)
        );
        assert!(sm.0.lock().unwrap().snapshot() == before);
        let removed =
            sm.0.lock()
                .unwrap()
                .apply(
                    position(1),
                    &Entry::Enrollment(Command::Remove {
                        cluster: [1; 16],
                        node: node(2),
                        proposed_at_ms: 101,
                    }),
                )
                .unwrap();
        assert!(matches!(removed, Some(Outcome::Removed { replay: false })));
        assert_eq!(
            callback([1; 16], node(2), key(2)),
            Err(VerifyRefusal::NotMember)
        );
        assert!(callback([1; 16], node(1), key(1)).is_ok());
    }

    #[test]
    fn held_mutex_refuses_before_release_and_explicit_later_call_accepts() {
        let (_dir, sm) = fixture();
        let callback = registry(&sm);
        let worker_callback = callback.clone();
        let (tx, rx) = mpsc::channel();
        let guard = sm.0.lock().unwrap();
        let spawned = thread::Builder::new().spawn(move || {
            let result = worker_callback([1; 16], node(2), key(2));
            let _ = tx.send(result);
        });
        // Save outcomes only while holding the guard. Even the lock mutant can
        // finish once we release it; every assertion follows release and join.
        let observed = spawned
            .as_ref()
            .ok()
            .map(|_| rx.recv_timeout(Duration::from_secs(5)));
        drop(guard);
        let joined = spawned.map(|worker| worker.join());
        assert!(matches!(joined, Ok(Ok(()))));
        assert!(matches!(
            observed,
            Some(Ok(Err(VerifyRefusal::Unavailable)))
        ));
        assert!(callback([1; 16], node(2), key(2)).is_ok());
    }

    #[test]
    fn mutex_poisoning_refuses_without_recovery() {
        let (_dir, sm) = fixture();
        let callback = registry(&sm);
        let worker_store = sm.clone();
        let spawned = thread::Builder::new().spawn(move || {
            if let Ok(_guard) = worker_store.0.lock() {
                // Deliberately poison the mutex; this panic creates the fixture,
                // rather than asserting a result before cleanup.
                panic!("deliberate registry mutex poison");
            }
        });
        let joined = spawned.map(|worker| worker.join());
        assert!(matches!(joined, Ok(Err(_))));
        assert!(sm.0.is_poisoned());
        assert_eq!(
            callback([1; 16], node(2), key(2)),
            Err(VerifyRefusal::Unavailable)
        );
        assert!(sm.0.is_poisoned());
    }

    #[test]
    fn callback_clones_do_not_retain_store_and_dropped_store_refuses() {
        let (_dir, sm) = fixture();
        let released = sm.0.lock().unwrap().released();
        let weak = Arc::downgrade(&sm.0);
        let before = Arc::strong_count(&sm.0);
        let callback = registry(&sm);
        let clone = callback.clone();
        assert_eq!(Arc::strong_count(&sm.0), before);
        drop(sm);
        assert!(weak.upgrade().is_none());
        // This acknowledges preceding field destructors, not OS/SQLite close success.
        assert!(released.is_set());
        assert_eq!(
            callback([1; 16], node(2), key(2)),
            Err(VerifyRefusal::Unavailable)
        );
        assert_eq!(
            clone([1; 16], node(1), key(1)),
            Err(VerifyRefusal::Unavailable)
        );
    }
}
