mod publish_only_probe;

use super::*;

#[test]
fn stop_before_startup_fences_registration_without_dispatching_close() {
    let fence = Admission::default();
    fence.stop();
    let job = Job::anchor();
    fence.register(job);
    {
        let state = job.state();
        assert!(state.process_owned && state.close_requested && !state.close_notified);
    }
    job.request_close();
    assert!(job.state().close_notified);
}

#[test]
fn attach_refusal_keeps_owned_dispatch_and_legacy_close_is_idempotent() {
    let fence = Admission::default();
    let job = Job::anchor();
    fence.register(job);
    job.learner_attach_finished(Err(Refusal::Identity));
    assert_eq!(job.observe().status(), Status::LearnerRefused);
    assert!(!job.state().close_notified);
    let legacy = Job::anchor();
    legacy.learner_attach_finished(Err(Refusal::Identity));
    assert!(legacy.state().close_notified);
}

#[tokio::test]
async fn close_request_notifies_once_after_quiescing() {
    let fence = Admission::default();
    let job = Job::anchor();
    fence.register(job);
    fence.stop();
    assert!(
        tokio::time::timeout(Duration::from_millis(20), job.close.notified())
            .await
            .is_err()
    );
    job.request_close();
    job.close.notified().await;
    job.request_close();
    assert!(
        tokio::time::timeout(Duration::from_millis(20), job.close.notified())
            .await
            .is_err()
    );
}

#[test]
fn membership_coverage_uses_index_and_exact_position_at_equality() {
    let target = LogPos {
        term: 3,
        leader: 1,
        index: 8,
    };
    assert!(covers_log(id(target), id(target)));
    assert!(!covers_log(id(LogPos { term: 4, ..target }), id(target)));
    assert!(!covers_log(
        id(LogPos {
            index: 7,
            term: 4,
            ..target
        }),
        id(target)
    ));
    assert!(covers_log(id(LogPos { index: 9, ..target }), id(target)));
}

#[test]
fn issuer_activation_defers_only_transient_authority_failures() {
    use crate::cluster::consensus::ReadError;
    use enrollment_ingress::ActivationError::{self, Deferred, Refused};
    use membership_observation::MembershipReadError;
    for transient in [ReadError::Busy, ReadError::NotLeader, ReadError::NoQuorum] {
        assert_eq!(ActivationError::from(transient), Deferred);
    }
    assert_eq!(ActivationError::from(ReadError::Unavailable), Refused);
    for transient in [
        MembershipReadError::Busy,
        MembershipReadError::NotLeader,
        MembershipReadError::RetryLater,
    ] {
        assert_eq!(ActivationError::from(transient), Deferred);
    }
    for terminal in [
        MembershipReadError::Unavailable,
        MembershipReadError::PeerRefused,
    ] {
        assert_eq!(ActivationError::from(terminal), Refused);
    }
}

mod ledger {
    use super::*;
    use crate::{
        cluster::{
            bound_open::{BoundIntent, BoundStores},
            enrollment::{CODE_TTL_MS, FoundMembers, JoinCode, MintReceiptInput, Outcome, Reject},
            restart_history::tests as history_fixture,
            startup_plan::tests::Layout,
        },
        service::cluster_owner::MintReceipt,
    };

    const CLUSTER: ClusterId = history_fixture::CLUSTER;
    const EXPIRY: u64 = 100 + CODE_TTL_MS;

    fn position(index: u64) -> LogPos {
        LogPos {
            term: 1,
            leader: 1,
            index,
        }
    }

    fn mint(n: u8) -> (JoinCode, Command) {
        let mut wire = vec![1];
        wire.extend(1u64.to_be_bytes());
        wire.extend([1; 32]);
        let anchor = BoundVoters::decode(&wire).unwrap().anchor(CLUSTER);
        let code = JoinCode::generate(CLUSTER, [2; 32], anchor, |bytes| {
            bytes.fill(n);
            Ok::<_, ()>(())
        })
        .unwrap();
        let command = Command::Mint {
            cluster: CLUSTER,
            actor: [7; 16],
            request: [n; 16],
            code: code.code_id(),
            verifier: code.verifier(),
            proposed_at_ms: 100,
        };
        (code, command)
    }

    fn receipt(command: &Command, position: LogPos, expiry: u64) -> MintReceipt {
        MintReceipt {
            digest: MintReceiptInput::new(command)
                .digest(position, expiry)
                .unwrap(),
            position,
        }
    }

