use super::*;
use crate::cluster::state_store::release_pair;
use futures_util::FutureExt;
use openraft::type_config::{
    alias::{WatchOf, WatchSenderOf},
    async_runtime::watch::{Watch, WatchSender},
};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
};

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

// Finish the timed loopback TLS handshake before pausing Tokio time.
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

async fn confirm_fixture(
    controller: &Controller,
    retained: &RetainedNode,
    peer: AuthenticatedPeer,
    address: SocketAddr,
    position: LogPos,
    deadline: Instant,
) -> Reply {
    controller
        .confirm_admission(retained, position, deadline, |node| async move {
            Ok((
                (),
                node.admission_observation(peer, address, Some(position))
                    .await,
            ))
        })
        .await
}

fn admission_position(index: u64) -> LogPos {
    LogPos {
        term: 3,
        leader: 1,
        index,
    }
}

fn learner_lag() -> AdmissionReadError {
    AdmissionReadError::LearnerBehind(LearnerLag {
        node: 2,
        committed: admission_position(14),
    })
}

struct AdmissionScript {
    observations: RefCell<VecDeque<AdmissionRead>>,
    reads: Cell<usize>,
    confirmations: Cell<usize>,
    waits: Cell<usize>,
    second_read: Option<tokio::sync::Notify>,
    submitted: AtomicBool,
    metrics: WatchReceiverOf<Arx, RaftDataMetrics<Arx>>,
    progress: WatchSenderOf<Arx, RaftDataMetrics<Arx>>,
    log_tail: Cell<Id>,
    appends: Cell<usize>,
    address: SocketAddr,
}

impl AdmissionScript {
    fn new(observations: impl IntoIterator<Item = AdmissionRead>) -> Self {
        let (progress, metrics) = WatchOf::<Arx>::channel(RaftDataMetrics {
            cluster_committed: Some(id(admission_position(14))),
            replication: Some(BTreeMap::from([(2, Some(id(admission_position(13))))])),
            ..Default::default()
        });
        Self {
            observations: RefCell::new(observations.into_iter().collect()),
            reads: Cell::new(0),
            confirmations: Cell::new(0),
            waits: Cell::new(0),
            second_read: None,
            submitted: AtomicBool::new(false),
            metrics,
            progress,
            log_tail: Cell::new(id(admission_position(15))),
            appends: Cell::new(0),
            address: "192.168.1.2:1234".parse().unwrap(),
        }
    }

    fn add() -> AdmissionRead {
        Ok(AdmissionObservation::Add {
            leader: id(admission_position(15)).leader_id,
            membership: id(admission_position(11)),
            tail: id(admission_position(15)),
        })
    }

    fn matched(&self, node: u64, index: u64) {
        assert!(self.progress.send_if_modified(|data| {
            data.replication
                .as_mut()
                .unwrap()
                .insert(node, Some(id(admission_position(index))));
            true
        }));
    }

