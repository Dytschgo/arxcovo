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
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
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
        let leaf = c
            .node(new)
            .enroll(&ca, &*clock, input(&code, 2), &der)
            .await
            .unwrap();
        assert_eq!(leaf.node.get(), 4);
        c.router.isolated.lock().unwrap().clear();
        c.converge().await;
        c.finish().await;
    })
    .await
    .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
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
async fn i6_actual_post_wait_clock() {
    timeout(Duration::from_secs(45), async {
        let _owner = APPLY_PAUSE_OWNER.lock().await;
        let mut c = Cluster::new(Params::default()).await;
        let l = c.leader().await;
        for case in 0..5u8 {
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
            let point = if case == 1 || case == 2 {
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
            if case == 4 {
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
                usize::from(case == 1 || case == 2)
            );
        }
        c.finish().await;
    })
    .await
    .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
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
                NOW,
            ))
            .await
            .unwrap();
        let mut forged = response.clone();
        forged.log_id.leader_id.node_id = (l as u64 + 1) % 3 + 1;
        assert!(matches!(
            c.node(l).committed(forged),
            Err(IssueError::WrongNode)
        ));
        let mut forged = response.clone();
        forged.data = Applied(None);
        assert!(matches!(
            c.node(l).committed(forged),
            Err(IssueError::Rejected(_))
        ));
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
