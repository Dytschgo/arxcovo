//! Hosted all-OS proof; never run locally on the Windows development host.
use super::*;
use super::{
    issue::found::founder_init,
    issue::{RedeemInput, SignerClock},
    mem_net::{Factory, MemRouter},
};
use crate::{
    cluster::{
        enrollment::{BoundVoters, JoinCode},
        pki::{
            ca::ClusterCa,
            preproof::PreproofClient,
            preproof_tests::{ProbeTime, exchange, server},
        },
    },
    service::application::tests::private_temp,
};
use rcgen::{CertificateParams, KeyPair};
use std::sync::Arc;
use tokio::time::{Instant, timeout_at};
struct Clock;
impl SignerClock for Clock {
    fn now_ms(&self) -> u64 {
        1_000_000
    }
}
impl super::checked_clock::CheckedClock for Clock {
    fn sample_ms(&self) -> Result<u64, super::checked_clock::ClockError> {
        Ok(SignerClock::now_ms(self))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn anchored_preproof_single_voter() {
    let entry = Instant::now();
    let work_deadline = entry + Duration::from_secs(35);
    let cleanup_deadline = entry + Duration::from_secs(40);
    let dir = private_temp();
    let router = Arc::new(MemRouter::default());
    use issue::found::observation::{self, Call, Cleanup};
    let mut captured = None;
    let mut refusal = None;
    let mut node = None;
    let mut blocking = None;
    let mut tls_work = None;
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
        let (found, snapshot) = observation::capture_call(
            Call::ProofInitial,
            n.found(&ca, &Clock, BasicNode::new("mem:1"), csr.der()),
        )
        .await;
        captured = Some(snapshot);
        let founder = match found {
            Ok(leaf) => leaf,
            Err(error) => {
                refusal = Some(error);
                return;
            }
        };
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
        let tls_code = code.clone();
        tls_work = Some(tokio::task::spawn_blocking(move || {
            let mut client = PreproofClient::new(&tls_code, &root, Arc::new(ProbeTime)).unwrap();
            let mut endpoint = server(founder.der, &key);
            let proof = exchange(&mut client, &mut endpoint, &tls_code, &list).unwrap();
            assert_eq!(&proof, tls_code.proof().bytes());
        }));
        tls_work.as_mut().unwrap().await.unwrap();
        tls_work = None;
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
    if let Some(work) = tls_work.take() {
        timeout_at(cleanup_deadline, work)
            .await
            .expect("TLS cleanup unresolved")
            .unwrap();
    }
    let mut cleanup = Cleanup::unknown();
    if refusal.is_some() {
        cleanup.router = match router.nodes.lock() {
            Ok(mut nodes) => {
                nodes.clear();
                observation::Status::Ok
            }
            Err(_) => observation::Status::Failed,
        };
    } else {
        router.nodes.lock().unwrap().clear();
    }
    drop(router);
    let n = node
        .as_mut()
        .expect("startup unresolved; cleanup unknown: blocking startup cannot be aborted");
    if let Some(error) = refusal {
        let shutdown = timeout_at(cleanup_deadline, n.shutdown()).await;
        cleanup.shutdown = observation::shutdown_status(&shutdown);
        cleanup.store = observation::status(n.sm_weak.upgrade().is_none());
        cleanup.log = observation::status(n.log_weak.upgrade().is_none());
        observation::emit_proof_refusal(captured.as_ref().unwrap(), cleanup);
        panic!("original founder refusal: {error}");
    }
    timeout_at(cleanup_deadline, n.shutdown())
        .await
        .expect("cleanup deadline exceeded")
        .expect("clean shutdown required");
    assert!(n.sm_weak.upgrade().is_none());
    assert!(n.log_weak.upgrade().is_none());
    if let Some(snapshot) = captured.as_ref() {
        observation::emit(
            snapshot,
            Cleanup {
                router: observation::Status::Ok,
                shutdown: observation::Status::Ok,
                store: observation::Status::Ok,
                log: observation::Status::Ok,
            },
        );
    }
    result.expect("work exceeded cooperative deadline");
}
