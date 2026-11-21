//! Guarded membership watermark, not atomic cross-store state or admission authority.
use super::*;
use crate::cluster::{
    enrollment::{ClusterId, EnrollmentState, KeyDigest, NodeId},
    pki::verify::AuthenticatedPeer,
};
use openraft::type_config::async_runtime::watch::WatchReceiver;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cluster) enum MembershipReadError {
    Busy,
    NotLeader,
    Unavailable,
    RetryLater,
    PeerRefused,
}
impl std::fmt::Display for MembershipReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Busy => "membership read busy",
            Self::NotLeader => "membership read not leader",
            Self::Unavailable => "membership read unavailable",
            Self::RetryLater => "membership view changed",
            Self::PeerRefused => "membership peer refused",
        })
    }
}
impl std::error::Error for MembershipReadError {}

pub(in crate::cluster) struct ObservedMembership {
    leader: LeaderId,
    membership_id: Id,
    membership: Members,
    applied: LogPos,
}
impl ObservedMembership {
    pub(in crate::cluster) fn leader(&self) -> LeaderId {
        self.leader
    }
    pub(in crate::cluster) fn membership_id(&self) -> Id {
        self.membership_id
    }
    pub(in crate::cluster) fn membership(&self) -> &Members {
        &self.membership
    }
    pub(in crate::cluster) fn applied(&self) -> LogPos {
        self.applied
    }
}

// No registry, store, TLS or permit ownership; validate bounds before cloning.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct Sample {
    vote: Vote<LeaderId>,
    current_leader: Option<u64>,
    effective: (Option<Id>, Members),
    committed: (Option<Id>, Members),
}
impl Sample {
    fn capture(raft: &Engine) -> Result<Self, MembershipReadError> {
        let metrics = raft.metrics();
        let m = metrics.borrow_watched();
        Self::from_metrics(&m)
    }
    fn from_metrics(m: &openraft::RaftMetrics<Arx>) -> Result<Self, MembershipReadError> {
        encode(m.membership_config.membership()).map_err(|_| MembershipReadError::Unavailable)?;
        encode(m.committed_membership_config.membership())
            .map_err(|_| MembershipReadError::Unavailable)?;
        Ok(Self {
            vote: m.vote,
            current_leader: m.current_leader,
            effective: (
                *m.membership_config.log_id(),
                m.membership_config.membership().clone(),
            ),
            committed: (
                *m.committed_membership_config.log_id(),
                m.committed_membership_config.membership().clone(),
            ),
        })
    }
}
type Peer = (ClusterId, NodeId, KeyDigest);
fn identity(peer: AuthenticatedPeer) -> Result<Peer, MembershipReadError> {
    peer.authenticated_identity()
        .map_err(|_| MembershipReadError::PeerRefused)
}

fn validate(
    local: u64,
    peer: Peer,
    sample: &Sample,
    state: &EnrollmentState,
    applied: Option<LogPos>,
    membership: Option<&(LogPos, MembershipBlob)>,
) -> Result<ObservedMembership, MembershipReadError> {
    let retry = MembershipReadError::RetryLater;
    if !sample.vote.committed
        || sample.vote.leader_id.node_id != local
        || sample.current_leader != Some(local)
    {
        return Err(MembershipReadError::NotLeader);
    }
    let applied = applied.ok_or(retry)?;
    let observed = uniform(local, sample, applied, membership)?;
    state
        .authorize_peer(peer.0, peer.1, peer.2)
        .map_err(|_| MembershipReadError::PeerRefused)?;
    Ok(observed)
}

fn uniform(
    local: u64,
    sample: &Sample,
    applied: LogPos,
    membership: Option<&(LogPos, MembershipBlob)>,
) -> Result<ObservedMembership, MembershipReadError> {
    let retry = MembershipReadError::RetryLater;
    let (position, blob) = membership.ok_or(retry)?;
    if !sample.vote.committed
        || sample.vote.leader_id.node_id != local
        || sample.current_leader != Some(local)
        || !super::learner_admission::covers(applied, *position)
    {
        return Err(retry);
    }
    let members = decode(blob).map_err(|_| MembershipReadError::Unavailable)?;
    let membership_id = id(*position);
    if members.get_joint_config().len() != 1
        || sample.effective.0 != Some(membership_id)
        || sample.committed.0 != Some(membership_id)
        || sample.effective.1 != members
        || sample.committed.1 != members
    {
        return Err(retry);
    }
    Ok(ObservedMembership {
        leader: sample.vote.leader_id,
        membership_id,
        membership: members,
        applied,
    })
}

fn finish(
    observed: ObservedMembership,
    established: LeaderId,
    before: &Sample,
    after: &Sample,
) -> Result<ObservedMembership, MembershipReadError> {
    if observed.leader != established || before != after {
        return Err(MembershipReadError::RetryLater);
    }
    Ok(observed)
}