    struct Fixture {
        state: Arc<Mutex<StateStore>>,
        log: Arc<Mutex<VoteLogStore>>,
        _bound: OwnedStores,
        _owner: Owner,
        _layout: Layout,
    }

    impl Fixture {
        fn new(configs: Vec<std::collections::BTreeSet<u64>>) -> Self {
            let layout = Layout::new();
            let mut owner = Owner::acquire(layout.plan("create-founder")).unwrap();
            let mut bound = BoundStores::acquire(
                &mut owner,
                BoundIntent::Fresh(Init {
                    cluster: CLUSTER,
                    first_free: NodeId::new(4).unwrap(),
                }),
            )
            .into_owned();
            let (state, log) = bound.take_pair().unwrap();
            let fixture = Self {
                state: Arc::new(Mutex::new(state)),
                log: Arc::new(Mutex::new(log)),
                _bound: bound,
                _owner: owner,
                _layout: layout,
            };
            let members = FoundMembers::new((1..=3).map(|n| {
                (
                    NodeId::new(n).unwrap(),
                    KeyDigest::of_spki(&[n as u8]).unwrap(),
                )
            }))
            .unwrap();
            fixture.append(
                position(0),
                Command::Found {
                    cluster: CLUSTER,
                    members,
                    proposed_at_ms: 100,
                },
            );
            let membership = WireMembership::new(
                configs,
                (1..=3).map(|n| (n, format!("10.0.0.{n}:9000"))).collect(),
            )
            .unwrap();
            let entry =
                StoredEntry::Membership(MembershipBlob::new(membership.encode().unwrap()).unwrap());
            fixture
                .log
                .lock()
                .unwrap()
                .append(&[(position(1), entry.clone())])
                .unwrap();
            fixture
                .state
                .lock()
                .unwrap()
                .apply(position(1), &entry)
                .unwrap();
            fixture
        }

        fn append(&self, pos: LogPos, command: Command) -> Outcome {
            let entry = StoredEntry::Enrollment(command);
            self.log
                .lock()
                .unwrap()
                .append(&[(pos, entry.clone())])
                .unwrap();
            self.state
                .lock()
                .unwrap()
                .apply(pos, &entry)
                .unwrap()
                .unwrap()
        }

        fn pending(&self, index: u64, n: u8) -> MintReceipt {
            let (_, cmd) = mint(n);
            let position = position(index);
            let proof = receipt(&cmd, position, EXPIRY);
            assert_eq!(
                self.append(position, cmd),
                Outcome::Minted {
                    expires_ms: EXPIRY,
                    replay: false
                }
            );
            proof
        }

        fn observe(&self, receipts: &[MintReceipt]) -> Option<LocalLedger> {
            observe_local_ledger(
                &self.log,
                &self.state,
                &LedgerSlots::for_test(receipts),
                CLUSTER,
                NodeId::new(1).unwrap(),
            )
        }

        fn binding(&self, member: NodeId) -> Arc<tokio::sync::Mutex<ConsensusNode>> {
            Arc::new(tokio::sync::Mutex::new(ConsensusNode {
                id: member.get(),
                read_permits: Arc::new(Semaphore::new(0)),
                issue_permits: Arc::new(Semaphore::new(0)),
                read_blocking: Arc::default(),
                raft: None,
                sm: Some(SmAdapter(self.state.clone())),
                log_released: self.log.lock().unwrap().released(),
                sm_released: self.state.lock().unwrap().released(),
                log_weak: Arc::downgrade(&self.log),
                sm_weak: Arc::downgrade(&self.state),
                permits: Arc::new(Semaphore::new(0)),
                owners: Mutex::default(),
            }))
        }
    }

