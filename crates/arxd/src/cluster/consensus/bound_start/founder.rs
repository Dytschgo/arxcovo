//! One founder operation on the process-retained bound-start job.
#[cfg(target_os = "linux")]
use super::peer_transport;
use super::{checked_clock, issue};
use crate::cluster::enrollment::MAX_TIME_MS;
use checked_clock::{CheckedClock, ClockError};
use issue::SignerClock;
use std::sync::atomic::{AtomicU8, Ordering};

const SAMPLES: u8 = 4;
const SAMPLING: u8 = SAMPLES + 1;
const REFUSED: u8 = SAMPLES + 2;

struct ClockBridge<'a> {
    clock: &'a dyn CheckedClock,
    phase: AtomicU8,
}

impl<'a> ClockBridge<'a> {
    fn new(clock: &'a dyn CheckedClock) -> Self {
        Self {
            clock,
            phase: AtomicU8::new(0),
        }
    }

    fn seal<T>(&self, value: T) -> Result<T, ClockError> {
        if self.phase.load(Ordering::Acquire) == SAMPLES {
            Ok(value)
        } else {
            Err(ClockError::Unavailable)
        }
    }

    fn refuse(&self) -> u64 {
        self.phase.store(REFUSED, Ordering::Release);
        u64::MAX
    }
}

impl SignerClock for ClockBridge<'_> {
    fn now_ms(&self) -> u64 {
        let phase = self.phase.load(Ordering::Acquire);
        if phase >= SAMPLES
            || self
                .phase
                .compare_exchange(phase, SAMPLING, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return self.refuse();
        }
        match self.clock.sample_ms() {
            Ok(ms)
                if ms <= MAX_TIME_MS
                    && self
                        .phase
                        .compare_exchange(SAMPLING, phase + 1, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok() =>
            {
                ms
            }
            _ => self.refuse(),
        }
    }
}

#[cfg(target_os = "linux")]
pub(super) use linux::{Error, Operation};
#[cfg(target_os = "linux")]
pub(super) use linux::{close_transport, prepare, refuse};

#[cfg(target_os = "linux")]
pub(super) mod linux {
    use super::*;
    use crate::cluster::consensus::bound_start::*;
    use crate::cluster::{
        enrollment::{BoundVoters, ClusterId, KeyDigest, NodeId},
        issuance_publication,
        join_receipt::Origin,
        pki::{ca::ClusterCa, csr::ProvenCsr, custody::FounderCustody},
        restart_history::{self, Fresh, Slot},
        startup_plan::Mode,
    };
    use checked_clock::HostClock;
    use issue::found::{FOUNDER, FoundError};
    use peer_network::EngineDispatch;
    use peer_transport::{self, Credentials, TransportOwner};
    use rustls::{pki_types::UnixTime, time_provider::TimeProvider};
    use std::path::PathBuf;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(in crate::cluster::consensus) enum Error {
        Preparation,
        Address,
        Closing,
        Clock,
        Custody,
        Found(FoundError),
        Publication(issuance_publication::Error),
        Eligibility,
        Attach(peer_transport::Refusal),
    }

    pub(in super::super) struct Operation {
        wrap: PathBuf,
        store: PathBuf,
        custody: Option<FounderCustody>,
        slot: Option<Slot>,
        history: Option<Fresh>,
        transport: Arc<tokio::sync::Mutex<TransportOwner>>,
        #[cfg(test)]
        fixture: Option<HostedFixture>,
    }

    impl Operation {
        pub(in crate::cluster::consensus::bound_start) fn delegated_build(
            &self,
        ) -> Option<peer_transport::BuildPeer> {
            Some(self.transport.try_lock().ok()?.build_peer())
        }

        pub(in crate::cluster::consensus::bound_start) fn installation_id(
            &self,
        ) -> Option<[u8; 16]> {
            Some(self.custody.as_ref()?.installation())
        }
    }

    #[cfg(test)]
    impl Operation {
        pub(in crate::cluster::consensus::bound_start) fn transport_for_test(
            &self,
        ) -> &tokio::sync::Mutex<TransportOwner> {
            &self.transport
        }
    }

    pub(in crate::cluster::consensus::bound_start) struct Prepared {
        cluster: ClusterId,
        address: String,
        enrollment: std::net::SocketAddr,
        clock: Arc<HostClock>,
        ca: ClusterCa,
        csr: Vec<u8>,
        key: KeyDigest,
    }

    // Created only from the sealed result of the one call on this job.
    pub struct Issuance {
        expected: ExpectedIdentity,
        leaf: Vec<u8>,
    }

