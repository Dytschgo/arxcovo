//! Private enrollment ingress for the published retained founder.
use super::*;
use crate::cluster::{
    enrollment::{
        BoundVoters, ClusterId, KeyDigest,
        session::{Backend, Close, ListenerOwner, Query, Redeem, Reply, SOCKET_BUDGET},
    },
    pki::{ca::ClusterCa, preproof::Refused},
};
use checked_clock::{CheckedClock, HostClock};
use issue::found::FOUNDER;
use openraft::type_config::async_runtime::watch::WatchReceiver;
use peer_transport::Credentials;
use std::net::SocketAddr;
use tokio::{net::TcpListener, sync::MutexGuard};

pub(super) struct ReadyIssuer {
    endpoint: SocketAddr,
    cluster: ClusterId,
    ca: ClusterCa,
    clock: Arc<HostClock>,
    key: KeyDigest,
    tls: Credentials,
}

impl ReadyIssuer {
    pub(super) fn new(
        endpoint: SocketAddr,
        cluster: ClusterId,
        ca: ClusterCa,
        clock: Arc<HostClock>,
        key: KeyDigest,
        tls: Credentials,
    ) -> Self {
        Self {
            endpoint,
            cluster,
            ca,
            clock,
            key,
            tls,
        }
    }

    // This is a local refusal check. Only the existing enroll guards establish
    // proposal and issuance authority.
    fn available(&self, node: &ConsensusNode) -> Result<(), Refused> {
        let raft = node.raft.as_ref().ok_or(Refused)?;
        let metrics = raft.metrics();
        let leader = {
            let m = metrics.borrow_watched();
            m.running_state.is_ok()
                && m.current_leader == Some(node.id)
                && m.state == openraft::ServerState::Leader
        };
        if !leader
            || !self
                .ca
                .leaf_fits(self.clock.sample_ms().map_err(|_| Refused)?)
        {
            return Err(Refused);
        }
        let sm = node.sm.as_ref().ok_or(Refused)?;
        let store = sm.0.try_lock().map_err(|_| Refused)?;
        store
            .state()
            .authorize_peer(self.cluster, FOUNDER, self.key)
            .map_err(|_| Refused)
    }
}

struct Issuer {
    ready: Arc<ReadyIssuer>,
    node: Arc<tokio::sync::Mutex<ConsensusNode>>,
    #[cfg(test)]
    observation: Option<Arc<hosted::Observation>>,
}

impl Issuer {
    fn admit(&self, deadline: Instant) -> Result<MutexGuard<'_, ConsensusNode>, Refused> {
        // No queue behind another dispatch or the retained node's owner.
        let node = self.node.try_lock().map_err(|_| Refused)?;
        self.ready.available(&node)?;
        if Instant::now() >= deadline {
            return Err(Refused);
        }
        Ok(node)
    }
}

