//! Private enrollment ingress for the eligible retained issuer.
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
use issuer_transfer::Voter;
use openraft::type_config::async_runtime::watch::WatchReceiver;
use peer_transport::Credentials;
use std::{
    net::SocketAddr,
    os::fd::{AsRawFd, RawFd},
};
use tokio::{net::TcpListener, sync::MutexGuard};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ActivationError {
    Deferred,
    Refused,
}

impl From<crate::cluster::consensus::ReadError> for ActivationError {
    fn from(error: crate::cluster::consensus::ReadError) -> Self {
        use crate::cluster::consensus::ReadError;
        match error {
            ReadError::Busy | ReadError::NotLeader | ReadError::NoQuorum => Self::Deferred,
            ReadError::Unavailable => Self::Refused,
        }
    }
}

impl From<membership_observation::MembershipReadError> for ActivationError {
    fn from(error: membership_observation::MembershipReadError) -> Self {
        use membership_observation::MembershipReadError;
        match error {
            MembershipReadError::Busy
            | MembershipReadError::NotLeader
            | MembershipReadError::RetryLater => Self::Deferred,
            MembershipReadError::Unavailable | MembershipReadError::PeerRefused => Self::Refused,
        }
    }
}

pub(super) struct ReadyIssuer {
    endpoint: SocketAddr,
    cluster: ClusterId,
    ca: ClusterCa,
    clock: Arc<HostClock>,
    key: KeyDigest,
    tls: Credentials,
    promoted: Option<(Voter, LogPos)>,
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
            promoted: None,
        }
    }

    pub(super) fn promoted(
        endpoint: SocketAddr,
        cluster: ClusterId,
        ca: ClusterCa,
        clock: Arc<HostClock>,
        voter: Voter,
        installed_at: LogPos,
        tls: Credentials,
    ) -> Self {
        Self {
            promoted: Some((voter, installed_at)),
            ..Self::new(endpoint, cluster, ca, clock, voter.key, tls)
        }
    }

    pub(super) fn member(&self) -> crate::cluster::enrollment::NodeId {
        self.promoted.map_or(FOUNDER, |(voter, _)| voter.node)
    }

    pub(super) fn peer_credentials(&self) -> Credentials {
        Credentials {
            root: self.tls.root.clone(),
            leaf: self.tls.leaf.clone(),
            key: self.tls.key.clone_key(),
            time: self.tls.time.clone(),
        }
    }

    pub(super) fn mint_root(&self) -> &[u8] {
        self.ca.cert_der()
    }

    pub(super) fn mint_clock(&self) -> Arc<HostClock> {
        self.clock.clone()
    }

    pub(super) fn transfer_identity(&self) -> (ClusterId, [u8; 32], KeyDigest) {
        (self.cluster, self.ca.fingerprint(), self.key)
    }

    pub(super) fn transfer_context(
        &self,
    ) -> Result<(ClusterId, &[u8], u64), peer_transport::Refusal> {
        Ok((
            self.cluster,
            self.ca.cert_der(),
            self.clock
                .sample_ms()
                .map_err(|_| peer_transport::Refusal::Identity)?,
        ))
    }

    // This is a local refusal check. Only the existing enroll guards establish
    // proposal and issuance authority.
    pub(super) fn available(&self, node: &ConsensusNode) -> Result<(), Refused> {
        self.activation_available(node).map_err(|_| Refused)
    }

    fn activation_available(&self, node: &ConsensusNode) -> Result<(), ActivationError> {
        use ActivationError::{Deferred, Refused};
        let raft = node.raft.as_ref().ok_or(Refused)?;
        let metrics = raft.metrics();
        let leader = {
            let m = metrics.borrow_watched();
            if m.running_state.is_err() {
                return Err(Refused);
            }
            m.current_leader == Some(node.id) && m.state == openraft::ServerState::Leader
        };
        if !self
            .ca
            .leaf_fits(self.clock.sample_ms().map_err(|_| Refused)?)
        {
            return Err(Refused);
        }
        if !leader {
            return Err(Deferred);
        }
        if let Some((voter, installed_at)) = self.promoted {
            return node
                .issuer_voter(self.cluster, voter, installed_at, true)
                .map_err(Into::into);
        }
        let sm = node.sm.as_ref().ok_or(Refused)?;
        let store = sm.0.try_lock().map_err(|error| match error {
            std::sync::TryLockError::WouldBlock => Deferred,
            std::sync::TryLockError::Poisoned(_) => Refused,
        })?;
        if store.is_poisoned() {
            return Err(Refused);
        }
        store
            .state()
            .authorize_peer(self.cluster, self.member(), self.key)
            .map_err(|_| Refused)
    }
}

#[derive(Default)]
pub(super) struct ExchangeCounts {
    anchors: std::sync::atomic::AtomicU8,
    redeems: std::sync::atomic::AtomicU8,
}

impl ExchangeCounts {
    pub(super) fn observe(&self) -> [u8; 2] {
        use std::sync::atomic::Ordering::Relaxed;
        [self.anchors.load(Relaxed), self.redeems.load(Relaxed)]
    }