    impl restart_history::FounderProof for FounderCustody {
        type Publication = Issuance;
    }

    impl restart_history::PublishedFounder for Issuance {
        fn expected(&self) -> &ExpectedIdentity {
            &self.expected
        }
        fn leaf(&self) -> &[u8] {
            &self.leaf
        }
    }

    struct TlsClock(Arc<HostClock>);

    impl std::fmt::Debug for TlsClock {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("checked founder clock")
        }
    }

    impl TimeProvider for TlsClock {
        fn current_time(&self) -> Option<UnixTime> {
            self.0
                .sample_ms()
                .ok()
                .map(|ms| UnixTime::since_unix_epoch(Duration::from_millis(ms)))
        }
    }

    // Only the retained founder and enrollment hosted fixtures construct this.
    #[cfg(test)]
    pub(in crate::cluster::consensus::bound_start) struct HostedFixture(
        pub(in crate::cluster::consensus::bound_start) (),
    );

    pub(in crate::cluster::consensus::bound_start) fn dispatch_founder(
        plan: Plan,
        intent: StartIntent,
        params: Params,
        wrap: PathBuf,
        store: PathBuf,
        #[cfg(test)] fixture: HostedFixture,
    ) -> Result<(&'static Job, Observer, tokio::task::JoinHandle<()>), Error> {
        dispatch_with_slot(
            plan,
            intent,
            params,
            wrap,
            store,
            None,
            None,
            #[cfg(test)]
            Some(fixture),
        )
    }

    pub(in crate::cluster::consensus::bound_start) fn dispatch_history_founder(
        plan: Plan,
        intent: StartIntent,
        params: Params,
        wrap: PathBuf,
        store: PathBuf,
        slot: Slot,
    ) -> Result<(&'static Job, Observer, tokio::task::JoinHandle<()>), Error> {
        dispatch_history(plan, intent, params, wrap, store, slot, None)
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::cluster::consensus::bound_start) fn dispatch_history(
        plan: Plan,
        intent: StartIntent,
        params: Params,
        wrap: PathBuf,
        store: PathBuf,
        slot: Slot,
        fence: Option<&super::super::runtime::Admission>,
    ) -> Result<(&'static Job, Observer, tokio::task::JoinHandle<()>), Error> {
        dispatch_with_slot(
            plan.with_history_intent(Mode::CreateFounder),
            intent,
            params,
            wrap,
            store,
            Some(slot),
            fence,
            #[cfg(test)]
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn dispatch_with_slot(
        plan: Plan,
        intent: StartIntent,
        params: Params,
        wrap: PathBuf,
        store: PathBuf,
        slot: Option<Slot>,
        fence: Option<&super::super::runtime::Admission>,
        #[cfg(test)] fixture: Option<HostedFixture>,
    ) -> Result<(&'static Job, Observer, tokio::task::JoinHandle<()>), Error> {
        if plan.mode != Mode::CreateFounder
            || intent.mode != Mode::CreateFounder
            || intent.node != FOUNDER
        {
            return Err(Error::Preparation);
        }
        let job = Job::anchor();
        let (transport, factory) = TransportOwner::pending_founder(intent.cluster, FOUNDER);
        job.state().founder = Some(Operation {
            wrap,
            store,
            custody: None,
            slot,
            history: None,
            transport: Arc::new(tokio::sync::Mutex::new(transport)),
            #[cfg(test)]
            fixture,
        });
        if let Some(fence) = fence {
            fence.register(job);
        }
        Ok(dispatch_on(
            job,
            plan,
            intent,
            None,
            params,
            factory,
            #[cfg(test)]
            None,
        ))
    }

    pub(in crate::cluster::consensus::bound_start) fn dispatch_recovered(
        ready: startup_recovery::HistoryReady,
        params: Params,
        fence: &super::super::runtime::Admission,
    ) -> (&'static Job, Observer, Option<tokio::task::JoinHandle<()>>) {
        let job = Job::anchor();
        fence.register(job);
        let observer = job.observe();
        match recovered_issuer(&ready) {
            Ok((intent, issuer)) => {
                let (transport, factory) = TransportOwner::pending_founder(intent.cluster, FOUNDER);
                let startup_recovery::HistoryReady {
                    owner,
                    history,
                    stores,
                    clock: _,
                } = ready;
                let mut state = job.state();
                state.owner = Some(owner);
                state.stores = Some(stores);
                state.founder_recovered = Some(history);
                state.founder_recovered_issuer = Some(issuer);
                state.founder = Some(Operation {
                    wrap: PathBuf::new(),
                    store: PathBuf::new(),
                    custody: None,
                    slot: None,
                    history: None,
                    transport: Arc::new(tokio::sync::Mutex::new(transport)),
                    #[cfg(test)]
                    fixture: None,
                });
                drop(state);
                job.set(Status::Prepared);
                let (_, _, supervisor) = drive(
                    job,
                    None,
                    intent,
                    None,
                    params,
                    factory.clone(),
                    Some(factory),
                    #[cfg(test)]
                    None,
                );
                (job, observer, Some(supervisor))
            }
            Err(reason) => {
                let mut state = job.state();
                state.recovery_hold =
                    Some(super::super::runtime::RecoveryHold::Ready(Box::new(ready)));
                state.founder_failure = Some(reason);
                drop(state);
                job.set(Status::Unconfirmed);
                (job, observer, None)
            }
        }
    }

    fn recovered_issuer(
        ready: &startup_recovery::HistoryReady,
    ) -> Result<(StartIntent, enrollment_ingress::ReadyIssuer), Error> {
        let owner = &ready.owner;
        if owner.plan.mode != Mode::RecoverFounder {
            return Err(Error::Preparation);
        }
        let evidence = ready.history.verified().map_err(|_| Error::Preparation)?;
        let expected = &evidence.expected;
        if expected.node != FOUNDER || !expected.origin.founder() {
            return Err(Error::Eligibility);
        }
        let custody = ready.history.founder().ok_or(Error::Custody)?;
        let ca = custody
            .load_recovered_ca(&ready.history, owner)
            .map_err(|_| Error::Custody)?;
        let now = ready.clock.sample_ms().map_err(|_| Error::Clock)?;
        let root = owner.read("ca.der", 4096).map_err(|_| Error::Custody)?;
        let public = crate::cluster::join_receipt::PublicIdentity::decode(
            &owner
                .read("identity.json", 8192)
                .map_err(|_| Error::Custody)?,
        )
        .map_err(|_| Error::Custody)?;
        let key = custody
            .member()
            .map_err(|_| Error::Custody)?
            .tls_key(expected, &root, public.leaf(), now)
            .map_err(|_| Error::Custody)?;
        let intent = StartIntent {
            mode: owner.plan.mode,
            cluster: expected.cluster,
            node: FOUNDER,
            peer: peer_transport::endpoint(&owner.plan.peer).map_err(|_| Error::Address)?,
            enrollment: peer_transport::endpoint(&owner.plan.enrollment)
                .map_err(|_| Error::Address)?,
        };
        let issuer = enrollment_ingress::ReadyIssuer::new(
            intent.enrollment,
            expected.cluster,
            ca,
            ready.clock.clone(),
            expected.key,
            Credentials {
                root: root.into(),
                leaf: public.leaf().to_vec().into(),
                key,
                time: Arc::new(TlsClock(ready.clock.clone())),
            },
        );
        Ok((intent, issuer))
    }

    pub(in super::super) fn has_recovered_issuer(job: &Job) -> bool {
        job.state().founder_recovered.is_some()
    }

    pub(in super::super) async fn attach_recovered(
        job: &'static Job,
        address: std::net::SocketAddr,
    ) -> Result<(), Error> {
        let (node, transport) = {
            let state = job.state();
            (
                state.node.clone().ok_or(Error::Preparation)?,
                state
                    .founder
                    .as_ref()
                    .ok_or(Error::Preparation)?
                    .transport
                    .clone(),
            )
        };
        let node = node.lock().await;
        let mut transport = transport.lock().await;
        let mut state = job.state();
        if state.close_requested || state.status != Status::Running {
            return Err(Error::Closing);
        }
        state.status = Status::Founding;
        job.changed.send_replace(Status::Founding);
        let issuer = state
            .founder_recovered_issuer
            .take()
            .ok_or(Error::Preparation)?;
        let listener = state.inherited.as_mut().and_then(|pair| pair.peer.take());
        transport
            .attach_founder_on(
                node.sm.as_ref().ok_or(Error::Eligibility)?,
                Arc::new(EngineDispatch {
                    raft: node.raft.clone().ok_or(Error::Eligibility)?,
                }),
                &address.to_string(),
                issuer.peer_credentials(),
                listener,
            )
            .map_err(Error::Attach)?;
        state.issuer = Some(Arc::new(issuer));
        Ok(())
    }

    #[cfg(test)]
    pub(in super::super) fn hosted_fixture(job: &Job) -> bool {
        job.state()
            .founder
            .as_ref()
            .is_some_and(|op| op.fixture.is_some())
    }

    pub(in super::super) fn refuse(job: &Job, reason: Error) {
        let process_owned = {
            let mut state = job.state();
            state.founder_failure.get_or_insert(reason);
            state.process_owned
        };
        job.set(Status::FounderRefused);
        if !process_owned {
            drop(job.request_close());
        }
    }

    pub(in super::super) fn prepare(job: &Job, intent: &StartIntent) -> Option<Prepared> {
        let mut state = job.state();
        state.founder.as_ref()?;
        let result = prepare_retained(&mut state, intent);
        drop(state);
        match result {
            Ok(prepared) => Some(prepared),
            Err(reason) => {
                refuse(job, reason);
                None
            }
        }
    }

    fn prepare_retained(state: &mut State, intent: &StartIntent) -> Result<Prepared, Error> {
        if state.close_requested {
            return Err(Error::Closing);
        }
        if state.status != Status::Prepared
            || intent.mode != Mode::CreateFounder
            || intent.node != FOUNDER
            || !matches!(
                state.observation,
                Some(Observation::UntouchedGenesis { cluster }) if cluster == intent.cluster
            )
        {
            return Err(Error::Preparation);
        }
        let owner = state.owner.as_mut().ok_or(Error::Preparation)?;
        let address = intent.peer.to_string();
        peer_transport::endpoint(&address).map_err(|_| Error::Address)?;

        let operation = state.founder.as_mut().ok_or(Error::Preparation)?;
        if let Some(slot) = operation.slot.take() {
            operation.history = Some(
                slot.check_fresh(owner, &operation.wrap, &operation.store)
                    .map_err(|_| Error::Custody)?,
            );
        }

        // The offline journal/inventory check still applies here. Engine has not
        // received the retained stores. Only proved untouched genesis gets floor zero.
        issuance_publication::reserve(owner).map_err(Error::Publication)?;
        let clock = Arc::new(HostClock::new(0));
        let now = clock.sample_ms().map_err(|_| Error::Clock)?;
        operation.custody = Some(
            FounderCustody::create(owner, &operation.wrap, &operation.store)
                .map_err(|_| Error::Custody)?,
        );
        let (ca, csr) = operation
            .custody
            .as_mut()
            .unwrap()
            .seal(intent.cluster, now)
            .map_err(|_| Error::Custody)?;
        if let Some(history) = operation.history.as_mut() {
            history
                .bind(operation.custody.as_ref().unwrap().history_auth())
                .map_err(|_| Error::Custody)?;
        }
        let key = ProvenCsr::parse(&csr).map_err(|_| Error::Custody)?.digest();
        Ok(Prepared {
            cluster: intent.cluster,
            address,
            enrollment: intent.enrollment,
            clock,
            ca,
            csr,
            key,
        })
    }

    impl Prepared {
        pub(in super::super) async fn run(self, job: &'static Job) -> Result<(), Error> {
            let (node, transport) = {
                let state = job.state();
                (
                    state.node.clone().ok_or(Error::Preparation)?,
                    state
                        .founder
                        .as_ref()
                        .ok_or(Error::Preparation)?
                        .transport
                        .clone(),
                )
            };
            let node = node.lock().await;
            // The driver owns this work until it returns. Close can be requested
            // after this gate, but the driver cannot close concurrently with Found.
            {
                let mut state = job.state();
                if state.close_requested || state.status != Status::Running {
                    return Err(Error::Closing);
                }
                state.status = Status::Founding;
                job.changed.send_replace(Status::Founding);
            }
            let bridge = ClockBridge::new(self.clock.as_ref());
            let call = node.found(
                &self.ca,
                &bridge,
                BasicNode::new(self.address.as_str()),
                &self.csr,
            );
            #[cfg(test)]
            let result = {
                use issue::found::observation::{self, Call, Cleanup};
                let (result, trace) = observation::capture_call(Call::ProofInitial, call).await;
                if result.is_err() {
                    observation::emit(&trace, Cleanup::unknown());
                }
                result
            };
            #[cfg(not(test))]
            let result = call.await;
            let issued = result.map_err(Error::Found)?;
            let issued = bridge.seal(issued).map_err(|_| Error::Clock)?;
            let e = issued.identity_commit;
            if issued.node != FOUNDER || e.leader != FOUNDER.get() {
                return Err(Error::Eligibility);
            }
            let issuance = Issuance {
                expected: ExpectedIdentity {
                    cluster: self.cluster,
                    node: issued.node,
                    key: self.key,
                    ca: self.ca.fingerprint(),
                    origin: Origin::Found {
                        term: e.term,
                        leader: e.leader,
                        index: e.index,
                    },
                },
                leaf: issued.der,
            };
            let root = self.ca.cert_der().to_vec();
            let clock = self.clock.clone();
            let credentials =
                tokio::task::spawn_blocking(move || publish(job, issuance, root, clock))
                    .await
                    .map_err(|_| Error::Preparation)??;
            let cluster = self.cluster;
            let key = self.key;
            let address = self.address.clone();
            let (leader, eligible) = node
                .fresh_read(move |store| own_eligible(store, cluster, key, &address, e))
                .await
                .map_err(|_| Error::Eligibility)?;
            if leader.node_id != FOUNDER.get() || leader.term < e.term || !eligible {
                return Err(Error::Eligibility);
            }
            let mut transport = transport.lock().await;
            let mut state = job.state();
            if state.close_requested || state.status != Status::Founding {
                return Err(Error::Closing);
            }
            let sm = node.sm.as_ref().ok_or(Error::Eligibility)?;
            let raft = node.raft.clone().ok_or(Error::Eligibility)?;
            let enrollment = Credentials {
                root: credentials.root.clone(),
                leaf: credentials.leaf.clone(),
                key: credentials.key.clone_key(),
                time: credentials.time.clone(),
            };
            let listener = state.inherited.as_mut().and_then(|pair| pair.peer.take());
            transport
                .attach_founder_on(
                    sm,
                    Arc::new(EngineDispatch { raft }),
                    &self.address,
                    credentials,
                    listener,
                )
                .map_err(Error::Attach)?;
            state.issuer = Some(Arc::new(enrollment_ingress::ReadyIssuer::new(
                self.enrollment,
                self.cluster,
                self.ca,
                self.clock,
                self.key,
                enrollment,
            )));
            Ok(())
        }
    }

    fn publish(
        job: &Job,
        issuance: Issuance,
        root: Vec<u8>,
        clock: Arc<HostClock>,
    ) -> Result<Credentials, Error> {
        let mut state = job.state();
        if state.close_requested || state.status != Status::Founding || state.node.is_none() {
            return Err(Error::Closing);
        }
        let State { owner, founder, .. } = &mut *state;
        let owner = owner.as_mut().ok_or(Error::Preparation)?;
        let operation = founder.as_mut().ok_or(Error::Custody)?;
        let custody = operation.custody.as_ref().ok_or(Error::Custody)?;
        let member = custody.member().map_err(|_| Error::Custody)?;
        let now = clock.sample_ms().map_err(|_| Error::Clock)?;
        let expected = Some(&issuance.expected);
        let installation = custody.installation();
        issuance_publication::store(
            owner,
            expected,
            installation,
            &root,
            &issuance.leaf,
            &member,
            Some(now),
        )
        .map_err(Error::Publication)?;
        let now = clock.sample_ms().map_err(|_| Error::Clock)?;
        issuance_publication::check_retained(owner, expected, installation, &member, Some(now))
            .map_err(Error::Publication)?;
        let key = member
            .tls_key(&issuance.expected, &root, &issuance.leaf, now)
            .map_err(|_| Error::Custody)?;
        if let Some(history) = &mut operation.history {
            history
                .founder(owner, custody, &issuance)
                .map_err(|_| Error::Publication(issuance_publication::Error::Unconfirmed))?;
        }
        Ok(Credentials {
            root: root.into(),
            leaf: issuance.leaf.into(),
            key,
            time: Arc::new(TlsClock(clock)),
        })
    }

    fn own_eligible(
        store: &StateStore,
        cluster: ClusterId,
        key: KeyDigest,
        address: &str,
        e: LogPos,
    ) -> Result<bool, StoreError> {
        let Some(applied) = store.applied() else {
            return Ok(false);
        };
        if !(applied.index > e.index || applied == e)
            || !store.state().founded()
            || store.state().authorize_peer(cluster, FOUNDER, key).is_err()
        {
            return Ok(false);
        }
        let Some((_, membership)) = store.membership() else {
            return Ok(false);
        };
        let membership =
            WireMembership::decode(membership.bytes()).map_err(|_| StoreError::Membership)?;
        Ok(BoundVoters::observe(store.state(), &membership)
            .is_ok_and(|voters| voters.contains(FOUNDER, key))
            && membership
                .nodes()
                .get(&FOUNDER.get())
                .is_some_and(|node| node == address))
    }

    async fn retained_issuer(
        job: &Job,
        target: NodeId,
    ) -> Result<
        (
            crate::cluster::consensus::issuer_transfer::Offer,
            Arc<tokio::sync::Mutex<ConsensusNode>>,
        ),
        peer_transport::Refusal,
    > {
        use peer_transport::Refusal as R;
        let (node, cluster, root, key) = {
            let state = job.state();
            if state.close_requested || state.status != Status::FounderReady {
                return Err(R::Closed);
            }
            let ready = state.issuer.as_ref().ok_or(R::NotReady)?;
            let (cluster, root, key) = ready.transfer_identity();
            (state.node.clone().ok_or(R::NotReady)?, cluster, root, key)
        };
        let offer = {
            let node = node.try_lock().map_err(|_| R::Busy)?;
            node.issuer_offer(cluster, root, target)
                .await
                .map_err(|error| {
                    #[cfg(test)]
                    eprintln!("issuer transfer fresh membership refused: {error:?}");
                    let _ = error;
                    R::Identity
                })?
        };
        if offer.claim.sender.1 != *key.as_bytes() {
            return Err(R::Identity);
        }
        Ok((offer, node))
    }

    pub(super) async fn voter_offer(
        job: &Job,
        target: NodeId,
    ) -> Result<
        (
            crate::cluster::consensus::issuer_transfer::Offer,
            Arc<tokio::sync::Mutex<TransportOwner>>,
        ),
        peer_transport::Refusal,
    > {
        use peer_transport::Refusal as R;
        let (offer, _) = retained_issuer(job, target).await?;
        let state = job.state();
        if state.close_requested {
            return Err(R::Closed);
        }
        let transport = state.founder.as_ref().ok_or(R::NotReady)?.transport.clone();
        Ok((offer, transport))
    }

    pub(super) async fn seal_voter_transfer(
        job: &Job,
        offer: crate::cluster::consensus::issuer_transfer::Offer,
        key: &[u8; 32],
        context: &[u8],
    ) -> Result<Vec<u8>, peer_transport::Refusal> {
        use peer_transport::Refusal as R;
        let (current, node) = retained_issuer(job, offer.claim.receiver.0).await?;
        if current != offer {
            return Err(R::Identity);
        }
        let node = node.try_lock().map_err(|_| R::Busy)?;
        let state = job.state();
        let ready = state.issuer.as_ref().ok_or(R::NotReady)?;
        if state.close_requested || ready.available(&node).is_err() {
            return Err(R::Identity);
        }
        let custody = state
            .founder
            .as_ref()
            .and_then(|op| op.custody.as_ref())
            .ok_or(R::NotReady)?;
        let (cluster, root, now) = ready.transfer_context()?;
        let sealed = custody
            .seal_for_voter(cluster, root, now, key, context)
            .map_err(|_| R::Identity)?;
        node.recheck_issuer_offer(offer).map_err(|_| R::Identity)?;
        Ok(sealed)
    }

    pub(in super::super) async fn close_transport(job: &Job) -> bool {
        let transport = job.state().founder.as_ref().map(|op| op.transport.clone());
        if let Some(transport) = transport {
            let mut transport = transport.lock().await;
            transport.close().await.is_ok() && transport.joined()
        } else {
            true
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        include!("founder_tests.rs");
    }
}

#[cfg(all(test, not(target_os = "linux")))]
#[path = "founder_tests.rs"]
mod tests;

#[cfg(target_os = "linux")]
impl super::Job {
    pub(super) async fn transfer_issuer_to(
        &'static self,
        target: crate::cluster::enrollment::NodeId,
    ) -> peer_transport::TransferOutcome {
        use peer_transport::TransferOutcome;
        let transfer = async {
            let (offer, transport) = linux::voter_offer(self, target).await?;
            let owner = transport.lock().await;
            owner.send_issuer_transfer(self, offer)
        }
        .await;
        match transfer {
            Ok(transfer) => transfer.outcome().await,
            Err(_) => TransferOutcome::Refused,
        }
    }

    pub(in crate::cluster::consensus) async fn seal_voter_transfer(
        &self,
        offer: super::issuer_transfer::Offer,
        key: &[u8; 32],
        context: &[u8],
    ) -> Result<Vec<u8>, peer_transport::Refusal> {
        linux::seal_voter_transfer(self, offer, key, context).await
    }
}