impl Backend for Issuer {
    async fn anchor(&self, query: Query, deadline: Instant) -> Result<BoundVoters, Refused> {
        let node = self.admit(deadline)?;
        #[cfg(test)]
        if let Some(observation) = &self.observation {
            observation
                .anchor_calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        let result = node
            .anchor_query(&query.body(), self.ready.clock.as_ref())
            .await;
        #[cfg(test)]
        if let Err(error) = &result
            && self.observation.is_some()
        {
            eprintln!("P1 actual anchor_query refused: {error:?}");
        }
        #[cfg(test)]
        if let Some(observation) = &self.observation {
            observation.record_anchor(&result);
        }
        let bytes = result.map_err(|_| Refused)?;
        let voters = BoundVoters::decode(&bytes).map_err(|_| Refused)?;
        if !voters.contains(FOUNDER, self.ready.key) {
            return Err(Refused);
        }
        self.ready.available(&node)?;
        Ok(voters)
    }

    async fn redeem(&self, request: Redeem, deadline: Instant) -> Result<Reply, Refused> {
        let (input, csr) = request.into_issuer();
        let node = self.admit(deadline)?;
        #[cfg(test)]
        if let Some(observation) = &self.observation {
            observation
                .redeem_calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        // Join through the operation's own deadline. An outer socket timeout
        // cannot cancel admitted work. All failures have one wire status and
        // the client treats it as possible effect after submission.
        let result = node
            .enroll(&self.ready.ca, self.ready.clock.as_ref(), input, &csr)
            .await;
        drop(node);
        #[cfg(test)]
        if let Some(observation) = &self.observation {
            observation
                .complete(result.as_ref().map(|_| ()).map_err(|e| *e))
                .await;
        }
        result.map(Reply::issued).map_err(|_| Refused)
    }
}

impl Job {
    // An explicit private activation on the enrollment endpoint from the
    // retained startup intent. The job registers it under the close gate.
    pub(super) fn start_enrollment(
        &self,
        #[cfg(test)] observation: Option<Arc<hosted::Observation>>,
    ) -> Result<(), Refused> {
        let mut state = self.state();
        if state.status != Status::FounderReady || state.close_requested || state.ingress.is_some()
        {
            return Err(Refused);
        }
        let ready = state.issuer.as_ref().ok_or(Refused)?;
        let node = state.node.as_ref().ok_or(Refused)?;
        {
            let live = node.try_lock().map_err(|_| Refused)?;
            ready.available(&live)?;
        }
        let listener = std::net::TcpListener::bind(ready.endpoint).map_err(|_| Refused)?;
        listener.set_nonblocking(true).map_err(|_| Refused)?;
        let listener = TcpListener::from_std(listener).map_err(|_| Refused)?;
        let ingress = ListenerOwner::start(
            listener,
            ready.tls.leaf.clone(),
            ready.tls.key.clone_key(),
            ready.tls.time.clone(),
            Issuer {
                ready: ready.clone(),
                node: node.clone(),
                #[cfg(test)]
                observation,
            },
        )?;
        state.ingress = Some(ingress);
        Ok(())
    }
}

pub(super) async fn close(job: &Job) -> bool {
    let ingress = job.state().ingress.take();
    let Some(mut ingress) = ingress else {
        return true;
    };
    // The job has already latched stop. Keep a failed join and the original
    // node/custody anchored, including when the observation times out.
    if ingress.close(SOCKET_BUDGET).await == Close::Joined {
        true
    } else {
        job.state().ingress = Some(ingress);
        false
    }
}

#[cfg(all(test, target_os = "linux"))]
mod hosted {
    use super::*;
    use crate::cluster::{
        enrollment::{JoinCode, NodeId, Outcome, session::SocketLimits},
        pki::{csr::ProvenCsr, node_uri, preproof::exchange},
        startup_plan::tests::Layout,
    };
    use founder::linux::{HostedFixture, dispatch_founder};
    use std::{
        fs,
        net::{TcpListener as StdListener, UdpSocket},
        os::unix::fs::PermissionsExt,
        sync::atomic::{AtomicUsize, Ordering::Relaxed},
    };
    use tokio::time::timeout;
    use x509_parser::{extensions::GeneralName, prelude::*};

    mod admission_campaign;
    mod catch_up_campaign;
    #[cfg(test)]
    mod durable_join_campaign;

    #[derive(Default)]
    pub(in crate::cluster::consensus::bound_start) struct Observation {
        pub(super) anchor_calls: AtomicUsize,
        pub(super) redeem_calls: AtomicUsize,
        #[cfg(test)]
        anchor_outcomes:
            Mutex<Vec<Result<Vec<u8>, crate::cluster::consensus::anchor_query::AnchorQueryError>>>,
        result: Mutex<Option<Result<(), issue::IssueError>>>,
        finished: Notify,
        release: Notify,
    }

    impl Observation {
        #[cfg(test)]
        pub(super) fn record_anchor(
            &self,
            result: &Result<Vec<u8>, crate::cluster::consensus::anchor_query::AnchorQueryError>,
        ) {
            self.anchor_outcomes.lock().unwrap().push(result.clone());
        }
        pub(super) async fn complete(&self, result: Result<(), issue::IssueError>) {
            if let Err(error) = result {
                eprintln!("P1 actual enroll refused: {error:?}");
            }
            *self.result.lock().unwrap() = Some(result);
            self.finished.notify_one();
            if result.is_ok() {
                // Pause only after the frozen enroll call returned. The
                // ingress still owns the admitted dispatch and its reply.
                self.release.notified().await;
            }
        }
    }

    async fn wait_for(observer: &mut Observer, wanted: Status, job: &Job) {
        timeout(Duration::from_secs(15), async {
            while observer.status() != wanted {
                let status = observer.status();
                assert!(
                    matches!(
                        status,
                        Status::Preparing
                            | Status::Prepared
                            | Status::EngineMayExist
                            | Status::Running
                            | Status::Founding
                            | Status::FounderReady
                    ),
                    "waiting for {wanted:?}, stopped at {status:?}: {:?}",
                    job.state().founder_failure
                );
                observer.changed().await.expect("observer closed");
            }
        })
        .await
        .expect("retained issuer observation elapsed");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn retained_issuer_serves_actual_enrollment_and_joins_before_shutdown() {
        let layout = Layout::new();
        let mut plan = layout.plan("create-founder");
        let route = UdpSocket::bind("0.0.0.0:0").unwrap();
        route.connect("192.0.2.1:9").unwrap(); // No datagram is sent.
        let ip = route.local_addr().unwrap().ip();
        let peer = StdListener::bind(SocketAddr::new(ip, 0)).unwrap();
        let enrollment = StdListener::bind(SocketAddr::new(ip, 0)).unwrap();
        let peer_addr = peer.local_addr().unwrap();
        let enrollment_addr = enrollment.local_addr().unwrap();
        plan.peer = peer_addr.to_string();
        plan.enrollment = enrollment_addr.to_string();
        let cluster = [1; 16];
        let intent = StartIntent {
            mode: plan.mode,
            node: FOUNDER,
            cluster,
            peer: peer_addr,
            enrollment: enrollment_addr,
        };
        let [wrap, store] = ["wrap", "custody"].map(|name| {
            let path = layout.root.path().join(name);
            fs::create_dir(&path).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            path
        });
        drop(peer);
        drop(enrollment);
        let (job, mut observer, mut supervisor) = dispatch_founder(
            plan,
            intent,
            Params::default(),
            wrap,
            store,
            HostedFixture(()),
        )
        .expect("retained founder dispatch");
        let observation = Arc::new(Observation::default());
        assert!(job.start_enrollment(Some(observation.clone())).is_err());
        assert!(job.state().ingress.is_none());
        wait_for(&mut observer, Status::FounderReady, job).await;

        let (node, founder_key, root, clock, time) = {
            let state = job.state();
            let ready = state.issuer.as_ref().unwrap();
            (
                state.node.as_ref().unwrap().clone(),
                ready.key,
                state.owner.as_ref().unwrap().read("ca.der", 4096).unwrap(),
                ready.clock.clone(),
                ready.tls.time.clone(),
            )
        };
        let (voters, code, mint_e) = {
            let node = node.lock().await;
            let voters = {
                let store = node.sm.as_ref().unwrap().0.lock().unwrap();
                let membership =
                    WireMembership::decode(store.membership().unwrap().1.bytes()).unwrap();
                BoundVoters::observe(store.state(), &membership).unwrap()
            };
            assert!(voters.contains(FOUNDER, founder_key));
            let code = JoinCode::generate(
                cluster,
                crate::cluster::pki::ca::fingerprint_of(&root),
                voters.anchor(cluster),
                getrandom::fill,
            )
            .unwrap();
            // Fixture-only Mint, with a fresh in-memory code.
            let (e, minted) = node
                .propose(Command::Mint {
                    cluster,
                    actor: [3; 16],
                    request: [4; 16],
                    code: code.code_id(),
                    verifier: code.verifier(),
                    proposed_at_ms: clock.sample_ms().unwrap(),
                })
                .await
                .expect("P1 fixture Mint proposal refused");
            assert!(matches!(
                minted,
                Some(Outcome::Minted { replay: false, .. })
            ));
            (voters, code, e)
        };
        job.start_enrollment(Some(observation.clone()))
            .expect("ready enrollment listener");
        assert!(job.start_enrollment(None).is_err());
        assert_eq!(observation.anchor_calls.load(Relaxed), 0);
        assert_eq!(observation.redeem_calls.load(Relaxed), 0);
        let key = rcgen::KeyPair::generate().unwrap();
        let csr = rcgen::CertificateParams::default()
            .serialize_request(&key)
            .unwrap();
        let key = ProvenCsr::parse(csr.der()).unwrap().digest();
        let request = [5; 16];
        let mut client = Box::pin(exchange(
            enrollment_addr,
            &code,
            &root,
            time,
            request,
            csr.der(),
            SocketLimits::default(),
        ));
        tokio::select! {
            outcome = &mut client => panic!("P1 exchange ended before dispatch witness: {:?}", outcome.err()),
            _ = timeout(Duration::from_secs(10), observation.finished.notified()) => {
                assert_eq!(
                    *observation.result.lock().unwrap(),
                    Some(Ok(())),
                    "P1 actual enroll refused or did not complete"
                );
            }
        }
        assert_eq!(observation.anchor_calls.load(Relaxed), 1);
        assert_eq!(observation.redeem_calls.load(Relaxed), 1);

        let mut observer = job.request_close();
        wait_for(&mut observer, Status::Closing, job).await;
        assert!(job.start_enrollment(None).is_err());
        assert!(
            !supervisor.is_finished(),
            "close escaped an admitted dispatch"
        );
        assert!(node.try_lock().unwrap().raft.is_some());
        assert!(
            !job.state()
                .founder
                .as_ref()
                .unwrap()
                .transport_for_test()
                .try_lock()
                .unwrap()
                .joined()
        );

        let original_e = {
            let node = node.lock().await;
            let log = node.log_weak.upgrade().unwrap();
            let entries = log.lock().unwrap().entries(0, 16, 16).unwrap();
            assert_eq!(
                entries
                    .iter()
                    .filter(|(_, e)| matches!(e, StoredEntry::Enrollment(Command::Found { .. })))
                    .count(),
                1
            );
            assert_eq!(
                entries
                    .iter()
                    .filter(|(e, entry)| {
                        assert!(
                            !matches!(entry, StoredEntry::Enrollment(Command::Mint { .. }))
                                || *e == mint_e
                        );
                        matches!(entry, StoredEntry::Enrollment(Command::Mint { .. }))
                    })
                    .count(),
                1
            );
            let mut redeems = entries.into_iter().filter_map(|(e, entry)| match entry {
                StoredEntry::Enrollment(Command::Redeem {
                    cluster: c,
                    request: r,
                    code: id,
                    key: k,
                    anchor,
                    ..
                }) => {
                    assert_eq!(
                        (c, r, id, k, anchor),
                        (cluster, request, code.code_id(), key, code.anchor())
                    );
                    Some(e)
                }
                _ => None,
            });
            let e = redeems.next().expect("committed Redeem");
            assert!(redeems.next().is_none());
            assert!(e.index > mint_e.index);
            e
        };
        drop(node);
        observation.release.notify_one();
        let completed = client.await.expect("P1 real TLS enrollment failed");
        assert_eq!(completed.voters().encode(), voters.encode());
        let (learner, e, leaf) = completed.reply().parts();
        assert_eq!(learner, NodeId::new(2).unwrap());
        assert_eq!(e, original_e);
        let (rest, leaf) = X509Certificate::from_der(leaf).unwrap();
        assert!(rest.is_empty());
        let (_, ca) = X509Certificate::from_der(&root).unwrap();
        leaf.verify_signature(Some(ca.public_key())).unwrap();
        assert_eq!(KeyDigest::of_spki(leaf.public_key().raw).unwrap(), key);
        assert_eq!(
            leaf.subject_alternative_name()
                .unwrap()
                .unwrap()
                .value
                .general_names
                .as_slice(),
            [GeneralName::URI(&node_uri(cluster, learner))]
        );

        timeout(Duration::from_secs(10), &mut supervisor)
            .await
            .expect("owned close elapsed")
            .unwrap();
        assert_eq!(
            observer.status(),
            Status::Clean,
            "shutdown: {:?}",
            job.state().close_failure
        );
        {
            let state = job.state();
            assert!(state.ingress.is_none() && state.node.is_none() && state.owner.is_none());
        }
        assert_eq!(observation.anchor_calls.load(Relaxed), 1);
        assert_eq!(observation.redeem_calls.load(Relaxed), 1);
        for address in [enrollment_addr, peer_addr] {
            let _released = TcpListener::bind(address).await.expect("listener released");
        }
    }
}