    async fn run(&self, node: &RetainedNode, controller: &Controller, deadline: Instant) -> Reply {
        controller
            .run_attempt(
                node,
                deadline,
                &self.submitted,
                |locked, position| async move {
                    let _held = locked;
                    if let Some(position) = position {
                        assert_eq!(position, admission_position(16));
                        assert_eq!(self.appends.get(), 1);
                        self.confirmations.set(self.confirmations.get() + 1);
                        return Ok((
                            (),
                            Ok(AdmissionObservation::Answer(Reply::Applied(
                                position,
                                self.address,
                            ))),
                        ));
                    }
                    assert_eq!(controller.hooks.calls.load(Ordering::Relaxed), 0);
                    self.reads.set(self.reads.get() + 1);
                    if self.reads.get() == 2
                        && let Some(ready) = &self.second_read
                    {
                        ready.notified().await;
                    }
                    Ok((
                        (),
                        self.observations
                            .borrow_mut()
                            .pop_front()
                            .expect("one scripted observation"),
                    ))
                },
                |_, lag| async move {
                    assert!(
                        node.try_lock().is_ok(),
                        "release retained node before waiting"
                    );
                    self.waits.set(self.waits.get() + 1);
                    wait_for_learner(self.metrics.clone(), lag).await
                },
                |(), guards| async move {
                    let [
                        Precondition::LastMembershipLogId {
                            last_membership_log_id,
                        },
                        Precondition::CommittedLeaderId {
                            committed_leader_id,
                        },
                        Precondition::LastLogId { last_log_id },
                    ] = guards
                    else {
                        panic!("expected all three guards")
                    };
                    assert_eq!(last_membership_log_id, Some(id(admission_position(11))));
                    assert_eq!(committed_leader_id, id(admission_position(15)).leader_id);
                    assert_eq!(last_log_id, Some(id(admission_position(15))));
                    if last_log_id != Some(self.log_tail.get()) {
                        return Err(RaftError::APIError(ClientWriteError::PreconditionFailed(
                            PreconditionFailed::LastLogIdMismatch {
                                expected: last_log_id,
                                actual: Some(self.log_tail.get()),
                            },
                        )));
                    }
                    self.appends.set(self.appends.get() + 1);
                    Ok(id(admission_position(16)))
                },
            )
            .await
    }
}

#[tokio::test]
async fn admission_reobserves_after_relevant_progress_under_one_intent() {
    let (peer, retained, controller, _dir) = no_engine_fixture().await;
    tokio::time::pause();
    let deadline = Instant::now() + BUDGET;
    let intent = Intent::waking(&controller.intent).unwrap();
    let permits = retained.try_lock().unwrap().permits.clone();
    let permit = permits.clone().try_acquire_owned().unwrap();
    let mut script = AdmissionScript::new([Err(learner_lag()), AdmissionScript::add()]);
    script.second_read = Some(tokio::sync::Notify::new());
    let mut run = pin!(async {
        let reply = script.run(&retained, &controller, deadline).await;
        drop(permit);
        drop(intent);
        reply
    });
    assert!(futures_util::poll!(&mut run).is_pending());
    assert_eq!(script.reads.get(), 1);
    assert_eq!(script.waits.get(), 1);
    script.matched(3, 14);
    assert!(futures_util::poll!(&mut run).is_pending());
    assert_eq!(
        script.reads.get(),
        1,
        "unrelated learner does not wake the read"
    );
    script.matched(2, 14);
    assert!(futures_util::poll!(&mut run).is_pending());
    assert_eq!(script.reads.get(), 2);
    assert_eq!(controller.hooks.calls.load(Ordering::Relaxed), 0);
    assert_eq!(permits.available_permits(), 0);
    assert_eq!(
        controller.admit(peer, script.address, deadline).await,
        Reply::Pending
    );
    assert!(controller.intent_released().now_or_never().is_none());
    script.second_read.as_ref().unwrap().notify_one();
    assert_eq!(
        run.await,
        Reply::Applied(admission_position(16), script.address)
    );
    assert_eq!(controller.hooks.calls.load(Ordering::Relaxed), 1);
    assert_eq!(script.confirmations.get(), 1);
    assert_eq!(permits.available_permits(), 1);
    assert!(controller.intent_released().now_or_never().is_some());
    assert!(take_diagnostics(&controller).is_empty());
}

#[tokio::test]
async fn admission_rechecks_published_progress_and_returns_exact_replay() {
    let (_peer, retained, controller, _dir) = no_engine_fixture().await;
    tokio::time::pause();
    let address = "192.168.1.2:1234".parse().unwrap();
    let reply = Reply::Applied(admission_position(15), address);
    let script =
        AdmissionScript::new([Err(learner_lag()), Ok(AdmissionObservation::Answer(reply))]);
    script.matched(2, 14);
    let start = Instant::now();
    assert_eq!(
        script.run(&retained, &controller, start + BUDGET).await,
        reply
    );
    assert_eq!(
        Instant::now(),
        start,
        "already published progress must not be missed"
    );
    assert_eq!(script.reads.get(), 2);
    assert_eq!(script.waits.get(), 1);
    assert_eq!(script.confirmations.get(), 0);
    assert_eq!(controller.hooks.calls.load(Ordering::Relaxed), 0);
    assert!(take_diagnostics(&controller).is_empty());
}

