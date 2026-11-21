use super::*;
use crate::cluster::state_store::release_pair;
use futures_util::FutureExt;

#[tokio::test]
async fn admission_worker_observes_only_after_registration_unlocks() {
    let node = tokio::sync::Mutex::new(0);
    let mut worker = register_with_node(&node, |_, start| {
        let mut worker = Box::pin(start.after_release(
            async {
                *node
                    .try_lock()
                    .expect("registration unlocked before observation") += 1;
                Reply::Pending
            },
            Reply::Refused,
        ));
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        assert!(std::future::Future::poll(worker.as_mut(), &mut cx).is_pending());
        assert!(node.try_lock().is_err());
        worker
    })
    .unwrap();
    assert_eq!(*node.try_lock().unwrap(), 0);
    assert_eq!(worker.as_mut().await, Reply::Pending);
    assert_eq!(*node.try_lock().unwrap(), 1);
}

#[tokio::test]
async fn lost_admission_start_refuses_without_submission() {
    let (release, start) = oneshot::channel();
    drop(release);
    let submitted = AtomicBool::new(false);
    assert_eq!(
        PromotionStart(start)
            .after_release(
                async {
                    submitted.store(true, Ordering::SeqCst);
                    Reply::Unknown
                },
                Reply::Refused,
            )
            .await,
        Reply::Refused
    );
    assert!(!submitted.load(Ordering::SeqCst));
}

#[test]
fn slot_denies_overlap_and_releases_on_drop() {
    let slot = Arc::new(AtomicBool::new(false));
    let owned = Intent::reserve(&slot).unwrap();
    assert!(matches!(Intent::reserve(&slot), Err(Reply::Pending)));
    drop(owned);
    assert!(Intent::reserve(&slot).is_ok());
}

#[tokio::test]
async fn assumed_peer_cannot_reserve_or_dispatch() {
    let (genuine, sm, _dir) = super::super::peer_transport::admission_test_peer().await;
    let sm_released = sm.0.lock().unwrap().released();
    let node = Arc::new(RetainedNode::new(ConsensusNode {
        id: 1,
        read_permits: Arc::new(Semaphore::new(READ_PERMITS)),
        issue_permits: Arc::new(Semaphore::new(issue::MAX_ISSUING)),
        read_blocking: Arc::default(),
        raft: None,
        log_released: release_pair().1,
        sm_released,
        log_weak: Weak::new(),
        sm_weak: Arc::downgrade(&sm.0),
        sm: Some(sm),
        permits: Arc::new(Semaphore::new(1)),
        owners: Mutex::default(),
    }));
    let controller = Controller::new(&node);
    let assumed = AuthenticatedPeer::assume(genuine.node());
    let address = "192.168.1.2:1234".parse().unwrap();

    // The occupied intent is the admission sentinel. It gives a distinct
    // reply before any Engine dependency and must be invisible to assumed peers.
    let occupied = Intent::waking(&controller.intent).unwrap();
    assert_eq!(
        controller
            .admit(genuine, address, Instant::now() + BUDGET)
            .await,
        Reply::Pending
    );
    assert_eq!(
        controller
            .admit(assumed, address, Instant::now() + BUDGET)
            .await,
        Reply::Refused
    );
    assert!(controller.intent.load(Ordering::Relaxed));
    drop(occupied);
    assert!(controller.intent_released().now_or_never().is_some());

    // With a reachable node, healthy permit pool and empty slot, the genuine
    // peer reserves and releases the intent at the deliberate no-Engine boundary.
    // The assumed peer must never reserve it, even transiently.
    assert_eq!(
        controller
            .admit(genuine, address, Instant::now() + BUDGET)
            .await,
        Reply::Refused
    );
    assert!(controller.intent_released().now_or_never().is_some());
    assert_eq!(
        controller
            .admit(assumed, address, Instant::now() + BUDGET)
            .await,
        Reply::Refused
    );
    assert!(controller.intent_released().now_or_never().is_none());
    assert!(!controller.intent.load(Ordering::Relaxed));
    let node = node.try_lock().unwrap();
    assert_eq!(node.permits.available_permits(), 1);
    assert!(node.owners.lock().unwrap().set.is_empty());
    assert_eq!(controller.hooks.calls.load(Ordering::Relaxed), 0);
}

#[test]
fn watermarks_compare_exact_identity_at_equality() {
    let p = LogPos {
        index: 12,
        leader: 1,
        term: 2,
    };
    assert!(covers(p, p));
    assert!(!covers(LogPos { leader: 2, ..p }, p));
    assert!(!covers(LogPos { term: 3, ..p }, p));
    assert!(!covers(LogPos { index: 11, ..p }, p));
    assert!(covers(LogPos { index: 13, ..p }, p));
}
