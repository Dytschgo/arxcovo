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

async fn no_engine_fixture() -> (
    AuthenticatedPeer,
    Arc<RetainedNode>,
    Arc<Controller>,
    tempfile::TempDir,
) {
    let (genuine, sm, dir) = super::super::peer_transport::admission_test_peer().await;
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
    (genuine, node, controller, dir)
}

fn take_diagnostics(controller: &Controller) -> Vec<(&'static str, &'static str)> {
    std::mem::take(&mut *controller.hooks.diagnostics.lock().unwrap())
}

#[tokio::test(start_paused = true)]
async fn admission_register_waits_for_node_release_within_store_wait() {
    let node = Arc::new(tokio::sync::Mutex::new(0usize));
    let held = node.clone().try_lock_owned().unwrap();
    let start = Instant::now();
    let deadline = start + BUDGET;
    let releaser = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(12)).await;
        drop(held);
    });
    let mut busy = false;
    let mut worker = register_with_node_bounded(
        &node,
        deadline,
        || Ok(()),
        |_, start| {
            let node = node.clone();
            Box::pin(start.after_release(
                async move {
                    *node
                        .try_lock()
                        .expect("registration unlocked before worker observation") += 1;
                    Reply::Pending
                },
                Reply::Refused,
            ))
        },
        || busy = true,
    )
    .await
    .unwrap();
    releaser.await.unwrap();
    assert!(!busy);
    assert!(Instant::now() >= start + Duration::from_millis(12));
    assert!(Instant::now() < start + peer_registry::STORE_WAIT);
    assert_eq!(*node.try_lock().unwrap(), 0);
    assert_eq!(worker.as_mut().await, Reply::Pending);
    assert_eq!(*node.try_lock().unwrap(), 1);

    let (genuine, retained, controller, _dir) = no_engine_fixture().await;
    let address: SocketAddr = "192.168.1.2:1234".parse().unwrap();
    let held = retained.clone().try_lock_owned().unwrap();
    let start = Instant::now();
    let deadline = start + BUDGET;
    let mut admit = std::pin::pin!(controller.admit(genuine, address, deadline));
    assert!(futures_util::poll!(&mut admit).is_pending());
    tokio::time::advance(Duration::from_millis(12)).await;
    assert!(futures_util::poll!(&mut admit).is_pending());
    drop(held);
    // Reaches no-Engine `register_admission` (`raft: None`) after acquiring `node`,
    // without recording `node-busy`.
    assert_eq!(admit.await, Reply::Refused);
    assert!(Instant::now() < start + peer_registry::STORE_WAIT);
    assert!(take_diagnostics(&controller).is_empty());
    assert!(controller.intent_released().now_or_never().is_some());
    assert!(!controller.intent.load(Ordering::Relaxed));
}

#[tokio::test(start_paused = true)]
async fn admission_worker_and_confirm_wait_for_node_release_within_store_wait() {
    let address: SocketAddr = "192.168.1.2:1234".parse().unwrap();
    let position = LogPos {
        term: 1,
        leader: 1,
        index: 7,
    };

    // Generic worker & confirm bounded lock acquisition succeeds after a 12 ms hold.
    for expected in [Reply::Pending, Reply::Applied(position, address)] {
        let node = Arc::new(tokio::sync::Mutex::new(expected));
        let held = node.clone().try_lock_owned().unwrap();
        let start = Instant::now();
        let deadline = start + BUDGET;
        let releaser = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(12)).await;
            drop(held);
        });
        let guard = lock_node_bounded(&node, deadline).await.unwrap();
        assert_eq!(*guard, expected);
        drop(guard);
        releaser.await.unwrap();
        assert!(Instant::now() >= start + Duration::from_millis(12));
        assert!(Instant::now() < start + peer_registry::STORE_WAIT);
    }

    // Exercise the actual Controller `admission-worker` and `admission-confirm` stages
    // against a non-Engine RetainedNode held for 12 ms.
    let (genuine, retained, controller, _dir) = no_engine_fixture().await;

    let held = retained.clone().try_lock_owned().unwrap();
    let submitted = AtomicBool::new(false);
    let start = Instant::now();
    let deadline = start + BUDGET;
    let mut worker = std::pin::pin!(controller.run(genuine, address, deadline, &submitted));
    assert!(futures_util::poll!(&mut worker).is_pending());
    tokio::time::advance(Duration::from_millis(12)).await;
    assert!(futures_util::poll!(&mut worker).is_pending());
    drop(held);
    assert_eq!(worker.await, Reply::Refused);
    assert!(!submitted.load(Ordering::SeqCst));
    assert!(Instant::now() < start + peer_registry::STORE_WAIT);
    assert!(take_diagnostics(&controller).is_empty());

    let held = retained.clone().try_lock_owned().unwrap();
    let start = Instant::now();
    let deadline = start + BUDGET;
    let mut confirm = std::pin::pin!(
        controller.confirm_admission(&retained, genuine, address, position, deadline)
    );
    assert!(futures_util::poll!(&mut confirm).is_pending());
    tokio::time::advance(Duration::from_millis(12)).await;
    assert!(futures_util::poll!(&mut confirm).is_pending());
    drop(held);
    assert_eq!(confirm.await, Reply::Unknown);
    assert!(Instant::now() < start + peer_registry::STORE_WAIT);
    assert!(take_diagnostics(&controller).is_empty());
}

