//! Local applied authorization without waiting for the store mutex.
//! Temporary ownership can run store destructors; this is not TLS task ownership.
use super::adapter::SmAdapter;
use crate::cluster::{
    enrollment::PeerRefusal,
    pki::verify::{Registry, VerifyRefusal},
};
use std::sync::Arc;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::{
        enrollment::{Command, FoundMembers, KeyDigest, NodeId, Outcome},
        state_store::{Entry, Init, LogPos, StateStore},
    };
    use crate::service::application::tests::private_temp;
    use std::{
        sync::{Mutex, mpsc},
        thread,
        time::Duration,
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
