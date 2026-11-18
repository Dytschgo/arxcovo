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
struct Sample {
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
    let (position, blob) = membership.ok_or(retry)?;
    if applied.index < position.index || (applied.index == position.index && applied != *position) {
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
    state
        .authorize_peer(peer.0, peer.1, peer.2)
        .map_err(|_| MembershipReadError::PeerRefused)?;
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