    #[test]
    fn ledger_checkpoint_qualifies_retained_origin_and_live_epoch_against_stores() {
        use openraft::type_config::{alias::WatchOf, async_runtime::watch::Watch};

        fn recovered_founder() -> (Layout, RecoveredHistory) {
            let mut fixture = history_fixture::Fixture::new(true, true, None);
            let address = fixture.owner.plan.peer.clone();
            fixture.seed(history_fixture::E, history_fixture::E, Some(&address));
            let layout = fixture.release();
            let base = layout.root.path();
            let ready = startup_recovery::recover_process(
                Owner::acquire(history_fixture::plan(base, Mode::RecoverFounder)).unwrap(),
                history_fixture::slot(base),
                &base.join("wrap"),
                &base.join("custody"),
                false,
            )
            .into_ready()
            .unwrap_or_else(|_| panic!("fixture recovery refused"));
            (layout, ready.history)
        }

        fn metrics(
            leader: u64,
            role: openraft::ServerState,
            membership: LogPos,
        ) -> openraft::RaftMetrics<Arx> {
            let mut m = openraft::RaftMetrics::<Arx>::new_initial(1);
            m.vote = openraft::Vote::new(1, leader);
            m.vote.committed = true;
            m.current_leader = Some(leader);
            m.state = role;
            m.membership_config = Arc::new(openraft::StoredMembership::new(
                Some(id(membership)),
                m.membership_config.membership().clone(),
            ));
            m.committed_membership_config = m.membership_config.clone();
            m
        }

        let (_layout, history) = recovered_founder();
        let installation = history.founder().unwrap().installation();
        let stores = Fixture::new(vec![[1, 2, 3].into()]);
        let slots = LedgerSlots::for_test(&[stores.pending(2, 1), stores.pending(3, 2)]);
        let member = NodeId::new(1).unwrap();
        let (peer, _) = peer_transport::TransportOwner::pending_founder(CLUSTER, member);
        let job = Job::anchor();
        {
            let mut state = job.state();
            state.process_owned = true;
            state.status = Status::FounderReady;
            state.founder_recovered = Some(history);
            state.node = Some(stores.binding(member));
        }
        let running = Running::founder(job, None, false);
        let leader = openraft::ServerState::Leader;
        let follower = openraft::ServerState::Follower;
        let checkpoint = |m, between: &mut dyn FnMut()| {
            running.ledger_checkpoint(
                &slots,
                4,
                |_| Some(peer.build_peer()),
                |_| {
                    // The first origin was captured before the live metrics lookup.
                    between();
                    Some(<WatchOf<Arx> as Watch>::channel(m).1)
                },
            )
        };

        let expected = checkpoint(metrics(1, leader, position(1)), &mut || {}).unwrap();
        assert_eq!(&expected[..4], &[LEDGER_VERSION, 4, 1, 0]);
        assert_eq!(&expected[4..28], &[1_u64; 3].map(u64::to_be_bytes).concat());
        assert_eq!(&expected[28..44], &installation);
        assert_eq!(&expected[44..60], &CLUSTER);
        assert_eq!(
            &expected[60..92],
            KeyDigest::of_spki(&[1]).unwrap().as_bytes()
        );
        assert_eq!(&expected[92..116], &position_bytes(position(1)));
        assert_eq!(&expected[116..140], &position_bytes(position(3)));
        assert_eq!(
            &expected[140..],
            &[1_u64, 2, 3].map(u64::to_be_bytes).concat()
        );

        assert!(checkpoint(metrics(1, leader, position(2)), &mut || {}).is_none());
        // A qualified follower epoch with the wrong same-term leader.
        assert!(checkpoint(metrics(2, follower, position(1)), &mut || {}).is_none());
        assert!(checkpoint(metrics(1, follower, position(1)), &mut || {}).is_none());

        let (_other_layout, other) = recovered_founder();
        assert_ne!(other.founder().unwrap().installation(), installation);
        let mut other = Some(other);
        assert!(
            checkpoint(metrics(1, leader, position(1)), &mut || {
                std::mem::swap(&mut job.state().founder_recovered, &mut other);
            })
            .is_none()
        );
        job.state().founder_recovered = other;
        assert!(
            checkpoint(metrics(1, leader, position(1)), &mut || {
                job.state().node = Some(stores.binding(member));
            })
            .is_none()
        );
        assert_eq!(
            checkpoint(metrics(1, leader, position(1)), &mut || {}),
            Some(expected)
        );
        {
            let mut state = job.state();
            state.node = None;
            state.founder_recovered = None;
        }
    }

