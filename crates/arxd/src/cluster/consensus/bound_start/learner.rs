//! Retained published learner composition and same-process reopen.
use super::*;
use crate::cluster::{
    enrollment::durable_join::{HistoryJoin, PublishedJoin, RedeemIssuer, RetainedJoin},
    issuance_publication::check_retained,
    join_receipt::PublicIdentity,
    pki::custody::{LearnerCustody, LearnerReopen},
    restart_history::{self, Fresh, Live, VoterOffer},
    startup_plan::{Mode, ReopenObjects},
};
use checked_clock::{CheckedClock, HostClock};
use enrollment_ingress::ActivationError;
use membership_observation::{IssuerMembership, IssuerProof};
use peer_network::EngineDispatch;
use peer_registry::{LearnerGate, Phase};
use peer_transport::{AdmissionClient, Credentials, TransportOwner};
use rustls::{pki_types::UnixTime, time_provider::TimeProvider};
use std::net::SocketAddr;

pub(super) struct Operation {
    input: Option<Input>,
    transport: Arc<tokio::sync::Mutex<TransportOwner>>,
    member: Option<Member>,
    local_time: Option<Arc<dyn TimeProvider>>,
    credentials: Option<Credentials>,
    clock: Option<Arc<HostClock>>,
    gate: Option<Arc<LearnerGate>>,
    advertised: Option<SocketAddr>,
    admission: Option<AdmissionClient>,
    issuer_membership: Option<LogPos>,
    issuer_install: Arc<tokio::sync::Mutex<()>>,
    #[cfg(test)]
    issuer_sample: Option<membership_observation::Sample>,
    #[cfg(test)]
    issuer_before_proof: Option<membership_observation::BeforeProof>,
    recovered: bool,
    process_recovered: bool,
    redeem_issuer: Option<RedeemIssuer>,
    #[cfg(test)]
    fixture: Option<HostedCatchUp>,
}

enum Input {
    Published(Box<PublishedJoin>, Option<Box<(Fresh, Live, RedeemIssuer)>>),
    Reopen(Box<Reopen>),
    Process(Box<startup_recovery::HistoryReady>),
}

struct Member {
    expected: Arc<ExpectedIdentity>,
    leaf: Vec<u8>,
    custody: LearnerCustody,
    history: Option<Box<(Fresh, Live)>>,
}

struct ReopenSeed {
    plan: Plan,
    objects: ReopenObjects,
    custody: LearnerReopen,
    expected: Arc<ExpectedIdentity>,
    leaf: Vec<u8>,
}

// Only a successful, joined owned close produces the consumable expectation.
pub(super) struct Reopen(ReopenSeed);

#[cfg(test)]
pub(super) struct HostedCatchUp(pub(super) ());

pub(super) struct Run {
    pub(super) job: &'static Job,
    pub(super) observer: Observer,
    supervisor: tokio::task::JoinHandle<()>,
}

struct TlsClock(Arc<HostClock>);
impl std::fmt::Debug for TlsClock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("checked learner clock")
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

impl Member {
    fn credentials(&self, owner: &Owner, clock: &Arc<HostClock>) -> Result<Credentials, Refusal> {
        let now = clock.sample_ms().map_err(|_| Refusal::Clock)?;
        let member = self.custody.member().map_err(|_| Refusal::Identity)?;
        check_retained(
            owner,
            Some(&self.expected),
            self.custody.installation(),
            &member,
            Some(now),
        )
        .map_err(|_| Refusal::Identity)?;
        let root = owner.read("ca.der", 4096)?;
        let record = PublicIdentity::decode(&owner.read("identity.json", 8192)?)
            .map_err(|_| Refusal::Identity)?;
        if record.leaf() != self.leaf {
            return Err(Refusal::Identity);
        }
        let key = member
            .learner_tls_key(&self.expected, &root, &self.leaf, now)
            .map_err(|_| Refusal::Identity)?;
        Ok(Credentials {
            root: root.into(),
            leaf: self.leaf.clone().into(),
            key,
            time: Arc::new(TlsClock(clock.clone())),
        })
    }
}

pub(super) fn start_published(
    join: RetainedJoin,
    params: Params,
    #[cfg(test)] fixture: Option<HostedCatchUp>,
) -> Result<Run, crate::cluster::enrollment::durable_join::Status> {
    Ok(start(
        Input::Published(Box::new(join.consume()?), None),
        params,
        None,
        #[cfg(test)]
        fixture,
    ))
}

pub(super) fn start_history_join(
    join: HistoryJoin,
    params: Params,
) -> Result<Run, crate::cluster::enrollment::durable_join::Status> {
    start_history(join, params, None)
}

pub(super) fn start_history(
    join: HistoryJoin,
    params: Params,
    fence: Option<&super::runtime::Admission>,
) -> Result<Run, crate::cluster::enrollment::durable_join::Status> {
    let (published, slot, issuance, peer) = join.consume()?;
    Ok(start(
        Input::Published(Box::new(published), Some(Box::new((slot, issuance, peer)))),
        params,
        fence,
        #[cfg(test)]
        None,
    ))
}

pub(super) fn recover(
    expectation: Reopen,
    params: Params,
    #[cfg(test)] fixture: Option<HostedCatchUp>,
) -> Run {
    start(
        Input::Reopen(Box::new(expectation)),
        params,
        None,
        #[cfg(test)]
        fixture,
    )
}

pub(super) fn recover_process(
    ready: startup_recovery::HistoryReady,
    params: Params,
    fence: &super::runtime::Admission,
) -> Run {
    start(
        Input::Process(Box::new(ready)),
        params,
        Some(fence),
        #[cfg(test)]
        None,
    )
}

