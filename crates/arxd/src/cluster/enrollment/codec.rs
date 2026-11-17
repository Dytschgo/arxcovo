//! Canonical bounded state section. SHA is corruption detection, never authentication.
use super::*;
use crate::cluster::state_store::StoreError;
use std::collections::BTreeSet;

pub(crate) struct Reader<'a>(pub(crate) &'a [u8]);
impl Reader<'_> {
    pub(crate) fn bytes(&mut self, n: usize) -> Result<&[u8], StoreError> {
        if n > self.0.len() {
            return Err(StoreError::Corrupt);
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    pub(crate) fn array<const N: usize>(&mut self) -> Result<[u8; N], StoreError> {
        self.bytes(N)?.try_into().map_err(|_| StoreError::Corrupt)
    }
    pub(crate) fn u64(&mut self) -> Result<u64, StoreError> {
        Ok(u64::from_be_bytes(self.array()?))
    }
    pub(crate) fn u16(&mut self) -> Result<u16, StoreError> {
        Ok(u16::from_be_bytes(self.array()?))
    }
    pub(crate) fn flag(&mut self) -> Result<bool, StoreError> {
        match self.bytes(1)?[0] {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(StoreError::Corrupt),
        }
    }
}
pub(crate) fn cluster(s: &EnrollmentState) -> ClusterId {
    s.cluster
}
pub(crate) fn encode(s: &EnrollmentState, b: &mut Vec<u8>) {
    b.extend(s.cluster);
    b.extend(s.next_node_id.get().to_be_bytes());
    b.extend(s.committed_ms.to_be_bytes());
    b.extend((s.records.len() as u16).to_be_bytes());
    for (id, r) in &s.records {
        b.extend(id);
        b.extend(r.actor);
        b.extend(r.request);
        b.extend(r.verifier.bytes());
        b.extend(r.expires_ms.to_be_bytes());
        b.push(u8::from(r.consumed.is_some()));
        if let Some(c) = &r.consumed {
            b.extend(c.request);
            b.extend(c.key.0);
            b.extend(c.node.get().to_be_bytes());
            b.extend(c.anchor.0);
            super::super::state_store::put_pos(b, c.origin);
            b.extend(c.original_applied_ms.to_be_bytes());
        }
    }
    b.push(u8::from(s.founded));
    b.push(s.members.len() as u8);
    for (n, k) in &s.members {
        b.extend(n.get().to_be_bytes());
        b.extend(k.0);
    }
}
pub(crate) fn decode(r: &mut Reader<'_>) -> Result<EnrollmentState, StoreError> {
    let cluster = r.array()?;
    let next_node_id = NodeId::new(r.u64()?).ok_or(StoreError::Corrupt)?;
    let committed_ms = r.u64()?;
    if committed_ms > MAX_TIME_MS {
        return Err(StoreError::Corrupt);
    }
    let count = r.u16()? as usize;
    if count > MAX_RECORDS || 4866 + count * 209 > 16384 {
        return Err(StoreError::Corrupt);
    }
    let mut records = BTreeMap::new();
    let mut owners = BTreeSet::new();
    let mut nodes = BTreeSet::new();
    let mut last = None;
    let mut pending = 0;
    for _ in 0..count {
        let id: CodeId = r.array()?;
        if last.is_some_and(|old| old >= id) {
            return Err(StoreError::Corrupt);
        }
        last = Some(id);
        let actor = r.array()?;
        let request = r.array()?;
        if !owners.insert((actor, request)) {
            return Err(StoreError::Corrupt);
        }
        let verifier = CodeVerifier::from_bytes(r.array()?);
        let expires_ms = r.u64()?;
        if !(CODE_TTL_MS..=committed_ms + CODE_TTL_MS).contains(&expires_ms)
            || expires_ms + TOMBSTONE_MS <= committed_ms
        {
            return Err(StoreError::Corrupt);
        }
        let consumed = if r.flag()? {
            let request = r.array()?;
            let key = KeyDigest(r.array()?);
            let node = NodeId::new(r.u64()?).ok_or(StoreError::Corrupt)?;
            if node >= next_node_id || !nodes.insert(node) {
                return Err(StoreError::Corrupt);
            }
            {
                let anchor = JoinAnchor(r.array()?);
                let origin = super::super::state_store::get_pos(r)?;
                let original_applied_ms = r.u64()?;
                if origin.index > i64::MAX as u64
                    || original_applied_ms > committed_ms
                    || original_applied_ms
                        .checked_add(2 * SKEW_MS)
                        .is_none_or(|t| t >= expires_ms)
                {
                    return Err(StoreError::Corrupt);
                }
                Some(Consumption {
                    request,
                    key,
                    node,
                    anchor,
                    origin,
                    original_applied_ms,
                })
            }
        } else {
            if committed_ms + 2 * SKEW_MS < expires_ms {
                pending += 1;
            }
            None
        };
        records.insert(
            id,
            CodeRecord {
                actor,
                request,
                verifier,
                expires_ms,
                consumed,
            },
        );
    }
    if pending > MAX_PENDING {
        return Err(StoreError::Corrupt);
    }
    let founded = r.flag()?;
    let count = r.bytes(1)?[0] as usize;
    if count > MAX_MEMBERS {
        return Err(StoreError::Corrupt);
    }
    let mut members = BTreeMap::new();
    let mut last = None;
    for _ in 0..count {
        let node = NodeId::new(r.u64()?).ok_or(StoreError::Corrupt)?;
        let key = KeyDigest(r.array()?);
        if node >= next_node_id
            || last.is_some_and(|n| n >= node)
            || members.values().any(|k| *k == key)
        {
            return Err(StoreError::Corrupt);
        }
        last = Some(node);
        members.insert(node, key);
    }
    if (founded && members.is_empty())
        || (!founded && (!members.is_empty() || !records.is_empty()))
        || records
            .values()
            .filter_map(|r| r.consumed.as_ref())
            .any(|c| members.get(&c.node).is_some_and(|k| *k != c.key))
    {
        return Err(StoreError::Corrupt);
    }
    Ok(EnrollmentState {
        founded,
        members,
        cluster,
        next_node_id,
        committed_ms,
        records,
    })
}

/// Persist only reduced verifiers. Times are validated by core apply, never this codec.
pub(crate) fn encode_command(c: &Command, b: &mut Vec<u8>) {
    match c {
        Command::Found {
            cluster,
            members,
            proposed_at_ms,
        } => {
            b.push(4);
            b.extend(cluster);
            b.extend(proposed_at_ms.to_be_bytes());
            b.push(members.as_slice().len() as u8);
            for (n, k) in members.as_slice() {
                b.extend(n.get().to_be_bytes());
                b.extend(k.0);
            }
        }
        Command::Remove {
            cluster,
            node,
            proposed_at_ms,
        } => {
            b.push(5);
            b.extend(cluster);
            b.extend(node.get().to_be_bytes());
            b.extend(proposed_at_ms.to_be_bytes());
        }
        Command::Mint {
            cluster,
            actor,
            request,
            code,
            verifier,
            proposed_at_ms,
        } => {
            b.push(1);
            b.extend(cluster);
            b.extend(actor);
            b.extend(request);
            b.extend(code);
            b.extend(verifier.bytes());
            b.extend(proposed_at_ms.to_be_bytes());
        }
        Command::Redeem {
            cluster,
            request,
            code,
            presented,
            key,
            anchor,
            proposed_at_ms,
        } => {
            b.push(6);
            b.extend(cluster);
            b.extend(request);
            b.extend(code);
            b.extend(presented.bytes());
            b.extend(key.0);
            b.extend(proposed_at_ms.to_be_bytes());
            b.extend(anchor.0);
        }
    }
}
pub(crate) fn decode_command(tag: u8, r: &mut Reader<'_>) -> Result<Command, StoreError> {
    Ok(match tag {
        4 => {
            let cluster = r.array()?;
            let proposed_at_ms = r.u64()?;
            let count = r.bytes(1)?[0] as usize;
            if count > MAX_FOUND_WIRE {
                return Err(StoreError::Corrupt);
            }
            let mut pairs = Vec::with_capacity(count);
            for _ in 0..count {
                pairs.push((
                    NodeId::new(r.u64()?).ok_or(StoreError::Corrupt)?,
                    KeyDigest(r.array()?),
                ));
            }
            Command::Found {
                cluster,
                proposed_at_ms,
                members: FoundMembers::new(pairs).map_err(|_| StoreError::Corrupt)?,
            }
        }
        5 => Command::Remove {
            cluster: r.array()?,
            node: NodeId::new(r.u64()?).ok_or(StoreError::Corrupt)?,
            proposed_at_ms: r.u64()?,
        },
        1 => Command::Mint {
            cluster: r.array()?,
            actor: r.array()?,
            request: r.array()?,
            code: r.array()?,
            verifier: CodeVerifier::from_bytes(r.array()?),
            proposed_at_ms: r.u64()?,
        },
        2 => return Err(StoreError::Unsupported),
        6 => Command::Redeem {
            cluster: r.array()?,
            request: r.array()?,
            code: r.array()?,
            presented: CodeVerifier::from_bytes(r.array()?),
            key: KeyDigest(r.array()?),
            proposed_at_ms: r.u64()?,
            anchor: JoinAnchor(r.array()?),
        },
        _ => return Err(StoreError::Corrupt),
    })
}

pub(crate) fn validate_origins(
    s: &EnrollmentState,
    applied: Option<LogPos>,
) -> Result<(), StoreError> {
    for c in s.records.values().filter_map(|r| r.consumed.as_ref()) {
        let a = applied.ok_or(StoreError::Corrupt)?;
        if c.origin.index > a.index
            || c.origin.term > a.term
            || (c.origin.index == a.index && c.origin != a)
        {
            return Err(StoreError::Corrupt);
        }
    }
    Ok(())
}
