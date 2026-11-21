//! Private enrollment ingress for the eligible retained issuer.
use super::*;
use crate::cluster::{
    enrollment::{
        BoundVoters, ClusterId, KeyDigest,
        session::{Backend, Close, ListenerOwner, Query, Redeem, Reply, SOCKET_BUDGET},
    },
    pki::{ca::ClusterCa, preproof::Refused},
};
use crate::service::cluster_owner::TIME_EVIDENCE_LEN;
use anchor_query::AnchorQueryError;
use checked_clock::{CheckedClock, ClockError, HostClock};
use issue::found::FOUNDER;
use issuer_transfer::Voter;
use openraft::type_config::async_runtime::watch::WatchReceiver;
use peer_transport::Credentials;
use sha2::{Digest, Sha256};
use std::{
    net::SocketAddr,
    os::fd::{AsRawFd, RawFd},
    sync::{Mutex, OnceLock},
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
    last_query: Mutex<Option<LastQuery>>,
}

impl ExchangeCounts {
    pub(super) fn observe(&self) -> [u8; 2] {
        use std::sync::atomic::Ordering::Relaxed;
        [self.anchors.load(Relaxed), self.redeems.load(Relaxed)]
    }

    fn count(counter: &std::sync::atomic::AtomicU8) -> u8 {
        use std::sync::atomic::Ordering::Relaxed;
        let previous = counter.fetch_update(Relaxed, Relaxed, |n| Some(n.saturating_add(1)));
        previous.unwrap_or_else(|n| n).saturating_add(1)
    }

    pub(super) fn absent_time_evidence() -> [u8; TIME_EVIDENCE_LEN] {
        let mut body = [0; TIME_EVIDENCE_LEN];
        body[0] = 1;
        body
    }

    fn time_evidence(
        &self,
        checkpoint: Option<Result<u64, ClockError>>,
    ) -> [u8; TIME_EVIDENCE_LEN] {
        let mut body = Self::absent_time_evidence();
        if let Ok(last) = self.last_query.try_lock()
            && let Some(last) = last
                .as_ref()
                .filter(|last| last.ordinal == self.observe()[0])
        {
            body[1] = last.ordinal;
            body[2] = match last.outcome {
                Ok(()) => 1,
                Err(AnchorQueryError::Refused) => 2,
                Err(AnchorQueryError::Busy) => 3,
                Err(AnchorQueryError::Unavailable) => 4,
            };
            body[3] = put_sample(last.sample, &mut body[5..13]);
            body[21..].copy_from_slice(&last.handle);
        }
        body[4] = put_sample(checkpoint, &mut body[13..21]);
        body
    }
}

fn put_sample(sample: Option<Result<u64, ClockError>>, bytes: &mut [u8]) -> u8 {
    match sample {
        None => 0,
        Some(Ok(ms)) => {
            bytes.copy_from_slice(&ms.to_be_bytes());
            1
        }
        Some(Err(_)) => 2,
    }
}

struct LastQuery {
    ordinal: u8,
    handle: [u8; 32],
    sample: Option<Result<u64, ClockError>>,
    outcome: Result<(), AnchorQueryError>,
}

struct QueryClock<'a> {
    source: &'a dyn CheckedClock,
    sample: OnceLock<Result<u64, ClockError>>,
}

impl CheckedClock for QueryClock<'_> {
    fn sample_ms(&self) -> Result<u64, ClockError> {
        let result = self.source.sample_ms();
        let _ = self.sample.set(result);
        result
    }
}

impl Job {
    pub(super) fn time_evidence(&self) -> [u8; TIME_EVIDENCE_LEN] {
        let Ok(state) = self.state.try_lock() else {
            return ExchangeCounts::absent_time_evidence();
        };
        let exchange = state.exchange.clone();
        let issuer = state.issuer.clone();
        drop(state);
        // A checkpoint uses the retained clock without taking the node's
        // admission lock. The campaign serializes it with client exchanges.
        let sample = issuer.map(|issuer| issuer.clock.sample_ms());
        exchange.time_evidence(sample)
    }
}