#[tokio::test(start_paused = true)]
async fn admission_paths_fail_closed_and_record_node_busy_at_cap_and_tighter_deadline() {
    let (genuine, retained, controller, _dir) = no_engine_fixture().await;
    let address: SocketAddr = "192.168.1.2:1234".parse().unwrap();
    let position = LogPos {
        term: 1,
        leader: 1,
        index: 9,
    };

    for (outer_budget, expected_wait) in [
        (BUDGET, peer_registry::STORE_WAIT),
        (Duration::from_millis(5), Duration::from_millis(5)),
    ] {
        // 1. admission-register
        let held = retained.clone().try_lock_owned().unwrap();
        let start = Instant::now();
        let deadline = start + outer_budget;
        let mut admit = std::pin::pin!(controller.admit(genuine, address, deadline));
        assert!(futures_util::poll!(&mut admit).is_pending());
        tokio::time::advance(expected_wait).await;
        assert_eq!(Instant::now(), start + expected_wait);
        assert_eq!(
            futures_util::poll!(&mut admit),
            std::task::Poll::Ready(Reply::Refused)
        );
        drop(held);
        assert_eq!(
            take_diagnostics(&controller),
            vec![("admission-register", "node-busy")]
        );
        assert!(!controller.intent.load(Ordering::Relaxed));

        // 2. admission-worker
        let held = retained.clone().try_lock_owned().unwrap();
        let submitted = AtomicBool::new(false);
        let start = Instant::now();
        let deadline = start + outer_budget;
        let mut worker = std::pin::pin!(controller.run(genuine, address, deadline, &submitted));
        assert!(futures_util::poll!(&mut worker).is_pending());
        tokio::time::advance(expected_wait).await;
        assert_eq!(Instant::now(), start + expected_wait);
        assert_eq!(
            futures_util::poll!(&mut worker),
            std::task::Poll::Ready(Reply::Refused)
        );
        drop(held);
        assert!(!submitted.load(Ordering::SeqCst));
        assert_eq!(
            take_diagnostics(&controller),
            vec![("admission-worker", "node-busy")]
        );

        // 3. admission-confirm
        let held = retained.clone().try_lock_owned().unwrap();
        let start = Instant::now();
        let deadline = start + outer_budget;
        let mut confirm = std::pin::pin!(
            controller.confirm_admission(&retained, genuine, address, position, deadline)
        );
        assert!(futures_util::poll!(&mut confirm).is_pending());
        tokio::time::advance(expected_wait).await;
        assert_eq!(Instant::now(), start + expected_wait);
        assert_eq!(
            futures_util::poll!(&mut confirm),
            std::task::Poll::Ready(Reply::Unknown)
        );
        drop(held);
        assert_eq!(
            take_diagnostics(&controller),
            vec![("admission-confirm", "node-busy")]
        );
    }
}

#[tokio::test(start_paused = true)]
async fn admission_register_stops_promptly_during_node_contention_without_node_busy() {
    let (genuine, retained, controller, _dir) = no_engine_fixture().await;
    let address: SocketAddr = "192.168.1.2:1234".parse().unwrap();
    let held = retained.clone().try_lock_owned().unwrap();
    let start = Instant::now();
    let deadline = start + BUDGET;
    let mut admit = std::pin::pin!(controller.admit(genuine, address, deadline));
    assert!(futures_util::poll!(&mut admit).is_pending());
    controller.stop();
    tokio::time::advance(Duration::from_millis(1)).await;
    assert_eq!(
        futures_util::poll!(&mut admit),
        std::task::Poll::Ready(Reply::Refused)
    );
    drop(held);
    assert!(Instant::now() < start + peer_registry::STORE_WAIT);
    assert!(take_diagnostics(&controller).is_empty());
    assert!(!controller.intent.load(Ordering::Relaxed));
}