    fn count(counter: &std::sync::atomic::AtomicU8) {
        use std::sync::atomic::Ordering::Relaxed;
        let _ = counter.fetch_update(Relaxed, Relaxed, |n| Some(n.saturating_add(1)));
    }
}

struct Issuer {
    ready: Arc<ReadyIssuer>,
    exchange: Option<Arc<ExchangeCounts>>,
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
        if let Some(exchange) = &self.exchange {
            ExchangeCounts::count(&exchange.anchors);
        }
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
        if !voters.contains(self.ready.member(), self.ready.key) {
            return Err(Refused);
        }
        self.ready.available(&node)?;
        Ok(voters)
    }

    async fn redeem(&self, request: Redeem, deadline: Instant) -> Result<Reply, Refused> {
        let (input, csr) = request.into_issuer();
        let node = self.admit(deadline)?;
        if let Some(exchange) = &self.exchange {
            ExchangeCounts::count(&exchange.redeems);
        }
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

/// Non-owning observation of the exact bound listener. Holding another socket
/// descriptor here would keep enrollment listening after its supervisor closed.
#[derive(Clone, Copy)]
pub(super) struct ServingListener {
    fd: RawFd,
    pub(super) cookie: u64,
    pub(super) endpoint: SocketAddr,
}

impl ServingListener {
    pub(super) fn capture(listener: &TcpListener, endpoint: SocketAddr) -> Option<Self> {
        if listener.local_addr().ok()? != endpoint || endpoint.port() == 0 {
            return None;
        }
        let fd = listener.as_raw_fd();
        let cookie = socket_option::<u64>(fd, libc::SO_COOKIE)?;
        Some(Self {
            fd,
            cookie,
            endpoint,
        })
    }

    pub(super) fn active(self) -> bool {
        socket_option::<u64>(self.fd, libc::SO_COOKIE) == Some(self.cookie)
            && socket_option::<libc::c_int>(self.fd, libc::SO_ACCEPTCONN) == Some(1)
            && socket_option::<u64>(self.fd, libc::SO_COOKIE) == Some(self.cookie)
    }
}

fn socket_option<T: Default>(fd: RawFd, option: libc::c_int) -> Option<T> {
    let mut value = T::default();
    let mut size = std::mem::size_of::<T>() as libc::socklen_t;
    // SAFETY: Linux writes at most size initialized bytes to value. The only
    // callers supply scalar types matching their respective kernel ABI.
    let status = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            option,
            std::ptr::from_mut(&mut value).cast(),
            &mut size,
        )
    };
    (status == 0 && size as usize == std::mem::size_of::<T>()).then_some(value)
}

impl Job {
    pub(super) fn issuer_serving(&self) -> bool {
        let state = self.state();
        if state.close_requested || state.ingress.is_none() {
            return false;
        }
        let (Some(listener), Some(issuer), Some(node)) =
            (state.serving_enrollment, &state.issuer, &state.node)
        else {
            return false;
        };
        listener.active()
            && node
                .try_lock()
                .is_ok_and(|node| issuer.available(&node).is_ok())
    }

    // An explicit private activation on the enrollment endpoint from the
    // retained startup intent. The job registers it under the close gate.
    pub(super) fn start_enrollment(
        &self,
        #[cfg(test)] observation: Option<Arc<hosted::Observation>>,
    ) -> Result<(), ActivationError> {
        use ActivationError::{Deferred, Refused};
        let mut state = self.state();
        if !matches!(state.status, Status::FounderReady | Status::LearnerReady)
            || state.close_requested
            || state.ingress.is_some()
        {
            return Err(Refused);
        }
        let ready = state.issuer.clone().ok_or(Refused)?;
        let node = state.node.clone().ok_or(Refused)?;
        {
            let live = node.try_lock().map_err(|_| Deferred)?;
            ready.activation_available(&live)?;
        }
        let inherited = state
            .inherited
            .as_mut()
            .and_then(|pair| pair.enrollment.take());
        let listener = match inherited {
            Some(listener) => {
                crate::service::cluster_runtime::listen_inherited(&listener)
                    .map_err(|_| Refused)?;
                listener
            }
            None => std::net::TcpListener::bind(ready.endpoint).map_err(|_| Refused)?,
        };
        if listener.local_addr().ok() != Some(ready.endpoint) {
            return Err(Refused);
        }
        listener.set_nonblocking(true).map_err(|_| Refused)?;
        let listener = TcpListener::from_std(listener).map_err(|_| Refused)?;
        let serving = ServingListener::capture(&listener, ready.endpoint);
        let ingress = ListenerOwner::start(
            listener,
            ready.tls.leaf.clone(),
            ready.tls.key.clone_key(),
            ready.tls.time.clone(),
            Issuer {
                ready,
                exchange: state.process_owned.then(|| state.exchange.clone()),
                node,
                #[cfg(test)]
                observation,
            },
        )
        .map_err(|_| Refused)?;
        state.ingress = Some(ingress);
        state.serving_enrollment = serving;
        Ok(())
    }
}

pub(super) async fn close(job: &Job) -> bool {
    let ingress = {
        let mut state = job.state();
        state.serving_enrollment = None;
        state.ingress.take()
    };
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
    mod issuer_transfer_campaign;
    mod promotion_campaign;

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