    #[test]
    fn ledger_epoch_and_custody_require_named_member_and_retained_authority() {
        let membership = Some(id(position(1)));
        for (node, state, role) in [
            (1, openraft::ServerState::Leader, 1),
            (2, openraft::ServerState::Follower, 2),
        ] {
            let mut metrics = openraft::RaftMetrics::<Arx>::new_initial(node);
            metrics.vote = openraft::Vote::new(1, 1);
            metrics.vote.committed = true;
            metrics.current_leader = Some(1);
            metrics.state = state;
            metrics.membership_config = Arc::new(openraft::StoredMembership::new(
                membership,
                metrics.membership_config.membership().clone(),
            ));
            metrics.committed_membership_config = metrics.membership_config.clone();
            assert_eq!(
                qualified_ledger_epoch(&metrics, node),
                Some((metrics.vote.leader_id, role, id(position(1))))
            );
            assert!(qualified_ledger_epoch(&metrics, 3).is_none());
            let mut unconfirmed = metrics.clone();
            unconfirmed.vote.committed = false;
            assert!(qualified_ledger_epoch(&unconfirmed, node).is_none());
            metrics.committed_membership_config = Arc::default();
            assert!(qualified_ledger_epoch(&metrics, node).is_none());
        }

        let job = Job::anchor();
        let mut state = job.state();
        state.status = Status::FounderReady;
        assert_eq!(qualified_ledger_custody(&state, false, || true), None);
        state.process_owned = true;
        assert_eq!(
            qualified_ledger_custody(&state, false, || panic!("founder queried voter custody")),
            Some(false)
        );
        state.status = Status::LearnerReady;
        assert_eq!(qualified_ledger_custody(&state, false, || true), None);
        assert_eq!(qualified_ledger_custody(&state, true, || false), None);
        assert_eq!(qualified_ledger_custody(&state, true, || true), Some(true));
        state.close_requested = true;
        assert_eq!(qualified_ledger_custody(&state, true, || true), None);
    }

    #[test]
    fn checkpoint_uses_qualified_founder_and_promoted_voter_identity() {
        let stores = Fixture::new(vec![[1, 2, 3].into()]);
        let first = stores.pending(2, 1);
        let second = stores.pending(3, 2);
        let slots = LedgerSlots::for_test(&[first, second]);
        let epoch = id(position(3)).leader_id;

        for (node, role, status, custody) in [(1, 1, 4, false), (2, 2, 3, true)] {
            let member = NodeId::new(node).unwrap();
            let key = KeyDigest::of_spki(&[node as u8]).unwrap();
            let origin = LedgerOrigin {
                binding: stores.binding(member),
                installation: [node as u8 + 10; 16],
                custody,
                cluster: CLUSTER,
                member,
            };
            let observe = || {
                observe_local_ledger(&stores.log, &stores.state, &slots, CLUSTER, member).unwrap()
            };
            let issuer = (!custody).then_some((CLUSTER, member, key));
            let checkpoint = origin
                .checkpoint(&origin.clone(), issuer, observe(), epoch, role, status)
                .unwrap();
            assert_eq!(&checkpoint[..4], &[1, status, role, u8::from(custody)]);
            assert_eq!(&checkpoint[4..12], &node.to_be_bytes());
            assert_eq!(&checkpoint[12..20], &1_u64.to_be_bytes());
            assert_eq!(&checkpoint[20..28], &1_u64.to_be_bytes());
            assert_eq!(&checkpoint[28..44], &[node as u8 + 10; 16]);
            assert_eq!(&checkpoint[44..60], &CLUSTER);
            assert_eq!(&checkpoint[60..92], key.as_bytes());
            assert_eq!(&checkpoint[92..116], &position_bytes(position(1)));
            assert_eq!(&checkpoint[116..140], &position_bytes(position(3)));
            assert_eq!(
                &checkpoint[140..],
                &[1_u64, 2, 3].map(u64::to_be_bytes).concat()
            );

            for current in [
                LedgerOrigin {
                    binding: stores.binding(member),
                    ..origin.clone()
                },
                LedgerOrigin {
                    installation: [55; 16],
                    ..origin.clone()
                },
                LedgerOrigin {
                    custody: !custody,
                    ..origin.clone()
                },
            ] {
                assert!(
                    origin
                        .checkpoint(&current, issuer, observe(), epoch, role, status)
                        .is_none()
                );
            }
            let wrong_key = KeyDigest::of_spki(&[99]).unwrap();
            assert!(
                origin
                    .checkpoint(
                        &origin,
                        Some((CLUSTER, member, wrong_key)),
                        observe(),
                        epoch,
                        role,
                        status
                    )
                    .is_none()
            );
        }
    }