fn start(
    input: Input,
    params: Params,
    fence: Option<&super::runtime::Admission>,
    #[cfg(test)] fixture: Option<HostedCatchUp>,
) -> Run {
    let (plan, expected) = match &input {
        Input::Published(p, _) => (&p.owner.plan, &p.expected),
        Input::Reopen(r) => (&r.0.plan, r.0.expected.as_ref()),
        Input::Process(ready) => (
            &ready.owner.plan,
            &ready
                .history
                .verified()
                .expect("authenticated input")
                .expected,
        ),
    };
    let intent = StartIntent {
        mode: plan.mode,
        cluster: expected.cluster,
        node: expected.node,
        // Owner already examined these canonical SocketAddr strings.
        peer: plan.peer.parse().expect("checked peer address"),
        enrollment: plan.enrollment.parse().expect("checked enrollment address"),
    };
    let process_recovered = matches!(input, Input::Process(_));
    let recovered = process_recovered || matches!(input, Input::Reopen(_));
    let (transport, factory) = TransportOwner::pending_founder(intent.cluster, intent.node);
    let job = Job::anchor();
    let mut state = job.state();
    state.strict_learner_recovery = recovered;
    state.learner = Some(Operation {
        input: Some(input),
        transport: Arc::new(tokio::sync::Mutex::new(transport)),
        member: None,
        local_time: None,
        credentials: None,
        clock: None,
        gate: None,
        advertised: None,
        admission: None,
        issuer_membership: None,
        issuer_install: Arc::default(),
        #[cfg(test)]
        issuer_sample: None,
        #[cfg(test)]
        issuer_before_proof: None,
        recovered,
        process_recovered,
        redeem_issuer: None,
        #[cfg(test)]
        fixture,
    });
    drop(state);
    if let Some(fence) = fence {
        fence.register(job);
    }
    let (_, observer, supervisor) = drive(
        job,
        None,
        intent,
        None,
        params,
        factory,
        None,
        #[cfg(test)]
        None,
    );
    Run {
        job,
        observer,
        supervisor,
    }
}

#[cfg(test)]
pub(super) fn hosted_fixture(job: &Job) -> bool {
    job.state()
        .learner
        .as_ref()
        .is_some_and(|op| op.fixture.is_some())
}

pub(super) fn prepare(job: &Job, intent: &StartIntent) {
    let mut state = job.state();
    let result = prepare_inner(&mut state, intent);
    state.status = match result {
        Ok(()) => Status::Prepared,
        Err(reason) => {
            state.refusal = Some(reason);
            if state.owner.is_some() && state.learner.as_ref().is_some_and(|op| op.recovered) {
                Status::Unconfirmed
            } else {
                Status::Refused
            }
        }
    };
    job.changed.send_replace(state.status);
}

fn prepare_inner(state: &mut State, intent: &StartIntent) -> Result<(), Refusal> {
    let op = state.learner.as_mut().ok_or(Refusal::Ownership)?;
    let input = op.input.take().ok_or(Refusal::Ownership)?;
    let clock = match input {
        Input::Published(published, history) => {
            let PublishedJoin {
                owner,
                custody,
                expected,
                leaf,
                time,
            } = *published;
            state.owner = Some(owner);
            let history = history.map(|history| {
                let (slot, live, peer) = *history;
                op.redeem_issuer = Some(peer);
                Box::new((slot, live))
            });
            op.member = Some(Member {
                expected: Arc::new(expected),
                leaf,
                custody,
                history,
            });
            op.local_time = Some(time);
            let owner = state.owner.as_mut().unwrap();
            let clock = Arc::new(HostClock::new(0));
            if op
                .local_time
                .as_ref()
                .and_then(|clock| clock.current_time())
                .is_none()
            {
                return Err(Refusal::Clock);
            }
            let member = op.member.as_ref().unwrap();
            owner.stage_published_join()?;
            // Credentials checks the exact learner custody inventory, including
            // the absence of any CA material in the member store.
            op.credentials = Some(member.credentials(owner, &clock)?);
            if state.process_owned
                && let Some((slot, _)) = member.history.as_deref()
            {
                slot.confirm_learner_without_ca()?;
                state.join_evidence.without_ca = true;
            }
            let (stores, ready) = startup_recovery::prepare_published_join(owner, intent);
            let failure = stores.first_failure_for_learner();
            state.stores = Some(stores);
            ready.ok_or(failure)?;
            clock
        }
        Input::Reopen(reopen) => {
            let Reopen(ReopenSeed {
                plan,
                objects,
                custody,
                expected,
                leaf,
            }) = *reopen;
            if plan.mode != Mode::RecoverJoiner {
                return Err(Refusal::Plan);
            }
            state.owner = Some(Owner::acquire(plan)?);
            let owner = state.owner.as_mut().unwrap();
            owner.verify_reopen_objects(&objects)?;
            let custody = custody.open().map_err(|_| Refusal::Identity)?;
            op.member = Some(Member {
                expected,
                leaf,
                custody,
                history: None,
            });
            let member = op.member.as_ref().unwrap();
            let (stores, clock) =
                startup_recovery::prepare_strict_joiner(owner, intent, &member.expected);
            let failure = stores.first_failure_for_learner();
            state.stores = Some(stores);
            Arc::new(clock.ok_or(failure)?)
        }
        Input::Process(ready) => {
            let startup_recovery::HistoryReady {
                owner,
                history,
                stores,
                clock,
            } = *ready;
            state.owner = Some(owner);
            state.stores = Some(stores);
            let (custody, evidence) = history.into_learner().map_err(|history| {
                state.recovery_hold = Some(super::runtime::RecoveryHold::Custody(history));
                Refusal::Identity
            })?;
            op.issuer_membership = evidence.voter_position();
            let owner = state.owner.as_ref().unwrap();
            // Install custody before any fallible artifact read so a refusal
            // retains the same ownership until the process exits.
            let member = op.member.insert(Member {
                expected: Arc::new(evidence.expected),
                leaf: Vec::new(),
                custody,
                history: None,
            });
            let record = PublicIdentity::decode(&owner.read("identity.json", 8192)?)
                .map_err(|_| Refusal::Identity)?;
            member.leaf = record.leaf().to_vec();
            clock
        }
    };
    let member = op.member.as_ref().unwrap();
    op.credentials = Some(member.credentials(state.owner.as_ref().unwrap(), &clock)?);
    op.clock = Some(clock);
    Ok(())
}

pub(super) async fn attach(job: &'static Job, address: &SocketAddr) {
    let Some((node, transport)) = ({
        let state = job.state();
        state.learner.as_ref().and_then(|op| {
            state
                .node
                .as_ref()
                .map(|node| (node.clone(), op.transport.clone()))
        })
    }) else {
        return;
    };
    let node = node.lock().await;
    let mut transport = transport.lock().await;
    let result = {
        let mut state = job.state();
        if state.close_requested {
            Err(Refusal::Ownership)
        } else {
            let listener = state.inherited.as_mut().and_then(|pair| pair.peer.take());
            attach_inner(
                state.learner.as_mut().unwrap(),
                &node,
                &mut transport,
                *address,
                listener,
            )
        }
    };
    let result = result.and_then(|()| {
        if job
            .state()
            .learner
            .as_ref()
            .is_some_and(|op| op.process_recovered)
        {
            // A restarted installation can use its authenticated completion.
            // It cannot receive a new installation through the legacy lane.
            return Ok(());
        }
        transport
            .receive_issuer_transfer(job)
            .map_err(|_| Refusal::Ownership)
    });
    job.learner_attach_finished(result);
}

