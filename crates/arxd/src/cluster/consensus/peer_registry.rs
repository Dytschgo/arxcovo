//! Local applied authorization. Inbound learner authorization can wait briefly for the store.
//! Temporary ownership can run store destructors; this is not TLS task ownership.
use super::adapter::SmAdapter;
use crate::cluster::{
    enrollment::PeerRefusal,
    pki::verify::{Registry, VerifyRefusal},
};
use std::{
    sync::{Arc, Mutex, MutexGuard, TryLockError},
    time::{Duration, Instant},
};

type AppliedStore = Mutex<super::StateStore>;
type StoreLock<'a> = Result<MutexGuard<'a, super::StateStore>, VerifyRefusal>;

fn try_applied_store(store: &AppliedStore) -> StoreLock<'_> {
    store.try_lock().map_err(|_| VerifyRefusal::Unavailable)
}

fn lock_inbound_store(store: &AppliedStore) -> StoreLock<'_> {
    wait_for_applied_store(store, Instant::now, std::thread::sleep)
}

// Two inbound checks can each spend at most 4 ms acquiring the store, a small
// share of the 75 ms replication deadline. Sleep only after WouldBlock.
const LEARNER_STORE_WAIT: Duration = Duration::from_millis(4);
const LEARNER_STORE_POLL: Duration = Duration::from_micros(250);

fn wait_for_applied_store(
    store: &AppliedStore,
    now: impl Fn() -> Instant,
    mut pause: impl FnMut(Duration),
) -> StoreLock<'_> {
    let deadline = now() + LEARNER_STORE_WAIT;
    let mut waited = false;
    loop {
        match store.try_lock() {
            Ok(guard) if !waited || now() < deadline => return Ok(guard),
            Err(TryLockError::WouldBlock) if !store.is_poisoned() => {}
            _ => return Err(VerifyRefusal::Unavailable),
        }

        let remaining = deadline.saturating_duration_since(now());
        if remaining.is_zero() {
            return Err(VerifyRefusal::Unavailable);
        }
        pause(LEARNER_STORE_POLL.min(remaining));
        if now() >= deadline {
            return Err(VerifyRefusal::Unavailable);
        }
        waited = true;
    }
}

pub(super) fn registry(sm: &SmAdapter) -> Registry {
    let store = Arc::downgrade(&sm.0);
    Arc::new(move |cluster, node, key| {
        let store = store.upgrade().ok_or(VerifyRefusal::Unavailable)?;
        let result = {
            let store = store.try_lock().map_err(|_| VerifyRefusal::Unavailable)?;
            store
                .state()
                .authorize_peer(cluster, node, key)
                .map_err(|refusal| match refusal {
                    PeerRefusal::WrongCluster => VerifyRefusal::WrongCluster,
                    PeerRefusal::NotMember => VerifyRefusal::NotMember,
                    PeerRefusal::KeyMismatch => VerifyRefusal::KeyMismatch,
                })
        };
        // Guard first, then temporary owner. If this is the last owner, dropping
        // it may run SQLite/file destructors; no hard wall-time claim follows.
        drop(store);
        result
    })
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
    store: std::sync::Weak<AppliedStore>,
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
        self.observe(try_applied_store, |_, phase| Ok(phase))
    }

    fn observe<T>(
        &self,
        acquire: impl for<'a> FnOnce(&'a AppliedStore) -> StoreLock<'a>,
        f: impl FnOnce(&super::StateStore, Phase) -> Result<T, VerifyRefusal>,
    ) -> Result<T, VerifyRefusal> {
        let store = self.store.upgrade().ok_or(VerifyRefusal::Unavailable)?;
        let store = acquire(&store)?;
        if store.is_poisoned() || self.clock.sample_ms().is_err() {
            return Err(VerifyRefusal::Unavailable);
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
        f(&store, *phase)
    }

    pub(super) fn admission_progress(
        &self,
    ) -> Result<(Phase, Option<super::LogPos>), VerifyRefusal> {
        self.observe(try_applied_store, |store, phase| {
            Ok((phase, store.applied()))
        })
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
        self.authorize_with(cluster, node, key, true, try_applied_store)
            .map(|_| ())
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

    pub(super) fn authorize(
        &self,
        cluster: crate::cluster::enrollment::ClusterId,
        node: crate::cluster::enrollment::NodeId,
        key: crate::cluster::enrollment::KeyDigest,
        bootstrap_allowed: bool,
    ) -> Result<Lane, VerifyRefusal> {
        self.authorize_with(cluster, node, key, bootstrap_allowed, lock_inbound_store)
    }

    fn authorize_with(
        &self,
        cluster: crate::cluster::enrollment::ClusterId,
        node: crate::cluster::enrollment::NodeId,
        key: crate::cluster::enrollment::KeyDigest,
        bootstrap_allowed: bool,
        acquire: impl for<'a> FnOnce(&'a AppliedStore) -> StoreLock<'a>,
    ) -> Result<Lane, VerifyRefusal> {
        self.observe(acquire, |store, phase| {
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
    use std::{cell::Cell, sync::mpsc, thread};

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

    #[test]
    fn learner_inbound_authorizes_when_applied_store_is_released_within_budget() {
        let (_dir, sm) = fixture();
        let gate = learner_gate(&sm);
        let mut held = Some(sm.0.lock().unwrap());
        let clock = Cell::new(Instant::now());
        // Drive only time and sleep. The first try-lock hits the held mutex,
        // then the simulated pause releases it before the next attempt.
        let result = gate.authorize_with([1; 16], node(1), key(1), true, |store| {
            wait_for_applied_store(
                store,
                || clock.get(),
                |delay| {
                    clock.set(clock.get() + delay);
                    drop(held.take().expect("one contention pause"));
                },
            )
        });
        assert_eq!(result, Ok(Lane::Strict));
        assert!(held.is_none());
    }

    #[test]
    fn learner_inbound_refuses_when_applied_store_outlasts_budget() {
        let (_dir, sm) = fixture();
        let gate = learner_gate(&sm);
        let held = sm.0.lock().unwrap();
        let start = Instant::now();
        let clock = Cell::new(start);
        let result = gate.authorize_with([1; 16], node(1), key(1), true, |store| {
            wait_for_applied_store(
                store,
                || clock.get(),
                |delay| {
                    clock.set(clock.get() + delay);
                },
            )
        });
        assert_eq!(result, Err(VerifyRefusal::Unavailable));
        assert_eq!(clock.get().duration_since(start), Duration::from_millis(4));
        drop(held);
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
