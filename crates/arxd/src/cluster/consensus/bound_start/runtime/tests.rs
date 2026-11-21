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
            startup_plan::tests::Layout,
        },
        service::cluster_owner::MintReceipt,
    };

    const CLUSTER: ClusterId = [23; 16];
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
        state: Mutex<StateStore>,
        log: Mutex<VoteLogStore>,
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
                state: Mutex::new(state),
                log: Mutex::new(log),
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