fn attach_inner(
    op: &mut Operation,
    node: &ConsensusNode,
    transport: &mut TransportOwner,
    address: SocketAddr,
    listener: Option<std::net::TcpListener>,
) -> Result<(), Refusal> {
    let sm = node.sm.as_ref().ok_or(Refusal::Store)?;
    let raft = node.raft.clone().ok_or(Refusal::Store)?;
    let expected = op
        .member
        .as_ref()
        .ok_or(Refusal::Identity)?
        .expected
        .clone();
    let clock = op.clock.as_ref().ok_or(Refusal::Clock)?.clone();
    let gate = LearnerGate::new(sm, expected, address, clock, !op.recovered);
    let phase = if op.recovered {
        Phase::Strict
    } else {
        Phase::Bootstrap
    };
    if gate.phase() != Ok(phase) {
        return Err(Refusal::Identity);
    }
    let credentials = op.credentials.take().ok_or(Refusal::Identity)?;
    transport
        .attach_learner(
            sm,
            Arc::new(EngineDispatch { raft }),
            address,
            credentials,
            gate.clone(),
            listener,
        )
        .map_err(|_| Refusal::Unavailable)?;
    op.gate = Some(gate);
    op.advertised = Some(address);
    Ok(())
}

pub(super) async fn close_transport(job: &Job) -> bool {
    let transport = job.state().learner.as_ref().map(|op| op.transport.clone());
    let Some(transport) = transport else {
        return true;
    };
    let mut transport = transport.lock().await;
    transport.close().await.is_ok() && transport.joined()
}

impl Run {
    pub(super) fn set_authenticated_admission_route(
        &self,
        issuer: SocketAddr,
    ) -> Result<(), Refusal> {
        {
            let state = self.job.state();
            let op = state.learner.as_ref().ok_or(Refusal::Ownership)?;
            if op.recovered {
                return Err(Refusal::Provenance);
            }
            let gate = op.gate.as_ref().ok_or(Refusal::Unavailable)?;
            let peer = op.redeem_issuer.as_ref().ok_or(Refusal::Provenance)?;
            let expect = gate.admission_anchor().map_err(|_| Refusal::Identity)?;
            if expect != (peer.node, peer.key.as_bytes()) {
                return Err(Refusal::Identity);
            }
        }
        self.set_admission_route(issuer)
    }

    pub(super) fn set_admission_route(&self, issuer: SocketAddr) -> Result<(), Refusal> {
        let mut state = self.job.state();
        if state.status != Status::LearnerReady || state.close_requested {
            return Err(Refusal::Ownership);
        }
        let op = state.learner.as_mut().ok_or(Refusal::Ownership)?;
        if op.admission.is_some() {
            return Err(Refusal::Plan);
        }
        let advertised = op.advertised.ok_or(Refusal::Plan)?;
        let transport = op.transport.try_lock().map_err(|_| Refusal::Unavailable)?;
        op.admission = Some(
            transport
                .admission_client(issuer, advertised)
                .map_err(|_| Refusal::Plan)?,
        );
        Ok(())
    }

    pub(super) async fn admit(&self) -> learner_admission::Reply {
        let result = {
            let mut state = self.job.state();
            if state.status != Status::LearnerReady || state.close_requested {
                return learner_admission::Reply::Refused;
            }
            let request = state
                .learner
                .as_ref()
                .and_then(|op| op.admission.as_ref())
                .and_then(|client| client.request());
            if request.is_some() && state.process_owned {
                state.join_evidence.requests = state.join_evidence.requests.saturating_add(1);
            }
            request
        };
        let Some(result) = result else {
            return learner_admission::Reply::Refused;
        };
        let reply = result.reply().await;
        if let learner_admission::Reply::Applied(position, _) = reply {
            let mut state = self.job.state();
            if state.process_owned {
                state.join_evidence.applied = Some(position);
            }
        }
        reply
    }

