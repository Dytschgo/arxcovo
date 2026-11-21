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
        transport: Arc<tokio::sync::Mutex<TransportOwner>>,
        #[cfg(test)]
        fixture: Option<HostedFixture>,
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
    struct Issuance {
        expected: ExpectedIdentity,
        leaf: Vec<u8>,
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
            transport: Arc::new(tokio::sync::Mutex::new(transport)),
            #[cfg(test)]
            fixture: Some(fixture),
        });
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

    #[cfg(test)]
    pub(in super::super) fn hosted_fixture(job: &Job) -> bool {
        job.state()
            .founder
            .as_ref()
            .is_some_and(|op| op.fixture.is_some())
    }

    pub(in super::super) fn refuse(job: &Job, reason: Error) {
        job.state().founder_failure.get_or_insert(reason);
        job.set(Status::FounderRefused);
        drop(job.request_close());
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

        // The offline journal/inventory check still applies here. Engine has not
        // received the retained stores. Only proved untouched genesis gets floor zero.
        issuance_publication::reserve(owner).map_err(Error::Publication)?;
        let clock = Arc::new(HostClock::new(0));
        let now = clock.sample_ms().map_err(|_| Error::Clock)?;
        let operation = state.founder.as_mut().ok_or(Error::Preparation)?;
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
            transport
                .attach_founder(
                    sm,
                    Arc::new(EngineDispatch { raft }),
                    &self.address,
                    credentials,
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
        let custody = founder
            .as_ref()
            .and_then(|op| op.custody.as_ref())
            .ok_or(Error::Custody)?;
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