#[tokio::test]
async fn admission_second_transient_or_immediate_veto_refuses_without_dispatch() {
    let (_peer, retained, controller, _dir) = no_engine_fixture().await;
    tokio::time::pause();
    for (errors, reads, category) in [
        (
            vec![AdmissionReadError::ViewChanged, learner_lag()],
            2,
            "view-changed",
        ),
        (vec![AdmissionReadError::Capacity], 1, "refused"),
        (vec![MembershipReadError::PeerRefused.into()], 1, "identity"),
    ] {
        let script = AdmissionScript::new(errors.into_iter().map(Err));
        let start = Instant::now();
        assert_eq!(
            script.run(&retained, &controller, start + BUDGET).await,
            Reply::Refused
        );
        assert_eq!(Instant::now(), start);
        assert_eq!(script.reads.get(), reads);
        assert_eq!(script.waits.get(), 0);
        assert!(!script.submitted.load(Ordering::SeqCst));
        assert_eq!(controller.hooks.calls.load(Ordering::Relaxed), 0);
        assert_eq!(
            take_diagnostics(&controller),
            [("admission-membership", category)]
        );
    }
}

#[tokio::test]
async fn admission_lag_wait_refuses_on_stop_or_either_cutoff() {
    let stopped = no_engine_fixture().await;
    let capped = no_engine_fixture().await;
    let expired = no_engine_fixture().await;
    tokio::time::pause();
    for ((_, retained, controller, _dir), budget, stop) in [
        (stopped, BUDGET, true),
        (capped, BUDGET, false),
        (expired, Duration::from_millis(5), false),
    ] {
        let script = AdmissionScript::new([Err(learner_lag())]);
        let start = Instant::now();
        let mut run = pin!(script.run(&retained, &controller, start + budget));
        assert!(futures_util::poll!(&mut run).is_pending());
        if stop {
            controller.stop();
            script.matched(2, 14);
            assert_eq!(Instant::now(), start);
            assert_eq!(futures_util::poll!(&mut run), Poll::Ready(Reply::Refused));
        } else {
            tokio::time::advance(budget.min(REOBSERVE_WAIT)).await;
            // Tokio may round sleep deadlines up to the next timer tick.
            assert_eq!(
                tokio::time::timeout(Duration::from_millis(5), &mut run).await,
                Ok(Reply::Refused)
            );
        }
        assert!(retained.try_lock().is_ok());
        assert_eq!(script.reads.get(), 1);
        assert_eq!(script.waits.get(), 1);
        assert_eq!(controller.hooks.calls.load(Ordering::Relaxed), 0);
        let diagnostics = take_diagnostics(&controller);
        if stop {
            assert!(diagnostics.is_empty());
        } else {
            assert_eq!(diagnostics, [("admission-membership", "view-changed")]);
        }
    }
}