    #[cfg(test)]
    pub(super) async fn conflicting_admission(
        &self,
        address: SocketAddr,
    ) -> learner_admission::Reply {
        let result = {
            let state = self.job.state();
            if state.status != Status::LearnerReady || state.close_requested {
                return learner_admission::Reply::Refused;
            }
            state
                .learner
                .as_ref()
                .and_then(|op| op.admission.as_ref())
                .and_then(|client| client.conflicting_address(address))
        };
        match result {
            Some(result) => result.reply().await,
            None => learner_admission::Reply::Refused,
        }
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(super) async fn observe_gated_inbound(&self, trace: &Arc<peer_transport::GatedCallTrace>) {
        let transport = self.job.state().learner.as_ref().unwrap().transport.clone();
        transport.lock().await.observe_gated_inbound(trace);
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(super) async fn arm_before_dispatch_gate(&self) {
        let transport = self.job.state().learner.as_ref().unwrap().transport.clone();
        transport.lock().await.arm_before_dispatch();
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(super) async fn release_before_dispatch_gate(&self) -> Option<tokio::time::Instant> {
        let transport = self.job.state().learner.as_ref().unwrap().transport.clone();
        transport.lock().await.release_before_dispatch()
    }

    pub(super) fn admission_progress(&self) -> Option<(Phase, Option<LogPos>)> {
        let gate = self.job.state().learner.as_ref()?.gate.as_ref()?.clone();
        gate.admission_progress().ok()
    }

    pub(super) fn phase(&self) -> Option<Phase> {
        self.job
            .state()
            .learner
            .as_ref()?
            .gate
            .as_ref()?
            .phase()
            .ok()
    }

    pub(super) fn persist_strict_history(&self) -> Result<(), ActivationError> {
        self.job.persist_strict_history()
    }

    fn pin_for_reopen(&self) -> Result<ReopenSeed, Refusal> {
        let state = self.job.state();
        let owner = state.owner.as_ref().ok_or(Refusal::Ownership)?;
        let op = state.learner.as_ref().ok_or(Refusal::Ownership)?;
        if op.recovered
            || state.status != Status::LearnerReady
            || state.close_requested
            || op.gate.as_ref().and_then(|gate| gate.phase().ok()) != Some(Phase::Strict)
        {
            return Err(Refusal::Provenance);
        }
        let member = op.member.as_ref().ok_or(Refusal::Identity)?;
        if member.history.is_some() {
            return Err(Refusal::Provenance);
        }
        Ok(ReopenSeed {
            plan: owner.plan.recover_joiner(),
            objects: owner.pin_reopen_objects()?,
            custody: member.custody.pin_reopen().map_err(|_| Refusal::Identity)?,
            expected: member.expected.clone(),
            leaf: member.leaf.clone(),
        })
    }

    pub(super) async fn close(self, retain_reopen: bool) -> Result<Option<Reopen>, Refusal> {
        let seed = retain_reopen.then(|| self.pin_for_reopen()).transpose();
        drop(self.job.request_close());
        self.supervisor.await.map_err(|_| Refusal::Ownership)?;
        let state = self.job.state();
        if state.status != Status::Clean || state.owner.is_some() || state.learner.is_some() {
            return Err(Refusal::Ownership);
        }
        seed.map(|seed| seed.map(Reopen))
    }

    #[cfg(test)]
    pub(super) fn has_receipt(&self) -> bool {
        self.job
            .state()
            .learner
            .as_ref()
            .and_then(|op| op.gate.as_ref())
            .is_some_and(|gate| gate.has_receipt())
    }

    #[cfg(test)]
    pub(super) fn binding(
        &self,
    ) -> (
        crate::cluster::enrollment::NodeId,
        crate::cluster::enrollment::KeyDigest,
    ) {
        let state = self.job.state();
        let member = state.learner.as_ref().unwrap().member.as_ref().unwrap();
        (member.expected.node, member.expected.key)
    }

    #[cfg(test)]
    pub(super) async fn dispatch_observer(
        &self,
    ) -> tokio::sync::mpsc::Receiver<peer_transport::Event> {
        let transport = self.job.state().learner.as_ref().unwrap().transport.clone();
        transport.lock().await.dispatch_observer()
    }

    #[cfg(test)]
    pub(super) fn original(&self) -> LogPos {
        self.job
            .state()
            .learner
            .as_ref()
            .unwrap()
            .member
            .as_ref()
            .unwrap()
            .expected
            .origin
            .position()
    }

    #[cfg(test)]
    pub(super) fn state_path(&self) -> Option<std::path::PathBuf> {
        self.job
            .state()
            .owner
            .as_ref()
            .map(|owner| owner.plan.state.clone())
    }
}

pub struct StrictWitness(LogPos);

impl restart_history::StrictProof for LearnerCustody {
    type Observation = StrictWitness;
}

impl restart_history::StrictObservation for StrictWitness {
    fn position(self) -> LogPos {
        self.0
    }
}

impl Job {
    fn persist_strict_history(&self) -> Result<(), ActivationError> {
        self.persist_strict_history_before(None)
    }

    fn persist_strict_history_before(
        &self,
        deadline: Option<Instant>,
    ) -> Result<(), ActivationError> {
        use ActivationError::Refused;
        let mut state = self.state.lock().map_err(|_| Refused)?;
        if state.close_requested || state.status != Status::LearnerReady {
            return Err(Refused);
        }
        let State {
            owner,
            learner,
            process_owned,
            ..
        } = &mut *state;
        let op = learner.as_mut().ok_or(Refused)?;
        let gate = op.gate.as_ref().ok_or(Refused)?;
        let position =
            observe_strict_before(gate, deadline, StateStore::applied)?.ok_or(Refused)?;
        let member = op.member.as_mut().ok_or(Refused)?;
        let (slot, live) = member.history.as_deref_mut().ok_or(Refused)?;
        let owner = owner.as_ref().ok_or(Refused)?;
        if *process_owned && live.links().is_ok() {
            return slot
                .confirm_strict(owner, &member.custody.history_auth(), live)
                .map_err(|_| Refused);
        }
        slot.persist_strict(owner, &member.custody, live, StrictWitness(position))
            .map_err(|_| Refused)
    }

    async fn wait_for_strict_history(&self) -> Result<(), peer_transport::Refusal> {
        use crate::cluster::pki::verify::VerifyRefusal;
        use peer_registry::ObservationRefusal;
        peer_registry::wait_for_store(|deadline| {
            self.persist_strict_history_before(deadline)
                .map_err(|error| match error {
                    ActivationError::Deferred => ObservationRefusal::StoreBusy,
                    ActivationError::Refused => VerifyRefusal::Unavailable.into(),
                })
        })
        .await
        .map_err(|_| peer_transport::Refusal::Identity)
    }

    pub(in crate::cluster::consensus) async fn check_issuer_transfer(
        &self,
        claim: super::issuer_transfer::Claim,
        sender: crate::cluster::pki::verify::AuthenticatedPeer,
    ) -> Result<super::issuer_transfer::Offer, peer_transport::Refusal> {
        let activation = {
            let state = self.state();
            state.process_owned.then(|| state.node.clone()).flatten()
        };
        if let Some(node) = activation {
            use openraft::type_config::async_runtime::watch::WatchReceiver;
            let refused = peer_transport::Refusal::Identity;
            let mut metrics = node.lock().await.raft.as_ref().ok_or(refused)?.metrics();
            // Within the already bounded transport check, wait for local
            // apply rather than treating replicated bytes as confirmation.
            loop {
                let applied = metrics.borrow_watched().last_applied;
                if let Some(applied) = applied
                    && applied.index >= claim.membership.index
                {
                    if applied.index == claim.membership.index
                        && (applied.leader_id.term, applied.leader_id.node_id)
                            != (claim.membership.term, claim.membership.leader)
                    {
                        return Err(refused);
                    }
                    break;
                }
                metrics.changed().await.map_err(|_| refused)?;
            }
            self.wait_for_strict_history().await?;
        }
        self.with_receiver_proof(claim, sender, |_, offer| offer)
            .await
    }

    async fn with_receiver_proof<T>(
        &self,
        claim: super::issuer_transfer::Claim,
        sender: crate::cluster::pki::verify::AuthenticatedPeer,
        use_offer: impl FnOnce(&mut Operation, super::issuer_transfer::Offer) -> T,
    ) -> Result<T, peer_transport::Refusal> {
        use crate::cluster::{c13_diagnostics::record, pki::verify::VerifyRefusal};
        let refused = peer_transport::Refusal::Identity;
        let node = self.state().node.clone().ok_or(refused)?;
        let mut use_offer = Some(use_offer);
        peer_registry::wait_for_receiver_store(
            &node,
            |node, deadline| {
                let mut state = self.state();
                let (_, op) = receiver_parts(&mut state).map_err(|_| VerifyRefusal::Unavailable)?;
                let gate = op.gate.as_ref().ok_or(VerifyRefusal::Unavailable)?.clone();
                gate.inspect_strict(deadline, |store| {
                    let expected = &op.member.as_ref().ok_or(refused)?.expected;
                    let address = op.advertised.ok_or(refused)?;
                    let membership = op.issuer_membership_view(node);
                    let offer =
                        receiver_offer(&membership, store, expected, address, claim, sender)
                            .ok_or(refused)?;
                    Ok(use_offer.take().expect("one guarded observation")(
                        op, offer,
                    ))
                })
            },
            |category| record("transfer-receiver", category),
        )
        .await
        .map_err(|_| refused)?
    }

    pub(in crate::cluster::consensus) async fn install_voter_custody(
        &self,
        claim: super::issuer_transfer::Claim,
        sender: crate::cluster::pki::verify::AuthenticatedPeer,
        channel: (&[u8; 32], &[u8], Vec<u8>),
    ) -> Result<(), peer_transport::Refusal> {
        let refused = peer_transport::Refusal::Identity;
        let install = {
            let mut state = self.state();
            receiver_parts(&mut state)?.1.issuer_install.clone()
        };
        let _install = install.lock().await;
        let history = {
            let mut state = self.state();
            receiver_parts(&mut state)?
                .1
                .member
                .as_ref()
                .is_some_and(|m| m.history.is_some())
        };
        if history {
            let result = self.install_voter_history(claim, sender, channel).await;
            return self.finish_receiver_proof(result);
        }
        let (prepared, root, now) = {
            let mut state = self.state();
            let (owner, op) = receiver_parts(&mut state)?;
            let member = op.member.as_ref().ok_or(refused)?;
            let clock = op.clock.as_ref().ok_or(refused)?;
            let tls = member.credentials(owner, clock).map_err(|_| refused)?;
            let now = clock.sample_ms().map_err(|_| refused)?;
            let prepared = member
                .custody
                .prepare_voter_ca(claim.cluster, &tls.root)
                .map_err(|_| refused)?;
            (prepared, tls.root, now)
        };
        let sealed = self
            .with_receiver_proof(claim, sender, |op, _| {
                op.member
                    .as_mut()
                    .ok_or(refused)?
                    .custody
                    .seal_voter_ca(prepared, &root, now, channel)
                    .map_err(|_| refused)
            })
            .await;
        let sealed = self.finish_receiver_proof(sealed)??;
        let written = {
            let mut state = self.state();
            let (owner, op) = receiver_parts(&mut state)?;
            op.member
                .as_ref()
                .ok_or(refused)?
                .custody
                .write_voter_ca(owner, sealed)
                .map_err(|_| refused)?
        };
        let confirmed = self
            .with_receiver_proof(claim, sender, |op, _| {
                op.member
                    .as_mut()
                    .ok_or(refused)?
                    .custody
                    .confirm_voter_ca(written, &root, now, || {
                        op.issuer_membership = Some(claim.membership)
                    })
                    .map_err(|_| refused)
            })
            .await;
        self.finish_receiver_proof(confirmed)?
    }

    async fn install_voter_history(
        &self,
        claim: super::issuer_transfer::Claim,
        sender: crate::cluster::pki::verify::AuthenticatedPeer,
        channel: (&[u8; 32], &[u8], Vec<u8>),
    ) -> Result<(), peer_transport::Refusal> {
        let refused = peer_transport::Refusal::Identity;
        let (prepared, root, now, links) = {
            let mut state = self.state();
            let (owner, op) = receiver_parts(&mut state)?;
            let member = op.member.as_mut().ok_or(refused)?;
            let links = member
                .history
                .as_deref()
                .ok_or(refused)?
                .1
                .links()
                .map_err(|_| refused)?;
            member.custody.spend_voter_history().map_err(|_| refused)?;
            let clock = op.clock.as_ref().ok_or(refused)?;
            let tls = member.credentials(owner, clock).map_err(|_| refused)?;
            let now = clock.sample_ms().map_err(|_| refused)?;
            let prepared = member
                .custody
                .prepare_voter_history(claim.cluster, &tls.root)
                .map_err(|_| refused)?;
            (prepared, tls.root, now, links)
        };
        let sealed = self
            .with_receiver_proof(claim, sender, |op, _| {
                op.member
                    .as_mut()
                    .ok_or(refused)?
                    .custody
                    .seal_voter_history(prepared, &root, now, channel)
                    .map_err(|_| refused)
            })
            .await??;
        let written = {
            let mut state = self.state();
            let (owner, op) = receiver_parts(&mut state)?;
            op.member
                .as_ref()
                .ok_or(refused)?
                .custody
                .write_voter_ca(owner, sealed)
                .map_err(|_| refused)?
        };
        let candidate = self
            .with_receiver_proof(claim, sender, |op, offer| {
                let claim = offer.claim;
                let proof = VoterOffer::from_guarded_offer(
                    claim.cluster,
                    claim.root,
                    claim.sender,
                    claim.receiver,
                    offer.sender_address,
                    offer.receiver_address,
                    (claim.epoch.term, claim.epoch.node_id),
                    claim.membership,
                );
                op.member
                    .as_ref()
                    .ok_or(refused)?
                    .custody
                    .candidate_voter_history(written, &root, now, proof, links)
                    .map_err(|_| refused)
            })
            .await??;
        let mut state = self.state();
        let (owner, op) = receiver_parts(&mut state)?;
        let member = op.member.as_mut().ok_or(refused)?;
        let (slot, live) = member.history.as_deref().ok_or(refused)?;
        let verified = slot
            .commit_voter(owner, member.custody.history_auth(), live, &candidate)
            .map_err(|_| refused)?;
        member
            .custody
            .complete_voter_history(candidate, verified, || {
                op.issuer_membership = Some(claim.membership);
            })
            .map_err(|_| refused)
    }

    fn finish_receiver_proof<T>(
        &self,
        result: Result<T, peer_transport::Refusal>,
    ) -> Result<T, peer_transport::Refusal> {
        result.inspect_err(|_| {
            let mut state = self.state();
            if let Some(member) = state.learner.as_mut().and_then(|op| op.member.as_mut()) {
                member.custody.refuse_voter_ca();
            }
        })
    }

    pub(super) async fn activate_promoted_issuer(&'static self) -> Result<(), ActivationError> {
        let (node, ..) = voter_context(&self.state())?;
        let node = node.try_lock().map_err(|_| ActivationError::Deferred)?;
        // The unchanged quorum read establishes leadership before publishing readiness.
        let (epoch, ()) = node.fresh_read(|_| Ok(())).await?;
        self.publish_promoted_issuer(&node, epoch, || {
            use openraft::type_config::async_runtime::watch::WatchReceiver;
            let raft = node.raft.as_ref().ok_or(ActivationError::Refused)?;
            let metrics = raft.metrics();
            let current = metrics.borrow_watched();
            if current.running_state.is_err() {
                return Err(ActivationError::Refused);
            }
            let local_leader = current.vote.committed
                && current.current_leader == Some(node.id)
                && current.state == openraft::ServerState::Leader;
            Ok(local_leader.then_some(current.vote.leader_id))
        })
    }

    fn publish_promoted_issuer(
        &self,
        node: &ConsensusNode,
        epoch: LeaderId,
        observe_epoch: impl Fn() -> Result<Option<LeaderId>, ActivationError>,
    ) -> Result<(), ActivationError> {
        let mut state = self.state();
        let (_, local, clock) = voter_context(&state)?;
        if epoch.node_id != local.node.get() {
            return Err(ActivationError::Refused);
        }
        let owner = state.owner.as_ref().ok_or(ActivationError::Refused)?;
        let endpoint = peer_transport::endpoint(&owner.plan.enrollment)
            .map_err(|_| ActivationError::Refused)?;
        if !peer_control_wire::qualified(endpoint) || state.issuer.is_some() {
            return Err(ActivationError::Refused);
        }
        let cluster = state
            .learner
            .as_ref()
            .unwrap()
            .member
            .as_ref()
            .unwrap()
            .expected
            .cluster;
        let check_epoch = || {
            (observe_epoch()? == Some(epoch))
                .then_some(())
                .ok_or(ActivationError::Deferred)
        };
        let (ca, tls, installed_at, _proof) = load_promoted(&state, node, Some(&check_epoch))?;
        state.issuer = Some(Arc::new(enrollment_ingress::ReadyIssuer::promoted(
            endpoint,
            cluster,
            ca,
            clock,
            local,
            installed_at,
            tls,
        )));
        Ok(())
    }
}

impl Operation {
    pub(super) fn voter_custody(&self) -> bool {
        self.issuer_membership.is_some()
    }

    pub(super) fn installation_id(&self) -> Option<[u8; 16]> {
        Some(self.member.as_ref()?.custody.installation())
    }

    fn issuer_membership_view<'a>(&self, node: &'a ConsensusNode) -> IssuerMembership<'a> {
        let membership = node.issuer_membership();
        #[cfg(test)]
        let membership = membership
            .with_sample(self.issuer_sample.clone())
            .with_before_proof(self.issuer_before_proof.clone());
        membership
    }
}

fn receiver_parts(state: &mut State) -> Result<(&Owner, &mut Operation), peer_transport::Refusal> {
    use peer_transport::Refusal;
    if state.close_requested {
        return Err(Refusal::Closed);
    }
    if state.status != Status::LearnerReady {
        return Err(Refusal::Identity);
    }
    Ok((
        state.owner.as_ref().ok_or(Refusal::Identity)?,
        state.learner.as_mut().ok_or(Refusal::Identity)?,
    ))
}

fn receiver_offer(
    membership: &IssuerMembership<'_>,
    store: &StateStore,
    expected: &ExpectedIdentity,
    address: SocketAddr,
    claim: super::issuer_transfer::Claim,
    sender: crate::cluster::pki::verify::AuthenticatedPeer,
) -> Option<super::issuer_transfer::Offer> {
    let original = expected.origin.position();
    if claim.cluster != expected.cluster
        || claim.root != expected.ca
        || claim.receiver != (expected.node, *expected.key.as_bytes())
        || claim.membership.index <= original.index
        || claim.membership.term < original.term
    {
        return None;
    }
    let offer = membership.offer_in(store, claim, sender).ok()?;
    (offer.receiver_address == address).then_some(offer)
}

type VoterContext = (
    Arc<tokio::sync::Mutex<ConsensusNode>>,
    super::issuer_transfer::Voter,
    Arc<HostClock>,
);

fn observe_strict<T>(
    gate: &LearnerGate,
    observe: impl FnOnce(&StateStore) -> T,
) -> Result<T, ActivationError> {
    observe_strict_before(gate, None, observe)
}

fn observe_strict_before<T>(
    gate: &LearnerGate,
    deadline: Option<Instant>,
    observe: impl FnOnce(&StateStore) -> T,
) -> Result<T, ActivationError> {
    use peer_registry::ObservationRefusal;
    gate.inspect_strict(deadline, observe)
        .map_err(|error| match error {
            ObservationRefusal::StoreBusy => ActivationError::Deferred,
            _ => ActivationError::Refused,
        })
}

fn voter_context(state: &State) -> Result<VoterContext, ActivationError> {
    use ActivationError::Refused;
    if state.close_requested || state.status != Status::LearnerReady {
        return Err(Refused);
    }
    let op = state.learner.as_ref().ok_or(Refused)?;
    let member = op.member.as_ref().ok_or(Refused)?;
    observe_strict(op.gate.as_ref().ok_or(Refused)?, |_| ())?;
    Ok((
        state.node.clone().ok_or(Refused)?,
        super::issuer_transfer::Voter {
            node: member.expected.node,
            key: member.expected.key,
            address: op.advertised.ok_or(Refused)?,
        },
        op.clock.clone().ok_or(Refused)?,
    ))
}

// Check identity before considering whether the quorum epoch is still live.
fn activation_proof<T>(
    proof: Result<T, membership_observation::MembershipReadError>,
    check_epoch: impl FnOnce() -> Result<(), ActivationError>,
) -> Result<T, ActivationError> {
    let proof = proof?;
    check_epoch()?;
    Ok(proof)
}

fn load_promoted<'a>(
    state: &State,
    node: &'a ConsensusNode,
    check_epoch: Option<&dyn Fn() -> Result<(), ActivationError>>,
) -> Result<
    (
        crate::cluster::pki::ca::ClusterCa,
        Credentials,
        LogPos,
        IssuerProof<'a>,
    ),
    ActivationError,
