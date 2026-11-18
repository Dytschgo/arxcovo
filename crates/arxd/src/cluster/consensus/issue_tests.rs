use super::super::three_node::{APPLY_PAUSE_OWNER, Cluster, code, mint};
use super::*;
use openraft::type_config::async_runtime::watch::WatchReceiver;
use rcgen::{CertificateParams, KeyPair};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::time::timeout;
const NOW: u64 = 1_000_000;
struct Clock(AtomicU64);
impl Clock {
    fn new() -> Self {
        Self(AtomicU64::new(NOW))
    }
    fn set(&self, n: u64) {
        self.0.store(n, Ordering::SeqCst);
    }
}
impl SignerClock for Clock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}
impl super::super::checked_clock::CheckedClock for Clock {
    fn sample_ms(&self) -> Result<u64, super::super::checked_clock::ClockError> {
        Ok(SignerClock::now_ms(self))
    }
}
fn csr() -> Vec<u8> {
    CertificateParams::default()
        .serialize_request(&KeyPair::generate().unwrap())
        .unwrap()
        .der()
        .to_vec()
}
fn input(c: &crate::cluster::enrollment::JoinCode, r: u8) -> RedeemInput {
    RedeemInput {
        cluster: c.cluster(),
        request: [r; 16],
        code: c.code_id(),
        proof: c.proof(),
        anchor: c.anchor(),
    }
}
fn arm(n: &ConsensusNode, point: PausePoint) -> (oneshot::Receiver<()>, oneshot::Sender<()>) {
    let (entered, wait) = oneshot::channel();
    let (release, resume) = oneshot::channel();
    let mut slot = ISSUE_PAUSE.lock().unwrap();
    assert!(slot.is_none());
    *slot = Some(IssuePause {
        store: Arc::as_ptr(&n.sm.as_ref().unwrap().0) as usize,
        point,
        entered,
        release: resume,
    });
    (wait, release)
}
async fn entered(wait: oneshot::Receiver<()>) {
    timeout(Duration::from_secs(2), wait)
        .await
        .unwrap()
        .unwrap();
}
fn assert_send<T: Send>(_: T) {}

