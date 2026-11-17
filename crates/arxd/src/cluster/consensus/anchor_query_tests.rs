//! Durable non-engine ownership proofs, plus one separately inventoried hosted engine proof.
use super::*;
use crate::cluster::{
    enrollment::{BoundVoters, FoundMembers, JoinCode, KeyDigest, NodeId},
    pki::{
        preproof::PreproofClient,
        preproof_tests::{Driver, ProbeTime, server},
    },
    state_store::release_pair,
};
use crate::service::application::tests::private_temp;
use rcgen::{CertificateParams, KeyPair, PublicKeyData};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    sync::{LazyLock, atomic::Ordering},
};
use tokio::task::AbortHandle;

static HANDLES: LazyLock<Mutex<BTreeMap<usize, Vec<AbortHandle>>>> = LazyLock::new(Mutex::default);
pub(super) fn record_owner(n: &ConsensusNode, handle: AbortHandle) {
    HANDLES
        .lock()
        .unwrap()
        .entry(Arc::as_ptr(&n.read_blocking) as usize)
        .or_default()
        .push(handle);
}
fn handles(n: &ConsensusNode) -> Vec<AbortHandle> {
    HANDLES
        .lock()
        .unwrap()
        .remove(&(Arc::as_ptr(&n.read_blocking) as usize))
        .unwrap_or_default()
}
struct Clock;
impl issue::SignerClock for Clock {
    fn now_ms(&self) -> u64 {
        1_000_000
    }
}
fn query(c: &JoinCode) -> Vec<u8> {
    let mut q = c.cluster().to_vec();
    q.extend(c.code_id());
    q
}
fn canonical(key: KeyDigest) -> Vec<u8> {
    let mut b = vec![1];
    b.extend(1u64.to_be_bytes());
    b.extend(key.as_bytes());
    b
}
fn code(key: KeyDigest, fingerprint: [u8; 32]) -> JoinCode {
    JoinCode::generate(
        [1; 16],
        fingerprint,
        BoundVoters::decode(&canonical(key))
            .unwrap()
            .anchor([1; 16]),
        |b| {
            b.fill(79);
            Ok::<_, ()>(())
        },
    )
    .unwrap()
}
fn durable(dir: &Path, key: KeyDigest, c: &JoinCode, consumed: bool) -> ConsensusNode {
    let mut store = StateStore::open(
        dir,
        Init {
            cluster: [1; 16],
            first_free: NodeId::new(2).unwrap(),
        },
    )
    .unwrap();
    let m = WireMembership::new(vec![[1].into()], [(1, "local".into())].into()).unwrap();
    let mut apply = |index, entry| {
        store
            .apply(
                LogPos {
                    term: 1,
                    leader: 1,
                    index,
                },
                &entry,
            )
            .unwrap()
    };
    assert_eq!(
        apply(
            0,
            StoredEntry::Membership(MembershipBlob::new(m.encode().unwrap()).unwrap())
        ),
        None
    );
    assert_eq!(
        apply(
            1,
            StoredEntry::Enrollment(Command::Found {
                cluster: [1; 16],
                members: FoundMembers::new([(NodeId::new(1).unwrap(), key)]).unwrap(),
                proposed_at_ms: 1_000_000,
            })
        ),
        Some(Outcome::Founded { replay: false })
    );
    assert_eq!(
        apply(
            2,
            StoredEntry::Enrollment(Command::Mint {
                cluster: [1; 16],
                actor: [1; 16],
                request: [1; 16],
                code: c.code_id(),
                verifier: c.verifier(),
                proposed_at_ms: 1_000_000,
            })
        ),
        Some(Outcome::Minted {
            expires_ms: 1_900_000,
            replay: false
        })
    );
    if consumed {
        assert!(matches!(
            apply(
                3,
                StoredEntry::Enrollment(Command::redeem(
                    [1; 16],
                    [2; 16],
                    c.code_id(),
                    &c.proof(),
                    KeyDigest::of_spki(b"joiner").unwrap(),
                    c.anchor(),
                    1_000_000
                ))
            ),
            Some(Outcome::Enrolled(_))
        ));
    }
    let sm_released = store.released();
    let sm = SmAdapter(Arc::new(Mutex::new(store)));
    let sm_weak = Arc::downgrade(&sm.0);
    let (ack, log_released) = release_pair();
    drop(ack);
    ConsensusNode {
        id: 1,
        read_permits: Arc::new(Semaphore::new(8)),
        issue_permits: Arc::new(Semaphore::new(4)),
        read_blocking: Arc::default(),
        raft: None,
        sm: Some(sm),
        log_released,
        sm_released,
        log_weak: Weak::<Mutex<VoteLogStore>>::new(),
        sm_weak,
        permits: Arc::new(Semaphore::new(64)),
        owners: Mutex::default(),
    }
}
struct Gate {
    release: Option<std::sync::mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Gate {
    fn hold(n: &ConsensusNode) -> Self {
        let store = n.sm.as_ref().unwrap().0.clone();
        let (release, wait) = std::sync::mpsc::channel();
        let (ready, entered) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let guard = store.lock().unwrap();
            ready.send(()).unwrap();
            let _ = wait.recv();
            drop(guard);
            drop(store);
        });
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        Self {
            release: Some(release),
            thread: Some(thread),
        }
    }
}
impl Drop for Gate {
    fn drop(&mut self) {
        let _ = self.release.take().unwrap().send(());
        self.thread.take().unwrap().join().unwrap();
    }
}
async fn entered(n: &ConsensusNode, count: usize) {
    timeout_at(Instant::now() + Duration::from_secs(2), async {
        while n.read_blocking.load(Ordering::SeqCst) != count {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("actual closure entry missing");
}
async fn units(n: &ConsensusNode) {
    timeout_at(Instant::now() + Duration::from_secs(2), async {
        while n.issue_permits.available_permits() != 4 || n.read_permits.available_permits() != 8 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("resources unresolved");
}
async fn joined(n: &ConsensusNode) {
    let mut set = {
        let mut o = n.owners.lock().unwrap();
        std::mem::take(&mut o.set)
    };
    timeout_at(Instant::now() + Duration::from_secs(2), async {
        while let Some(r) = set.join_next().await {
            r.unwrap();
        }
    })
    .await
    .expect("owners unresolved");
    units(n).await;
    handles(n);
}
fn saved_assert(f: impl FnOnce()) -> std::thread::Result<()> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
}
fn propagate(r: std::thread::Result<()>) {
    if let Err(p) = r {
        std::panic::resume_unwind(p);
    }
}

#[tokio::test]
async fn owned_query_dropped_callers_and_read_admission() {
    let dir = private_temp();
    let key = KeyDigest::of_spki(b"founder").unwrap();
    let c = code(key, [2; 32]);
    let q = query(&c);
    let mut n = durable(dir.path(), key, &c, false);
    struct CountClock(std::sync::atomic::AtomicUsize);
    impl issue::SignerClock for CountClock {
        fn now_ms(&self) -> u64 {
            self.0.fetch_add(1, Ordering::SeqCst);
            1_000_000
        }
    }
    let clock = CountClock(std::sync::atomic::AtomicUsize::new(0));
    assert_eq!(n.anchor_query(&q, &clock).await.unwrap(), canonical(key));
    joined(&n).await;
    assert_eq!(clock.0.load(Ordering::SeqCst), 1);
    n.read_blocking.store(0, Ordering::SeqCst);
    let gate = Gate::hold(&n);
    let baseline = n.sm_weak.strong_count();
    for i in 1..=4 {
        let mut call = Box::pin(n.anchor_query(&q, &Clock));
        assert!(futures_util::poll!(call.as_mut()).is_pending());
        entered(&n, i).await;
        drop(call);
    }
    let check = saved_assert(|| {
        assert_eq!(n.issue_permits.available_permits(), 0);
        assert_eq!(n.read_permits.available_permits(), 4);
        assert_eq!(n.sm_weak.strong_count(), baseline + 4);
        assert_eq!(n.owners.lock().unwrap().set.len(), 4);
    });
    let fifth = n.anchor_query(&q, &Clock).await;
    drop(gate);
    joined(&n).await;
    assert_eq!(n.sm_weak.strong_count(), 1);
    propagate(check);
    assert_eq!(fifth, Err(AnchorQueryError::Busy));
    let all = n.read_permits.clone().try_acquire_many_owned(8).unwrap();
    assert_eq!(
        n.anchor_query(&q, &Clock).await,
        Err(AnchorQueryError::Busy)
    );
    assert_eq!(n.issue_permits.available_permits(), 4);
    drop(all);
    n.shutdown().await.unwrap();
    assert!(n.sm_weak.upgrade().is_none());
}

#[tokio::test]
async fn owned_query_timeout_shutdown_retains_submitted_resources() {
    let dir = private_temp();
    let key = KeyDigest::of_spki(b"founder").unwrap();
    let c = code(key, [2; 32]);
    let mut n = durable(dir.path(), key, &c, false);
    let gate = Gate::hold(&n);
    let q = query(&c);
    let result = n.anchor_query(&q, &Clock).await;
    entered(&n, 1).await;
    let before = saved_assert(|| {
        assert_eq!(result, Err(AnchorQueryError::Unavailable));
        assert_eq!(n.issue_permits.available_permits(), 3);
        assert_eq!(n.read_permits.available_permits(), 7);
    });
    let owners = handles(&n);
    let report = n.shutdown().await;
    let finished = timeout_at(Instant::now() + Duration::from_secs(2), async {
        while owners.iter().any(|h| !h.is_finished()) {
            tokio::task::yield_now().await;
        }
    })
    .await;
    let after = saved_assert(|| {
        finished.expect("abort completion unresolved");
        let report = report.expect_err("blocked work must be orphaned").0;
        assert_eq!(report.engine, EngineEnd::Absent);
        assert_eq!(report.read_held, 1);
        assert_eq!(n.issue_permits.available_permits(), 3);
        assert_eq!(
            n.read_permits.available_permits(),
            7,
            "M1 post-abort read retention"
        );
        assert!(n.sm_weak.upgrade().is_some());
    });
    let closed = n.anchor_query(&q, &Clock).await;
    drop(gate);
    units(&n).await;
    timeout_at(Instant::now() + Duration::from_secs(2), async {
        while !n.sm_released.is_set() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(n.sm_weak.upgrade().is_none());
    assert_eq!(closed, Err(AnchorQueryError::Unavailable));
    assert_eq!(n.read_blocking.load(Ordering::SeqCst), 1);
    propagate(before);
    propagate(after);
}

#[tokio::test]
async fn owned_query_bounds_poison_and_no_submission() {
    let dir = private_temp();
    let key = KeyDigest::of_spki(b"founder").unwrap();
    let c = code(key, [2; 32]);
    let q = query(&c);
    let mut n = durable(dir.path(), key, &c, false);
    struct NeverClock;
    impl issue::SignerClock for NeverClock {
        fn now_ms(&self) -> u64 {
            panic!("malformed input sampled clock")
        }
    }
    for len in [0, 31, 33, 4096] {
        assert_eq!(
            n.anchor_query(&vec![0; len], &NeverClock).await,
            Err(AnchorQueryError::Refused)
        );
    }
    let mut cancelled = Box::pin(n.anchor_query(&q, &Clock));
    assert!(futures_util::poll!(cancelled.as_mut()).is_pending());
    drop(cancelled);
    joined(&n).await;
    assert_eq!(n.read_blocking.load(Ordering::SeqCst), 0);
    struct SlowClock;
    impl issue::SignerClock for SlowClock {
        fn now_ms(&self) -> u64 {
            std::thread::sleep(READ_BUDGET + Duration::from_millis(20));
            1_000_000
        }
    }
    assert_eq!(
        n.anchor_query(&q, &SlowClock).await,
        Err(AnchorQueryError::Unavailable)
    );
    joined(&n).await;
    assert_eq!(n.read_blocking.load(Ordering::SeqCst), 0);
    let store = n.sm.as_ref().unwrap().0.clone();
    let _ = std::thread::spawn(move || {
        let _guard = store.lock().unwrap();
        panic!("synthetic poison");
    })
    .join();
    assert_eq!(
        n.anchor_query(&q, &Clock).await,
        Err(AnchorQueryError::Unavailable)
    );
    joined(&n).await;
    n.shutdown().await.unwrap();
    assert_eq!(
        n.anchor_query(&q, &Clock).await,
        Err(AnchorQueryError::Unavailable)
    );
    units(&n).await;
    let _ = saved_assert(|| {
        let _guard = n.owners.lock().unwrap();
        panic!("synthetic owners poison");
    });
    assert_eq!(
        n.anchor_query(&q, &Clock).await,
        Err(AnchorQueryError::Unavailable)
    );
    units(&n).await;
    n.owners.clear_poison();
}

fn plaintext(s: &mut rustls::ServerConnection) -> Vec<u8> {
    let mut bytes = [0; 65];
    let count = match s.reader().read(&mut bytes) {
        Ok(n) => n,
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => 0,
        Err(_) => panic!("unexpected plaintext error"),
    };
    bytes[..count].to_vec()
}
fn synthetic_tls() -> (Vec<u8>, Vec<u8>, KeyPair) {
    let ca_key = KeyPair::generate().unwrap();
    let mut ca = CertificateParams::default();
    ca.not_before = time::OffsetDateTime::from_unix_timestamp(0).unwrap();
    ca.not_after = time::OffsetDateTime::from_unix_timestamp(100000).unwrap();
    ca.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Constrained(0));
    ca.key_usages = vec![rcgen::KeyUsagePurpose::KeyCertSign];
    let root = ca.self_signed(&ca_key).unwrap().der().to_vec();
    let issuer = rcgen::Issuer::new(ca, ca_key);
    let key = KeyPair::generate().unwrap();
    let mut leaf = CertificateParams::default();
    leaf.not_before = time::OffsetDateTime::from_unix_timestamp(0).unwrap();
    leaf.not_after = time::OffsetDateTime::from_unix_timestamp(100000).unwrap();
    leaf.key_usages = vec![rcgen::KeyUsagePurpose::DigitalSignature];
    leaf.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
    leaf.subject_alt_names = vec![rcgen::SanType::URI(
        crate::cluster::pki::node_uri([1; 16], NodeId::new(1).unwrap())
            .try_into()
            .unwrap(),
    )];
    (
        root,
        leaf.signed_by(&key, &issuer).unwrap().der().to_vec(),
        key,
    )
}
async fn tls_query(
    n: &ConsensusNode,
    c: &JoinCode,
    root: &[u8],
    der: Vec<u8>,
    key: &KeyPair,
) -> (Result<Vec<u8>, AnchorQueryError>, Vec<u8>) {
    let mut client = PreproofClient::new(c, root, Arc::new(ProbeTime)).unwrap();
    let mut endpoint = server(der, key);
    let mut driver = Driver::new();
    driver.pump(&mut client, &mut endpoint).unwrap();
    client.query().unwrap();
    driver.pump(&mut client, &mut endpoint).unwrap();
    let received = plaintext(&mut endpoint);
    assert_eq!(received, query(c));
    let result = n.anchor_query(&received, &Clock).await;
    if let Ok(list) = &result {
        endpoint.writer().write_all(list).unwrap();
        driver.pump(&mut client, &mut endpoint).unwrap();
        let mut bytes = [0; 641];
        let count = client.read_anchor(&mut bytes).unwrap();
        client.accept_anchor(&bytes[..count]).unwrap();
        client.release_proof(c).unwrap();
        driver.pump(&mut client, &mut endpoint).unwrap();
    }
    (result, plaintext(&mut endpoint))
}
#[tokio::test]
async fn owned_query_consumed_tls_zero_proof() {
    let (root, der, key) = synthetic_tls();
    let digest = KeyDigest::of_spki(&key.subject_public_key_info()).unwrap();
    let c = code(digest, Sha256::digest(&root).into());
    let dir = private_temp();
    let mut n = durable(dir.path(), digest, &c, true);
    let (result, proof) = tls_query(&n, &c, &root, der, &key).await;
    joined(&n).await;
    n.shutdown().await.unwrap();
    assert!(n.sm_weak.upgrade().is_none());
    assert_eq!(proof.len(), 0, "M2 consumed query server plaintext");
    assert_eq!(result, Err(AnchorQueryError::Refused));
}

use super::super::{
    issue::RedeemInput,
    issue::found::founder_init,
    mem_net::{Factory, MemRouter},
};
use crate::cluster::pki::ca::ClusterCa;
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn owned_anchor_query_single_voter() {
    let entry = Instant::now();
    let work_deadline = entry + Duration::from_secs(35);
    let cleanup_deadline = entry + Duration::from_secs(40);
    let dir = private_temp();
    let router = Arc::new(MemRouter::default());
    let mut node = None;
    let mut blocking = None;
    let result = timeout_at(work_deadline, async {
        let n = ConsensusNode::start(
            1,
            dir.path(),
            founder_init([1; 16]),
            Params::default(),
            Factory {
                source: 1,
                router: router.clone(),
            },
        )
        .await
        .unwrap();
        router
            .nodes
            .lock()
            .unwrap()
            .insert(1, n.raft.as_ref().unwrap().clone());
        node = Some(n);
        let n = node.as_ref().unwrap();
        let ca = ClusterCa::ephemeral([1; 16], 1_000_000);
        let key = KeyPair::generate().unwrap();
        let csr = CertificateParams::default()
            .serialize_request(&key)
            .unwrap();
        let founder = n
            .found(&ca, &Clock, BasicNode::new("mem:1"), csr.der())
            .await
            .unwrap();
        let store = n.sm.as_ref().unwrap().0.clone();
        blocking = Some(tokio::task::spawn_blocking(move || {
            let list = {
                let guard = store.lock().unwrap();
                let m = WireMembership::decode(guard.membership().unwrap().1.bytes()).unwrap();
                BoundVoters::observe(guard.state(), &m).unwrap().encode()
            };
            drop(store);
            list
        }));
        let list = blocking.as_mut().unwrap().await.unwrap();
        blocking = None;
        let code = JoinCode::generate(
            [1; 16],
            ca.fingerprint(),
            BoundVoters::decode(&list).unwrap().anchor([1; 16]),
            |b| {
                b.fill(121);
                Ok::<_, ()>(())
            },
        )
        .unwrap();
        n.propose(Command::Mint {
            cluster: code.cluster(),
            actor: [1; 16],
            request: [1; 16],
            code: code.code_id(),
            verifier: code.verifier(),
            proposed_at_ms: 1_000_000,
        })
        .await
        .unwrap();
        let root = ca.cert_der().to_vec();
        let (query_result, proof) = tls_query(n, &code, &root, founder.der, &key).await;
        assert_eq!(query_result.unwrap(), list);
        assert_eq!(&proof, code.proof().bytes());
        let join_key = KeyPair::generate().unwrap();
        let csr = CertificateParams::default()
            .serialize_request(&join_key)
            .unwrap();
        let leaf = n
            .enroll(
                &ca,
                &Clock,
                RedeemInput {
                    cluster: code.cluster(),
                    request: [2; 16],
                    code: code.code_id(),
                    proof: code.proof(),
                    anchor: code.anchor(),
                },
                csr.der(),
            )
            .await
            .unwrap();
        assert_eq!(leaf.node.get(), 2);
        let replay = n
            .enroll(
                &ca,
                &Clock,
                RedeemInput {
                    cluster: code.cluster(),
                    request: [2; 16],
                    code: code.code_id(),
                    proof: code.proof(),
                    anchor: code.anchor(),
                },
                csr.der(),
            )
            .await
            .unwrap();
        assert_eq!(leaf.identity_commit, replay.identity_commit);
    })
    .await;
    if let Some(work) = blocking.take() {
        timeout_at(cleanup_deadline, work)
            .await
            .expect("blocking cleanup unresolved")
            .unwrap();
    }
    router.nodes.lock().unwrap().clear();
    drop(router);
    let n = node
        .as_mut()
        .expect("startup unresolved; cleanup unknown: blocking startup cannot be aborted");
    timeout_at(cleanup_deadline, n.shutdown())
        .await
        .expect("cleanup deadline exceeded")
        .expect("clean shutdown required");
    assert!(n.sm_weak.upgrade().is_none());
    assert!(n.log_weak.upgrade().is_none());
    handles(n);
    result.expect("work exceeded cooperative deadline");
}