> {
    use crate::cluster::pki::custody::Error;
    let (_, local, clock) = voter_context(state)?;
    let op = state.learner.as_ref().ok_or(ActivationError::Refused)?;
    let installed_at = op.issuer_membership.ok_or(ActivationError::Refused)?;
    let member = op.member.as_ref().ok_or(ActivationError::Refused)?;
    let cluster = member.expected.cluster;
    let owner = state.owner.as_ref().ok_or(ActivationError::Refused)?;
    let tls = member
        .credentials(owner, &clock)
        .map_err(|_| ActivationError::Refused)?;
    let now = clock.sample_ms().map_err(|_| ActivationError::Refused)?;
    let membership = op.issuer_membership_view(node);
    let mut activation_error = ActivationError::Refused;
    let (ca, proof) = member
        .custody
        .load_voter_ca(cluster, tls.root.as_ref(), now, || {
            // Bracket the guarded identity proof with the same quorum epoch.
            // A proof refusal retains its own cause even if the vote changes.
            let check = || check_epoch.map_or(Ok(()), |observe| observe());
            let before = check();
            if before == Err(ActivationError::Refused) {
                return Err(Error::Ownership);
            }
            let proof = activation_proof(
                membership
                    .voter_proof(cluster, local, installed_at, check_epoch.is_some(), None)
                    .and_then(|proof| {
                        proof
                            .covers_member(&member.expected, local.address)
                            .then_some(proof)
                            .ok_or(membership_observation::MembershipReadError::PeerRefused)
                    }),
                || check().and(before),
            )
            .map_err(|error| {
                activation_error = error;
                Error::Ownership
            })?;
            Ok(proof)
        })
        .map_err(|_| activation_error)?;
    Ok((ca, tls, installed_at, proof))
}

