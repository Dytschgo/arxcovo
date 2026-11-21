//! Retained published learner composition and same-process reopen.
use super::*;
use crate::cluster::{
    enrollment::durable_join::{PublishedJoin, RetainedJoin},
    issuance_publication::check_retained,
    join_receipt::PublicIdentity,
    pki::custody::{LearnerCustody, LearnerReopen},
    startup_plan::{Mode, ReopenObjects},
};
use checked_clock::{CheckedClock, HostClock};
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
    #[cfg(test)]
    issuer_sample: Option<membership_observation::Sample>,
    recovered: bool,
    #[cfg(test)]
    fixture: Option<HostedCatchUp>,
}

enum Input {
    Published(Box<PublishedJoin>),
    Reopen(Box<Reopen>),
}

struct Member {
    expected: Arc<ExpectedIdentity>,
    leaf: Vec<u8>,
    custody: LearnerCustody,
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
        Input::Published(Box::new(join.consume()?)),
        params,
        #[cfg(test)]
        fixture,
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
        #[cfg(test)]
        fixture,
    )
}

fn start(input: Input, params: Params, #[cfg(test)] fixture: Option<HostedCatchUp>) -> Run {
    let (plan, expected) = match &input {
        Input::Published(p) => (&p.owner.plan, &p.expected),
        Input::Reopen(r) => (&r.0.plan, r.0.expected.as_ref()),
    };
    let intent = StartIntent {
        mode: plan.mode,
        cluster: expected.cluster,
        node: expected.node,
        // Owner already examined these canonical SocketAddr strings.
        peer: plan.peer.parse().expect("checked peer address"),
        enrollment: plan.enrollment.parse().expect("checked enrollment address"),
    };
    let recovered = matches!(input, Input::Reopen(_));
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
        #[cfg(test)]
        issuer_sample: None,
        recovered,
        #[cfg(test)]
        fixture,
    });
    drop(state);
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
        Input::Published(published) => {
            let PublishedJoin {
                owner,
                custody,
                expected,
                leaf,
                time,
            } = *published;
            state.owner = Some(owner);
            op.member = Some(Member {
                expected: Arc::new(expected),
                leaf,
                custody,
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
            op.credentials = Some(member.credentials(owner, &clock)?);
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
            });
            let member = op.member.as_ref().unwrap();
            let (stores, clock) =
                startup_recovery::prepare_strict_joiner(owner, intent, &member.expected);
            let failure = stores.first_failure_for_learner();
            state.stores = Some(stores);
            Arc::new(clock.ok_or(failure)?)
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
            attach_inner(
                state.learner.as_mut().unwrap(),
                &node,
                &mut transport,
                *address,
            )
        }
    };
    let result = result.and_then(|()| {
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
            let state = self.job.state();
            if state.status != Status::LearnerReady || state.close_requested {
                return learner_admission::Reply::Refused;
            }
            state
                .learner
                .as_ref()
                .and_then(|op| op.admission.as_ref())
                .and_then(|client| client.request())
        };
        match result {
            Some(result) => result.reply().await,
            None => learner_admission::Reply::Refused,
        }
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
    pub(super) async fn observe_gated_dispatch(&self) -> oneshot::Receiver<Option<SocketAddr>> {
        let transport = self.job.state().learner.as_ref().unwrap().transport.clone();
        transport.lock().await.observe_gated_dispatch()
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(super) async fn before_dispatch_gate(&self, arm: bool) {
        let transport = self.job.state().learner.as_ref().unwrap().transport.clone();
        let transport = transport.lock().await;
        if arm {
            transport.arm_before_dispatch();
        } else {
            transport.release_before_dispatch();
        }
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
        if !self.job.state().close_requested {
            drop(self.job.request_close());
        }
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

impl Job {
    pub(in crate::cluster::consensus) fn check_issuer_transfer(
        &self,
        claim: super::issuer_transfer::Claim,
        sender: crate::cluster::pki::verify::AuthenticatedPeer,
    ) -> Result<super::issuer_transfer::Offer, peer_transport::Refusal> {
        let refused = peer_transport::Refusal::Identity;
        let state = self.state();
        let (node, local, ..) = voter_context(&state).ok_or(refused)?;
        let op = state.learner.as_ref().ok_or(refused)?;
        let expected = &op.member.as_ref().ok_or(refused)?.expected;
        let node = node.try_lock().map_err(|_| peer_transport::Refusal::Busy)?;
        let membership = op.issuer_membership_view(&node);
        let (offer, _proof) = receiver_proof(&membership, expected, local.address, claim, sender)
            .map_err(|_| refused)?;
        Ok(offer)
    }

    pub(in crate::cluster::consensus) fn install_voter_custody(
        &self,
        claim: super::issuer_transfer::Claim,
        sender: crate::cluster::pki::verify::AuthenticatedPeer,
        channel: (&[u8; 32], &[u8], Vec<u8>),
    ) -> Result<(), peer_transport::Refusal> {
        use crate::cluster::pki::custody::Error;
        self.check_issuer_transfer(claim, sender)?;
        let refused = peer_transport::Refusal::Identity;
        let node = self.state().node.clone().ok_or(refused)?;
        let node = node.try_lock().map_err(|_| peer_transport::Refusal::Busy)?;
        let mut state = self.state();
        if state.close_requested {
            return Err(peer_transport::Refusal::Closed);
        }
        let State { owner, learner, .. } = &mut *state;
        let owner = owner.as_ref().ok_or(refused)?;
        let op = learner.as_mut().ok_or(refused)?;
        let membership = op.issuer_membership_view(&node);
        let address = op.advertised.ok_or(refused)?;
        let gate = op.gate.as_ref().ok_or(refused)?;
        let member = op.member.as_mut().ok_or(refused)?;
        let clock = op.clock.as_ref().ok_or(refused)?;
        let tls = member.credentials(owner, clock).map_err(|_| refused)?;
        let now = clock.sample_ms().map_err(|_| refused)?;
        let expected = &member.expected;
        let prove = || {
            // This gate acquires the store itself. Run it before taking the
            // guard that binds the exact offer to each custody operation.
            if gate.phase() != Ok(Phase::Strict) {
                return Err(Error::Ownership);
            }
            receiver_proof(&membership, expected, address, claim, sender)
                .map(|(_, proof)| proof)
                .map_err(|_| Error::Ownership)
        };
        member
            .custody
            .receive_voter_ca(
                owner,
                claim.cluster,
                tls.root.as_ref(),
                now,
                channel,
                (prove, || op.issuer_membership = Some(claim.membership)),
            )
            .map_err(|_| refused)
    }

    pub(super) async fn activate_promoted_issuer(&'static self) -> Result<(), Refusal> {
        let (node, ..) = voter_context(&self.state()).ok_or(Refusal::Identity)?;
        let node = node.try_lock().map_err(|_| Refusal::Ownership)?;
        // The unchanged quorum read establishes leadership before publishing readiness.
        let (epoch, ()) = node
            .fresh_read(|_| Ok(()))
            .await
            .map_err(|_| Refusal::Unavailable)?;
        self.publish_promoted_issuer(&node, epoch)
    }

    fn publish_promoted_issuer(
        &self,
        node: &ConsensusNode,
        epoch: LeaderId,
    ) -> Result<(), Refusal> {
        let mut state = self.state();
        let (_, local, clock) = voter_context(&state).ok_or(Refusal::Identity)?;
        if epoch.node_id != local.node.get() {
            return Err(Refusal::Identity);
        }
        let owner = state.owner.as_ref().ok_or(Refusal::Ownership)?;
        let endpoint =
            peer_transport::endpoint(&owner.plan.enrollment).map_err(|_| Refusal::Identity)?;
        if !peer_control_wire::qualified(endpoint) || state.issuer.is_some() {
            return Err(Refusal::Identity);
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
        let (ca, tls, installed_at, _proof) = load_promoted(&state, node, Some(epoch))?;
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
    fn issuer_membership_view<'a>(&self, node: &'a ConsensusNode) -> IssuerMembership<'a> {
        let membership = node.issuer_membership();
        #[cfg(test)]
        let membership = membership.with_sample(self.issuer_sample.clone());
        membership
    }
}

fn receiver_proof<'a>(
    membership: &IssuerMembership<'a>,
    expected: &ExpectedIdentity,
    address: SocketAddr,
    claim: super::issuer_transfer::Claim,
    sender: crate::cluster::pki::verify::AuthenticatedPeer,
) -> Result<(super::issuer_transfer::Offer, IssuerProof<'a>), Refusal> {
    let original = expected.origin.position();
    if claim.cluster != expected.cluster
        || claim.root != expected.ca
        || claim.receiver != (expected.node, *expected.key.as_bytes())
        || claim.membership.index <= original.index
        || claim.membership.term < original.term
    {
        return Err(Refusal::Identity);
    }
    let (offer, proof) = membership
        .offer_proof(claim, sender)
        .map_err(|_| Refusal::Identity)?;
    if offer.receiver_address != address || !proof.covers_member(expected, address) {
        return Err(Refusal::Identity);
    }
    Ok((offer, proof))
}

type VoterContext = (
    Arc<tokio::sync::Mutex<ConsensusNode>>,
    super::issuer_transfer::Voter,
    Arc<HostClock>,
);

fn voter_context(state: &State) -> Option<VoterContext> {
    if state.close_requested || state.status != Status::LearnerReady {
        return None;
    }
    let op = state.learner.as_ref()?;
    let member = op.member.as_ref()?;
    if op.gate.as_ref()?.phase().ok()? != Phase::Strict {
        return None;
    }
    Some((
        state.node.clone()?,
        super::issuer_transfer::Voter {
            node: member.expected.node,
            key: member.expected.key,
            address: op.advertised?,
        },
        op.clock.clone()?,
    ))
}

fn load_promoted<'a>(
    state: &State,
    node: &'a ConsensusNode,
    epoch: Option<LeaderId>,
) -> Result<
    (
        crate::cluster::pki::ca::ClusterCa,
        Credentials,
        LogPos,
        IssuerProof<'a>,
    ),
    Refusal,
> {
    use crate::cluster::pki::custody::Error;
    let (_, local, clock) = voter_context(state).ok_or(Refusal::Identity)?;
    let op = state.learner.as_ref().ok_or(Refusal::Identity)?;
    let installed_at = op.issuer_membership.ok_or(Refusal::Identity)?;
    let member = op.member.as_ref().ok_or(Refusal::Identity)?;
    let cluster = member.expected.cluster;
    let owner = state.owner.as_ref().ok_or(Refusal::Ownership)?;
    let tls = member.credentials(owner, &clock)?;
    let now = clock.sample_ms().map_err(|_| Refusal::Clock)?;
    let membership = op.issuer_membership_view(node);
    let (ca, proof) = member
        .custody
        .load_voter_ca(cluster, tls.root.as_ref(), now, || {
            let proof = membership
                .voter_proof(cluster, local, installed_at, epoch.is_some(), epoch)
                .map_err(|_| Error::Ownership)?;
            if !proof.covers_member(&member.expected, local.address) {
                return Err(Error::Ownership);
            }
            Ok(proof)
        })
        .map_err(|_| Refusal::Identity)?;
    Ok((ca, tls, installed_at, proof))
}

#[cfg(test)]
impl Run {
    pub(super) fn loaded_voter_root(&self) -> Result<Vec<u8>, Refusal> {
        let state = self.job.state();
        let (node, ..) = voter_context(&state).ok_or(Refusal::Identity)?;
        let node = node.try_lock().map_err(|_| Refusal::Ownership)?;
        let (ca, _, _, _proof) = load_promoted(&state, &node, None)?;
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
        owner.stage_published_join().unwrap();
        let member = Member {
            expected,
            leaf,
            custody,
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
            issuer_sample: Some(sample),
            recovered: false,
            fixture: None,
        });
        state.status = Status::LearnerReady;
        (job, credentials, gate)
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
        // Exercise the production readiness transition after a synthetic quorum epoch.
        assert_eq!(
            self.publish_promoted_issuer(&live, survivor.1).is_ok(),
            confirmed_at.is_some()
        );
        assert_eq!(self.state().issuer.is_some(), confirmed_at.is_some());

        let mut state = self.state();
        state.issuer.take();
        state.learner.take();
        state.node.take();
        state.owner.take();
    }
}
