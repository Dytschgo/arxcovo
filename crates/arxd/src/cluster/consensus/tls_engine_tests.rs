use super::super::three_node::{Cluster, code, mint};
use super::*;
use crate::cluster::pki::{self, verify::*};
use openraft::type_config::async_runtime::watch::WatchReceiver;
use rcgen::{CertificateParams, KeyPair};
use rustls::pki_types::CertificateDer;
use rustls::server::danger::ClientCertVerifier;
use tokio::time::timeout;
struct Clock;
impl SignerClock for Clock {
    fn now_ms(&self) -> u64 {
        pki::NOW
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
        let dir = tempfile::tempdir().unwrap();
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