#[cfg(test)]
impl Run {
    pub(super) fn loaded_voter_root(&self) -> Result<Vec<u8>, Refusal> {
        let state = self.job.state();
        let (node, ..) = voter_context(&state).map_err(|_| Refusal::Identity)?;
        let node = node.try_lock().map_err(|_| Refusal::Ownership)?;
        let (ca, _, _, _proof) =
            load_promoted(&state, &node, None).map_err(|_| Refusal::Identity)?;
        Ok(ca.cert_der().to_vec())
    }
}

#[cfg(test)]
impl Job {
    pub(in crate::cluster::consensus) fn issuer_receiver_fixture(
        published: PublishedJoin,
        sm: &SmAdapter,
        sample: membership_observation::Sample,
        clock: Arc<HostClock>,
    ) -> (&'static Self, Credentials, Arc<LearnerGate>) {
        let PublishedJoin {
            mut owner,
            custody,
            expected,
            leaf,
            time,
        } = published;
        let expected = Arc::new(expected);
        let address = owner.plan.peer.parse().unwrap();
        let gate = LearnerGate::new(sm, expected.clone(), address, clock.clone(), false);
        if !owner.has_stores() {
            owner.stage_published_join().unwrap();
        }
        let member = Member {
            expected,
            leaf,
            custody,
            history: None,
        };
        let credentials = member.credentials(&owner, &clock).unwrap();
        let (transport, _) =
            TransportOwner::pending_founder(member.expected.cluster, member.expected.node);
        let node = ConsensusNode {
            id: member.expected.node.get(),
            read_permits: Arc::new(Semaphore::new(READ_PERMITS)),
            issue_permits: Arc::new(Semaphore::new(issue::MAX_ISSUING)),
            read_blocking: Arc::default(),
            raft: None,
            sm: Some(sm.clone()),
            log_released: crate::cluster::state_store::release_pair().1,
            sm_released: sm.0.lock().unwrap().released(),
            log_weak: Weak::new(),
            sm_weak: Arc::downgrade(&sm.0),
            permits: Arc::new(Semaphore::new(64)),
            owners: Mutex::default(),
        };
        let job = Self::anchor();
        let mut state = job.state();
        state.owner = Some(owner);
        state.node = Some(Arc::new(tokio::sync::Mutex::new(node)));
        state.learner = Some(Operation {
            input: None,
            transport: Arc::new(tokio::sync::Mutex::new(transport)),
            member: Some(member),
            local_time: Some(time),
            credentials: None,
            clock: Some(clock),
            gate: Some(gate.clone()),
            advertised: Some(address),
            admission: None,
            issuer_membership: None,
            issuer_install: Arc::default(),
            issuer_sample: Some(sample),
            issuer_before_proof: None,
            recovered: false,
            process_recovered: false,
            redeem_issuer: None,
            fixture: None,
        });
        state.status = Status::LearnerReady;
        (job, credentials, gate)
    }