#[tokio::test]
async fn admission_reobserved_add_with_stale_tail_dispatches_once_without_append() {
    let (_peer, retained, controller, _dir) = no_engine_fixture().await;
    tokio::time::pause();
    let script =
        AdmissionScript::new([Err(AdmissionReadError::ViewChanged), AdmissionScript::add()]);
    let (entered, observed) = oneshot::channel();
    let (resume, release) = oneshot::channel();
    *controller.hooks.barrier.lock().unwrap() = Some((entered, release));
    let mut run = pin!(script.run(&retained, &controller, Instant::now() + BUDGET));
    assert!(futures_util::poll!(&mut run).is_pending());
    observed.await.unwrap();
    assert_eq!(script.reads.get(), 2);
    assert_eq!(controller.hooks.calls.load(Ordering::Relaxed), 0);
    script.log_tail.set(id(admission_position(16)));
    resume.send(()).unwrap();
    assert_eq!(run.await, Reply::StaleTail);
    assert_eq!(controller.hooks.calls.load(Ordering::Relaxed), 1);
    assert_eq!(script.reads.get(), 2);
    assert_eq!(script.appends.get(), 0);
    assert_eq!(script.confirmations.get(), 0);
    assert_eq!(
        *controller.hooks.stale.lock().unwrap(),
        Some((admission_position(15), admission_position(16)))
    );
    assert!(take_diagnostics(&controller).is_empty());
}

#[tokio::test]
async fn admission_register_waits_for_node_release_within_store_wait() {
    let (genuine, retained, controller, _dir) = no_engine_fixture().await;
    tokio::time::pause();

    let node = Arc::new(tokio::sync::Mutex::new(0usize));
    let gate = StopGate::new();
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
        &gate,
        deadline,
        |_, start| {
            let worker_node = node.clone();
            let mut worker = Box::pin(start.after_release(
                async move {
                    *worker_node
                        .try_lock()
                        .expect("registration unlocked before worker observation") += 1;
                    Reply::Pending
                },
                Reply::Refused,
            ));
            // Polling the worker before registration returns must observe the
            // gate as pending while registration still holds the node lock.
            let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
            assert!(std::future::Future::poll(worker.as_mut(), &mut cx).is_pending());
            assert!(node.try_lock().is_err());
            worker
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

#[tokio::test]
async fn admission_worker_and_confirm_wait_for_node_release_within_store_wait() {
    let (genuine, retained, controller, _dir) = no_engine_fixture().await;
    tokio::time::pause();

    let address: SocketAddr = "192.168.1.2:1234".parse().unwrap();
    let position = LogPos {
        term: 1,
        leader: 1,
        index: 7,
    };

    // Generic worker & confirm bounded lock acquisition succeeds after a 12 ms hold.
    for expected in [Reply::Pending, Reply::Applied(position, address)] {
        let node = Arc::new(tokio::sync::Mutex::new(expected));
        let gate = StopGate::new();
        let held = node.clone().try_lock_owned().unwrap();
        let start = Instant::now();
        let deadline = start + BUDGET;
        let releaser = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(12)).await;
            drop(held);
        });
        let mut busy = false;
        let guard = wait_for_node_bounded(
            &node,
            &gate,
            deadline,
            |guard, open| {
                drop(open);
                guard
            },
            || busy = true,
        )
        .await
        .unwrap();
        assert!(!busy);
        assert_eq!(*guard, expected);
        drop(guard);
        releaser.await.unwrap();
        assert!(Instant::now() >= start + Duration::from_millis(12));
        assert!(Instant::now() < start + peer_registry::STORE_WAIT);
    }

    // Exercise the actual Controller `admission-worker` and `admission-confirm` stages
    // against a non-Engine RetainedNode held for 12 ms.
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
    let mut confirm = std::pin::pin!(confirm_fixture(
        &controller,
        &retained,
        genuine,
        address,
        position,
        deadline
    ));
    assert!(futures_util::poll!(&mut confirm).is_pending());
    tokio::time::advance(Duration::from_millis(12)).await;
    assert!(futures_util::poll!(&mut confirm).is_pending());
    drop(held);
    assert_eq!(confirm.await, Reply::Unknown);
    assert!(Instant::now() < start + peer_registry::STORE_WAIT);
    assert!(take_diagnostics(&controller).is_empty());
}