fn query_handle(query: &[u8; 32]) -> [u8; 32] {
    Sha256::new()
        .chain_update(b"arxcovo-d5-query-v1\0")
        .chain_update(query)
        .finalize()
        .into()
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
        let ordinal = self
            .exchange
            .as_ref()
            .map(|exchange| ExchangeCounts::count(&exchange.anchors));
        #[cfg(test)]
        if let Some(observation) = &self.observation {
            observation
                .anchor_calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        let body = query.body();
        let result = if let Some((exchange, ordinal)) = self.exchange.as_ref().zip(ordinal) {
            let clock = QueryClock {
                source: self.ready.clock.as_ref(),
                sample: OnceLock::new(),
            };
            let result = node.anchor_query(&body, &clock).await;
            if ordinal < u8::MAX
                && let Ok(mut slot) = exchange.last_query.try_lock()
            {
                *slot = Some(LastQuery {
                    ordinal,
                    handle: query_handle(&body),
                    sample: clock.sample.into_inner(),
                    outcome: result.as_ref().map(|_| ()).map_err(|error| *error),
                });
            }
            result
        } else {
            node.anchor_query(&body, self.ready.clock.as_ref()).await
        };
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
            observation.record_intent(input.request, &csr);
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
    use futures_util::FutureExt;
    use std::{
        fs,
        net::{TcpListener as StdListener, UdpSocket},
        os::unix::fs::PermissionsExt,
        panic::AssertUnwindSafe,
        sync::atomic::{AtomicUsize, Ordering::Relaxed},
    };
    use tokio::time::timeout;
    use x509_parser::{extensions::GeneralName, prelude::*};

    mod admission_campaign;
    mod catch_up_campaign;
    mod clock_reuse_campaign;
    #[cfg(test)]
    mod durable_join_campaign;
    mod issuer_transfer_campaign;
    mod lost_reply_campaign;
    mod promotion_campaign;

    #[derive(Default)]
    pub(in crate::cluster::consensus::bound_start) struct Observation {
        pub(super) anchor_calls: AtomicUsize,
        pub(super) redeem_calls: AtomicUsize,
        #[cfg(test)]
        anchor_outcomes:
            Mutex<Vec<Result<Vec<u8>, crate::cluster::consensus::anchor_query::AnchorQueryError>>>,
        result: Mutex<Option<Result<(), issue::IssueError>>>,
        // Opt-in for C15 only: dispatched attempt ID and public CSR, never the
        // proof or code. Standing campaigns leave it absent.
        redeem_intents: Option<Mutex<Vec<RedeemIntent>>>,
        finished: Notify,
        release: Notify,
    }

    type RedeemIntent = (crate::cluster::enrollment::RequestId, Vec<u8>);
    // C15 dispatches three Redeems; a fourth slot exposes an unexpected extra.
    const REDEEM_INTENT_CAP: usize = 4;

    impl Observation {
        pub(super) fn recording_intents() -> Self {
            Self {
                redeem_intents: Some(Mutex::new(Vec::with_capacity(REDEEM_INTENT_CAP))),
                ..Self::default()
            }
        }

        pub(super) fn record_intent(
            &self,
            request: crate::cluster::enrollment::RequestId,
            csr: &[u8],
        ) {
            let Some(intents) = &self.redeem_intents else {
                return;
            };
            let mut intents = intents.lock().unwrap();
            if intents.len() < REDEEM_INTENT_CAP {
                intents.push((request, csr.to_vec()));
            }
        }

        pub(super) fn intent(&self, index: usize) -> Option<RedeemIntent> {
            self.redeem_intents
                .as_ref()?
                .lock()
                .unwrap()
                .get(index)
                .cloned()
        }

        pub(super) fn intent_count(&self) -> Option<usize> {
            Some(self.redeem_intents.as_ref()?.lock().unwrap().len())
        }
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

    async fn observe_retained_status(
        observer: &mut Observer,
        wanted: Status,
        job: &Job,
    ) -> Result<(), String> {
        timeout(Duration::from_secs(15), async {
            loop {
                let status = observer.status();
                if status == wanted {
                    return Ok(());
                }
                if !matches!(
                    status,
                    Status::Preparing
                        | Status::Prepared
                        | Status::EngineMayExist
                        | Status::Running
                        | Status::Founding
                        | Status::FounderReady
                ) {
                    return Err(format!(
                        "waiting for {wanted:?}, stopped at {status:?}: {:?}",
                        job.state().founder_failure
                    ));
                }
                observer.changed().await.ok_or("retained observer closed")?;
            }
        })
        .await
        .map_err(|_| format!("retained {wanted:?} observation elapsed"))?
    }

    async fn observe_enroll(observation: &Observation) -> Result<(), String> {
        timeout(Duration::from_secs(10), observation.finished.notified())
            .await
            .map_err(|_| "enrollment observation elapsed")?;
        match *observation.result.lock().unwrap() {
            Some(Ok(())) => Ok(()),
            Some(Err(reason)) => Err(format!("actual enroll refused: {reason:?}")),
            None => Err("enrollment completion missing".into()),
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    enum CloseObservation {
        Joined(Status),
        SupervisorFailed,
        TimedOut,
    }

    // A driver may already have requested close to assert the in-flight hold.
    // The failure stays owned here across the separate bounded close observation.
    async fn finish_attempt<E, O>(
        first: Result<(), E>,
        close: &mut Option<O>,
        release: impl FnOnce(),
        request: impl FnOnce() -> O,
        observe: impl AsyncFnOnce(&mut O) -> CloseObservation,
    ) -> (Result<(), E>, CloseObservation) {
        if first.is_err() {
            release();
        }
        let observed = observe(close.get_or_insert_with(request)).await;
        (first, observed)
    }

    fn observe_or_request_close(job: &Job, before_request: impl FnOnce()) -> Observer {
        let mut state = job.state();
        let already_owned = state.close_requested
            || state.close_notified
            || state.close_dispatched
            // The hosted founder publishes the refusal just before requesting close.
            || (state.status == Status::FounderRefused && !state.process_owned);
        if !already_owned {
            before_request();
            // Claim and signal the same slot as Job::request_close under its state mutex.
            Job::stop_admission(&mut state);
            state.close_notified = true;
            job.close.notify_one();
        }
        job.observe()
    }

    async fn observe_close(
        supervisor: tokio::task::JoinHandle<()>,
        observer: &Observer,
    ) -> CloseObservation {
        match timeout(Duration::from_secs(10), supervisor).await {
            Ok(Ok(())) => CloseObservation::Joined(observer.status()),
            Ok(Err(_)) => CloseObservation::SupervisorFailed,
            Err(_) => CloseObservation::TimedOut,
        }
    }

    enum FirstFailure {
        Event(String),
        Assertion(Box<dyn std::any::Any + Send>),
    }

    fn latch_attempt(
        case: &str,
        attempt: std::thread::Result<Result<(), String>>,
    ) -> Result<(), FirstFailure> {
        match attempt {
            Ok(Ok(())) => Ok(()),
            Ok(Err(reason)) => {
                eprintln!("{case} first failure: {reason}");
                Err(FirstFailure::Event(reason))
            }
            // The panic hook already reported the assertion.
            Err(panic) => Err(FirstFailure::Assertion(panic)),
        }
    }

    fn require_attempt(
        case: &str,
        first: Result<(), FirstFailure>,
        close: &CloseObservation,
        job: &Job,
    ) {
        eprintln!(
            "{case} owned close: {close:?}; shutdown: {:?}",
            job.state().close_failure
        );
        match first {
            Ok(()) => {}
            Err(FirstFailure::Event(reason)) => panic!("{case} first failure: {reason}"),
            Err(FirstFailure::Assertion(panic)) => std::panic::resume_unwind(panic),
        }
        assert_eq!(*close, CloseObservation::Joined(Status::Clean));
    }

    #[tokio::test]
    async fn first_found_refusal_survives_owned_close() {
        let job = Job::anchor();
        founder::linux::refuse(job, founder::Error::Preparation);
        assert!(job.state().close_requested);
        let first = observe_retained_status(&mut job.observe(), Status::FounderReady, job).await;
        let supervisor = tokio::spawn(async move {
            job.close.notified().await;
            assert!(job.begin_close());
            job.finish_close(None, true, false);
        });
        let released = std::cell::Cell::new(false);
        let (first, close) = finish_attempt(
            first,
            &mut None,
            || released.set(true),
            || observe_or_request_close(job, || panic!("founder already requested close")),
            async |observer| {
                assert!(released.get());
                observe_close(supervisor, observer).await
            },
        )
        .await;
        assert_eq!(
            first.unwrap_err(),
            "waiting for FounderReady, stopped at FounderRefused: Some(Preparation)"
        );
        assert_eq!(
            close,
            CloseObservation::Joined(Status::ClosedAfterFounderRefusal)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn startup_timeout_serializes_the_fixture_request_with_founder_refusal() {
        let job = Job::anchor();
        let first = observe_retained_status(&mut job.observe(), Status::FounderReady, job).await;
        let founder_entered_gap = std::cell::Cell::new(false);
        let fixture_requests = std::cell::Cell::new(0);
        let close_observations = std::cell::Cell::new(0);
        let (first, close) = finish_attempt(
            first,
            &mut None,
            || {},
            || {
                observe_or_request_close(job, || {
                    // Attempt the founder's first lock exactly in the old gap.
                    // If it can enter, execute the frozen publication and request there.
                    let access = job.state.try_lock();
                    let founder_can_enter = access.is_ok();
                    drop(access);
                    if founder_can_enter {
                        founder::linux::refuse(job, founder::Error::Preparation);
                        founder_entered_gap.set(true);
                    }
                    fixture_requests.set(fixture_requests.get() + 1);
                })
            },
            async |observer| {
                close_observations.set(close_observations.get() + 1);
                let gap_opened = founder_entered_gap.get();
                let supervisor = tokio::spawn(async move {
                    assert!(job.close.notified().now_or_never().is_some());
                    if !gap_opened {
                        // Release the founder after consuming the first permit so that
                        // an extra notification would be visible rather than coalesced.
                        founder::linux::refuse(job, founder::Error::Preparation);
                    }
                    assert!(job.close.notified().now_or_never().is_none());
                    assert!(job.begin_close());
                    job.finish_close(None, true, false);
                });
                observe_close(supervisor, observer).await
            },
        )
        .await;
        assert!(
            !founder_entered_gap.get(),
            "founder entered during the fixture claim"
        );
        assert_eq!(fixture_requests.get(), 1);
        assert_eq!(close_observations.get(), 1);
        assert_eq!(
            first.unwrap_err(),
            "retained FounderReady observation elapsed"
        );
        assert_eq!(
            close,
            CloseObservation::Joined(Status::ClosedAfterFounderRefusal)
        );
    }

    #[tokio::test]
    async fn first_enroll_refusal_survives_unconfirmed_owned_close() {
        let observation = Observation::default();
        observation
            .complete(Err(issue::IssueError::Unavailable))
            .await;
        let first = observe_enroll(&observation).await;
        let (first, close) = finish_attempt(
            first,
            &mut None,
            || observation.release.notify_one(),
            || Status::Unconfirmed,
            async |status| CloseObservation::Joined(*status),
        )
        .await;
        assert_eq!(first.unwrap_err(), "actual enroll refused: Unavailable");
        assert_eq!(close, CloseObservation::Joined(Status::Unconfirmed));
    }

    #[tokio::test(start_paused = true)]
    async fn elapsed_enroll_observation_releases_hold_without_requesting_close_twice() {
        let observation = Observation::default();
        let armed_hold = observation.release.notified();
        tokio::pin!(armed_hold);
        assert!(futures_util::poll!(&mut armed_hold).is_pending());
        let first = observe_enroll(&observation).await;
        let mut already_requested = Some(armed_hold);
        let (first, close) = finish_attempt(
            first,
            &mut already_requested,
            || observation.release.notify_one(),
            || panic!("close was already requested"),
            async |hold| {
                assert!(futures_util::poll!(hold).is_ready());
                CloseObservation::TimedOut
            },
        )
        .await;
        assert_eq!(first.unwrap_err(), "enrollment observation elapsed");
        assert_eq!(close, CloseObservation::TimedOut);
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
        let (job, mut observer, supervisor) = dispatch_founder(
            plan,
            intent,
            Params::default(),
            wrap,
            store,
            HostedFixture(()),
        )
        .expect("retained founder dispatch");
        let observation = Arc::new(Observation::default());
        let mut close = None;
        let first = AssertUnwindSafe(drive_p1(
            job,
            &mut observer,
            &supervisor,
            &mut close,
            &observation,
            enrollment_addr,
            cluster,
        ))
        .catch_unwind()
        .await;
        let (first, close) = finish_attempt(
            latch_attempt("P1", first),
            &mut close,
            || observation.release.notify_one(),
            || observe_or_request_close(job, || {}),
            async |observer| observe_close(supervisor, observer).await,
        )
        .await;
        if !matches!(
            close,
            CloseObservation::Joined(Status::Clean | Status::ClosedAfterFounderRefusal)
        ) {
            let _ = layout.root.keep();
        }
        require_attempt("P1", first, &close, job);
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

    async fn drive_p1(
        job: &Job,
        observer: &mut Observer,
        supervisor: &tokio::task::JoinHandle<()>,
        close: &mut Option<Observer>,
        observation: &Arc<Observation>,
        enrollment_addr: SocketAddr,
        cluster: ClusterId,
    ) -> Result<(), String> {
        assert!(job.start_enrollment(Some(observation.clone())).is_err());
        assert!(job.state().ingress.is_none());
        observe_retained_status(observer, Status::FounderReady, job).await?;

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
            biased;
            result = observe_enroll(observation) => result?,
            outcome = &mut client => {
                return Err(format!("exchange ended before dispatch witness: {:?}", outcome.err()));
            }
        }
        assert_eq!(observation.anchor_calls.load(Relaxed), 1);
        assert_eq!(observation.redeem_calls.load(Relaxed), 1);

        let observer = close.get_or_insert_with(|| observe_or_request_close(job, || {}));
        observe_retained_status(observer, Status::Closing, job).await?;
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
        let completed = client
            .await
            .map_err(|reason| format!("real TLS enrollment failed: {reason:?}"))?;
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

        Ok(())
    }
}

#[cfg(test)]
mod time_evidence_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

    struct Source(AtomicUsize);

    impl CheckedClock for Source {
        fn sample_ms(&self) -> Result<u64, ClockError> {
            match self.0.fetch_add(1, Relaxed) {
                0 => Ok(123),
                _ => Err(ClockError::Unavailable),
            }
        }
    }

    #[test]
    fn forwarding_preserves_each_result_and_captures_the_first() {
        let source = Source(AtomicUsize::new(0));
        let clock = QueryClock {
            source: &source,
            sample: OnceLock::new(),
        };
        assert_eq!(clock.sample_ms(), Ok(123));
        assert_eq!(clock.sample_ms(), Err(ClockError::Unavailable));
        assert_eq!(clock.sample.into_inner(), Some(Ok(123)));
        let clock = QueryClock {
            source: &source,
            sample: OnceLock::new(),
        };
        assert_eq!(clock.sample_ms(), Err(ClockError::Unavailable));
        assert_eq!(
            clock.sample.into_inner(),
            Some(Err(ClockError::Unavailable))
        );
        assert_eq!(source.0.load(Relaxed), 3);
    }

    #[test]
    fn time_evidence_rejects_busy_and_stale_slots() {
        let exchange = ExchangeCounts::default();
        let ordinal = ExchangeCounts::count(&exchange.anchors);
        let handle = query_handle(&[8; 32]);
        assert_ne!(handle, query_handle(&[9; 32]));
        {
            let mut slot = exchange.last_query.lock().unwrap();
            *slot = Some(LastQuery {
                ordinal,
                handle,
                sample: Some(Ok(900_001)),
                outcome: Err(AnchorQueryError::Refused),
            });
            assert_eq!(
                exchange.time_evidence(None),
                ExchangeCounts::absent_time_evidence()
            );
        }

        let body = exchange.time_evidence(Some(Ok(900_002)));
        assert_eq!(&body[..5], &[1, 1, 2, 1, 1]);
        assert_eq!(&body[5..13], &900_001_u64.to_be_bytes());
        assert_eq!(&body[13..21], &900_002_u64.to_be_bytes());
        assert_eq!(&body[21..], &handle);
        assert_eq!(ExchangeCounts::count(&exchange.anchors), 2);
        assert_eq!(
            exchange.time_evidence(None),
            ExchangeCounts::absent_time_evidence()
        );
    }
}