impl ConsensusNode {
    /// Registry and applied membership come from one existing fresh-read store guard.
    /// Metrics checks bracket that read; later mutation still needs guarded submission
    /// and a post-apply authorization check. No perpetual authorization is granted.
    pub(in crate::cluster) async fn observe_membership(
        &self,
        peer: AuthenticatedPeer,
    ) -> Result<ObservedMembership, MembershipReadError> {
        let peer = identity(peer)?;
        let raft = self.raft.as_ref().ok_or(MembershipReadError::Unavailable)?;
        let before = Sample::capture(raft)?;
        let sample = before.clone();
        let local = self.id;
        let (leader, observed) = self
            .fresh_read(move |store| {
                Ok(validate(
                    local,
                    peer,
                    &sample,
                    store.state(),
                    store.applied(),
                    store.membership(),
                ))
            })
            .await
            .map_err(|e| match e {
                ReadError::Busy => MembershipReadError::Busy,
                ReadError::NotLeader => MembershipReadError::NotLeader,
                ReadError::NoQuorum | ReadError::Unavailable => MembershipReadError::Unavailable,
            })?;
        let observed = observed?;
        let after = Sample::capture(raft)?;
        finish(observed, leader, &before, &after)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::enrollment::FoundMembers;

    fn candidate(membership: Members) -> ObservedMembership {
        ObservedMembership {
            leader: id(position(1)).leader_id,
            membership_id: id(position(1)),
            membership,
            applied: position(5),
        }
    }

    #[test]
    fn admission_fresh_candidate_counts_revoked_slots_and_checks_exact_match() {
        let (_, _, _, peer) = fixture();
        let mut state = EnrollmentState::new(peer.0, node(4));
        assert!(matches!(
            state.apply(
                &Command::Found {
                    cluster: peer.0,
                    members: FoundMembers::new([
                        (node(1), key(1)),
                        (node(2), key(2)),
                        (node(3), key(3))
                    ])
                    .unwrap(),
                    proposed_at_ms: 100,
                },
                position(0),
                None,
            ),
            Outcome::Founded { replay: false }
        ));
        let address = "192.168.0.2:9000".parse().unwrap();
        let membership = members(&[1], &[(1, "192.168.0.1:9000"), (3, "192.168.0.3:9000")]);
        let mut progress = Progress {
            committed: position(4),
            tail: position(5),
            matched: BTreeMap::from([(
                3,
                Some(LogPos {
                    term: 3,
                    ..position(4)
                }),
            )]),
        };
        assert!(matches!(
            decision(
                candidate(membership.clone()),
                &state,
                peer,
                address,
                Some(&progress),
                None
            ),
            Err(MembershipReadError::RetryLater)
        ));
        progress.matched.insert(3, Some(position(4)));
        assert!(matches!(
            decision(candidate(membership.clone()), &state, peer, address, Some(&progress), None),
            Ok(AdmissionObservation::Add { leader, membership, tail })
                if leader == id(position(1)).leader_id && membership == id(position(1)) && tail == id(position(5))
        ));
        assert!(matches!(
            state.apply(
                &Command::Remove {
                    cluster: peer.0,
                    node: node(3),
                    proposed_at_ms: 101
                },
                position(2),
                None,
            ),
            Outcome::Removed { replay: false }
        ));
        progress.matched.clear();
        assert!(matches!(
            decision(
                candidate(membership),
                &state,
                peer,
                address,
                Some(&progress),
                None
            ),
            Ok(AdmissionObservation::Add { .. })
        ));
        let full = Members::new(
            vec![BTreeSet::from([1])],
            std::iter::once(1)
                .chain(3..=17)
                .map(|n| (n, BasicNode::new("192.168.0.1:9000")))
                .collect::<BTreeMap<_, _>>(),
        )
        .unwrap();
        assert!(matches!(
            decision(
                candidate(full),
                &state,
                peer,
                address,
                Some(&progress),
                None
            ),
            Err(MembershipReadError::RetryLater)
        ));
    }

    #[test]
    fn admission_reconciles_voter_address_and_distinguishes_late_revocation() {
        let (mut state, _, _, peer) = fixture();
        let address = "192.168.0.2:9000".parse().unwrap();
        let membership = members(&[1, 2], &[(1, "192.168.0.1:9000"), (2, "192.168.0.2:9000")]);
        assert!(matches!(
            decision(candidate(membership.clone()), &state, peer, address, None, None),
            Ok(AdmissionObservation::Answer(Reply::Applied(p, a))) if p == position(1) && a == address
        ));
        assert!(matches!(
            decision(
                candidate(membership.clone()),
                &state,
                peer,
                "192.168.0.2:9001".parse().unwrap(),
                None,
                None
            ),
            Ok(AdmissionObservation::Answer(Reply::AddressConflict))
        ));
        assert!(matches!(
            state.apply(
                &Command::Remove {
                    cluster: peer.0,
                    node: peer.1,
                    proposed_at_ms: 101
                },
                position(2),
                None,
            ),
            Outcome::Removed { replay: false }
        ));
        assert!(matches!(
            decision(
                candidate(membership.clone()),
                &state,
                peer,
                address,
                None,
                None
            ),
            Err(MembershipReadError::PeerRefused)
        ));
        assert!(matches!(
            decision(
                candidate(membership.clone()),
                &state,
                peer,
                address,
                None,
                Some(LogPos {
                    leader: 7,
                    ..position(1)
                })
            ),
            Err(MembershipReadError::RetryLater)
        ));
        assert!(matches!(
            decision(candidate(membership), &state, peer, address, None, Some(position(1))),
            Ok(AdmissionObservation::Answer(Reply::Revoked(p, a))) if p == position(1) && a == address
        ));
    }
    fn node(n: u64) -> NodeId {
        NodeId::new(n).unwrap()
    }
    fn key(n: u8) -> KeyDigest {
        KeyDigest::of_spki(&[n]).unwrap()
    }
    fn position(index: u64) -> LogPos {
        LogPos {
            term: 2,
            leader: 1,
            index,
        }
    }
    fn members(voters: &[u64], nodes: &[(u64, &str)]) -> Members {
        Members::new(
            vec![voters.iter().copied().collect()],
            nodes
                .iter()
                .map(|(n, a)| (*n, BasicNode::new(*a)))
                .collect::<BTreeMap<_, _>>(),
        )
        .unwrap()
    }
    fn fixture() -> (EnrollmentState, Sample, (LogPos, MembershipBlob), Peer) {
        let mut state = EnrollmentState::new([1; 16], node(4));
        let outcome = state.apply(
            &Command::Found {
                cluster: [1; 16],
                members: FoundMembers::new([(node(1), key(1)), (node(2), key(2))]).unwrap(),
                proposed_at_ms: 100,
            },
            position(0),
            None,
        );
        assert!(matches!(outcome, Outcome::Founded { replay: false }));
        let m = members(&[1], &[(1, "a"), (2, "b")]);
        let sample = Sample {
            vote: Vote {
                leader_id: id(position(1)).leader_id,
                committed: true,
            },
            current_leader: Some(1),
            effective: (Some(id(position(1))), m.clone()),
            committed: (Some(id(position(1))), m.clone()),
        };
        (
            state,
            sample,
            (position(1), encode(&m).unwrap()),
            ([1; 16], node(2), key(2)),
        )
    }
    fn checked(
        s: &EnrollmentState,
        m: &Sample,
        blob: &(LogPos, MembershipBlob),
        peer: Peer,
    ) -> Result<ObservedMembership, MembershipReadError> {
        validate(1, peer, m, s, Some(position(2)), Some(blob))
    }
    #[test]
    fn exact_uniform_observation_and_immutability() {
        let (s, m, b, p) = fixture();
        let saved = s.clone();
        let o = checked(&s, &m, &b, p).unwrap();
        assert!(o.leader() == m.vote.leader_id && o.membership_id() == id(b.0));
        assert!(o.membership() == &m.effective.1 && o.applied() == position(2));
        assert!(finish(o, m.vote.leader_id, &m, &m).is_ok());
        assert!(s == saved);
    }
    #[test]
    fn full_effective_and_committed_identity_and_contents() {
        let (s, m, b, p) = fixture();
        for effective in [true, false] {
            for change in 0..5 {
                let mut altered = m.clone();
                let view = if effective {
                    &mut altered.effective
                } else {
                    &mut altered.committed
                };
                match change {
                    0 => view.0 = Some(id(position(0))),
                    1 => view.0 = None,
                    2 => view.1 = members(&[1, 2], &[(1, "a"), (2, "b")]),
                    3 => view.1 = members(&[1], &[(1, "a"), (2, "b"), (3, "c")]),
                    4 => view.1 = members(&[1], &[(1, "a"), (2, "changed")]),
                    _ => unreachable!(),
                }
                assert!(checked(&s, &altered, &b, p).is_err());
            }
        }
    }
    #[test]
    fn missing_joint_and_watermark_refuse() {
        let (s, m, b, p) = fixture();
        assert!(validate(1, p, &m, &s, None, Some(&b)).is_err());
        assert!(validate(1, p, &m, &s, Some(position(2)), None).is_err());
        assert!(validate(1, p, &m, &s, Some(position(0)), Some(&b)).is_err());
        let wrong = LogPos {
            term: 3,
            ..position(1)
        };
        assert!(validate(1, p, &m, &s, Some(wrong), Some(&b)).is_err());
        assert!(validate(1, p, &m, &s, Some(position(1)), Some(&b)).is_ok());
        for last in [[1].into_iter().collect(), [1, 2].into_iter().collect()] {
            let mut altered = m.clone();
            let joint = Members::new::<BTreeMap<_, _>>(
                vec![[1].into(), last],
                [(1, BasicNode::new("a")), (2, BasicNode::new("b"))].into(),
            )
            .unwrap();
            altered.effective.1 = joint.clone();
            altered.committed.1 = joint.clone();
            assert!(checked(&s, &altered, &(b.0, encode(&joint).unwrap()), p).is_err());
        }
    }
    #[test]
    fn leader_epoch_and_bracket_changes_refuse() {
        let (s, m, b, p) = fixture();
        for change in 0..4 {
            let mut altered = m.clone();
            match change {
                0 => altered.vote.committed = false,
                1 => altered.vote.leader_id.node_id = 2,
                2 => altered.current_leader = None,
                3 => altered.current_leader = Some(2),
                _ => unreachable!(),
            }
            assert!(checked(&s, &altered, &b, p).is_err());
        }
        let mut epoch = m.vote.leader_id;
        epoch.term += 1;
        assert!(finish(checked(&s, &m, &b, p).unwrap(), epoch, &m, &m).is_err());
        for change in 0..4 {
            let mut after = m.clone();
            match change {
                0 => after.vote.leader_id.term += 1,
                1 => after.current_leader = Some(2),
                2 => after.effective.1 = members(&[1], &[(1, "a"), (2, "new")]),
                3 => after.committed.0 = Some(id(position(2))),
                _ => unreachable!(),
            }
            assert!(
                finish(
                    checked(&s, &m, &b, p).unwrap(),
                    m.vote.leader_id,
                    &m,
                    &after
                )
                .is_err()
            );
        }
    }
    #[test]
    fn real_registry_binding_and_removal_refuse() {
        let (mut s, m, b, p) = fixture();
        let before = s.clone();
        for peer in [([2; 16], p.1, p.2), (p.0, node(3), p.2), (p.0, p.1, key(3))] {
            assert!(matches!(
                checked(&s, &m, &b, peer),
                Err(MembershipReadError::PeerRefused)
            ));
        }
        assert!(s == before);
        assert!(matches!(
            s.apply(
                &Command::Remove {
                    cluster: p.0,
                    node: p.1,
                    proposed_at_ms: 101
                },
                position(2),
                None
            ),
            Outcome::Removed { replay: false }
        ));
        assert!(matches!(
            checked(&s, &m, &b, p),
            Err(MembershipReadError::PeerRefused)
        ));
    }
    #[test]
    fn assumed_peer_has_no_read_identity() {
        use futures_util::FutureExt;
        // No engine/store or read capacity exists: any later-path access would
        // fail differently. Poll the actual API once without a Tokio runtime.
        let (_, log_released) = crate::cluster::state_store::release_pair();
        let (_, sm_released) = crate::cluster::state_store::release_pair();
        let n = ConsensusNode {
            id: 1,
            read_permits: Arc::new(Semaphore::new(0)),
            issue_permits: Arc::new(Semaphore::new(0)),
            read_blocking: Arc::default(),
            raft: None,
            sm: None,
            log_released,
            sm_released,
            log_weak: Weak::new(),
            sm_weak: Weak::new(),
            permits: Arc::new(Semaphore::new(0)),
            owners: Mutex::default(),
        };
        assert!(matches!(
            n.observe_membership(AuthenticatedPeer::assume(node(2)))
                .now_or_never(),
            Some(Err(MembershipReadError::PeerRefused))
        ));
        assert_eq!(n.read_blocking.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(n.owners.lock().unwrap().set.is_empty());
    }
    #[test]
    fn bounded_metrics_projection_ignores_unrelated_progress() {
        let (_, sample, _, _) = fixture();
        let mut metrics = openraft::RaftMetrics::<Arx>::new_initial(1);
        metrics.vote = sample.vote;
        metrics.current_leader = sample.current_leader;
        metrics.membership_config = std::sync::Arc::new(openraft::StoredMembership::new(
            sample.effective.0,
            sample.effective.1.clone(),
        ));
        metrics.committed_membership_config = std::sync::Arc::new(openraft::StoredMembership::new(
            sample.committed.0,
            sample.committed.1.clone(),
        ));
        assert!(Sample::from_metrics(&metrics).unwrap() == sample);
        metrics.last_applied = Some(id(position(9)));
        metrics.last_log_index = Some(10);
        metrics.local_committed = Some(id(position(10)));
        assert!(Sample::from_metrics(&metrics).unwrap() == sample);
        for effective in [true, false] {
            let oversized = Members::new::<BTreeMap<_, _>>(
                vec![[1].into()],
                [(1, BasicNode::new("x".repeat(201)))].into(),
            )
            .unwrap();
            let mut altered = metrics.clone();
            let oversized = std::sync::Arc::new(openraft::StoredMembership::new(
                sample.effective.0,
                oversized,
            ));
            if effective {
                altered.membership_config = oversized;
            } else {
                altered.committed_membership_config = oversized;
            }
            assert!(matches!(
                Sample::from_metrics(&altered),
                Err(MembershipReadError::Unavailable)
            ));
        }
    }
}

use super::learner_admission::{Reply, covers};
use crate::cluster::enrollment::{MAX_MEMBERS, PeerRefusal};
use std::net::SocketAddr;

/// A bounded leader progress bracket. Only members in the sampled canonical
/// membership participate in the match comparison.
#[derive(Clone, PartialEq, Eq)]
struct Progress {
    committed: LogPos,
    tail: LogPos,
    matched: BTreeMap<u64, Option<LogPos>>,
}

impl Progress {
    fn capture(node: &ConsensusNode, sample: &Sample) -> Result<Self, MembershipReadError> {
        let unavailable = MembershipReadError::Unavailable;
        let raft = node.raft.as_ref().ok_or(unavailable)?;
        let metrics = raft.data_metrics();
        let progress = {
            let data = metrics.borrow_watched();
            let replication = data.replication.as_ref().ok_or(unavailable)?;
            Self {
                committed: pos(data.cluster_committed.ok_or(unavailable)?),
                tail: pos(data.last_log.ok_or(unavailable)?),
                matched: sample
                    .effective
                    .1
                    .non_write_voter_ids()
                    .map(|node| (node, replication.get(&node).copied().flatten().map(pos)))
                    .collect(),
            }
        };
        let log = node.log_weak.upgrade().ok_or(unavailable)?;
        let last = log
            .try_lock()
            .map_err(|_| unavailable)?
            .log_state()
            .map_err(|_| unavailable)?
            .last;
        if last != Some(progress.tail) || !covers(progress.tail, progress.committed) {
            return Err(MembershipReadError::RetryLater);
        }
        Ok(progress)
    }
}

pub(super) enum AdmissionObservation {
    Answer(Reply),
    Add {
        leader: LeaderId,
        membership: Id,
        tail: Id,
    },
}

fn decision(
    observed: ObservedMembership,
    state: &EnrollmentState,
    peer: Peer,
    address: SocketAddr,
    progress: Option<&Progress>,
    submitted: Option<LogPos>,
) -> Result<AdmissionObservation, MembershipReadError> {
    use AdmissionObservation::{Add, Answer};
    let retry = MembershipReadError::RetryLater;
    let binding = state.authorize_peer(peer.0, peer.1, peer.2);
    let watermark = pos(observed.membership_id);
    let node = peer.1.get();
    let member = observed.membership.get_node(&node);

    if let Some(submitted) = submitted {
        if !covers(watermark, submitted) || member.is_none_or(|m| m.addr != address.to_string()) {
            return Err(retry);
        }
        return match binding {
            Ok(()) => Ok(Answer(Reply::Applied(watermark, address))),
            Err(PeerRefusal::NotMember) => Ok(Answer(Reply::Revoked(watermark, address))),
            Err(_) => Err(MembershipReadError::PeerRefused),
        };
    }

    binding.map_err(|_| MembershipReadError::PeerRefused)?;
    if let Some(member) = member {
        return Ok(Answer(if member.addr == address.to_string() {
            Reply::Applied(watermark, address)
        } else {
            Reply::AddressConflict
        }));
    }

    let progress = progress.ok_or(retry)?;
    let probe = KeyDigest::of_spki(b"bounded admission registry presence").expect("bounded digest");
    if observed.membership.nodes().count() >= MAX_MEMBERS
        || !covers(observed.applied, progress.committed)
        || !covers(progress.committed, watermark)
        || observed.membership.non_write_voter_ids().any(|other| {
            let other_id = NodeId::new(other).expect("canonical membership node");
            let bound = matches!(
                state.authorize_peer(peer.0, other_id, probe),
                Ok(()) | Err(PeerRefusal::KeyMismatch)
            );
            bound
                && !progress
                    .matched
                    .get(&other)
                    .copied()
                    .flatten()
                    .is_some_and(|p| covers(p, progress.committed))
        })
    {
        return Err(retry);
    }
    Ok(Add {
        leader: observed.leader,
        membership: observed.membership_id,
        tail: id(progress.tail),
    })
}

impl ConsensusNode {
    pub(super) async fn admission_observation(
        &self,
        peer: AuthenticatedPeer,
        address: SocketAddr,
        submitted: Option<LogPos>,
    ) -> Result<AdmissionObservation, MembershipReadError> {
        let peer = identity(peer)?;
        let unavailable = MembershipReadError::Unavailable;
        let retry = MembershipReadError::RetryLater;
        let raft = self.raft.as_ref().ok_or(unavailable)?;
        let before = Sample::capture(raft)?;
        let progress =
            if submitted.is_none() && before.effective.1.get_node(&peer.1.get()).is_none() {
                Some(Progress::capture(self, &before)?)
            } else {
                None
            };
        let initial = before.clone();
        let initial_progress = progress.clone();
        let local = self.id;
        let (leader, result) = self
            .fresh_read(move |store| {
                if store.is_poisoned() {
                    return Ok(Err(unavailable));
                }
                Ok(store
                    .applied()
                    .ok_or(retry)
                    .and_then(|applied| uniform(local, &initial, applied, store.membership()))
                    .and_then(|observed| {
                        decision(
                            observed,
                            store.state(),
                            peer,
                            address,
                            initial_progress.as_ref(),
                            submitted,
                        )
                    }))
            })
            .await
            .map_err(|_| unavailable)?;
        if leader != before.vote.leader_id
            || Sample::capture(raft)? != before
            || progress.is_some_and(|p| Progress::capture(self, &before).ok() != Some(p))
        {
            return Err(retry);
        }
        result
    }
}

/// A bounded wake projection. The watcher treats this as a hint, then takes a
/// separate fresh observation under the shared membership intent.
#[derive(PartialEq, Eq)]
pub(super) struct PromotionTrigger {
    sample: Sample,
    committed: Id,
    applied: Id,
    last_log_index: Option<u64>,
    matched: BTreeMap<u64, Option<Id>>,
}

impl PromotionTrigger {
    pub(super) fn from_metrics(m: &openraft::RaftMetrics<Arx>) -> Option<Self> {
        if m.running_state.is_err() || !local_leader(m.id, m.vote, m.current_leader) {
            return None;
        }
        let sample = Sample::from_metrics(m).ok()?;
        let membership = &sample.effective.1;
        let committed = m.cluster_committed?;
        let applied = m.last_applied?;
        if sample.effective != sample.committed
            || !covers(pos(applied), pos(committed))
            || !covers(pos(committed), pos(sample.effective.0?))
            || committed.leader_id != m.vote.leader_id
        {
            return None;
        }
        let replication = m.replication.as_ref()?;
        let matched: BTreeMap<_, _> = membership
            .non_write_voter_ids()
            .map(|node| (node, replication.get(&node).copied().flatten()))
            .collect();
        if membership.get_joint_config().len() == 1
            && (!membership.voter_ids().eq([1])
                || matched
                    .values()
                    .filter(|matched| {
                        matched.is_some_and(|matched| covers(pos(matched), pos(committed)))
                    })
                    .count()
                    < 2)
        {
            return None;
        }
        Some(Self {
            sample,
            committed,
            applied,
            last_log_index: m.last_log_index,
            matched,
        })
    }
}

fn local_leader(local: u64, vote: Vote<LeaderId>, current_leader: Option<u64>) -> bool {
    vote.committed && vote.leader_id.node_id == local && current_leader == Some(local)
}

fn stable_promotion(
    leader: LeaderId,
    before: &Sample,
    before_progress: &Progress,
    after: &Sample,
    after_progress: &Progress,
) -> Result<(), MembershipReadError> {
    if leader != before.vote.leader_id || before != after || before_progress != after_progress {
        return Err(MembershipReadError::RetryLater);
    }
    Ok(())
}

pub(super) struct PromotionView {
    pub(super) leader: LeaderId,
    pub(super) watermark: Id,
    pub(super) members: Members,
    pub(super) committed: LogPos,
    pub(super) tail: Id,
    pub(super) matched: BTreeMap<u64, Option<LogPos>>,
    pub(super) bindings: BTreeMap<u64, KeyDigest>,
}

fn promotion_view(
    store: &StateStore,
    cluster: ClusterId,
    local: u64,
    sample: &Sample,
    progress: &Progress,
) -> Result<PromotionView, MembershipReadError> {
    let retry = MembershipReadError::RetryLater;
    if store.is_poisoned() {
        return Err(MembershipReadError::Unavailable);
    }
    let (position, blob) = store.membership().ok_or(retry)?;
    let watermark = id(*position);
    let members = decode(blob).map_err(|_| MembershipReadError::Unavailable)?;
    if !local_leader(local, sample.vote, sample.current_leader)
        || sample.effective.0 != Some(watermark)
        || sample.committed.0 != Some(watermark)
        || sample.effective.1 != members
        || sample.committed.1 != members
        || !store
            .applied()
            .is_some_and(|applied| covers(applied, progress.committed))
        || !covers(progress.committed, *position)
        || id(progress.committed).leader_id != sample.vote.leader_id
    {
        return Err(retry);
    }
    let mut bindings = BTreeMap::new();
    for (id, _) in members.nodes() {
        let node = NodeId::new(*id).ok_or(retry)?;
        match store.state().bound_key(cluster, node) {
            Ok(key) => {
                bindings.insert(*id, key);
            }
            Err(PeerRefusal::NotMember) => {}
            Err(_) => return Err(MembershipReadError::PeerRefused),
        }
    }
    Ok(PromotionView {
        leader: sample.vote.leader_id,
        watermark,
        members,
        committed: progress.committed,
        tail: id(progress.tail),
        matched: progress.matched.clone(),
        bindings,
    })
}

impl ConsensusNode {
    pub(super) async fn promotion_observation(
        &self,
        cluster: ClusterId,
    ) -> Result<PromotionView, MembershipReadError> {
        let unavailable = MembershipReadError::Unavailable;
        let raft = self.raft.as_ref().ok_or(unavailable)?;
        let before = Sample::capture(raft)?;
        if !local_leader(self.id, before.vote, before.current_leader) {
            return Err(MembershipReadError::NotLeader);
        }
        let progress = Progress::capture(self, &before)?;
        let initial = before.clone();
        let initial_progress = progress.clone();
        let local = self.id;
        let (leader, view) = self
            .fresh_read(move |store| {
                Ok(promotion_view(
                    store,
                    cluster,
                    local,
                    &initial,
                    &initial_progress,
                ))
            })
            .await
            .map_err(|error| match error {
                ReadError::Busy => MembershipReadError::Busy,
                ReadError::NotLeader => MembershipReadError::NotLeader,
                ReadError::NoQuorum | ReadError::Unavailable => unavailable,
            })?;
        let after = Sample::capture(raft)?;
        let after_progress = Progress::capture(self, &after)?;
        stable_promotion(leader, &before, &progress, &after, &after_progress)?;
        view
    }
}

#[cfg(test)]
mod promotion_observation_tests {
    use super::*;

    #[test]
    fn lost_epoch_or_changed_full_tail_refuses_submission_view() {
        let leader = id(LogPos {
            term: 2,
            leader: 1,
            index: 8,
        })
        .leader_id;
        let members = Members::new(
            vec![[1].into()],
            BTreeMap::from([(1, BasicNode::new("192.168.0.1:9000"))]),
        )
        .unwrap();
        let mut sample = Sample {
            vote: Vote {
                committed: true,
                leader_id: leader,
            },
            current_leader: Some(1),
            effective: (None, members.clone()),
            committed: (None, members),
        };
        let progress = Progress {
            committed: LogPos {
                term: 2,
                leader: 1,
                index: 8,
            },
            tail: LogPos {
                term: 2,
                leader: 1,
                index: 9,
            },
            matched: BTreeMap::new(),
        };
        assert!(stable_promotion(leader, &sample, &progress, &sample, &progress).is_ok());
        let changed_tail = Progress {
            tail: LogPos {
                term: 3,
                ..progress.tail
            },
            ..progress.clone()
        };
        assert_eq!(
            stable_promotion(leader, &sample, &progress, &sample, &changed_tail),
            Err(MembershipReadError::RetryLater),
        );
        let before = sample.clone();
        sample.current_leader = Some(3);
        assert_eq!(
            stable_promotion(leader, &before, &progress, &sample, &progress),
            Err(MembershipReadError::RetryLater),
        );
        assert_eq!(
            stable_promotion(
                openraft::impls::leader_id_adv::LeaderId {
                    term: 3,
                    node_id: 1
                },
                &before,
                &progress,
                &before,
                &progress
            ),
            Err(MembershipReadError::RetryLater),
        );
    }
}

#[cfg(target_os = "linux")]
mod issuer_proof {
    use super::*;
    use crate::cluster::consensus::issuer_transfer::{
        Claim, Offer, Voter, bound_voter, promoted_targets,
    };

    fn voter(
        store: &StateStore,
        view: &ObservedMembership,
        cluster: ClusterId,
        node: NodeId,
    ) -> Result<Voter, MembershipReadError> {
        bound_voter(view.membership(), store.state(), cluster, node)
            .ok_or(MembershipReadError::PeerRefused)
    }

    // Mirrors peer_registry::observe_store without waiting: a held store is
    // Busy, while an absent store or a poisoned mutex is Unavailable.
    fn acquire(
        node: &ConsensusNode,
    ) -> Result<std::sync::MutexGuard<'_, StateStore>, MembershipReadError> {
        let sm = node.sm.as_ref().ok_or(MembershipReadError::Unavailable)?;
        match sm.0.try_lock() {
            Ok(store) => Ok(store),
            Err(std::sync::TryLockError::WouldBlock) if !sm.0.is_poisoned() => {
                Err(MembershipReadError::Busy)
            }
            Err(_) => Err(MembershipReadError::Unavailable),
        }
    }

    // A failed persist keeps the last good image readable. Refuse it before
    // any applied field authorizes an issuer operation.
    fn locked(
        store: &StateStore,
        sample: &Sample,
        leader: u64,
    ) -> Result<ObservedMembership, MembershipReadError> {
        if store.is_poisoned() {
            return Err(MembershipReadError::Unavailable);
        }
        uniform(
            leader,
            sample,
            store.applied().ok_or(MembershipReadError::RetryLater)?,
            store.membership(),
        )
    }

    fn offer(
        store: &StateStore,
        view: &ObservedMembership,
        cluster: ClusterId,
        root: [u8; 32],
        sender: NodeId,
        target: NodeId,
    ) -> Result<Offer, MembershipReadError> {
        let membership = pos(view.membership_id());
        if promoted_targets(view.membership(), sender)
            .is_none_or(|voters| !voters.contains(&target.get()))
        {
            return Err(MembershipReadError::PeerRefused);
        }
        let sender = voter(store, view, cluster, sender)?;
        let target = voter(store, view, cluster, target)?;
        Ok(Offer {
            claim: Claim {
                cluster,
                root,
                sender: (sender.node, *sender.key.as_bytes()),
                receiver: (target.node, *target.key.as_bytes()),
                epoch: view.leader(),
                membership,
            },
            sender_address: sender.address,
            receiver_address: target.address,
        })
    }

    impl ConsensusNode {
        pub(in crate::cluster::consensus) fn issuer_offer(
            &self,
            cluster: ClusterId,
            root: [u8; 32],
            target: NodeId,
        ) -> Result<
            impl std::future::Future<Output = Result<Offer, MembershipReadError>> + use<>,
            MembershipReadError,
        > {
            let raft = self.raft.as_ref().ok_or(MembershipReadError::Unavailable)?;
            let before = Sample::capture(raft)?;
            let raft = raft.clone();
            let sample = before.clone();
            let leader = self.id;
            if before.current_leader != Some(leader) {
                return Err(MembershipReadError::NotLeader);
            }
            let sender = NodeId::new(self.id).ok_or(MembershipReadError::PeerRefused)?;
            let read = self
                .start_fresh_read(move |store| {
                    Ok(locked(store, &sample, sender.get())
                        .and_then(|view| offer(store, &view, cluster, root, sender, target)))
                })
                .map_err(issuer_read_error)?;
            Ok(async move {
                let (epoch, outcome) = read.await.map_err(issuer_read_error)?;
                let offer = outcome?;
                let after = Sample::capture(&raft)?;
                if after.current_leader != Some(leader) {
                    return Err(MembershipReadError::NotLeader);
                }
                if offer.claim.epoch != epoch || after != before {
                    return Err(MembershipReadError::RetryLater);
                }
                Ok(offer)
            })
        }

        pub(in crate::cluster::consensus) fn recheck_issuer_offer(
            &self,
            previous: Offer,
        ) -> Result<(), MembershipReadError> {
            let claim = previous.claim;
            self.with_issuer_view(self.id, |store, view| {
                let current = offer(
                    store,
                    view,
                    claim.cluster,
                    claim.root,
                    claim.sender.0,
                    claim.receiver.0,
                )?;
                if current != previous {
                    return Err(MembershipReadError::PeerRefused);
                }
                Ok(())
            })
        }

        pub(in crate::cluster::consensus) fn issuer_membership(&self) -> IssuerMembership<'_> {
            IssuerMembership {
                node: self,
                #[cfg(test)]
                sample: None,
                #[cfg(test)]
                before_proof: None,
            }
        }

        pub(in crate::cluster::consensus) fn issuer_voter(
            &self,
            cluster: ClusterId,
            local: Voter,
            installed_at: LogPos,
            require_leader: bool,
        ) -> Result<(), MembershipReadError> {
            drop(self.issuer_membership().voter_proof(
                cluster,
                local,
                installed_at,
                require_leader,
                None,
            )?);
            Ok(())
        }

        fn with_issuer_view<T>(
            &self,
            leader: u64,
            inspect: impl FnOnce(&StateStore, &ObservedMembership) -> Result<T, MembershipReadError>,
        ) -> Result<T, MembershipReadError> {
            let unavailable = MembershipReadError::Unavailable;
            let raft = self.raft.as_ref().ok_or(unavailable)?;
            let before = Sample::capture(raft)?;
            if before.current_leader != Some(leader) {
                return Err(MembershipReadError::NotLeader);
            }
            let store = acquire(self)?;
            let view = locked(&store, &before, leader)?;
            let result = inspect(&store, &view)?;
            let after = Sample::capture(raft)?;
            if after.current_leader != Some(leader) {
                return Err(MembershipReadError::NotLeader);
            }
            if after != before {
                return Err(MembershipReadError::RetryLater);
            }
            Ok(result)
        }
    }