#[tokio::test]
async fn admission_paths_fail_closed_and_record_node_busy_at_cap_and_tighter_deadline() {
    let (genuine, retained, controller, _dir) = no_engine_fixture().await;
    tokio::time::pause();
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
        let mut confirm = std::pin::pin!(confirm_fixture(
            &controller,
            &retained,
            genuine,
            address,
            position,
            deadline
        ));
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

#[tokio::test]
async fn admission_paths_wake_immediately_on_stop_without_node_busy_or_post_stop_work() {
    let [register, worker, confirm] = [
        no_engine_fixture().await,
        no_engine_fixture().await,
        no_engine_fixture().await,
    ];
    tokio::time::pause();

    let address: SocketAddr = "192.168.1.2:1234".parse().unwrap();
    let position = LogPos {
        term: 1,
        leader: 1,
        index: 11,
    };

    // 1. admission-register wakes immediately on stop (0 ms elapsed) with no node-busy.
    {
        let (genuine, retained, controller, _dir) = register;
        let held = retained.clone().try_lock_owned().unwrap();
        let start = Instant::now();
        let deadline = start + BUDGET;
        let mut admit = std::pin::pin!(controller.admit(genuine, address, deadline));
        assert!(futures_util::poll!(&mut admit).is_pending());
        controller.stop();
        assert_eq!(
            futures_util::poll!(&mut admit),
            std::task::Poll::Ready(Reply::Refused)
        );
        assert_eq!(Instant::now(), start);
        drop(held);
        assert!(take_diagnostics(&controller).is_empty());
        assert!(!controller.intent.load(Ordering::Relaxed));
    }

    // 2. admission-worker wakes immediately on stop with Reply::Refused and no node-busy.
    {
        let (genuine, retained, controller, _dir) = worker;
        let held = retained.clone().try_lock_owned().unwrap();
        let submitted = AtomicBool::new(false);
        let start = Instant::now();
        let deadline = start + BUDGET;
        let mut worker = std::pin::pin!(controller.run(genuine, address, deadline, &submitted));
        assert!(futures_util::poll!(&mut worker).is_pending());
        controller.stop();
        assert_eq!(
            futures_util::poll!(&mut worker),
            std::task::Poll::Ready(Reply::Refused)
        );
        assert_eq!(Instant::now(), start);
        drop(held);
        assert!(!submitted.load(Ordering::SeqCst));
        assert!(take_diagnostics(&controller).is_empty());
    }

    // 3. admission-confirm wakes immediately on stop with Reply::Unknown and no node-busy.
    {
        let (genuine, retained, controller, _dir) = confirm;
        let held = retained.clone().try_lock_owned().unwrap();
        let start = Instant::now();
        let deadline = start + BUDGET;
        let mut confirm = std::pin::pin!(confirm_fixture(
            &controller,
            &retained,
            genuine,
            address,
            position,
            deadline
        ));
        assert!(futures_util::poll!(&mut confirm).is_pending());
        controller.stop();
        assert_eq!(
            futures_util::poll!(&mut confirm),
            std::task::Poll::Ready(Reply::Unknown)
        );
        assert_eq!(Instant::now(), start);
        drop(held);
        assert!(take_diagnostics(&controller).is_empty());
    }

    // 4. Even if the lock clears after stop() before the waiter is re-polled,
    // no post-stop lock acquisition or work runs and no busy diagnostic fires.
    {
        let node = Arc::new(tokio::sync::Mutex::new(()));
        let gate = StopGate::new();
        let held = node.clone().try_lock_owned().unwrap();
        let deadline = Instant::now() + BUDGET;
        let mut acquired = false;
        let mut busy = false;
        {
            let mut wait = std::pin::pin!(wait_for_node_bounded(
                &node,
                &gate,
                deadline,
                |_, _| {
                    acquired = true;
                },
                || busy = true,
            ));
            assert!(futures_util::poll!(&mut wait).is_pending());
            gate.stop();
            drop(held);
            assert_eq!(futures_util::poll!(&mut wait), std::task::Poll::Ready(None));
        }
        assert!(!acquired);
        assert!(!busy);
    }
}

#[tokio::test]
async fn admission_paths_refuse_already_expired_deadline_without_locking_node() {
    let (genuine, retained, controller, _dir) = no_engine_fixture().await;
    tokio::time::pause();
    let address: SocketAddr = "192.168.1.2:1234".parse().unwrap();
    let position = LogPos {
        term: 1,
        leader: 1,
        index: 12,
    };

    // Unlocked node + already-expired deadline must fail immediately without acquiring.
    let node = tokio::sync::Mutex::new(());
    let gate = StopGate::new();
    let expired = Instant::now();
    let mut acquired = false;
    let mut busy = false;
    {
        let mut wait = std::pin::pin!(wait_for_node_bounded(
            &node,
            &gate,
            expired,
            |_, _| acquired = true,
            || busy = true,
        ));
        assert_eq!(futures_util::poll!(&mut wait), std::task::Poll::Ready(None));
    }
    assert!(!acquired);
    assert!(!busy);

    let mut registered = false;
    let mut reg_busy = false;
    {
        let mut reg = std::pin::pin!(register_with_node_bounded(
            &node,
            &gate,
            expired,
            |_, _| registered = true,
            || reg_busy = true,
        ));
        assert_eq!(
            futures_util::poll!(&mut reg),
            std::task::Poll::Ready(Err(Reply::Refused))
        );
    }
    assert!(!registered);
    assert!(!reg_busy);

    // Exercise all three Controller paths against both unlocked and held node at an expired deadline.
    for hold_node in [false, true] {
        let held = hold_node.then(|| retained.clone().try_lock_owned().unwrap());
        let expired = Instant::now();

        let mut admit = std::pin::pin!(controller.admit(genuine, address, expired));
        assert_eq!(
            futures_util::poll!(&mut admit),
            std::task::Poll::Ready(Reply::Refused)
        );
        assert!(controller.intent_released().now_or_never().is_none());

        let submitted = AtomicBool::new(false);
        let mut worker = std::pin::pin!(controller.run(genuine, address, expired, &submitted));
        assert_eq!(
            futures_util::poll!(&mut worker),
            std::task::Poll::Ready(Reply::Refused)
        );
        assert!(!submitted.load(Ordering::SeqCst));

        let mut confirm = std::pin::pin!(confirm_fixture(
            &controller,
            &retained,
            genuine,
            address,
            position,
            expired
        ));
        assert_eq!(
            futures_util::poll!(&mut confirm),
            std::task::Poll::Ready(Reply::Unknown)
        );

        drop(held);
        assert!(take_diagnostics(&controller).is_empty());
    }

    // A deadline can also expire between acquiring the node and submission.
    let submitted = AtomicBool::new(false);
    assert_eq!(
        controller
            .submit_membership(Instant::now(), &submitted, || -> std::future::Ready<()> {
                panic!("membership dispatch invoked after the deadline");
            })
            .await,
        Err(Reply::Refused)
    );
    assert!(!submitted.load(Ordering::SeqCst));
    assert_eq!(controller.hooks.calls.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn admission_stop_at_exhaustion_boundary_records_zero_node_busy() {
    let budgets = [
        (BUDGET, peer_registry::STORE_WAIT),
        (Duration::from_millis(5), Duration::from_millis(5)),
    ];
    let mut fixtures = Vec::with_capacity(budgets.len());
    for _ in budgets {
        fixtures.push([
            no_engine_fixture().await,
            no_engine_fixture().await,
            no_engine_fixture().await,
        ]);
    }
    tokio::time::pause();

    let address: SocketAddr = "192.168.1.2:1234".parse().unwrap();
    let position = LogPos {
        term: 1,
        leader: 1,
        index: 13,
    };

    // Direct StopGate atomic gate check: busy reporting runs while holding the
    // closed mutex so `stop()` cannot interleave between check and record.
    let gate = StopGate::new();
    let mut count = 0usize;
    gate.report_if_open(|| {
        assert!(gate.closed.try_lock().is_err());
        count += 1;
    });
    assert_eq!(count, 1);
    gate.stop();
    gate.report_if_open(|| count += 1);
    assert_eq!(count, 1);

    for ((outer_budget, expected_wait), [register, worker, confirm]) in
        budgets.into_iter().zip(fixtures)
    {
        // 1. admission-register stopped at exact exhaustion boundary.
        {
            let (genuine, retained, controller, _dir) = register;
            let held = retained.clone().try_lock_owned().unwrap();
            let start = Instant::now();
            let deadline = start + outer_budget;
            let mut admit = std::pin::pin!(controller.admit(genuine, address, deadline));
            assert!(futures_util::poll!(&mut admit).is_pending());
            tokio::time::advance(expected_wait).await;
            assert_eq!(Instant::now(), start + expected_wait);
            controller.stop();
            assert_eq!(
                futures_util::poll!(&mut admit),
                std::task::Poll::Ready(Reply::Refused)
            );
            drop(held);
            assert!(take_diagnostics(&controller).is_empty());
            assert!(!controller.intent.load(Ordering::Relaxed));
        }

        // 2. admission-worker stopped at exact exhaustion boundary.
        {
            let (genuine, retained, controller, _dir) = worker;
            let held = retained.clone().try_lock_owned().unwrap();
            let submitted = AtomicBool::new(false);
            let start = Instant::now();
            let deadline = start + outer_budget;
            let mut worker = std::pin::pin!(controller.run(genuine, address, deadline, &submitted));
            assert!(futures_util::poll!(&mut worker).is_pending());
            tokio::time::advance(expected_wait).await;
            assert_eq!(Instant::now(), start + expected_wait);
            controller.stop();
            assert_eq!(
                futures_util::poll!(&mut worker),
                std::task::Poll::Ready(Reply::Refused)
            );
            drop(held);
            assert!(!submitted.load(Ordering::SeqCst));
            assert!(take_diagnostics(&controller).is_empty());
        }

        // 3. admission-confirm stopped at exact exhaustion boundary.
        {
            let (genuine, retained, controller, _dir) = confirm;
            let held = retained.clone().try_lock_owned().unwrap();
            let start = Instant::now();
            let deadline = start + outer_budget;
            let mut confirm = std::pin::pin!(confirm_fixture(
                &controller,
                &retained,
                genuine,
                address,
                position,
                deadline
            ));
            assert!(futures_util::poll!(&mut confirm).is_pending());
            tokio::time::advance(expected_wait).await;
            assert_eq!(Instant::now(), start + expected_wait);
            controller.stop();
            assert_eq!(
                futures_util::poll!(&mut confirm),
                std::task::Poll::Ready(Reply::Unknown)
            );
            drop(held);
            assert!(take_diagnostics(&controller).is_empty());
        }
    }
}

#[tokio::test]
async fn admission_stop_after_acquisition_does_not_poll_observation() {
    let worker = no_engine_fixture().await;
    let confirm = no_engine_fixture().await;
    tokio::time::pause();

    for (stage, reply, fixture) in [
        ("admission-worker", Reply::Refused, worker),
        ("admission-confirm", Reply::Unknown, confirm),
    ] {
        let (_peer, retained, controller, _dir) = fixture;
        let node = controller
            .lock_node_for_stage(&retained, Instant::now() + BUDGET, stage)
            .await
            .expect("node acquired before stop");
        controller.stop();
        let read = async {
            drop(node);
            panic!("observation polled after stop");
        };
        assert_eq!(controller.gate.observe::<()>(read, reply).await, Err(reply));
        assert!(retained.try_lock().is_ok());
        assert!(take_diagnostics(&controller).is_empty());
    }
}

#[tokio::test]
async fn admission_stop_cancels_in_flight_observations_even_when_input_is_ready() {
    let worker = no_engine_fixture().await;
    let confirm = no_engine_fixture().await;
    tokio::time::pause();

    let address = "192.168.1.2:1234".parse().unwrap();
    let position = LogPos {
        term: 1,
        leader: 1,
        index: 14,
    };
    for (stage, reply, fixture) in [
        ("admission-worker", Reply::Refused, worker),
        ("admission-confirm", Reply::Unknown, confirm),
    ] {
        let (_peer, retained, controller, _dir) = fixture;
        let start = Instant::now();
        let node = controller
            .lock_node_for_stage(&retained, start + BUDGET, stage)
            .await
            .expect("node acquired before stop");
        let (complete, ready) = oneshot::channel();
        let mut observed = std::pin::pin!(controller.gate.observe(
            async {
                ready.await.unwrap();
                drop(node);
                Reply::Applied(position, address)
            },
            reply,
        ));
        assert!(futures_util::poll!(&mut observed).is_pending());
        assert!(retained.try_lock().is_err());
        controller.stop();
        complete.send(()).unwrap();
        assert_eq!(futures_util::poll!(&mut observed), Poll::Ready(Err(reply)));
        assert_eq!(Instant::now(), start);
        assert!(retained.try_lock().is_ok(), "observation dropped on stop");
        assert!(take_diagnostics(&controller).is_empty());
    }
}

#[tokio::test]
async fn admission_stop_after_add_observation_refuses_without_dispatch() {
    let (_peer, retained, controller, _dir) = no_engine_fixture().await;
    tokio::time::pause();
    let deadline = Instant::now() + BUDGET;
    let node = controller
        .lock_node_for_stage(&retained, deadline, "admission-worker")
        .await
        .expect("node acquired before stop");
    let tail = id(LogPos {
        term: 1,
        leader: 1,
        index: 15,
    });
    let observed = controller
        .gate
        .observe(
            async {
                drop(node);
                AdmissionObservation::Add {
                    leader: tail.leader_id,
                    membership: tail,
                    tail,
                }
            },
            Reply::Refused,
        )
        .await;
    assert!(matches!(observed, Ok(AdmissionObservation::Add { .. })));
    let submitted = AtomicBool::new(false);
    controller.stop();
    assert_eq!(
        controller
            .submit_membership(deadline, &submitted, || -> std::future::Ready<()> {
                panic!("membership dispatch invoked after stop");
            })
            .await,
        Err(Reply::Refused)
    );
    assert!(!submitted.load(Ordering::SeqCst));
    assert_eq!(controller.hooks.calls.load(Ordering::Relaxed), 0);
    assert!(take_diagnostics(&controller).is_empty());
}

#[tokio::test]
async fn admission_stop_preserves_started_proposal_ownership() {
    let (_peer, _retained, controller, _dir) = no_engine_fixture().await;
    tokio::time::pause();
    let deadline = Instant::now() + BUDGET;
    let submitted = AtomicBool::new(false);
    let (complete, ready) = oneshot::channel();
    let mut proposal = std::pin::pin!(controller.submit_membership(deadline, &submitted, || {
        async {
            assert!(controller.gate.closed.try_lock().is_err());
            assert!(submitted.load(Ordering::SeqCst));
            ready.await
        }
    }));
    assert!(futures_util::poll!(&mut proposal).is_pending());
    controller.stop();
    assert!(futures_util::poll!(&mut proposal).is_pending());
    complete.send(()).unwrap();
    assert_eq!(futures_util::poll!(&mut proposal), Poll::Ready(Ok(Ok(()))));
    assert!(submitted.load(Ordering::SeqCst));
    assert_eq!(controller.hooks.calls.load(Ordering::Relaxed), 1);
    assert!(take_diagnostics(&controller).is_empty());
}