struct FailingClock {
    calls: AtomicU64,
    fail_at: u64,
    error: ClockError,
    now: u64,
}
impl CheckedClock for FailingClock {
    fn sample_ms(&self) -> Result<u64, ClockError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) + 1 == self.fail_at {
            Err(self.error)
        } else {
            Ok(self.now)
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    target_os = "windows",
    ignore = "Windows whole-fixture quarantine: docs/adr/0024-windows-three-node-test-quarantine.md"
)]
async fn i1_happy_retry() {
    timeout(Duration::from_secs(45), async {
        let mut c = Cluster::new(Params::default()).await;
        let l = c.leader().await;
        let code = code(11);
        c.write(mint(&code, 1, NOW)).await;
        let ca = ClusterCa::ephemeral([1; 16], NOW);
        let clock = Clock::new();
        let der = csr();
        assert_send(c.node(l).enroll(&ca, &clock, input(&code, 2), &der));
        let first = c
            .node(l)
            .enroll(&ca, &clock, input(&code, 2), &der)
            .await
            .unwrap();
        let second = c
            .node(l)
            .enroll(&ca, &clock, input(&code, 2), &der)
            .await
            .unwrap();
        assert_eq!(first.node.get(), 4);
        assert_eq!(first.node, second.node);
        assert_ne!(first.der, second.der);
        use x509_parser::prelude::*;
        let (_, request) = X509CertificationRequest::from_der(&der).unwrap();
        let (first_rest, first_cert) = X509Certificate::from_der(&first.der).unwrap();
        let (second_rest, second_cert) = X509Certificate::from_der(&second.der).unwrap();
        assert!(first_rest.is_empty() && second_rest.is_empty());
        assert_eq!(
            first_cert.public_key().raw,
            request.certification_request_info.subject_pki.raw
        );
        assert_eq!(second_cert.public_key().raw, first_cert.public_key().raw);
        assert_ne!(first_cert.raw_serial(), second_cert.raw_serial());
        c.converge().await;
        for i in 0..3 {
            assert_eq!(
                c.node(i)
                    .local_authorize(
                        [1; 16],
                        first.node,
                        ProvenCsr::parse(&der).unwrap().digest()
                    )
                    .await,
                Ok(())
            );
        }
        c.finish().await;
    })
    .await
    .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    target_os = "windows",
    ignore = "Windows whole-fixture quarantine: docs/adr/0024-windows-three-node-test-quarantine.md"
)]
async fn i2_follower_no_append() {
    timeout(Duration::from_secs(45),async {
    let mut c=Cluster::new(Params::default()).await; let l=c.leader().await; let f=(l+1)%3; let code=code(12); c.write(mint(&code,1,NOW)).await; c.converge().await;
    let ca=ClusterCa::ephemeral([1;16],NOW); let clock=Clock::new(); let der=csr();
    let before=c.node(f).raft.as_ref().unwrap().metrics().borrow_watched().last_log_index;
    assert!(matches!(c.node(f).enroll(&ca,&clock,input(&code,2),&der).await,Err(IssueError::NotLeader(Some(n))) if n==l as u64+1));
    assert_eq!(before,c.node(f).raft.as_ref().unwrap().metrics().borrow_watched().last_log_index);
    c.node(l).enroll(&ca,&clock,input(&code,2),&der).await.unwrap(); c.finish().await;
 }).await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    target_os = "windows",
    ignore = "Windows whole-fixture quarantine: docs/adr/0024-windows-three-node-test-quarantine.md"
)]
async fn i3_single_consumption_race() {
    timeout(Duration::from_secs(45), async {
        let mut c = Cluster::new(Params::default()).await;
        let l = c.leader().await;
        let code = code(13);
        c.write(mint(&code, 1, NOW)).await;
        let ca = ClusterCa::ephemeral([1; 16], NOW);
        let clock = Clock::new();
        let a = csr();
        let b = csr();
        let (a, b) = tokio::join!(
            c.node(l).enroll(&ca, &clock, input(&code, 2), &a),
            c.node(l).enroll(&ca, &clock, input(&code, 3), &b)
        );
        assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
        assert!(
            matches!(a, Err(IssueError::Rejected(Reject::Consumed)))
                || matches!(b, Err(IssueError::Rejected(Reject::Consumed)))
        );
        c.converge().await;
        let sm = c.node(l).sm.as_ref().unwrap().clone();
        blocking(move || {
            assert_eq!(
                sm.0.lock().unwrap().state().authorize_peer(
                    [1; 16],
                    NodeId::new(5).unwrap(),
                    KeyDigest::of_spki(&[1]).unwrap()
                ),
                Err(PeerRefusal::NotMember)
            );
            Ok(())
        })
        .await
        .unwrap();
        c.finish().await;
    })
    .await
    .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    target_os = "windows",
    ignore = "Windows whole-fixture quarantine: docs/adr/0024-windows-three-node-test-quarantine.md"
)]
async fn i4_leadership_loss() {
    timeout(Duration::from_secs(45), async {
        let _owner = APPLY_PAUSE_OWNER.lock().await;
        let mut c = Cluster::new(Params::default()).await;
        let l = c.leader().await;
        let code = code(11);
        c.write(mint(&code, 1, NOW)).await;
        let ca = Arc::new(ClusterCa::ephemeral([1; 16], NOW));
        let clock = Arc::new(Clock::new());
        let der = csr();
        let (wait, release) = arm(c.node(l), PausePoint::AfterCommit);
        let n = c.node(l).clone();
        let ca2 = ca.clone();
        let clock2 = clock.clone();
        let req = input(&code, 2);
        let der2 = der.clone();
        let start = Instant::now();
        let task = tokio::spawn(async move { n.enroll(&ca2, &*clock2, req, &der2).await });
        entered(wait).await;
        let old_term = c
            .node(l)
            .raft
            .as_ref()
            .unwrap()
            .metrics()
            .borrow_watched()
            .current_term;
        c.router.isolated.lock().unwrap().insert(l as u64 + 1);
        let new = timeout(Duration::from_secs(4), async {
            loop {
                let leader = (0..3).find(|i| {
                    *i != l
                        && c.node(*i)
                            .raft
                            .as_ref()
                            .unwrap()
                            .metrics()
                            .borrow_watched()
                            .state
                            == openraft::ServerState::Leader
                });
                if let Some(i) = leader {
                    return i;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        release.send(()).unwrap();
        assert!(matches!(
            task.await.unwrap(),
            Err(IssueError::Unavailable | IssueError::StaleIssuer)
        ));
        assert!(start.elapsed() <= Duration::from_millis(5500));
        assert_eq!(c.node(l).issue_permits.available_permits(), MAX_ISSUING);
        timeout(Duration::from_secs(2), async {
            while c.node(l).read_permits.available_permits() != READ_PERMITS {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let digest = ProvenCsr::parse(&der).unwrap().digest();
        let replay = c
            .node(new)
            .propose_response(Command::redeem(
                code.cluster(),
                [2; 16],
                code.code_id(),
                &code.proof(),
                digest,
                code.anchor(),
                NOW,
            ))
            .await
            .unwrap();
        let Some(Outcome::Enrolled(grant)) = replay.data.0 else {
            panic!("new leader must commit the identical enrollment replay");
        };
        assert!(grant.replay());
        assert_eq!(grant.node().get(), 4);
        assert_eq!(grant.key(), digest);
        let leaf = c
            .node(new)
            .enroll(&ca, &*clock, input(&code, 2), &der)
            .await
            .unwrap();
        assert_eq!(leaf.node, grant.node());
        use x509_parser::prelude::*;
        let (rest, cert) = X509Certificate::from_der(&leaf.der).unwrap();
        assert!(rest.is_empty());
        assert_eq!(KeyDigest::of_spki(cert.public_key().raw).unwrap(), digest);
        let (_, request) = X509CertificationRequest::from_der(&der).unwrap();
        assert_eq!(
            cert.public_key().raw,
            request.certification_request_info.subject_pki.raw
        );
        c.router.isolated.lock().unwrap().clear();
        c.node(l)
            .raft
            .as_ref()
            .unwrap()
            .wait(Some(Duration::from_secs(5)))
            .metrics(
                |m| m.current_term > old_term,
                "I4 deposed issuer observes new term",
            )
            .await
            .expect("I4 deposed issuer observes new term before convergence");
        c.converge().await;
        c.finish().await;
    })
    .await
    .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    target_os = "windows",
    ignore = "Windows whole-fixture quarantine: docs/adr/0024-windows-three-node-test-quarantine.md"
)]
async fn i5_remove_between_guards() {
    timeout(Duration::from_secs(45), async {
        let _owner = APPLY_PAUSE_OWNER.lock().await;
        let mut c = Cluster::new(Params::default()).await;
        let l = c.leader().await;
        let code = code(12);
        c.write(mint(&code, 1, NOW)).await;
        let ca = Arc::new(ClusterCa::ephemeral([1; 16], NOW));
        let clock = Arc::new(Clock::new());
        let der = csr();
        let (wait, release) = arm(c.node(l), PausePoint::AfterSign);
        let n = c.node(l).clone();
        let ca2 = ca.clone();
        let clock2 = clock.clone();
        let req = input(&code, 2);
        let der2 = der.clone();
        let task = tokio::spawn(async move { n.enroll(&ca2, &*clock2, req, &der2).await });
        entered(wait).await;
        c.write(Command::Remove {
            cluster: [1; 16],
            node: NodeId::new(4).unwrap(),
            proposed_at_ms: NOW,
        })
        .await;
        release.send(()).unwrap();
        assert!(matches!(task.await.unwrap(), Err(IssueError::Revoked)));
        assert!(matches!(
            c.node(l).enroll(&ca, &*clock, input(&code, 2), &der).await,
            Err(IssueError::Rejected(Reject::Revoked))
        ));
        c.finish().await;
    })
    .await
    .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    target_os = "windows",
    ignore = "Windows whole-fixture quarantine: docs/adr/0024-windows-three-node-test-quarantine.md"
)]
async fn i6_actual_post_wait_clock() {
    timeout(Duration::from_secs(45), async {
        let _owner = APPLY_PAUSE_OWNER.lock().await;
        let mut c = Cluster::new(Params::default()).await;
        let l = c.leader().await;
        for case in 0..6u8 {
            let code = code(40 + case);
            let base = NOW + u64::from(case) * 2_000_000;
            c.write(mint(&code, 40 + case, base)).await;
            let ca = Arc::new(ClusterCa::ephemeral([1; 16], base));
            let clock = Arc::new(Clock(AtomicU64::new(base)));
            let der = csr();
            if case == 3 {
                c.write(Command::Remove {
                    cluster: [1; 16],
                    node: NodeId::new(99).unwrap(),
                    proposed_at_ms: base + 40_000,
                })
                .await;
                clock.set(base + 40_000);
            }
            let point = if case == 1 || case == 2 || case == 5 {
                PausePoint::AfterSign
            } else {
                PausePoint::AfterCommit
            };
            let (wait, release) = arm(c.node(l), point);
            let n = c.node(l).clone();
            let ca2 = ca.clone();
            let clock2 = clock.clone();
            let req = input(&code, 50 + case);
            let task = tokio::spawn(async move { n.enroll(&ca2, &*clock2, req, &der).await });
            entered(wait).await;
            let expected = match case {
                2 => {
                    clock.set(base - 1);
                    IssueError::ClockRollback
                }
                3 => {
                    clock.set(base);
                    IssueError::Signer(IssueRefusal::SignerClockBehind)
                }
                _ => IssueError::Signer(IssueRefusal::GrantExpired),
            };
            if case == 4 || case == 5 {
                let (apply_entered, apply_wait) = oneshot::channel();
                let (apply_release, apply_resume) = std::sync::mpsc::channel();
                *adapter::APPLY_PAUSE.lock().unwrap() = Some(adapter::ApplyPause {
                    store: Arc::as_ptr(&c.node(l).sm.as_ref().unwrap().0) as usize,
                    entered: apply_entered,
                    release: apply_resume,
                });
                let n = c.node(l).clone();
                let write = tokio::spawn(async move {
                    n.propose(Command::Remove {
                        cluster: [1; 16],
                        node: NodeId::new(99).unwrap(),
                        proposed_at_ms: base,
                    })
                    .await
                });
                entered(apply_wait).await;
                release.send(()).unwrap();
                timeout(Duration::from_secs(1), async {
                    while c.node(l).read_permits.available_permits() != READ_PERMITS - 1 {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .unwrap();
                clock.set(base + crate::cluster::enrollment::CODE_TTL_MS);
                apply_release.send(()).unwrap();
                write.await.unwrap().unwrap();
            } else {
                if case < 2 {
                    clock.set(base + crate::cluster::enrollment::CODE_TTL_MS);
                }
                release.send(()).unwrap();
            }
            assert!(matches!(task.await.unwrap(),Err(e) if e==expected));
            assert_eq!(
                ca.signs.load(Ordering::SeqCst),
                usize::from(case == 1 || case == 2 || case == 5)
            );
        }
        // Checked failures at proposal, pre-sign, and pre-release boundaries.
        // Keep this matrix in the existing engine fixture/quarantine and budget.
        // Synthetic time advances beyond the original six cases' high water.
        for (case, (fail_at, error)) in (1..=3)
            .flat_map(|stage| {
                [
                    ClockError::Unavailable,
                    ClockError::InvalidTime,
                    ClockError::Anomaly,
                    ClockError::Behind,
                    ClockError::Rollback,
                ]
                .map(|error| (stage, error))
            })
            .enumerate()
        {
            let base = NOW + 12_000_000 + case as u64 * 2_000_000;
            let code = code(180 + case as u8);
            c.write(mint(&code, 180 + case as u8, base)).await;
            let ca = ClusterCa::ephemeral([1; 16], base);
            let clock = FailingClock {
                calls: AtomicU64::new(0),
                fail_at,
                error,
                now: base,
            };
            let der = csr();
            let n = c.node(l);
            let before = n
                .raft
                .as_ref()
                .unwrap()
                .metrics()
                .borrow_watched()
                .last_log_index;
            let result = n.enroll(&ca, &clock, input(&code, 2), &der).await;
            let expected = if error == ClockError::Rollback {
                IssueError::ClockRollback
            } else {
                IssueError::Unavailable
            };
            assert_eq!(
                result.err(),
                Some(expected),
                "no credential may be released"
            );
            assert_eq!(
                expected.joiner_reply(),
                if error == ClockError::Rollback {
                    JoinerReply::Rejected
                } else {
                    JoinerReply::RetryLater
                }
            );
            assert_eq!(clock.calls.load(Ordering::SeqCst), fail_at);
            assert_eq!(ca.signs.load(Ordering::SeqCst), usize::from(fail_at == 3));
            assert_eq!(n.issue_permits.available_permits(), MAX_ISSUING);
            if fail_at == 1 {
                assert_eq!(
                    before,
                    n.raft
                        .as_ref()
                        .unwrap()
                        .metrics()
                        .borrow_watched()
                        .last_log_index,
                    "clock refusal must precede proposal"
                );
            }
            let response = n
                .propose_response(Command::redeem(
                    code.cluster(),
                    [2; 16],
                    code.code_id(),
                    &code.proof(),
                    ProvenCsr::parse(&der).unwrap().digest(),
                    code.anchor(),
                    base,
                ))
                .await
                .unwrap();
            let Some(Outcome::Enrolled(grant)) = response.data.0 else {
                panic!("clock refusal must leave a redeemable code or consumed replay");
            };
            assert_eq!(grant.replay(), fail_at > 1);
            // Release only this case's synthetic member so later cases test
            // clock refusal rather than the unchanged registry capacity.
            let removed = n
                .propose_response(Command::Remove {
                    cluster: code.cluster(),
                    node: grant.node(),
                    proposed_at_ms: base,
                })
                .await
                .unwrap();
            assert!(matches!(
                removed.data.0,
                Some(Outcome::Removed { replay: false })
            ));
        }
        c.finish().await;
    })
    .await
    .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    target_os = "windows",
    ignore = "Windows whole-fixture quarantine: docs/adr/0024-windows-three-node-test-quarantine.md"
)]
async fn i7_adapter_rejects_forged_shapes_and_mismatches() {
    timeout(Duration::from_secs(45), async {
        let mut c = Cluster::new(Params::default()).await;
        let l = c.leader().await;
        let code = code(11);
        c.write(mint(&code, 1, NOW)).await;
        let der = csr();
        let proven = ProvenCsr::parse(&der).unwrap();
        let response = c
            .node(l)
            .propose_response(Command::redeem(
                [1; 16],
                [2; 16],
                code.code_id(),
                &code.proof(),
                proven.digest(),
                code.anchor(),
                NOW,
            ))
            .await
            .unwrap();
        let mut forged = openraft::raft::ClientWriteResponse::<Arx> {
            log_id: response.log_id,
            data: response.data.clone(),
            membership: response.membership.clone(),
        };
        forged.log_id.leader_id.node_id = (l as u64 + 1) % 3 + 1;
        assert!(matches!(
            c.node(l).committed(forged),
            Err(IssueError::WrongNode)
        ));
        let mut forged = openraft::raft::ClientWriteResponse::<Arx> {
            log_id: response.log_id,
            data: response.data.clone(),
            membership: response.membership.clone(),
        };
        forged.data = Applied(None);
        assert!(matches!(
            c.node(l).committed(forged),
            Err(IssueError::Rejected(_))
        ));
        for (term, index) in [
            (response.log_id.leader_id.term, response.log_id.index - 1),
            (
                response.log_id.leader_id.term - 1,
                response.log_id.index + 1,
            ),
            (response.log_id.leader_id.term + 1, response.log_id.index),
        ] {
            let mut forged = openraft::raft::ClientWriteResponse::<Arx> {
                log_id: response.log_id,
                data: response.data.clone(),
                membership: response.membership.clone(),
            };
            forged.log_id.leader_id.term = term;
            forged.log_id.index = index;
            assert!(matches!(
                c.node(l).committed(forged),
                Err(IssueError::Rejected(Reject::UnknownCode))
            ));
        }
        let mut later = openraft::raft::ClientWriteResponse::<Arx> {
            log_id: response.log_id,
            data: response.data.clone(),
            membership: response.membership.clone(),
        };
        later.log_id.index += 1;
        assert!(c.node(l).committed(later).is_ok());
        let mut ticket = c.node(l).committed(response).unwrap();
        c.node(l)
            .issue_guard(&ticket, code.code_id(), proven.digest())
            .await
            .unwrap();
        ticket.leader.term += 1;
        assert_eq!(
            c.node(l)
                .issue_guard(&ticket, code.code_id(), proven.digest())
                .await,
            Err(IssueError::StaleIssuer)
        );
        ticket.leader.term -= 1;
        ticket.leader.node_id = (l as u64 + 1) % 3 + 1;
        assert_eq!(
            c.node(l)
                .issue_guard(&ticket, code.code_id(), proven.digest())
                .await,
            Err(IssueError::StaleIssuer)
        );
        let mismatch = test_ticket(KeyDigest::of_spki(&[1]).unwrap());
        assert_eq!(
            ClusterCa::ephemeral([1; 16], NOW).sign_leaf(&mismatch, &proven, NOW),
            Err(IssueError::Revoked)
        );
        for e in [
            IssueError::Busy,
            IssueError::NotLeader(Some(1)),
            IssueError::Unavailable,
            IssueError::StaleIssuer,
        ] {
            assert_eq!(e.joiner_reply(), JoinerReply::RetryLater);
            assert!(!format!("{e} {e:?}").contains("input-marker"));
        }
        c.finish().await;
    })
    .await
    .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    target_os = "windows",
    ignore = "Windows whole-fixture quarantine: docs/adr/0024-windows-three-node-test-quarantine.md"
)]
async fn i8_removed_leaf_remains_chain_valid_stale_registry() {
    timeout(Duration::from_secs(45), async {
        use x509_parser::prelude::*;
        let mut c = Cluster::new(Params::default()).await;
        let l = c.leader().await;
        let f = (l + 1) % 3;
        let code = code(11);
        c.write(mint(&code, 1, NOW)).await;
        let ca = ClusterCa::ephemeral([1; 16], NOW);
        let clock = Clock::new();
        let der = csr();
        let key = ProvenCsr::parse(&der).unwrap().digest();
        let leaf = c
            .node(l)
            .enroll(&ca, &clock, input(&code, 2), &der)
            .await
            .unwrap();
        c.converge().await;
        c.router.isolated.lock().unwrap().insert(f as u64 + 1);
        c.write(Command::Remove {
            cluster: [1; 16],
            node: leaf.node,
            proposed_at_ms: NOW,
        })
        .await;
        let index = c
            .node(l)
            .raft
            .as_ref()
            .unwrap()
            .metrics()
            .borrow_watched()
            .last_applied
            .unwrap()
            .index;
        for i in 0..3 {
            if i != f {
                c.node(i)
                    .raft
                    .as_ref()
                    .unwrap()
                    .wait(Some(Duration::from_secs(2)))
                    .applied_index_at_least(Some(index), "remove")
                    .await
                    .unwrap();
                assert_eq!(
                    c.node(i).local_authorize([1; 16], leaf.node, key).await,
                    Err(PeerRefusal::NotMember)
                );
            }
        }
        assert_eq!(
            c.node(f).local_authorize([1; 16], leaf.node, key).await,
            Ok(())
        );
        let (_, cert) = X509Certificate::from_der(&leaf.der).unwrap();
        let (_, root) = X509Certificate::from_der(ca.cert_der()).unwrap();
        cert.verify_signature(Some(root.public_key())).unwrap();
        c.router.isolated.lock().unwrap().clear();
        c.converge().await;
        assert_eq!(
            c.node(f).local_authorize([1; 16], leaf.node, key).await,
            Err(PeerRefusal::NotMember)
        );
        cert.verify_signature(Some(root.public_key())).unwrap();
        c.finish().await;
    })
    .await
    .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    target_os = "windows",
    ignore = "Windows whole-fixture quarantine: docs/adr/0024-windows-three-node-test-quarantine.md"
)]
async fn i9_private_material_never_persisted() {
    timeout(Duration::from_secs(45), async {
        let mut c = Cluster::new(Params::default()).await;
        let l = c.leader().await;
        let code = code(11);
        c.write(mint(&code, 1, NOW)).await;
        let ca = ClusterCa::ephemeral([1; 16], NOW);
        c.extra_markers.push(ca.private_marker());
        c.node(l)
            .enroll(&ca, &Clock::new(), input(&code, 2), &csr())
            .await
            .unwrap();
        c.converge().await;
        c.finish().await;
    })
    .await
    .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    target_os = "windows",
    ignore = "Windows whole-fixture quarantine: docs/adr/0024-windows-three-node-test-quarantine.md"
)]
async fn hook_store_identity_and_admission() {
    timeout(Duration::from_secs(45), async {
        let _owner = APPLY_PAUSE_OWNER.lock().await;
        let mut a = Cluster::new(Params::default()).await;
        let mut b = Cluster::new(Params::default()).await;
        let (wait, release) = arm(a.node(0), PausePoint::AfterCommit);
        b.node(0).issue_pause(PausePoint::AfterCommit).await;
        assert!(ISSUE_PAUSE.lock().unwrap().is_some());
        let n = a.node(0).clone();
        let task = tokio::spawn(async move { n.issue_pause(PausePoint::AfterCommit).await });
        entered(wait).await;
        release.send(()).unwrap();
        task.await.unwrap();
        let permits: Vec<_> = (0..MAX_ISSUING)
            .map(|_| a.node(0).issue_permits.try_acquire().unwrap())
            .collect();
        assert!(matches!(
            a.node(0)
                .enroll(
                    &ClusterCa::ephemeral([1; 16], NOW),
                    &Clock::new(),
                    input(&code(11), 2),
                    &[]
                )
                .await,
            Err(IssueError::Busy)
        ));
        assert_eq!(a.node(0).permits.available_permits(), 64);
        drop(permits);
        a.finish().await;
        b.finish().await;
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    target_os = "windows",
    ignore = "Windows whole-fixture quarantine: docs/adr/0024-windows-three-node-test-quarantine.md"
)]
async fn retained_record_is_required_even_while_member_remains_authorized() {
    timeout(Duration::from_secs(45), async {
        let mut c = Cluster::new(Params::default()).await;
        let l = c.leader().await;
        let code = code(91);
        c.write(mint(&code, 90, NOW)).await;
        let der = csr();
        let key = ProvenCsr::parse(&der).unwrap().digest();
        let response = c
            .node(l)
            .propose_response(Command::redeem(
                code.cluster(),
                [92; 16],
                code.code_id(),
                &code.proof(),
                key,
                code.anchor(),
                NOW,
            ))
            .await
            .unwrap();
        let ticket = c.node(l).committed(response).unwrap();
        c.node(l)
            .issue_guard(&ticket, code.code_id(), key)
            .await
            .unwrap();
        let prune = NOW
            + crate::cluster::enrollment::CODE_TTL_MS
            + crate::cluster::enrollment::TOMBSTONE_MS;
        c.write(Command::Remove {
            cluster: code.cluster(),
            node: NodeId::new(99).unwrap(),
            proposed_at_ms: prune,
        })
        .await;
        assert_eq!(
            c.node(l)
                .local_authorize(code.cluster(), ticket.node(), key)
                .await,
            Ok(())
        );
        assert_eq!(
            c.node(l).issue_guard(&ticket, code.code_id(), key).await,
            Err(IssueError::Rejected(Reject::UnknownCode))
        );
        c.write(mint(&code, 93, prune)).await;
        assert_eq!(
            c.node(l)
                .local_authorize(code.cluster(), ticket.node(), key)
                .await,
            Ok(())
        );
        assert_eq!(
            c.node(l).issue_guard(&ticket, code.code_id(), key).await,
            Err(IssueError::Rejected(Reject::Consumed))
        );
        c.converge().await;
        c.finish().await;
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn anchored_single_voter_receipt_replay_and_revocation() {
    use super::found::founder_init;
    use crate::cluster::consensus::mem_net::{Factory, MemRouter};
    use crate::cluster::enrollment::JoinCode;
    use crate::service::application::tests::private_temp;
    let entry = Instant::now();
    let work_deadline = entry + Duration::from_secs(35);
    let cleanup_deadline = entry + Duration::from_secs(40);
    let dir = private_temp();
    let router = Arc::new(MemRouter::default());
    let mut node = None;
    let mut release_hook = None;
    let result = timeout_at(work_deadline, async {
        let n = ConsensusNode::start(1, dir.path(), founder_init([1;16]), Params::default(),
            Factory { source:1, router:router.clone() }).await.unwrap();
        router.nodes.lock().unwrap().insert(1, n.raft.as_ref().unwrap().clone());
        node = Some(n);
        let n = node.as_ref().unwrap();
        let ca = ClusterCa::ephemeral([1;16],NOW); let clock = Clock::new();
        let founder_csr = csr();
        timeout(Duration::from_secs(5),n.found(&ca,&clock,BasicNode::new("mem:1"),&founder_csr)).await.unwrap().unwrap();
        let anchor = {
            let store=n.sm.as_ref().unwrap().0.lock().unwrap();
            let membership=crate::cluster::membership::Membership::decode(store.membership().unwrap().1.bytes()).unwrap();
            store.state().voter_anchor(&membership).unwrap()
        };
        let code=JoinCode::generate([1;16],ca.fingerprint(),anchor,|b| { b.fill(111);Ok::<_,()>(()) }).unwrap();
        n.propose(mint(&code,1,NOW)).await.unwrap();
        let der=csr(); let proven=ProvenCsr::parse(&der).unwrap();
        let first=n.enroll(&ca,&clock,input(&code,2),&der).await.unwrap();
        let replay=n.enroll(&ca,&clock,input(&code,2),&der).await.unwrap();
        assert_eq!(first.node,replay.node);assert_eq!(first.identity_commit,replay.identity_commit);
        assert!(n.sm.as_ref().unwrap().0.lock().unwrap().applied().unwrap().index>first.identity_commit.index);
        use x509_parser::prelude::*;
        let (_,cert)=X509Certificate::from_der(&replay.der).unwrap();
        let (_,req)=X509CertificationRequest::from_der(&der).unwrap();
        assert_eq!(cert.public_key().raw,req.certification_request_info.subject_pki.raw);
        assert_eq!(proven.digest(),KeyDigest::of_spki(cert.public_key().raw).unwrap());
        let (wait,release)=arm(n,PausePoint::AfterCommit); release_hook=Some(release);
        let enrollment=n.enroll(&ca,&clock,input(&code,2),&der);tokio::pin!(enrollment);
        tokio::select! { _=entered(wait)=>{}, r=&mut enrollment=>panic!("enrollment completed before revoke: {:?}",r.err()) }
        n.propose(Command::Remove{cluster:[1;16],node:first.node,proposed_at_ms:NOW}).await.unwrap();
        release_hook.take().unwrap().send(()).unwrap();
        assert!(matches!(enrollment.await,Err(IssueError::Revoked)));
        let store=n.sm.as_ref().unwrap().0.lock().unwrap();
        assert_eq!(store.state().authorize_peer([1;16],first.node,proven.digest()),Err(PeerRefusal::NotMember));
    }).await;
    if let Some(release) = release_hook.take() {
        let _ = release.send(());
    }
    router.nodes.lock().unwrap().clear();
    let n = node
        .as_mut()
        .expect("startup unresolved; cooperative deadline cannot cancel blocking startup");
    timeout_at(cleanup_deadline, n.shutdown())
        .await
        .expect("cleanup deadline exceeded")
        .expect("clean shutdown required");
    assert!(n.log_weak.upgrade().is_none());
    assert!(n.sm_weak.upgrade().is_none());
    result.expect("single-voter work exceeded entry budget");
}