    #[test]
    fn every_named_receipt_matches_applied_state_and_exact_retained_position() {
        let stores = Fixture::new(vec![[1, 2, 3].into()]);
        let slots = std::array::from_fn::<_, 4, _>(|i| stores.pending(i as u64 + 2, i as u8 + 1));
        for receipts in [&slots[..2], &slots[..]] {
            let observed = stores.observe(receipts).unwrap();
            assert_eq!(observed.voters, [1, 2, 3]);
            assert_eq!(observed.membership, position(1));
            assert_eq!(observed.applied, position(5));
        }
        for position in [
            LogPos {
                term: 2,
                ..slots[3].position
            },
            LogPos {
                leader: 2,
                ..slots[3].position
            },
            position(6),
        ] {
            let mut bad = slots;
            bad[3].position = position;
            assert!(stores.observe(&bad).is_none());
        }
        let (_, fourth) = mint(4);
        let mut bad = slots;
        bad[3] = receipt(&fourth, position(5), EXPIRY + 1);
        assert!(stores.observe(&bad).is_none());
        assert!(stores.observe(&slots).is_some());
        stores.log.lock().unwrap().purge(position(2)).unwrap();
        assert!(stores.observe(&slots).is_none());
        assert!(stores.log.lock().unwrap().log_state().is_ok());
    }

    #[test]
    fn log_only_mint_refuses_with_stale_and_advanced_applied_watermarks() {
        let stores = Fixture::new(vec![[1, 2, 3].into()]);
        let first = stores.pending(2, 1);
        let (_, mint) = mint(2);
        let second = receipt(&mint, position(3), EXPIRY);
        stores
            .log
            .lock()
            .unwrap()
            .append(&[(position(3), StoredEntry::Enrollment(mint))])
            .unwrap();
        assert!(stores.observe(&[first, second]).is_none());
        for n in [3, 4] {
            stores
                .state
                .lock()
                .unwrap()
                .apply(position(n), &StoredEntry::Blank)
                .unwrap();
        }
        assert_eq!(stores.state.lock().unwrap().applied(), Some(position(4)));
        assert!(stores.observe(&[first, second]).is_none());
        assert_eq!(stores.log.lock().unwrap().live(), Ok(4));
    }

    #[test]
    fn collisions_consumption_and_replay_do_not_match_acknowledgements() {
        let stores = Fixture::new(vec![[1, 2, 3].into()]);
        let first = stores.pending(2, 1);
        let second = stores.pending(3, 2);
        let (used, command) = mint(2);
        let mut collision = command.clone();
        if let Command::Mint { request, .. } = &mut collision {
            *request = [55; 16];
        }
        let colliding = receipt(&collision, position(4), EXPIRY);
        assert_eq!(
            stores.append(position(4), collision),
            Outcome::Rejected(Reject::CodeIdCollision)
        );
        assert!(stores.observe(&[first, colliding]).is_none());
        assert_eq!(
            stores.append(position(5), command),
            Outcome::Minted {
                expires_ms: EXPIRY,
                replay: true
            }
        );
        assert!(
            stores
                .observe(&[
                    first,
                    MintReceipt {
                        position: position(5),
                        ..second
                    }
                ])
                .is_none()
        );
        let anchor = {
            let state = stores.state.lock().unwrap();
            let wire = WireMembership::decode(state.membership().unwrap().1.bytes()).unwrap();
            state.state().voter_anchor(&wire).unwrap()
        };
        assert!(matches!(
            stores.append(
                position(6),
                Command::redeem(
                    CLUSTER,
                    [66; 16],
                    used.code_id(),
                    &used.proof(),
                    KeyDigest::of_spki(&[77]).unwrap(),
                    anchor,
                    100,
                )
            ),
            Outcome::Enrolled(_)
        ));
        assert!(stores.observe(&[first, second]).is_none());
    }

    #[test]
    fn nonuniform_or_contended_or_poisoned_stores_refuse() {
        let joint = Fixture::new(vec![[1, 2, 3].into(), [1, 2, 3].into()]);
        let slots = [joint.pending(2, 1), joint.pending(3, 2)];
        assert!(joint.observe(&slots).is_none());

        let stores = Fixture::new(vec![[1, 2, 3].into()]);
        let slots = [stores.pending(2, 1), stores.pending(3, 2)];
        {
            let _busy = stores.log.lock().unwrap();
            assert!(stores.observe(&slots).is_none());
        }
        {
            let _busy = stores.state.lock().unwrap();
            assert!(stores.observe(&slots).is_none());
        }
        assert!(stores.observe(&slots).is_some());
        assert!(
            std::thread::scope(|scope| scope
                .spawn(|| {
                    let _guard = stores.state.lock().unwrap();
                    panic!("poison control");
                })
                .join())
            .is_err()
        );
        assert!(stores.observe(&slots).is_none());
    }
}
