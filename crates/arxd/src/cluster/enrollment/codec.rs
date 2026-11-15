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
        }
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
    if count > MAX_RECORDS {
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
            Some(Consumption { request, key, node })
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
    Ok(EnrollmentState {
        cluster,
        next_node_id,
        committed_ms,
        records,
    })
}

/// Persist only reduced verifiers. Times are validated by core apply, never this codec.
pub(crate) fn encode_command(c: &Command, b: &mut Vec<u8>) {
    match c {
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
            proposed_at_ms,
        } => {
            b.push(2);
            b.extend(cluster);
            b.extend(request);
            b.extend(code);
            b.extend(presented.bytes());
            b.extend(key.0);
            b.extend(proposed_at_ms.to_be_bytes());
        }
    }
}
pub(crate) fn decode_command(tag: u8, r: &mut Reader<'_>) -> Result<Command, StoreError> {
    Ok(match tag {
        1 => Command::Mint {
            cluster: r.array()?,
            actor: r.array()?,
            request: r.array()?,
            code: r.array()?,
            verifier: CodeVerifier::from_bytes(r.array()?),
            proposed_at_ms: r.u64()?,
        },
        2 => Command::Redeem {
            cluster: r.array()?,
            request: r.array()?,
            code: r.array()?,
            presented: CodeVerifier::from_bytes(r.array()?),
            key: KeyDigest(r.array()?),
            proposed_at_ms: r.u64()?,
        },
        _ => return Err(StoreError::Corrupt),
    })
}