    pub(in crate::cluster::consensus) async fn set_receiver_fixture_history(
        &self,
        slot: Fresh,
        live: Live,
        sm: &SmAdapter,
    ) {
        {
            let mut state = self.state();
            let member = state.learner.as_mut().unwrap().member.as_mut().unwrap();
            assert!(member.history.replace(Box::new((slot, live))).is_none());
        }
        tokio::time::pause();
        let mut persisted = std::pin::pin!(self.wait_for_strict_history());
        {
            let _held = sm.0.lock().unwrap();
            let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
            assert!(std::future::Future::poll(persisted.as_mut(), &mut cx).is_pending());
            assert!(self.state.try_lock().is_ok());
        }
        assert_eq!(persisted.await, Ok(()));
        tokio::time::resume();
    }

    pub(in crate::cluster::consensus) fn fault_receiver_fixture_history(
        &self,
        fault: restart_history::TestFault,
    ) {
        let mut state = self.state();
        state
            .learner
            .as_mut()
            .unwrap()
            .member
            .as_mut()
            .unwrap()
            .history
            .as_deref_mut()
            .unwrap()
            .0
            .fault(fault);
    }

    pub(in crate::cluster::consensus) fn receiver_fixture_completion(
        &self,
    ) -> (Option<LogPos>, bool, bool) {
        let state = self.state();
        let op = state.learner.as_ref().unwrap();
        let (confirmed, available) = op.member.as_ref().unwrap().custody.voter_custody_state();
        (op.issuer_membership, confirmed, available)
    }

    pub(in crate::cluster::consensus) fn assert_fixture_custody(
        &self,
        root: &[u8],
        confirmed_at: Option<LogPos>,
        survivor: (membership_observation::Sample, LeaderId),
    ) {
        let node = {
            let mut state = self.state();
            let op = state.learner.as_mut().unwrap();
            assert_eq!(op.issuer_membership, confirmed_at);
            let member = op.member.as_ref().unwrap();
            assert_eq!(
                member.custody.voter_custody_state(),
                (confirmed_at.is_some(), false)
            );
            let now = op.clock.as_ref().unwrap().sample_ms().unwrap();
            let load = member
                .custody
                .load_voter_ca(member.expected.cluster, root, now, || Ok(()));
            if confirmed_at.is_some() {
                assert_eq!(load.unwrap().0.cert_der(), root);
            } else {
                assert!(matches!(
                    load,
                    Err(crate::cluster::pki::custody::Error::Ownership)
                ));
            }
            op.issuer_sample = Some(survivor.0);
            state.node.clone().unwrap()
        };
        let live = node.try_lock().unwrap();
        assert_eq!(
            load_promoted(&self.state(), &live, None).is_ok(),
            confirmed_at.is_some()
        );
        // This fixture has no Engine. Model the quorum read and the subsequent
        // local epoch observations independently of its guarded membership sample.
        if confirmed_at.is_some() {
            assert_eq!(
                self.publish_promoted_issuer(&live, survivor.1, || Ok(None)),
                Err(ActivationError::Deferred)
            );
            assert!(self.state().issuer.is_none());
        }
        assert_eq!(
            self.publish_promoted_issuer(&live, survivor.1, || Ok(Some(survivor.1)))
                .is_ok(),
            confirmed_at.is_some()
        );
        assert_eq!(self.state().issuer.is_some(), confirmed_at.is_some());

        self.release_fixture();
    }

