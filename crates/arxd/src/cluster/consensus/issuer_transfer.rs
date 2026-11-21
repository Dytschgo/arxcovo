//! Identity and membership context for the one-session voter custody handoff.
#![cfg(any(test, target_os = "linux"))]
use super::{LeaderId, LogPos};
use crate::cluster::enrollment::{ClusterId, NodeId};
use crate::cluster::enrollment::{EnrollmentState, KeyDigest};
use std::net::SocketAddr;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) struct Voter {
    pub(super) node: NodeId,
    pub(super) key: KeyDigest,
    pub(super) address: SocketAddr,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) struct Claim {
    pub(super) cluster: ClusterId,
    pub(super) root: [u8; 32],
    pub(super) sender: (NodeId, [u8; 32]),
    pub(super) receiver: (NodeId, [u8; 32]),
    pub(super) epoch: LeaderId,
    pub(super) membership: LogPos,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) struct Offer {
    pub(super) claim: Claim,
    pub(super) sender_address: SocketAddr,
    pub(super) receiver_address: SocketAddr,
}

pub(super) const CLAIM_LEN: usize = 160;
pub(super) const CHALLENGE_LEN: usize = 32;

impl Claim {
    pub(super) fn encode(self) -> [u8; CLAIM_LEN] {
        let mut out = [0; CLAIM_LEN];
        out[..16].copy_from_slice(&self.cluster);
        out[16..48].copy_from_slice(&self.root);
        out[48..56].copy_from_slice(&self.sender.0.get().to_be_bytes());
        out[56..88].copy_from_slice(&self.sender.1);
        out[88..96].copy_from_slice(&self.receiver.0.get().to_be_bytes());
        out[96..128].copy_from_slice(&self.receiver.1);
        out[128..136].copy_from_slice(&self.epoch.term.to_be_bytes());
        out[136..144].copy_from_slice(&self.membership.term.to_be_bytes());
        out[144..152].copy_from_slice(&self.membership.leader.to_be_bytes());
        out[152..160].copy_from_slice(&self.membership.index.to_be_bytes());
        out
    }

    pub(super) fn decode(bytes: [u8; CLAIM_LEN]) -> Option<Self> {
        let word = |offset| u64::from_be_bytes(bytes[offset..offset + 8].try_into().unwrap());
        let sender = NodeId::new(word(48))?;
        let receiver = NodeId::new(word(88))?;
        let epoch = LeaderId {
            node_id: sender.get(),
            term: word(128),
        };
        let membership = LogPos {
            term: word(136),
            leader: word(144),
            index: word(152),
        };
        if sender == receiver
            || membership.term == 0
            || membership.leader == 0
            || membership.index == 0
            || epoch.term < membership.term
        {
            return None;
        }
        Some(Self {
            cluster: bytes[..16].try_into().unwrap(),
            root: bytes[16..48].try_into().unwrap(),
            sender: (sender, bytes[56..88].try_into().unwrap()),
            receiver: (receiver, bytes[96..128].try_into().unwrap()),
            epoch,
            membership,
        })
    }

    pub(super) fn transcript(
        self,
        challenge: &[u8; CHALLENGE_LEN],
    ) -> [u8; CLAIM_LEN + CHALLENGE_LEN] {
        let mut transcript = [0; CLAIM_LEN + CHALLENGE_LEN];
        transcript[..CLAIM_LEN].copy_from_slice(&self.encode());
        transcript[CLAIM_LEN..].copy_from_slice(challenge);
        transcript
    }
}

#[cfg(test)]
mod tests;

/// Step 11 creates a single uniform set of three voters. The original issuer
/// has no handoff target in the original, learner-only or joint configurations.
pub(super) fn promoted_targets(members: &super::Members, issuer: NodeId) -> Option<[u64; 2]> {
    let [voters] = members.get_joint_config().as_slice() else {
        return None;
    };
    if voters.len() != 3 || issuer.get() != 1 || !voters.contains(&issuer.get()) {
        return None;
    }
    let mut targets = voters.iter().copied().filter(|id| *id != issuer.get());
    Some([targets.next()?, targets.next()?])
}

pub(super) fn bound_voter(
    members: &super::Members,
    state: &EnrollmentState,
    cluster: ClusterId,
    node: NodeId,
) -> Option<Voter> {
    if !members.voter_ids().any(|id| id == node.get()) {
        return None;
    }
    let address = super::peer_transport::endpoint(&members.get_node(&node.get())?.addr).ok()?;
    if !super::peer_control_wire::qualified(address) {
        return None;
    }
    let key = state.bound_key(cluster, node).ok()?;
    Some(Voter { node, key, address })
}
