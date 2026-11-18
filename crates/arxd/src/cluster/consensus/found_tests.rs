use super::super::super::mem_net::{Factory, MemRouter};
use super::*;
use crate::service::application::tests::private_temp;
use openraft::type_config::async_runtime::watch::WatchReceiver;
use rcgen::{CertificateParams, KeyPair};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::time::timeout;
use x509_parser::prelude::*;
const NOW: u64 = 1_000_000;
struct Clock;
impl SignerClock for Clock {
    fn now_ms(&self) -> u64 {
        NOW
    }
}
fn request() -> (KeyPair, Vec<u8>) {
    let key = KeyPair::generate().unwrap();
    let der = CertificateParams::default()
        .serialize_request(&key)
        .unwrap()
        .der()
        .to_vec();
    (key, der)
}
async fn start(dir: &Path, id: u64, free: u64) -> ConsensusNode {
    ConsensusNode::start(
        id,
        dir,
        Init {
            cluster: [1; 16],
            first_free: NodeId::new(free).unwrap(),
        },
        Params::default(),
        Factory {
            source: id,
            router: Arc::new(MemRouter::default()),
        },
    )
    .await
    .unwrap()
}
async fn issue(n: &ConsensusNode, ca: &ClusterCa, csr: &[u8]) -> Result<IssuedLeaf, FoundError> {
    n.found(ca, &Clock, BasicNode::new("mem:1"), csr).await
}
fn last(n: &ConsensusNode) -> Option<u64> {
    n.raft
        .as_ref()
        .unwrap()
        .metrics()
        .borrow_watched()
        .last_log_index
}
fn uninitialized(n: &ConsensusNode) {
    let m = n.raft.as_ref().unwrap().metrics().borrow_watched().clone();
    assert_eq!(m.last_log_index, None);
    assert_eq!(m.current_term, 0);
    assert_eq!(m.current_leader, None);
    let s = n.sm.as_ref().unwrap().0.lock().unwrap();
    assert_eq!(s.applied(), None);
    assert!(!s.state().founded());
}
fn leaf(ca: &ClusterCa, other: &ClusterCa, der: &[u8], csr: &[u8], node: NodeId) {
    let (_, cert) = X509Certificate::from_der(der).unwrap();
    let (_, root) = X509Certificate::from_der(ca.cert_der()).unwrap();
    let (_, wrong) = X509Certificate::from_der(other.cert_der()).unwrap();
    let (_, req) = X509CertificationRequest::from_der(csr).unwrap();
    cert.verify_signature(Some(root.public_key())).unwrap();
    assert!(cert.verify_signature(Some(wrong.public_key())).is_err());
    assert_eq!(
        cert.public_key().raw,
        req.certification_request_info.subject_pki.raw
    );
    assert_eq!(
        cert.subject_alternative_name()
            .unwrap()
            .unwrap()
            .value
            .general_names,
        vec![GeneralName::URI(
            crate::cluster::pki::node_uri([1; 16], node).as_str()
        )]
    );
    assert!(der.len() <= 4096);
}
macro_rules! bounded {
    ($name:ident, $body:expr) => {
        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn $name() {
            timeout(Duration::from_secs(20), $body).await.unwrap();
        }
    };
}
bounded!(f1_happy_registry, async {
    let d = private_temp();
    let mut n = start(d.path(), 1, 2).await;
    let (_, csr) = request();
    let ca = ClusterCa::ephemeral([1; 16], NOW);
    let other = ClusterCa::ephemeral([1; 16], NOW);
    let issued = issue(&n, &ca, &csr).await.unwrap();
    assert_eq!(issued.node, FOUNDER);
    leaf(&ca, &other, &issued.der, &csr, FOUNDER);
    let key = ProvenCsr::parse(&csr).unwrap().digest();
    n.fresh_read(move |s| {
        assert_eq!(s.state().authorize_peer([1; 16], FOUNDER, key), Ok(()));
        assert_eq!(
            s.state()
                .authorize_peer([1; 16], FOUNDER, KeyDigest::of_spki(b"other").unwrap()),
            Err(PeerRefusal::KeyMismatch)
        );
        assert_eq!(
            s.state()
                .authorize_peer([1; 16], NodeId::new(2).unwrap(), key),
            Err(PeerRefusal::NotMember)
        );
        assert_eq!(s.state().next_node_id().get(), 2);
        Ok(())
    })
    .await
    .unwrap();
    assert_eq!(
        n.raft
            .as_ref()
            .unwrap()
            .metrics()
            .borrow_watched()
            .membership_config
            .membership()
            .voter_ids()
            .collect::<Vec<_>>(),
        vec![1]
    );
    n.shutdown().await.unwrap();
});
bounded!(f2_once_and_concurrent, async {
    for concurrent in [false, true] {
        let d = private_temp();
        let mut n = start(d.path(), 1, 2).await;
        let (_, a) = request();
        let (_, b) = request();
        let ca = ClusterCa::ephemeral([1; 16], NOW);
        if concurrent {
            let (a, b) = tokio::join!(issue(&n, &ca, &a), issue(&n, &ca, &b));
            assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
            assert!(
                matches!(a, Err(FoundError::AlreadyInitialized))
                    || matches!(b, Err(FoundError::AlreadyInitialized))
            );
        } else {
            issue(&n, &ca, &a).await.unwrap();
        }
        let before = last(&n);
        let signs = ca.signs.load(Ordering::SeqCst);
        for csr in [&a, &b] {
            assert!(matches!(
                issue(&n, &ca, csr).await,
                Err(FoundError::AlreadyInitialized)
            ));
        }
        assert_eq!(before, last(&n));
        assert_eq!(signs, ca.signs.load(Ordering::SeqCst));
        assert_eq!(signs, 1);
        {
            let sm = n.sm.as_ref().unwrap().0.lock().unwrap();
            assert_eq!(sm.state().next_node_id().get(), 2);
        }
        n.shutdown().await.unwrap();
        let mut log = VoteLogStore::open(d.path(), [1; 16]).unwrap();
        let entries = log.entries(0, before.unwrap() + 1, 64).unwrap();
        assert_eq!(
            entries
                .iter()
                .filter(|(_, e)| matches!(e, StoredEntry::Enrollment(Command::Found { .. })))
                .count(),
            1
        );
    }
});
bounded!(f3_preinitialize_refusals, async {
    for case in 0..3 {
        let d = private_temp();
        let mut n = start(d.path(), if case == 0 { 2 } else { 1 }, 2).await;
        let (_, csr) = request();
        let ca = ClusterCa::ephemeral([1; 16], NOW);
        let short = ClusterCa::ephemeral_with_validity([1; 16], NOW, 30 * 86400 + 59);
        let result = match case {
            0 => issue(&n, &ca, &csr).await,
            1 => issue(&n, &ca, b"bad").await,
            _ => issue(&n, &short, &csr).await,
        };
        match case {
            0 => assert!(matches!(result, Err(FoundError::NotFounder))),
            1 => assert!(matches!(result, Err(FoundError::BadCsr(_)))),
            _ => assert!(matches!(result, Err(FoundError::CaExpiring))),
        }
        uninitialized(&n);
        if case != 0 {
            issue(&n, &ca, &csr).await.unwrap();
        }
        n.shutdown().await.unwrap();
    }
});
bounded!(f3b_persisted_allocation, async {
    for free in [1, 4] {
        let d = private_temp();
        let (_, csr) = request();
        let ca = ClusterCa::ephemeral([1; 16], NOW);
        for configured in [free, 2] {
            let mut n = start(d.path(), 1, configured).await;
            assert!(matches!(
                issue(&n, &ca, &csr).await,
                Err(FoundError::FounderAllocation)
            ));
            uninitialized(&n);
            n.shutdown().await.unwrap();
        }
    }
});
bounded!(f4_registry_restart, async {
    let d = private_temp();
    let (_, csr) = request();
    let ca = ClusterCa::ephemeral([1; 16], NOW);
    let key = ProvenCsr::parse(&csr).unwrap().digest();
    let mut n = start(d.path(), 1, 2).await;
    issue(&n, &ca, &csr).await.unwrap();
    n.shutdown().await.unwrap();
    let mut n = start(d.path(), 1, 2).await;
    n.raft
        .as_ref()
        .unwrap()
        .wait(Some(Duration::from_secs(3)))
        .metrics(
            |m| m.state == openraft::ServerState::Leader,
            "restart leader",
        )
        .await
        .unwrap();
    assert_eq!(
        n.fresh_read(move |s| Ok(s.state().authorize_peer([1; 16], FOUNDER, key)))
            .await
            .unwrap()
            .1,
        Ok(())
    );
    assert!(matches!(
        issue(&n, &ca, &csr).await,
        Err(FoundError::AlreadyInitialized)
    ));
    n.shutdown().await.unwrap();
});
bounded!(f5_joiner_two, async {
    use crate::cluster::enrollment::JoinCode;
    let d = private_temp();
    let mut n = start(d.path(), 1, 2).await;
    let (_, a) = request();
    let (_, b) = request();
    let ca = ClusterCa::ephemeral([1; 16], NOW);
    let other = ClusterCa::ephemeral([1; 16], NOW);
    let founder = issue(&n, &ca, &a).await.unwrap();
    let code = JoinCode::generate([1; 16], ca.fingerprint(), observed_anchor(&n), |b| {
        b.fill(15);
        Ok::<_, ()>(())
    })
    .unwrap();
    assert_eq!(
        code.ca_fingerprint(),
        crate::cluster::pki::ca::fingerprint_of(ca.cert_der())
    );
    assert_ne!(code.ca_fingerprint(), other.fingerprint());
    n.propose(Command::Mint {
        cluster: [1; 16],
        actor: [3; 16],
        request: [4; 16],
        code: code.code_id(),
        verifier: code.verifier(),
        proposed_at_ms: NOW,
    })
    .await
    .unwrap();
    let joined = n
        .enroll(
            &ca,
            &Clock,
            RedeemInput {
                cluster: [1; 16],
                request: [5; 16],
                code: code.code_id(),
                proof: code.proof(),
                anchor: code.anchor(),
            },
            &b,
        )
        .await
        .unwrap();
    assert_eq!(joined.node.get(), 2);
    leaf(&ca, &other, &joined.der, &b, joined.node);
    let (_, f) = X509Certificate::from_der(&founder.der).unwrap();
    let (_, j) = X509Certificate::from_der(&joined.der).unwrap();
    assert_ne!(f.raw_serial(), j.raw_serial());
    let key = ProvenCsr::parse(&b).unwrap().digest();
    assert_eq!(
        n.fresh_read(move |s| {
            assert_eq!(s.state().next_node_id().get(), 3);
            Ok(s.state()
                .authorize_peer([1; 16], NodeId::new(2).unwrap(), key))
        })
        .await
        .unwrap()
        .1,
        Ok(())
    );
    n.shutdown().await.unwrap();
});
bounded!(f6_adapters, async {
    use openraft::errors::{NotAllowed, NotInMembers};
    let d = private_temp();
    let mut n = start(d.path(), 1, 2).await;
    let (_, csr) = request();
    let ca = ClusterCa::ephemeral([1; 16], NOW);
    observation::capture(issue(&n, &ca, &csr)).await.unwrap();
    let key = ProvenCsr::parse(&csr).unwrap().digest();
    let response = |node, data| openraft::raft::ClientWriteResponse::<Arx> {
        log_id: LogId::new(
            LeaderId {
                term: 1,
                node_id: node,
            },
            7,
        ),
        data: Applied(Some(data)),
        membership: None,
    };
    assert!(matches!(
        n.committed_founder(
            response(2, Outcome::Founded { replay: false }),
            [1; 16],
            key
        ),
        Err(FoundError::WrongNode)
    ));
    assert!(matches!(
        n.committed_founder(response(1, Outcome::Founded { replay: true }), [1; 16], key),
        Err(FoundError::Rejected(Reject::AlreadyFounded))
    ));
    let grant = super::super::test_ticket(key).grant;
    assert!(matches!(
        n.committed_founder(response(1, Outcome::Enrolled(grant)), [1; 16], key),
        Err(FoundError::Rejected(Reject::UnknownCode))
    ));
    let t = FounderTicket {
        cluster: [1; 16],
        key,
        leader: LeaderId {
            term: 999,
            node_id: 1,
        },
        index: 7,
    };
    assert_eq!(n.founder_guard(&t).await, Err(FoundError::StaleIssuer));
    let (leader, ()) = n.fresh_read(|_| Ok(())).await.unwrap();
    let current = |cluster, key| FounderTicket {
        cluster,
        key,
        leader,
        index: 7,
    };
    assert_eq!(n.founder_guard(&current([1; 16], key)).await, Ok(()));
    let other_key = KeyDigest::of_spki(b"different founder key").unwrap();
    assert_ne!(other_key, key);
    assert_eq!(
        n.founder_guard(&current([1; 16], other_key)).await,
        Err(FoundError::Revoked)
    );
    assert_eq!(
        n.founder_guard(&current([2; 16], key)).await,
        Err(FoundError::Rejected(Reject::WrongCluster))
    );
    assert_eq!(
        init_refusal(RaftError::APIError(InitializeError::NotAllowed(
            NotAllowed {
                last_log_id: None,
                vote: Vote::new(0, 1)
            }
        ))),
        FoundError::AlreadyInitialized
    );
    assert_eq!(
        init_refusal(RaftError::APIError(InitializeError::NotInMembers(
            NotInMembers {
                node_id: 1,
                membership: Membership::new(
                    vec![[2].into()],
                    BTreeMap::from([(2, BasicNode::new("mem:2"))])
                )
                .unwrap()
            }
        ))),
        FoundError::Unavailable
    );
    n.shutdown().await.unwrap();
});
struct ScriptClock {
    times: Vec<u64>,
    calls: AtomicUsize,
    reads: Arc<AtomicUsize>,
    observed: Mutex<Vec<usize>>,
    block: bool,
}
impl SignerClock for ScriptClock {
    fn now_ms(&self) -> u64 {
        let i = self.calls.fetch_add(1, Ordering::SeqCst);
        self.observed
            .lock()
            .unwrap()
            .push(self.reads.load(Ordering::SeqCst));
        if self.block && i == 3 {
            std::thread::sleep(ISSUE_BUDGET + Duration::from_millis(200));
        }
        self.times[i]
    }
}
bounded!(f7_clock_guard_order, async {
    for times in [vec![NOW, 1000, 999], vec![NOW, 1000, 1000, 999]] {
        let d = private_temp();
        let mut n = start(d.path(), 1, 2).await;
        let (_, csr) = request();
        let ca = ClusterCa::ephemeral([1; 16], NOW);
        let c = ScriptClock {
            times: times.clone(),
            calls: AtomicUsize::new(0),
            reads: n.read_blocking.clone(),
            observed: Mutex::default(),
            block: false,
        };
        assert!(matches!(
            n.found(&ca, &c, BasicNode::new("mem:1"), &csr).await,
            Err(FoundError::ClockRollback)
        ));
        assert_eq!(c.calls.load(Ordering::SeqCst), times.len());
        assert_eq!(*c.observed.lock().unwrap(), vec![0, 0, 1, 2][..times.len()]);
        assert_eq!(ca.signs.load(Ordering::SeqCst), times.len() - 3);
        n.shutdown().await.unwrap();
    }
});
bounded!(f8_store_privacy, async {
    let d = private_temp();
    let mut n = start(d.path(), 1, 2).await;
    let (key, csr) = request();
    let ca = ClusterCa::ephemeral([1; 16], NOW);
    issue(&n, &ca, &csr).await.unwrap();
    n.shutdown().await.unwrap();
    let (_, req) = X509CertificationRequest::from_der(&csr).unwrap();
    let digest = ProvenCsr::parse(&csr).unwrap().digest();
    let markers = [
        ca.private_marker(),
        key.serialize_der(),
        req.signature_value.data.to_vec(),
    ];
    let mut positive = false;
    for entry in std::fs::read_dir(d.path()).unwrap() {
        let p = entry.unwrap().path();
        if p.is_file() {
            let bytes = std::fs::read(p).unwrap();
            positive |= bytes.windows(32).any(|b| b == digest.as_bytes());
            for marker in &markers {
                assert!(!bytes.windows(marker.len()).any(|b| b == marker));
            }
        }
    }
    assert!(positive);
});
bounded!(f9_post_deadline, async {
    let d = private_temp();
    let mut n = start(d.path(), 1, 2).await;
    let (_, csr) = request();
    let ca = ClusterCa::ephemeral([1; 16], NOW);
    let c = ScriptClock {
        times: vec![NOW; 4],
        calls: AtomicUsize::new(0),
        reads: n.read_blocking.clone(),
        observed: Mutex::default(),
        block: true,
    };
    assert!(matches!(
        n.found(&ca, &c, BasicNode::new("mem:1"), &csr).await,
        Err(FoundError::Unavailable)
    ));
    assert_eq!(ca.signs.load(Ordering::SeqCst), 1);
    assert_eq!(*c.observed.lock().unwrap(), vec![0, 0, 1, 2]);
    n.shutdown().await.unwrap();
});
struct Gate {
    release: Option<std::sync::mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Gate {
    fn new(n: &ConsensusNode) -> Self {
        let sm = n.sm.as_ref().unwrap().clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let (ready, wait) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let g = sm.0.lock().unwrap();
            ready.send(()).unwrap();
            rx.recv().unwrap();
            drop(g);
        });
        wait.recv().unwrap();
        Self {
            release: Some(tx),
            thread: Some(thread),
        }
    }
}
impl Drop for Gate {
    fn drop(&mut self) {
        self.release.take().unwrap().send(()).unwrap();
        self.thread.take().unwrap().join().unwrap();
    }
}
async fn drop_admitted(n: &ConsensusNode, ca: &ClusterCa, csr: &[u8], remaining: usize) {
    let future = issue(n, ca, csr);
    tokio::pin!(future);
    tokio::select! {biased; r=&mut future=>panic!("unexpected result: {:?}",r.err()), _=async {while n.read_permits.available_permits()!=remaining {tokio::task::yield_now().await;}}=>{}}
}
async fn released(n: &ConsensusNode) {
    timeout(Duration::from_secs(2), async {
        while n.read_permits.available_permits() != 8 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
bounded!(f10_drop_bounded, async {
    let d = private_temp();
    let mut n = start(d.path(), 1, 2).await;
    let (_, csr) = request();
    let ca = ClusterCa::ephemeral([1; 16], NOW);
    let gate = Gate::new(&n);
    let base = n.sm_weak.strong_count();
    for remaining in (0..8).rev() {
        drop_admitted(&n, &ca, &csr, remaining).await;
        assert_eq!(n.issue_permits.available_permits(), 4);
        assert_eq!(n.owners.lock().unwrap().set.len(), 8 - remaining);
        assert_eq!(n.sm_weak.strong_count(), base + 8 - remaining);
    }
    assert!(matches!(issue(&n, &ca, &csr).await, Err(FoundError::Busy)));
    assert_eq!(n.linearizable_state().await, Err(ReadError::Busy));
    assert_eq!(ca.signs.load(Ordering::SeqCst), 0);
    drop(gate);
    released(&n).await;
    uninitialized(&n);
    let (found, snapshot) =
        observation::capture_call(observation::Call::F10Final, issue(&n, &ca, &csr)).await;
    match found {
        Ok(_) => (),
        Err(error) => {
            let shutdown = n.shutdown().await;
            let mut cleanup = observation::Cleanup::unknown();
            cleanup.shutdown = observation::status(shutdown.is_ok());
            cleanup.store = observation::status(n.sm_weak.upgrade().is_none());
            cleanup.log = observation::status(n.log_weak.upgrade().is_none());
            observation::emit(&snapshot, cleanup);
            panic!("original founder refusal: {error}");
        }
    }
    n.shutdown().await.unwrap();
    observation::emit(&snapshot, observation::Cleanup::unknown());
});
bounded!(f10_outer_budget, async {
    let d = private_temp();
    let mut n = start(d.path(), 1, 2).await;
    let (_, csr) = request();
    let ca = ClusterCa::ephemeral([1; 16], NOW);
    let gate = Gate::new(&n);
    let base = n.sm_weak.strong_count();
    assert!(matches!(
        issue(&n, &ca, &csr).await,
        Err(FoundError::Unavailable)
    ));
    assert_eq!(n.issue_permits.available_permits(), 4);
    assert_eq!(n.read_permits.available_permits(), 7);
    assert_eq!(n.sm_weak.strong_count(), base + 1);
    drop(gate);
    released(&n).await;
    uninitialized(&n);
    issue(&n, &ca, &csr).await.unwrap();
    n.shutdown().await.unwrap();
});
bounded!(f10_shutdown_owns_closure, async {
    let d = private_temp();
    let mut n = start(d.path(), 1, 2).await;
    let (_, csr) = request();
    let ca = ClusterCa::ephemeral([1; 16], NOW);
    let gate = Gate::new(&n);
    drop_admitted(&n, &ca, &csr, 7).await;
    // Ensure the blocking closure has begun before shutdown may abort its async owner.
    // A closure queued behind the held mutex already owns the same Arc and permit.
    tokio::task::yield_now().await;
    let r = n.shutdown().await.unwrap_err().0;
    assert_eq!(r.owners_waited, 1);
    assert_eq!(r.owners_left, 1);
    assert_eq!(r.read_held, 1);
    assert!(r.sm_strong >= 1);
    assert_eq!(n.read_permits.available_permits(), 7);
    let before = n.sm_weak.strong_count();
    drop(gate);
    released(&n).await;
    timeout(Duration::from_secs(2), async {
        while n.sm_weak.strong_count() != before - 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(matches!(
        issue(&n, &ca, &csr).await,
        Err(FoundError::Unavailable)
    ));
});

fn observed_anchor(n: &ConsensusNode) -> crate::cluster::enrollment::JoinAnchor {
    let store = n.sm.as_ref().unwrap().0.lock().unwrap();
    let membership =
        crate::cluster::membership::Membership::decode(store.membership().unwrap().1.bytes())
            .unwrap();
    store.state().voter_anchor(&membership).unwrap()
}
