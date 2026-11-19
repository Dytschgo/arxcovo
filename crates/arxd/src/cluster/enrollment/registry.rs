//! Durable bindings; authorization observes local apply only, with unbounded staleness.
use super::*;
pub const MAX_MEMBERS: usize = 16;
pub const MAX_FOUND_WIRE: usize = 64;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoundMembers(Vec<(NodeId, KeyDigest)>);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FoundTooLarge;
impl FoundMembers {
    pub fn new(
        pairs: impl IntoIterator<Item = (NodeId, KeyDigest)>,
    ) -> Result<Self, FoundTooLarge> {
        let mut members = Vec::new();
        for pair in pairs.into_iter().take(MAX_FOUND_WIRE + 1) {
            if members.len() == MAX_FOUND_WIRE {
                return Err(FoundTooLarge);
            }
            members.push(pair);
        }
        Ok(Self(members))
    }
    pub fn as_slice(&self) -> &[(NodeId, KeyDigest)] {
        &self.0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerRefusal {
    WrongCluster,
    NotMember,
    KeyMismatch,
}
impl std::fmt::Display for PeerRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::WrongCluster => "wrong cluster",
            Self::NotMember => "not member",
            Self::KeyMismatch => "key mismatch",
        })
    }
}
impl EnrollmentState {
    pub(in crate::cluster) fn committed_ms(&self) -> u64 {
        self.committed_ms
    }
    pub(in crate::cluster) fn founded(&self) -> bool {
        self.founded
    }
    pub(in crate::cluster) fn next_node_id(&self) -> NodeId {
        self.next_node_id
    }

    /// Local applied state only. Insufficient for production control/execution under isolation.
    pub fn authorize_peer(
        &self,
        cluster: ClusterId,
        node: NodeId,
        key: KeyDigest,
    ) -> Result<(), PeerRefusal> {
        if cluster != self.cluster {
            return Err(PeerRefusal::WrongCluster);
        }
        match self.members.get(&node) {
            None => Err(PeerRefusal::NotMember),
            Some(k) if *k != key => Err(PeerRefusal::KeyMismatch),
            Some(_) => Ok(()),
        }
    }
    pub(super) fn found(&mut self, pairs: &FoundMembers) -> Outcome {
        let pairs = pairs.as_slice();
        if !(1..=MAX_MEMBERS).contains(&pairs.len())
            || pairs.windows(2).any(|w| w[0].0 >= w[1].0)
            || pairs.iter().any(|(n, _)| *n >= self.next_node_id)
            || pairs
                .iter()
                .enumerate()
                .any(|(i, (_, k))| pairs[..i].iter().any(|(_, old)| old == k))
        {
            return Outcome::Rejected(Reject::BadFounders);
        }
        if self.founded {
            return if pairs.iter().all(|(n, k)| self.members.get(n) == Some(k)) {
                Outcome::Founded { replay: true }
            } else {
                Outcome::Rejected(Reject::AlreadyFounded)
            };
        }
        self.members.extend(pairs.iter().copied());
        self.founded = true;
        Outcome::Founded { replay: false }
    }
    pub(super) fn remove(&mut self, node: NodeId) -> Outcome {
        if node >= self.next_node_id {
            return Outcome::Rejected(Reject::UnknownNode);
        }
        if !self.members.contains_key(&node) {
            return Outcome::Removed { replay: true };
        }
        if self.members.len() == 1 {
            return Outcome::Rejected(Reject::LastMember);
        }
        self.members.remove(&node);
        Outcome::Removed { replay: false }
    }
}

#[cfg(test)]
mod startup_floor_tests {
    use super::*;
    #[test]
    fn committed_floor_reads_actual_apply_without_mutation() {
        let mut s = EnrollmentState::new([1; 16], NodeId::new(2).unwrap());
        assert_eq!(s.committed_ms(), 0);
        let command = Command::Found {
            cluster: [1; 16],
            members: FoundMembers::new([(
                NodeId::new(1).unwrap(),
                KeyDigest::of_spki(b"synthetic").unwrap(),
            )])
            .unwrap(),
            proposed_at_ms: 8765,
        };
        s.apply(
            &command,
            LogPos {
                term: 1,
                leader: 1,
                index: 1,
            },
            None,
        );
        let before = s.clone();
        assert_eq!(s.committed_ms(), 8765);
        assert_eq!(s, before);
    }
}