    fn issuer_read_error(error: ReadError) -> MembershipReadError {
        match error {
            ReadError::Busy => MembershipReadError::Busy,
            ReadError::NotLeader => MembershipReadError::NotLeader,
            _ => MembershipReadError::Unavailable,
        }
    }
    #[cfg(test)]
    pub(in crate::cluster::consensus) type BeforeProof = Arc<dyn Fn() + Send + Sync>;

    pub(in crate::cluster::consensus) struct IssuerMembership<'a> {
        node: &'a ConsensusNode,
        #[cfg(test)]
        sample: Option<Sample>,
        #[cfg(test)]
        before_proof: Option<BeforeProof>,
    }

    // Retain the exact applied image until the guarded use has finished.
    #[must_use]
    pub(in crate::cluster::consensus) struct IssuerProof<'a>(std::sync::MutexGuard<'a, StateStore>);

    impl IssuerProof<'_> {
        pub(in crate::cluster::consensus) fn covers_member(
            &self,
            expected: &crate::cluster::join_receipt::ExpectedIdentity,
            address: std::net::SocketAddr,
        ) -> bool {
            let store = &self.0;
            store.applied().is_some_and(|applied| {
                super::super::peer_registry::crossed(
                    expected,
                    address,
                    applied,
                    store.membership(),
                    store.state(),
                )
            })
        }
    }

    impl<'a> IssuerMembership<'a> {
        #[cfg(test)]
        pub(in crate::cluster::consensus) fn with_sample(mut self, sample: Option<Sample>) -> Self {
            self.sample = sample;
            self
        }

        // Runs after the preliminary checks and before the final store observation.
        #[cfg(test)]
        pub(in crate::cluster::consensus) fn with_before_proof(
            mut self,
            hook: Option<BeforeProof>,
        ) -> Self {
            self.before_proof = hook;
            self
        }

        fn sample(&self) -> Result<Sample, MembershipReadError> {
            #[cfg(test)]
            if let Some(sample) = &self.sample {
                return Ok(sample.clone());
            }
            Sample::capture(
                self.node
                    .raft
                    .as_ref()
                    .ok_or(MembershipReadError::Unavailable)?,
            )
        }

        fn lock_view(
            &self,
            before: &Sample,
            leader: u64,
        ) -> Result<(IssuerProof<'a>, ObservedMembership), MembershipReadError> {
            let store = acquire(self.node)?;
            let view = locked(&store, before, leader)?;
            Ok((IssuerProof(store), view))
        }

        pub(in crate::cluster::consensus) fn offer_in(
            &self,
            store: &StateStore,
            claim: Claim,
            peer: AuthenticatedPeer,
        ) -> Result<Offer, MembershipReadError> {
            let refused = MembershipReadError::PeerRefused;
            let peer = identity(peer)?;
            if (peer.0, peer.1, *peer.2.as_bytes())
                != (claim.cluster, claim.sender.0, claim.sender.1)
                || self.node.id != claim.receiver.0.get()
            {
                return Err(refused);
            }

            let before = self.sample()?;
            let view = locked(store, &before, claim.sender.0.get())?;
            let observed = offer(
                store,
                &view,
                claim.cluster,
                claim.root,
                claim.sender.0,
                claim.receiver.0,
            )?;
            if observed.claim != claim {
                return Err(refused);
            }
            if self.sample()? != before {
                return Err(MembershipReadError::RetryLater);
            }
            Ok(observed)
        }

        pub(in crate::cluster::consensus) fn voter_proof(
            &self,
            cluster: ClusterId,
            local: Voter,
            installed_at: LogPos,
            require_leader: bool,
            epoch: Option<LeaderId>,
        ) -> Result<IssuerProof<'a>, MembershipReadError> {
            let before = self.sample()?;
            let leader = before
                .current_leader
                .ok_or(MembershipReadError::NotLeader)?;
            if local.node.get() != self.node.id || (require_leader && leader != self.node.id) {
                return Err(MembershipReadError::NotLeader);
            }
            #[cfg(test)]
            if let Some(hook) = &self.before_proof {
                hook();
            }
            let (proof, view) = self.lock_view(&before, leader)?;
            if pos(view.membership_id()) != installed_at
                || epoch.is_some_and(|epoch| epoch != view.leader())
                || voter(&proof.0, &view, cluster, local.node)? != local
            {
                return Err(MembershipReadError::PeerRefused);
            }
            if self.sample()? != before {
                return Err(MembershipReadError::RetryLater);
            }
            Ok(proof)
        }
    }

    #[cfg(test)]
    impl Sample {
        pub(in crate::cluster::consensus) fn issuer_fixture(
            epoch: LeaderId,
            position: Id,
            members: Members,
        ) -> Self {
            Self {
                vote: Vote {
                    leader_id: epoch,
                    committed: true,
                },
                current_leader: Some(epoch.node_id),
                effective: (Some(position), members.clone()),
                committed: (Some(position), members),
            }
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
pub(super) use issuer_proof::BeforeProof;
#[cfg(target_os = "linux")]
pub(super) use issuer_proof::{IssuerMembership, IssuerProof};