    /// `fail_write` runs between the preliminary gate and the final applied
    /// proof of a readiness attempt. It must leave the store internally poisoned.
    pub(in crate::cluster::consensus) fn assert_poisoned_final_proof_refuses(
        &self,
        root: &[u8],
        survivor: (membership_observation::Sample, LeaderId),
        fail_write: membership_observation::BeforeProof,
    ) {
        use membership_observation::MembershipReadError;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let (node, cluster, local) = {
            let mut state = self.state();
            let op = state.learner.as_mut().unwrap();
            op.issuer_sample = Some(survivor.0.clone());
            let member = op.member.as_ref().unwrap();
            let local = super::issuer_transfer::Voter {
                node: member.expected.node,
                key: member.expected.key,
                address: op.advertised.unwrap(),
            };
            let cluster = member.expected.cluster;
            (state.node.clone().unwrap(), cluster, local)
        };
        let live = node.try_lock().unwrap();
        let sm = live.sm.clone().unwrap();
        let installed_at = self.state().learner.as_ref().unwrap().issuer_membership;
        let installed_at = installed_at.expect("confirmed voter custody");
        let final_proof = || {
            live.issuer_membership()
                .with_sample(Some(survivor.0.clone()))
                .voter_proof(cluster, local, installed_at, true, Some(survivor.1))
                .map(drop)
        };

        // Healthy control: this image loads the signer and passes the final proof.
        assert!(voter_context(&self.state()).is_ok());
        let (ca, _, _, proof) = load_promoted(&self.state(), &live, None).unwrap();
        assert_eq!(ca.cert_der(), root);
        drop(proof);
        assert_eq!(final_proof(), Ok(()));

        let calls = Arc::new(AtomicUsize::new(0));
        self.state().learner.as_mut().unwrap().issuer_before_proof = Some({
            let calls = calls.clone();
            Arc::new(move || {
                if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    fail_write();
                }
            })
        });
        assert_eq!(
            self.publish_promoted_issuer(&live, survivor.1, || Ok(Some(survivor.1))),
            Err(ActivationError::Refused)
        );
        // The failure reached the final proof and refused before signer load.
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(self.state().issuer.is_none());
        assert!(sm.0.try_lock().unwrap().is_poisoned());
        assert_eq!(final_proof(), Err(MembershipReadError::Unavailable));

        // A poisoned mutex is unavailable, never a retryable busy store.
        let store = sm.clone();
        let poisoned = std::thread::spawn(move || {
            let _guard = store.0.lock().unwrap();
            panic!("deliberate issuer store mutex poison");
        })
        .join();
        assert!(poisoned.is_err());
        assert!(sm.0.is_poisoned());
        assert_eq!(final_proof(), Err(MembershipReadError::Unavailable));
        drop(live);
        self.release_fixture();
    }

    fn release_fixture(&self) {
        let mut state = self.state();
        state.issuer.take();
        state.learner.take();
        state.node.take();
        state.owner.take();
    }
}

#[cfg(test)]
mod activation_tests {
    use super::*;
    use crate::cluster::enrollment::NodeId;
    use crate::cluster::join_receipt::{ExpectedIdentity, Origin};
    use enrollment_ingress::ActivationError::{Deferred, Refused};
    use membership_observation::MembershipReadError;

    #[test]
    fn issuer_strict_recheck_defers_only_a_busy_store() {
        use checked_clock::ClockError;

        struct BrokenClock;
        impl CheckedClock for BrokenClock {
            fn sample_ms(&self) -> Result<u64, ClockError> {
                Err(ClockError::Unavailable)
            }
        }

        let dir = crate::service::application::tests::private_temp();
        let node = NodeId::new(2).unwrap();
        let key = crate::cluster::enrollment::KeyDigest::of_spki(&[2]).unwrap();
        let store = StateStore::open(
            dir.path(),
            crate::cluster::state_store::Init {
                cluster: [1; 16],
                first_free: NodeId::new(3).unwrap(),
            },
        )
        .unwrap();
        let sm = SmAdapter(Arc::new(std::sync::Mutex::new(store)));
        let gate = |clock: Arc<dyn CheckedClock>| {
            LearnerGate::new(
                &sm,
                Arc::new(ExpectedIdentity {
                    cluster: [1; 16],
                    node,
                    key,
                    ca: [0; 32],
                    origin: Origin::Redeem {
                        term: 1,
                        leader: 1,
                        index: 1,
                        anchors: vec![],
                    },
                }),
                "192.168.0.2:7001".parse().unwrap(),
                clock,
                false,
            )
        };
        let healthy_clock = gate(Arc::new(HostClock::new(0)));
        let broken_clock = gate(Arc::new(BrokenClock));
        {
            let _held = sm.0.lock().unwrap();
            assert_eq!(observe_strict(&healthy_clock, |_| ()), Err(Deferred));
        }
        // Failed clock observation must precede even a normal gate callback.
        assert!(matches!(
            broken_clock.inspect_strict(None, |_| ()),
            Err(peer_registry::ObservationRefusal::Unavailable("clock"))
        ));
        assert_eq!(observe_strict(&broken_clock, |_| ()), Err(Refused));
        assert_eq!(observe_strict(&healthy_clock, |_| ()), Err(Refused));
        let poison = sm.0.clone();
        assert!(
            std::thread::spawn(move || {
                let _held = poison.lock().unwrap();
                panic!("deliberate activation store mutex poison");
            })
            .join()
            .is_err()
        );
        assert_eq!(observe_strict(&healthy_clock, |_| ()), Err(Refused));
        drop(sm);
        assert_eq!(observe_strict(&healthy_clock, |_| ()), Err(Refused));
    }

    #[test]
    fn issuer_epoch_change_cannot_mask_an_identity_refusal() {
        for proof in [
            MembershipReadError::PeerRefused,
            MembershipReadError::Unavailable,
        ] {
            assert_eq!(
                activation_proof::<()>(Err(proof), || Err(Deferred)),
                Err(Refused)
            );
        }
        assert_eq!(activation_proof(Ok(()), || Err(Deferred)), Err(Deferred));
        assert_eq!(activation_proof(Ok(()), || Err(Refused)), Err(Refused));
        assert_eq!(activation_proof(Ok(()), || Ok(())), Ok(()));
    }
}
