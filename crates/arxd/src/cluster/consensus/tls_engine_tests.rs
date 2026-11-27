use super::super::three_node::{Cluster, code, mint};
use super::found::{FOUNDER, founder_init};
use super::*;
use crate::cluster::enrollment::JoinCode;
use crate::cluster::pki::ca::ClusterCa as TestCa;
use crate::cluster::pki::{self, verify::*};
use openraft::type_config::async_runtime::watch::WatchReceiver;
use rcgen::{CertificateParams, KeyPair, PublicKeyData};
use rustls::pki_types::CertificateDer;
use rustls::server::danger::ClientCertVerifier;
use rustls::{
    ClientConnection, ServerConnection,
    pki_types::{PrivatePkcs8KeyDer, ServerName},
};
use tokio::time::timeout;
use x509_parser::prelude::*;
struct Clock;
impl SignerClock for Clock {
    fn now_ms(&self) -> u64 {
        pki::NOW
    }
}
impl super::super::checked_clock::CheckedClock for Clock {
    fn sample_ms(&self) -> Result<u64, super::super::checked_clock::ClockError> {
        Ok(SignerClock::now_ms(self))
    }
}
async fn enroll(c: &mut Cluster, ca: &ClusterCa, key: &KeyPair, marker: u8) -> IssuedLeaf {
    let code = code(marker);
    c.extra_markers.push(key.serialize_der());
    c.extra_markers.push(ca.private_marker());

    use sha2::{Digest, Sha256};
    let secret = vec![marker.wrapping_add(128); 32];
    let proof = Sha256::new()
        .chain_update(b"arxcovo/join/proof/v1")
        .chain_update(code.cluster())
        .chain_update(code.code_id())
        .chain_update(&secret)
        .finalize()
        .to_vec();
    c.extra_markers
        .extend([secret, proof, code.expose_for_owner_transfer().into_bytes()]);
    c.write(mint(&code, marker, pki::NOW)).await;
    let csr = CertificateParams::default().serialize_request(key).unwrap();
    let l = c.leader().await;
    c.node(l)
        .enroll(
            ca,
            &Clock,
            RedeemInput {
                cluster: [1; 16],
                request: [marker; 16],
                code: code.code_id(),
                proof: code.proof(),
                anchor: code.anchor(),
            },
            csr.der(),
        )
        .await
        .unwrap()
}
async fn handshake(
    ca: &ClusterCa,
    leaf: &IssuedLeaf,
    key: &KeyPair,
    registry: Registry,
    expected: u64,
) -> Result<(AuthenticatedPeer, AuthenticatedPeer), rustls::Error> {
    let (c, s) = pki::connections(ca, &leaf.der, key, registry, pki::node(expected), 1001);
    tokio::task::spawn_blocking(move || pki::pump(c, s))
        .await
        .unwrap()
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    target_os = "windows",
    ignore = "Windows whole-fixture quarantine: docs/adr/0024-windows-three-node-test-quarantine.md"
)]
async fn enrolled_target_binding_revocation_and_stale_acceptance() {
    timeout(Duration::from_secs(45), async {
        let mut c = Cluster::new(Params::default()).await;
        let ca = ClusterCa::ephemeral([1; 16], pki::NOW);
        let key = KeyPair::generate().unwrap();
        let leaf = enroll(&mut c, &ca, &key, 31).await;
        let key5 = KeyPair::generate().unwrap();
        let leaf5 = enroll(&mut c, &ca, &key5, 32).await;
        assert_eq!(leaf.node.get(), 4);
        assert_eq!(leaf5.node.get(), 5);
        c.converge().await;
        let l = c.leader().await;
        let f = (l + 1) % 3;
        handshake(&ca, &leaf5, &key5, c.node(l).peer_registry(), 5)
            .await
            .unwrap();
        assert_eq!(
            pki::refusal(
                handshake(&ca, &leaf, &key, c.node(l).peer_registry(), 5)
                    .await
                    .err()
                    .unwrap()
            ),
            VerifyRefusal::TargetMismatch
        );
        let (a, b) = handshake(&ca, &leaf, &key, c.node(l).peer_registry(), 4)
            .await
            .unwrap();
        assert_eq!(a.node(), pki::node(4));
        assert_eq!(b.node(), pki::node(4));
        c.router.isolated.lock().unwrap().insert(f as u64 + 1);
        c.write(Command::Remove {
            cluster: [1; 16],
            node: leaf.node,
            proposed_at_ms: pki::NOW,
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
                    .applied_index_at_least(Some(index), "TLS removal local apply")
                    .await
                    .unwrap();
                assert_eq!(
                    pki::refusal(
                        handshake(&ca, &leaf, &key, c.node(i).peer_registry(), 4)
                            .await
                            .err()
                            .unwrap()
                    ),
                    VerifyRefusal::NotMember
                );
            }
        }
        handshake(&ca, &leaf, &key, c.node(f).peer_registry(), 4)
            .await
            .unwrap();
        c.router.isolated.lock().unwrap().clear();
        c.converge().await;
        assert_eq!(
            pki::refusal(
                handshake(&ca, &leaf, &key, c.node(f).peer_registry(), 4)
                    .await
                    .err()
                    .unwrap()
            ),
            VerifyRefusal::NotMember
        );
        c.finish().await;
    })
    .await
    .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    not(target_os = "windows"),
    ignore = "intermittent NotLeader quarantine: docs/ROADMAP.md Known issues (Engine fix hold)"
)]
#[cfg_attr(
    target_os = "windows",
    ignore = "Windows whole-fixture quarantine: docs/adr/0024-windows-three-node-test-quarantine.md"
)]
async fn authenticated_sender_mismatch_and_assumed_founder_control() {
    timeout(Duration::from_secs(45), async {
        let mut c = Cluster::new(Params::default()).await;
        let ca = ClusterCa::ephemeral([1; 16], pki::NOW);
        let key = KeyPair::generate().unwrap();
        let leaf = enroll(&mut c, &ca, &key, 33).await;
        c.converge().await;
        let l = c.leader().await;
        let oldterm = c
            .node(l)
            .raft
            .as_ref()
            .unwrap()
            .metrics()
            .borrow_watched()
            .current_term;
        let (real, _) = handshake(&ca, &leaf, &key, c.node(l).peer_registry(), 4)
            .await
            .unwrap();
        let before = c.router.mismatch.load(std::sync::atomic::Ordering::Relaxed);
        // source == claimed on every real leader RPC; authenticated node 4 differs.
        c.router
            .authenticated
            .lock()
            .unwrap()
            .insert(l as u64 + 1, real);
        timeout(Duration::from_secs(5), async {
            loop {
                let new = (0..3).any(|i| {
                    i != l
                        && c.node(i)
                            .raft
                            .as_ref()
                            .unwrap()
                            .metrics()
                            .borrow_watched()
                            .state
                            == openraft::ServerState::Leader
                });
                if new && c.router.mismatch.load(std::sync::atomic::Ordering::Relaxed) > before {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        // E7 cleanup: old leader must observe the new term before converge.
        timeout(Duration::from_secs(5), async {
            loop {
                if c.node(l)
                    .raft
                    .as_ref()
                    .unwrap()
                    .metrics()
                    .borrow_watched()
                    .current_term
                    > oldterm
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("old TLS sender observes new term");
        c.router.authenticated.lock().unwrap().insert(
            l as u64 + 1,
            AuthenticatedPeer::assume(pki::node(l as u64 + 1)),
        );
        c.converge().await;
        assert!(
            c.router
                .mismatch
                .swap(0, std::sync::atomic::Ordering::Relaxed)
                > before
        );
        c.finish().await;
    })
    .await
    .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    target_os = "macos",
    ignore = "ADR 0056 macOS single-case quarantine: main run 45906354582 attempt 1 ([DIAG-3n] lag=288@738 ticks=10 cand=Cpu)"
)]
#[cfg_attr(
    target_os = "windows",
    ignore = "Windows whole-fixture quarantine: docs/adr/0024-windows-three-node-test-quarantine.md"
)]
async fn store_lock_wait_does_not_block_runtime() {
    timeout(Duration::from_secs(45), async {
        let mut c = Cluster::new(Params::default()).await;
        let ca = ClusterCa::ephemeral([1; 16], pki::NOW);
        let key = KeyPair::generate().unwrap();
        let leaf = enroll(&mut c, &ca, &key, 34).await;
        c.converge().await;
        let l = c.leader().await;
        let sm = c.node(l).sm.as_ref().unwrap().clone();
        let (entered, wait) = oneshot::channel();
        let (release, resume) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            let guard = sm.0.lock().unwrap();
            entered.send(()).unwrap();
            resume.recv().unwrap();
            drop(guard);
        });
        wait.await.unwrap();
        let (client, server) = pki::connections(
            &ca,
            &leaf.der,
            &key,
            c.node(l).peer_registry(),
            pki::node(4),
            1001,
        );
        let task = tokio::task::spawn_blocking(move || pki::pump(client, server));
        let start = Instant::now();
        let mut ticks = 0;
        let mut beat = tokio::time::interval(Duration::from_millis(10));
        while start.elapsed() < Duration::from_millis(300) {
            beat.tick().await;
            ticks += 1;
            assert!(!task.is_finished());
        }
        assert!(ticks >= 20);
        release.send(()).unwrap();
        holder.join().unwrap();
        task.await.unwrap().unwrap();
        c.finish().await;
    })
    .await
    .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn live_verifier_does_not_retain_store_and_poison_refuses() {
    timeout(Duration::from_secs(45), async {
        use super::super::mem_net::{Factory, MemRouter};
        let dir = crate::service::application::tests::private_temp();
        assert!(
            crate::security::private(dir.path()).is_ok(),
            "TLS store directory must be private before startup"
        );
        let router = Arc::new(MemRouter::default());
        let mut n = ConsensusNode::start(
            1,
            dir.path(),
            Init {
                cluster: [1; 16],
                first_free: NodeId::new(4).unwrap(),
            },
            Params::default(),
            Factory { source: 1, router },
        )
        .await
        .unwrap();
        let ca = ClusterCa::ephemeral([1; 16], pki::NOW);
        let key = KeyPair::generate().unwrap();
        let der = CertificateDer::from(pki::leaf(&ca, &key));
        let v = pki::verifier(&ca, n.peer_registry()).server();
        let now = rustls::pki_types::UnixTime::since_unix_epoch(Duration::from_secs(1001));
        let sm = n.sm.as_ref().unwrap().clone();
        assert!(
            std::thread::spawn(move || {
                let _guard = sm.0.lock().unwrap();
                panic!("test poison");
            })
            .join()
            .is_err()
        );
        assert_eq!(
            pki::refusal(v.verify_client_cert(&der, &[], now).err().unwrap()),
            VerifyRefusal::Unavailable
        );
        n.shutdown().await.unwrap();
        assert_eq!(
            pki::refusal(v.verify_client_cert(&der, &[], now).err().unwrap()),
            VerifyRefusal::Unavailable
        );
    })
    .await
    .unwrap();
}

#[test]
fn tls_single_node_directory_precondition() {
    let dir = crate::service::application::tests::private_temp();
    assert!(
        crate::security::private(dir.path()).is_ok(),
        "TLS store directory must pass the first open_db check"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn founder_joiner_mutual_tls_real_identities() {
    timeout(Duration::from_secs(20), async {
        use super::super::mem_net::{Factory, MemRouter};
        let dir = crate::service::application::tests::private_temp();
        assert!(crate::security::private(dir.path()).is_ok());
        let mut n = ConsensusNode::start(
            1,
            dir.path(),
            founder_init([1; 16]),
            Params::default(),
            Factory {
                source: 1,
                router: Arc::new(MemRouter::default()),
            },
        )
        .await
        .unwrap();
        let ca = TestCa::ephemeral([1; 16], pki::NOW);
        let fkey = KeyPair::generate().unwrap();
        let fcsr = CertificateParams::default()
            .serialize_request(&fkey)
            .unwrap();
        let founder = n
            .found(&ca, &Clock, BasicNode::new("mem:1"), fcsr.der())
            .await
            .unwrap();
        assert_eq!(founder.node, FOUNDER);
        let jkey = KeyPair::generate().unwrap();
        let jcsr = CertificateParams::default()
            .serialize_request(&jkey)
            .unwrap();
        assert_ne!(
            fkey.subject_public_key_info(),
            jkey.subject_public_key_info()
        );
        let code = JoinCode::generate([1; 16], ca.fingerprint(), observed_anchor(&n), |b| {
            b.fill(15);
            Ok::<_, ()>(())
        })
        .unwrap();
        n.propose(Command::Mint {
            cluster: [1; 16],
            actor: [3; 16],
            request: [4; 16],
            code: code.code_id(),
            verifier: code.verifier(),
            proposed_at_ms: pki::NOW,
        })
        .await
        .unwrap();
        let joiner = n
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
                jcsr.der(),
            )
            .await
            .unwrap();
        assert_eq!(joiner.node, pki::node(2));
        // One committed founder view for both roles; no joiner replica or second host.
        let reg = n.peer_registry();
        let (fc, fs, jc, js, reg, fd) = tokio::task::spawn_blocking(move || {
            let (_, root) = X509Certificate::from_der(ca.cert_der()).unwrap();
            for (leaf, key, id) in [(&founder, &fkey, 1), (&joiner, &jkey, 2)] {
                let (_, cert) = X509Certificate::from_der(&leaf.der).unwrap();
                cert.verify_signature(Some(root.public_key())).unwrap();
                assert_eq!(cert.public_key().raw, key.subject_public_key_info());
                assert_eq!(
                    cert.subject_alternative_name()
                        .unwrap()
                        .unwrap()
                        .value
                        .general_names,
                    vec![GeneralName::URI(
                        pki::node_uri([1; 16], pki::node(id)).as_str()
                    )]
                );
            }
            let (_, fcert) = X509Certificate::from_der(&founder.der).unwrap();
            let (_, jcert) = X509Certificate::from_der(&joiner.der).unwrap();
            assert_ne!(fcert.raw_serial(), jcert.raw_serial());
            let fd = KeyDigest::of_spki(&fkey.subject_public_key_info()).unwrap();
            let jd = KeyDigest::of_spki(&jkey.subject_public_key_info()).unwrap();
            assert_eq!(reg([1; 16], pki::node(1), fd), Ok(()));
            assert_eq!(reg([1; 16], pki::node(2), jd), Ok(()));
            assert_eq!(
                reg([1; 16], pki::node(1), jd),
                Err(VerifyRefusal::KeyMismatch)
            );
            assert_eq!(
                reg([1; 16], pki::node(2), fd),
                Err(VerifyRefusal::KeyMismatch)
            );
            assert_eq!(
                reg([1; 16], pki::node(3), jd),
                Err(VerifyRefusal::NotMember)
            );
            let time = Arc::new(pki::FixedTime(pki::NOW / 1000));
            let fs = Arc::new(
                server_config(
                    pki::verifier(&ca, reg.clone()).server(),
                    founder.der.clone().into(),
                    PrivatePkcs8KeyDer::from(fkey.serialize_der()).into(),
                    time.clone(),
                )
                .unwrap(),
            );
            let jc = Arc::new(
                client_config(
                    pki::verifier(&ca, reg.clone()).client(pki::node(1)),
                    joiner.der.clone().into(),
                    PrivatePkcs8KeyDer::from(jkey.serialize_der()).into(),
                    time.clone(),
                )
                .unwrap(),
            );
            let fc = Arc::new(
                client_config(
                    pki::verifier(&ca, reg.clone()).client(pki::node(2)),
                    founder.der.into(),
                    PrivatePkcs8KeyDer::from(fkey.serialize_der()).into(),
                    time.clone(),
                )
                .unwrap(),
            );
            let js = Arc::new(
                server_config(
                    pki::verifier(&ca, reg.clone()).server(),
                    joiner.der.clone().into(),
                    PrivatePkcs8KeyDer::from(jkey.serialize_der()).into(),
                    time.clone(),
                )
                .unwrap(),
            );
            for (client, server, expected) in [
                (jc.clone(), fs.clone(), (1, 2)),
                (fc.clone(), js.clone(), (2, 1)),
            ] {
                let (a, b) = pki::pump(
                    ClientConnection::new(client, ServerName::try_from(SERVER_NAME).unwrap())
                        .unwrap(),
                    ServerConnection::new(server).unwrap(),
                )
                .unwrap();
                assert_eq!(
                    (a.node(), b.node()),
                    (pki::node(expected.0), pki::node(expected.1))
                );
            }
            let wrong = Arc::new(
                client_config(
                    pki::verifier(&ca, reg.clone()).client(pki::node(2)),
                    joiner.der.into(),
                    PrivatePkcs8KeyDer::from(jkey.serialize_der()).into(),
                    time,
                )
                .unwrap(),
            );
            assert_eq!(
                pki::refusal(
                    pki::pump(
                        ClientConnection::new(wrong, ServerName::try_from(SERVER_NAME).unwrap())
                            .unwrap(),
                        ServerConnection::new(fs.clone()).unwrap()
                    )
                    .err()
                    .unwrap()
                ),
                VerifyRefusal::TargetMismatch
            );
            (fc, fs, jc, js, reg, fd)
        })
        .await
        .unwrap();
        assert_eq!(
            n.propose(Command::Remove {
                cluster: [1; 16],
                node: pki::node(2),
                proposed_at_ms: pki::NOW,
            })
            .await
            .unwrap()
            .1,
            Some(Outcome::Removed { replay: false })
        );
        let retained = (fc.clone(), fs.clone(), jc.clone(), js.clone());
        tokio::task::spawn_blocking(move || {
            let (fc, fs, jc, js) = retained;
            for (client, server) in [(jc, fs), (fc, js)] {
                assert_eq!(
                    pki::refusal(
                        pki::pump(
                            ClientConnection::new(
                                client,
                                ServerName::try_from(SERVER_NAME).unwrap()
                            )
                            .unwrap(),
                            ServerConnection::new(server).unwrap()
                        )
                        .err()
                        .unwrap()
                    ),
                    VerifyRefusal::NotMember
                );
            }
            assert_eq!(reg([1; 16], pki::node(1), fd), Ok(()));
        })
        .await
        .unwrap();
        // Retained verifiers own only a weak store reference.
        n.shutdown().await.unwrap();
        tokio::task::spawn_blocking(move || {
            assert_eq!(
                pki::refusal(
                    pki::pump(
                        ClientConnection::new(jc, ServerName::try_from(SERVER_NAME).unwrap())
                            .unwrap(),
                        ServerConnection::new(fs).unwrap()
                    )
                    .err()
                    .unwrap()
                ),
                VerifyRefusal::Unavailable
            );
            drop((fc, js));
        })
        .await
        .unwrap();
    })
    .await
    .unwrap();
}

// Log replay uses the mem_net double's default sender == source check only.
// Production TLS cannot bootstrap this empty registry; no transport trust claim.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn learner_catch_up_gives_joiner_own_registry() {
    let deadline = Instant::now() + Duration::from_secs(20);
    timeout_at(deadline, async {
        use super::super::mem_net::{Factory, MemRouter};
        let fdir = crate::service::application::tests::private_temp();
        let jdir = crate::service::application::tests::private_temp();
        assert!(crate::security::private(fdir.path()).is_ok());
        assert!(crate::security::private(jdir.path()).is_ok());
        let router = Arc::new(MemRouter::default());
        let mut n1 = ConsensusNode::start(
            1,
            fdir.path(),
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
            .insert(1, n1.raft.as_ref().unwrap().clone());
        let ca = TestCa::ephemeral([1; 16], pki::NOW);
        let fkey = KeyPair::generate().unwrap();
        let fcsr = CertificateParams::default()
            .serialize_request(&fkey)
            .unwrap();
        let founder = n1
            .found(&ca, &Clock, BasicNode::new("mem:1"), fcsr.der())
            .await
            .unwrap();
        assert_eq!(founder.node, FOUNDER);
        let jkey = KeyPair::generate().unwrap();
        let jcsr = CertificateParams::default()
            .serialize_request(&jkey)
            .unwrap();
        assert_ne!(
            fkey.subject_public_key_info(),
            jkey.subject_public_key_info()
        );
        let code = JoinCode::generate([1; 16], ca.fingerprint(), observed_anchor(&n1), |b| {
            b.fill(15);
            Ok::<_, ()>(())
        })
        .unwrap();
        n1.propose(Command::Mint {
            cluster: [1; 16],
            actor: [3; 16],
            request: [4; 16],
            code: code.code_id(),
            verifier: code.verifier(),
            proposed_at_ms: pki::NOW,
        })
        .await
        .unwrap();
        let joiner = n1
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
                jcsr.der(),
            )
            .await
            .unwrap();
        assert_eq!(joiner.node, pki::node(2));
        let mut n2 = ConsensusNode::start(
            2,
            jdir.path(),
            founder_init([1; 16]),
            Params::default(),
            Factory {
                source: 2,
                router: router.clone(),
            },
        )
        .await
        .unwrap();
        router
            .nodes
            .lock()
            .unwrap()
            .insert(2, n2.raft.as_ref().unwrap().clone());
        {
            let m = n2.raft.as_ref().unwrap().metrics().borrow_watched().clone();
            assert_eq!(m.last_log_index, None);
            assert_eq!(m.current_term, 0);
        }
        let code2 = JoinCode::generate([1; 16], ca.fingerprint(), observed_anchor(&n1), |b| {
            b.fill(16);
            Ok::<_, ()>(())
        })
        .unwrap();
        let freg = n1.peer_registry();
        let jreg = n2.peer_registry();
        let (fc, fs, jc, js, fd, jd) = tokio::task::spawn_blocking(move || {
            let fd = KeyDigest::of_spki(&fkey.subject_public_key_info()).unwrap();
            let jd = KeyDigest::of_spki(&jkey.subject_public_key_info()).unwrap();
            assert_eq!(
                jreg([1; 16], pki::node(1), fd),
                Err(VerifyRefusal::NotMember)
            );
            assert_eq!(
                jreg([1; 16], pki::node(2), jd),
                Err(VerifyRefusal::NotMember)
            );
            let time = Arc::new(pki::FixedTime(pki::NOW / 1000));
            let fs = Arc::new(
                server_config(
                    pki::verifier(&ca, freg.clone()).server(),
                    founder.der.clone().into(),
                    PrivatePkcs8KeyDer::from(fkey.serialize_der()).into(),
                    time.clone(),
                )
                .unwrap(),
            );
            let fc = Arc::new(
                client_config(
                    pki::verifier(&ca, freg).client(pki::node(2)),
                    founder.der.into(),
                    PrivatePkcs8KeyDer::from(fkey.serialize_der()).into(),
                    time.clone(),
                )
                .unwrap(),
            );
            let js = Arc::new(
                server_config(
                    pki::verifier(&ca, jreg.clone()).server(),
                    joiner.der.clone().into(),
                    PrivatePkcs8KeyDer::from(jkey.serialize_der()).into(),
                    time.clone(),
                )
                .unwrap(),
            );
            let jc = Arc::new(
                client_config(
                    pki::verifier(&ca, jreg).client(pki::node(1)),
                    joiner.der.into(),
                    PrivatePkcs8KeyDer::from(jkey.serialize_der()).into(),
                    time,
                )
                .unwrap(),
            );
            for (client, server) in [(fc.clone(), js.clone()), (jc.clone(), fs.clone())] {
                assert_eq!(
                    pki::refusal(
                        pki::pump(
                            ClientConnection::new(
                                client,
                                ServerName::try_from(SERVER_NAME).unwrap()
                            )
                            .unwrap(),
                            ServerConnection::new(server).unwrap(),
                        )
                        .err()
                        .unwrap()
                    ),
                    VerifyRefusal::NotMember
                );
            }
            (fc, fs, jc, js, fd, jd)
        })
        .await
        .unwrap();
        assert_eq!(
            n1.fresh_read(move |s| Ok(s.state().authorize_peer([1; 16], pki::node(2), jd)))
                .await
                .unwrap()
                .1,
            Ok(())
        );
        let l = n1
            .raft
            .as_ref()
            .unwrap()
            .add_learner(2, BasicNode::new("mem:2"), false)
            .await
            .unwrap()
            .log_id
            .index;
        {
            let m = n1.raft.as_ref().unwrap().metrics().borrow_watched().clone();
            assert_eq!(
                m.membership_config
                    .membership()
                    .voter_ids()
                    .collect::<BTreeSet<_>>(),
                BTreeSet::from([1])
            );
            assert_eq!(
                m.membership_config
                    .membership()
                    .non_write_voter_ids()
                    .collect::<BTreeSet<_>>(),
                BTreeSet::from([2])
            );
        }
        n2.raft
            .as_ref()
            .unwrap()
            .wait(Some(deadline.saturating_duration_since(Instant::now())))
            .applied_index_at_least(Some(l), "learner replay")
            .await
            .unwrap();
        let expected = n1.linearizable_state().await.unwrap();
        {
            let sm = n2.sm.as_ref().unwrap().0.lock().unwrap();
            assert_eq!((sm.applied(), sm.snapshot()), expected);
        }
        // Post-catch-up membership fence.
        for n in [&n1, &n2] {
            let m = n.raft.as_ref().unwrap().metrics().borrow_watched().clone();
            assert_eq!(
                m.membership_config
                    .membership()
                    .voter_ids()
                    .collect::<BTreeSet<_>>(),
                BTreeSet::from([1])
            );
            assert_eq!(
                m.membership_config
                    .membership()
                    .non_write_voter_ids()
                    .collect::<BTreeSet<_>>(),
                BTreeSet::from([2])
            );
            if m.id == 2 {
                assert_eq!(m.state, openraft::ServerState::Learner);
                assert_eq!(m.current_leader, Some(1));
            }
        }
        let retained = (fc.clone(), fs.clone(), jc.clone(), js.clone());
        tokio::task::spawn_blocking(move || {
            let (fc, fs, jc, js) = retained;
            for (client, server, expected) in [(fc, js, (2, 1)), (jc, fs, (1, 2))] {
                let (a, b) = pki::pump(
                    ClientConnection::new(client, ServerName::try_from(SERVER_NAME).unwrap())
                        .unwrap(),
                    ServerConnection::new(server).unwrap(),
                )
                .unwrap();
                assert_eq!(
                    (a.node(), b.node()),
                    (pki::node(expected.0), pki::node(expected.1))
                );
            }
        })
        .await
        .unwrap();
        router.isolated.lock().unwrap().insert(2);
        let (last, outcome) = n1
            .propose(Command::Mint {
                cluster: [1; 16],
                actor: [3; 16],
                request: [6; 16],
                code: code2.code_id(),
                verifier: code2.verifier(),
                proposed_at_ms: pki::NOW,
            })
            .await
            .unwrap();
        assert!(outcome.is_some());
        assert_eq!(
            n1.raft
                .as_ref()
                .unwrap()
                .metrics()
                .borrow_watched()
                .membership_config
                .membership()
                .voter_ids()
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([1])
        );
        router.isolated.lock().unwrap().remove(&2);
        n2.raft
            .as_ref()
            .unwrap()
            .wait(Some(deadline.saturating_duration_since(Instant::now())))
            .applied_index_at_least(Some(last.index), "learner resumes")
            .await
            .unwrap();
        assert_eq!(
            router.snapshots.load(std::sync::atomic::Ordering::Relaxed),
            0
        );
        let durable = {
            let sm = n2.sm.as_ref().unwrap().0.lock().unwrap();
            (sm.applied(), sm.snapshot())
        };
        let weak1 = (n1.log_weak.clone(), n1.sm_weak.clone());
        let weak2 = (n2.log_weak.clone(), n2.sm_weak.clone());
        router.isolated.lock().unwrap().insert(2);
        router.nodes.lock().unwrap().clear();
        n2.shutdown().await.unwrap();
        assert!(weak2.0.upgrade().is_none());
        assert!(weak2.1.upgrade().is_none());
        let mut reopened = ConsensusNode::start(
            2,
            jdir.path(),
            founder_init([1; 16]),
            Params::default(),
            Factory {
                source: 2,
                router: router.clone(),
            },
        )
        .await
        .unwrap();
        {
            let sm = reopened.sm.as_ref().unwrap().0.lock().unwrap();
            assert_eq!((sm.applied(), sm.snapshot()), durable);
        }
        let reg = reopened.peer_registry();
        tokio::task::spawn_blocking(move || {
            assert_eq!(reg([1; 16], pki::node(1), fd), Ok(()));
            assert_eq!(reg([1; 16], pki::node(2), jd), Ok(()));
            assert_eq!(
                reg([1; 16], pki::node(3), jd),
                Err(VerifyRefusal::NotMember)
            );
        })
        .await
        .unwrap();
        let weak_reopened = (reopened.log_weak.clone(), reopened.sm_weak.clone());
        reopened.shutdown().await.unwrap();
        n1.shutdown().await.unwrap();
        assert!(weak1.0.upgrade().is_none());
        assert!(weak1.1.upgrade().is_none());
        assert!(weak_reopened.0.upgrade().is_none());
        assert!(weak_reopened.1.upgrade().is_none());
        drop((fc, fs, jc, js));
    })
    .await
    .unwrap();
}

fn observed_anchor(n: &ConsensusNode) -> crate::cluster::enrollment::JoinAnchor {
    let store = n.sm.as_ref().unwrap().0.lock().unwrap();
    let membership =
        crate::cluster::membership::Membership::decode(store.membership().unwrap().1.bytes())
            .unwrap();
    store.state().voter_anchor(&membership).unwrap()
}
