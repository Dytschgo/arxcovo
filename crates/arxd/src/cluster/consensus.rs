//! Real OpenRaft boundary over durable local stores; no production ingress.
#![allow(dead_code)]
use super::{
    enrollment::{Command, Outcome, codec::Reader},
    log_store::StoredVote,
    state_store::{Entry as StoredEntry, LogPos, MembershipBlob, StoreError},
};
use openraft::{BasicNode, Entry, EntryPayload, LogId, Membership, Vote};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests;
openraft::declare_raft_types!(pub Arx: D = Command, R = Applied);
#[derive(Clone, Debug)]
pub struct Applied(pub Option<Outcome>);
type Id = LogId<openraft::impls::leader_id_adv::LeaderId<u64, u64>>;
type Members = Membership<u64, BasicNode>;
fn id(p: LogPos) -> Id {
    LogId::new(
        openraft::impls::leader_id_adv::LeaderId {
            term: p.term,
            node_id: p.leader,
        },
        p.index,
    )
}
fn pos(p: Id) -> LogPos {
    LogPos {
        term: p.leader_id.term,
        leader: p.leader_id.node_id,
        index: p.index,
    }
}
fn vote(v: StoredVote) -> Vote<openraft::impls::leader_id_adv::LeaderId<u64, u64>> {
    Vote {
        leader_id: openraft::impls::leader_id_adv::LeaderId {
            term: v.term,
            node_id: v.node,
        },
        committed: v.committed,
    }
}
fn stored_vote(v: Vote<openraft::impls::leader_id_adv::LeaderId<u64, u64>>) -> StoredVote {
    StoredVote {
        term: v.leader_id.term,
        node: v.leader_id.node_id,
        committed: v.committed,
    }
}
fn encode(m: &Members) -> Result<MembershipBlob, StoreError> {
    let configs = m.get_joint_config();
    let nodes: Vec<_> = m.nodes().collect();
    if !(1..=2).contains(&configs.len()) || !(1..=16).contains(&nodes.len()) {
        return Err(StoreError::Membership);
    }
    let mut b = vec![1, configs.len() as u8];
    for c in configs {
        if !(1..=16).contains(&c.len()) {
            return Err(StoreError::Membership);
        }
        b.push(c.len() as u8);
        for n in c {
            if *n == 0 || !nodes.iter().any(|(i, _)| *i == n) {
                return Err(StoreError::Membership);
            }
            b.extend(n.to_be_bytes());
        }
    }
    b.push(nodes.len() as u8);
    for (n, node) in nodes {
        let a = node.addr.as_bytes();
        if *n == 0
            || !(1..=200).contains(&a.len())
            || !a
                .iter()
                .all(|c| c.is_ascii_alphanumeric() || b"._:[]-".contains(c))
        {
            return Err(StoreError::Membership);
        }
        b.extend(n.to_be_bytes());
        b.push(a.len() as u8);
        b.extend(a);
    }
    MembershipBlob::new(b).map_err(|_| StoreError::Membership)
}
fn decode(b: &MembershipBlob) -> Result<Members, StoreError> {
    fn inner(b: &MembershipBlob) -> Result<Members, StoreError> {
        let mut r = Reader(b.bytes());
        if r.bytes(1)?[0] != 1 {
            return Err(StoreError::Membership);
        }
        let count = r.bytes(1)?[0];
        if !(1..=2).contains(&count) {
            return Err(StoreError::Membership);
        }
        let mut configs = Vec::new();
        for _ in 0..count {
            let n = r.bytes(1)?[0];
            if !(1..=16).contains(&n) {
                return Err(StoreError::Membership);
            }
            let mut c = BTreeSet::new();
            let mut prev = 0;
            for _ in 0..n {
                let v = r.u64()?;
                if v <= prev {
                    return Err(StoreError::Membership);
                }
                prev = v;
                c.insert(v);
            }
            configs.push(c);
        }
        let n = r.bytes(1)?[0];
        if !(1..=16).contains(&n) {
            return Err(StoreError::Membership);
        }
        let mut nodes = BTreeMap::new();
        let mut prev = 0;
        for _ in 0..n {
            let v = r.u64()?;
            if v <= prev {
                return Err(StoreError::Membership);
            }
            prev = v;
            let len = r.bytes(1)?[0] as usize;
            let a = std::str::from_utf8(r.bytes(len)?).map_err(|_| StoreError::Membership)?;
            nodes.insert(v, BasicNode::new(a));
        }
        if !r.0.is_empty() || configs.iter().flatten().any(|v| !nodes.contains_key(v)) {
            return Err(StoreError::Membership);
        }
        let m = Membership::new(configs, nodes).map_err(|_| StoreError::Membership)?;
        if encode(&m)?.bytes() != b.bytes() {
            return Err(StoreError::Membership);
        }
        Ok(m)
    }
    inner(b).map_err(|_| StoreError::Membership)
}
fn to_stored(
    e: openraft::type_config::alias::EntryOf<Arx>,
) -> Result<(LogPos, StoredEntry), StoreError> {
    Ok((
        pos(e.log_id),
        match e.payload {
            EntryPayload::Blank => StoredEntry::Blank,
            EntryPayload::Normal(c) => StoredEntry::Enrollment(c),
            EntryPayload::Membership(m) => StoredEntry::Membership(encode(&m)?),
        },
    ))
}
fn from_stored(
    (p, e): (LogPos, StoredEntry),
) -> Result<openraft::type_config::alias::EntryOf<Arx>, StoreError> {
    Ok(Entry {
        log_id: id(p),
        payload: match e {
            StoredEntry::Blank => EntryPayload::Blank,
            StoredEntry::Enrollment(c) => EntryPayload::Normal(c),
            StoredEntry::Membership(m) => EntryPayload::Membership(decode(&m)?),
        },
    })
}
