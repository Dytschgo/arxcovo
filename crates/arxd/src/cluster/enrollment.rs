//! Proposal-time enrollment and a private bootstrap transport. Neither proves consensus commit.
mod anchor;
pub(in crate::cluster) use anchor::BoundVoters;
mod code;
mod registry;
#[allow(dead_code)] // Owner paste and locally qualified routes are wired in later steps.
pub(super) mod transfer;
// Compiled for 7a verification. A live backend is a separate integration.
#[cfg(target_os = "linux")]
pub(super) mod durable_join;
#[allow(dead_code)]
pub(super) mod session;
pub use registry::{FoundMembers, FoundTooLarge, MAX_FOUND_WIRE, MAX_MEMBERS, PeerRefusal};
pub(super) mod codec;
use super::{membership::Membership, state_store::LogPos};
pub use code::{CodeError, CodeVerifier, JoinCode, JoinSecret, RedeemProof};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, num::NonZeroU64};
use subtle::ConstantTimeEq;

pub type ClusterId = [u8; 16];
pub type CodeId = [u8; 16];
pub type RequestId = [u8; 16];
pub type ActorId = [u8; 16];
pub type CaFingerprint = [u8; 32];
pub type NodeId = NonZeroU64;
pub const CODE_TTL_MS: u64 = 900_000;
pub const SKEW_MS: u64 = 30_000;
pub const TOMBSTONE_MS: u64 = 86_400_000;
pub const MAX_PENDING: usize = 8;
pub const MAX_RECORDS: usize = 64;
pub const MAX_CODE_TEXT: usize = 4096;
pub const MAX_SPKI: usize = 2048;
pub const MAX_TIME_MS: u64 = 253_402_300_799_999;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JoinAnchor([u8; 32]);
impl JoinAnchor {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyDigest([u8; 32]);
impl KeyDigest {
    /// Hash exact caller-canonical SPKI DER bytes; does not parse or verify possession.
    pub fn of_spki(bytes: &[u8]) -> Result<Self, CodeError> {
        if bytes.is_empty() || bytes.len() > MAX_SPKI {
            return Err(CodeError::Malformed);
        }
        Ok(Self(Sha256::digest(bytes).into()))
    }
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Found {
        cluster: ClusterId,
        members: FoundMembers,
        proposed_at_ms: u64,
    },
    Remove {
        cluster: ClusterId,
        node: NodeId,
        proposed_at_ms: u64,
    },
    /// Propose only from a fresh in-memory JoinCode::generate value. Retry that
    /// value only before first_proposal + CODE_TTL_MS - 4*SKEW_MS, then discard.
    /// Never re-propose a Mint decoded from storage/wire or accept a peer verifier.
    Mint {
        cluster: ClusterId,
        actor: ActorId,
        request: RequestId,
        code: CodeId,
        verifier: CodeVerifier,
        proposed_at_ms: u64,
    },
    Redeem {
        cluster: ClusterId,
        request: RequestId,
        code: CodeId,
        presented: CodeVerifier,
        key: KeyDigest,
        anchor: JoinAnchor,
        proposed_at_ms: u64,
    },
}
impl Command {
    /// Reduce the bearer proof before proposing or forwarding. Future ingress must accept
    /// proofs only, never client verifiers. Committed members have trusted cluster authority;
    /// apply's verifier comparison is consistency checking, not member authentication.
    pub fn redeem(
        cluster: ClusterId,
        request: RequestId,
        code: CodeId,
        proof: &RedeemProof,
        key: KeyDigest,
        anchor: JoinAnchor,
        proposed_at_ms: u64,
    ) -> Self {
        Self::Redeem {
            cluster,
            request,
            code,
            presented: CodeVerifier::of_proof(proof),
            key,
            anchor,
            proposed_at_ms,
        }
    }
}

// Keep a receipt-only copy of the actual proposal until its original expiry
// is known. No encoded command or verifier buffer is needed for the hash.
#[cfg(target_os = "linux")]
pub(in crate::cluster) struct MintReceiptInput(Command);

#[cfg(target_os = "linux")]
impl MintReceiptInput {
    pub(in crate::cluster) fn new(mint: &Command) -> Self {
        Self(mint.clone())
    }

    pub(in crate::cluster) fn digest(&self, pos: LogPos, expires_ms: u64) -> Option<[u8; 32]> {
        mint_digest(&self.0, pos, expires_ms)
    }
}

#[cfg(target_os = "linux")]
impl Drop for MintReceiptInput {
    fn drop(&mut self) {
        if let Command::Mint { verifier, .. } = &mut self.0 {
            // SAFETY: verifier is exclusively borrowed and valid for the write.
            unsafe { std::ptr::write_volatile(verifier, CodeVerifier::from_bytes([0; 32])) };
        }
    }
}

#[cfg(target_os = "linux")]
fn mint_digest(mint: &Command, pos: LogPos, expires_ms: u64) -> Option<[u8; 32]> {
    use sha2::{Digest, Sha256};

    let Command::Mint {
        cluster,
        actor,
        request,
        code,
        verifier,
        proposed_at_ms,
    } = mint
    else {
        return None;
    };
    Some(
        Sha256::new()
            .chain_update(b"arxcovo/c16/mint-receipt/v1")
            .chain_update(pos.term.to_be_bytes())
            .chain_update(pos.leader.to_be_bytes())
            .chain_update(pos.index.to_be_bytes())
            .chain_update(cluster)
            .chain_update(actor)
            .chain_update(request)
            .chain_update(code)
            .chain_update(verifier.bytes())
            .chain_update(proposed_at_ms.to_be_bytes())
            .chain_update(expires_ms.to_be_bytes())
            .finalize()
            .into(),
    )
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reject {
    AnchorChanged,
    NotFounded,
    BadFounders,
    AlreadyFounded,
    KeyInUse,
    MembersFull,
    Revoked,
    UnknownNode,
    LastMember,
    WrongCluster,
    TimeOutOfRange,
    ClockBehind,
    RequestConflict,
    CodeIdCollision,
    TooManyPending,
    StateFull,
    UnknownCode,
    BadProof,
    KeyConflict,
    Consumed,
    Expired,
    NodeIdsExhausted,
}
impl std::fmt::Display for Reject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::AnchorChanged => "join anchor changed",
            Self::NotFounded => "not founded",
            Self::BadFounders => "bad founders",
            Self::AlreadyFounded => "already founded",
            Self::KeyInUse => "key in use",
            Self::MembersFull => "members full",
            Self::Revoked => "revoked",
            Self::UnknownNode => "unknown node",
            Self::LastMember => "last member",
            Self::WrongCluster => "wrong cluster",
            Self::TimeOutOfRange => "time out of range",
            Self::ClockBehind => "clock behind",
            Self::RequestConflict => "request conflict",
            Self::CodeIdCollision => "code ID collision",
            Self::TooManyPending => "too many pending",
            Self::StateFull => "state full",
            Self::UnknownCode => "unknown code",
            Self::BadProof => "bad proof",
            Self::KeyConflict => "key conflict",
            Self::Consumed => "consumed",
            Self::Expired => "expired",
            Self::NodeIdsExhausted => "node IDs exhausted",
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssueRefusal {
    SignerClockBehind,
    GrantExpired,
}
impl std::fmt::Display for IssueRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::SignerClockBehind => "signer clock behind",
            Self::GrantExpired => "grant expired",
        })
    }
}
/// Only apply constructs grants. This is NOT proof of consensus commit: local clones can apply.
/// A future trusted adapter must use durably quorum-committed entries, sign only for key(),
/// and freshly check before signing AND before release/persistence. Never cache a guard pass.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnrollmentGrant {
    cluster: ClusterId,
    node: NodeId,
    key: KeyDigest,
    request: RequestId,
    code_expires_ms: u64,
    anchor: JoinAnchor,
    origin: LogPos,
    original_applied_ms: u64,
    applied_ms: u64,
    replay: bool,
}
impl EnrollmentGrant {
    pub fn cluster(&self) -> ClusterId {
        self.cluster
    }
    pub fn node(&self) -> NodeId {
        self.node
    }
    pub fn key(&self) -> KeyDigest {
        self.key
    }
    pub fn request(&self) -> RequestId {
        self.request
    }
    pub fn code_expires_ms(&self) -> u64 {
        self.code_expires_ms
    }
    pub fn anchor(&self) -> JoinAnchor {
        self.anchor
    }
    pub fn origin(&self) -> LogPos {
        self.origin
    }
    pub fn original_applied_ms(&self) -> u64 {
        self.original_applied_ms
    }
    pub fn applied_ms(&self) -> u64 {
        self.applied_ms
    }
    pub fn replay(&self) -> bool {
        self.replay
    }
    pub fn check_issuable(&self, signer_now_ms: u64) -> Result<(), IssueRefusal> {
        if signer_now_ms
            .checked_add(SKEW_MS)
            .is_some_and(|t| t < self.applied_ms)
        {
            return Err(IssueRefusal::SignerClockBehind);
        }
        if signer_now_ms
            .checked_add(2 * SKEW_MS)
            .is_none_or(|t| t >= self.code_expires_ms)
        {
            return Err(IssueRefusal::GrantExpired);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Founded { replay: bool },
    Removed { replay: bool },
    Minted { expires_ms: u64, replay: bool },
    Enrolled(EnrollmentGrant),
    Rejected(Reject),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinerReply {
    Enrolled { node: NodeId },
    RetryLater,
    Rejected,
}
impl Outcome {
    pub fn joiner_reply(&self) -> JoinerReply {
        match self {
            Self::Enrolled(grant) => JoinerReply::Enrolled { node: grant.node },
            Self::Rejected(Reject::ClockBehind) => JoinerReply::RetryLater,
            _ => JoinerReply::Rejected,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Consumption {
    request: RequestId,
    key: KeyDigest,
    node: NodeId,
    anchor: JoinAnchor,
    origin: LogPos,
    original_applied_ms: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct CodeRecord {
    actor: ActorId,
    request: RequestId,
    verifier: CodeVerifier,
    expires_ms: u64,
    consumed: Option<Consumption>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnrollmentState {
    founded: bool,
    members: BTreeMap<NodeId, KeyDigest>,
    cluster: ClusterId,
    next_node_id: NodeId,
    committed_ms: u64,
    records: BTreeMap<CodeId, CodeRecord>,
}
impl EnrollmentState {
    pub fn new(cluster: ClusterId, first_free: NodeId) -> Self {
        Self {
            cluster,
            founded: false,
            members: BTreeMap::new(),
            next_node_id: first_free,
            committed_ms: 0,
            records: BTreeMap::new(),
        }
    }

    /// Match an exact logged Mint to the applied pending record. This is a
    /// statement about applied state at its clock floor.
    #[cfg(target_os = "linux")]
    pub(in crate::cluster) fn pending_mint_receipt(
        &self,
        mint: &Command,
        pos: LogPos,
    ) -> Option<[u8; 32]> {
        let Command::Mint {
            cluster,
            actor,
            request,
            code,
            verifier,
            ..
        } = mint
        else {
            return None;
        };
        let record = self.records.get(code)?;
        if !self.founded
            || *cluster != self.cluster
            || record.actor != *actor
            || record.request != *request
            || !bool::from(record.verifier.bytes().ct_eq(verifier.bytes()))
            || record.consumed.is_some()
            || self.committed_ms.checked_add(2 * SKEW_MS)? >= record.expires_ms
        {
            return None;
        }
        mint_digest(mint, pos, record.expires_ms)
    }
    /// Check the retained consumption, independently of local membership.
    pub fn check_grant(&self, grant: &EnrollmentGrant, code: &CodeId) -> Result<(), Reject> {
        if grant.cluster != self.cluster {
            return Err(Reject::WrongCluster);
        }
        let record = self.records.get(code).ok_or(Reject::UnknownCode)?;
        let consumed = record.consumed.as_ref().ok_or(Reject::Consumed)?;
        if consumed.request != grant.request || consumed.node != grant.node {
            return Err(Reject::Consumed);
        }
        if consumed.key != grant.key {
            return Err(Reject::KeyConflict);
        }
        if consumed.anchor != grant.anchor
            || consumed.origin != grant.origin
            || consumed.original_applied_ms != grant.original_applied_ms
            || record.expires_ms != grant.code_expires_ms
            || grant.original_applied_ms > grant.applied_ms
            || grant.applied_ms > self.committed_ms
        {
            return Err(Reject::UnknownCode);
        }
        Ok(())
    }
    pub(in crate::cluster) fn voter_anchor(
        &self,
        membership: &Membership,
    ) -> Result<JoinAnchor, Reject> {
        Ok(BoundVoters::observe(self, membership)?.anchor(self.cluster))
    }
    pub(in crate::cluster) fn apply(
        &mut self,
        command: &Command,
        pos: LogPos,
        membership: Option<&Membership>,
    ) -> Outcome {
        let (cluster, proposed) = match command {
            Command::Mint {
                cluster,
                proposed_at_ms,
                ..
            }
            | Command::Redeem {
                cluster,
                proposed_at_ms,
                ..
            }
            | Command::Found {
                cluster,
                proposed_at_ms,
                ..
            }
            | Command::Remove {
                cluster,
                proposed_at_ms,
                ..
            } => (cluster, *proposed_at_ms),
        };
        if *cluster != self.cluster {
            return Outcome::Rejected(Reject::WrongCluster);
        }
        if proposed > MAX_TIME_MS {
            return Outcome::Rejected(Reject::TimeOutOfRange);
        }
        let Some(skewed) = proposed.checked_add(SKEW_MS) else {
            return Outcome::Rejected(Reject::TimeOutOfRange);
        };
        if skewed < self.committed_ms {
            return Outcome::Rejected(Reject::ClockBehind);
        }
        let eff = self.committed_ms.max(proposed);
        // Validate all timestamp sums before the common mutation step, including retained data.
        let Some(expires) = eff.checked_add(CODE_TTL_MS) else {
            return Outcome::Rejected(Reject::TimeOutOfRange);
        };
        let Some(usable_time) = eff.checked_add(2 * SKEW_MS) else {
            return Outcome::Rejected(Reject::TimeOutOfRange);
        };
        if self
            .records
            .values()
            .any(|r| r.expires_ms.checked_add(TOMBSTONE_MS).is_none())
        {
            return Outcome::Rejected(Reject::TimeOutOfRange);
        }
        // Only authenticated, retained first redemption reaches the voter fence.
        // Refusal here leaves enrollment time, pruning and allocation untouched.
        if let Command::Redeem {
            code,
            presented,
            anchor,
            ..
        } = command
            && let Some(record) = self.records.get(code)
            && record.expires_ms + TOMBSTONE_MS > eff
            && record.consumed.is_none()
            && bool::from(record.verifier.bytes().ct_eq(presented.bytes()))
            && membership.and_then(|m| self.voter_anchor(m).ok()) != Some(*anchor)
        {
            return Outcome::Rejected(Reject::AnchorChanged);
        }
        self.committed_ms = eff;
        self.records.retain(|_, r| {
            r.expires_ms
                .checked_add(TOMBSTONE_MS)
                .is_some_and(|t| t > eff)
        });
        if !self.founded && !matches!(command, Command::Found { .. }) {
            return Outcome::Rejected(Reject::NotFounded);
        }
        match command {
            Command::Found { members, .. } => self.found(members),
            Command::Remove { node, .. } => self.remove(*node),
            Command::Mint {
                actor,
                request,
                code,
                verifier,
                ..
            } => {
                if let Some((old_code, record)) = self
                    .records
                    .iter()
                    .find(|(_, r)| r.actor == *actor && r.request == *request)
                {
                    return if old_code == code && record.verifier == *verifier {
                        Outcome::Minted {
                            expires_ms: record.expires_ms,
                            replay: true,
                        }
                    } else {
                        Outcome::Rejected(Reject::RequestConflict)
                    };
                }
                if self.records.contains_key(code) {
                    return Outcome::Rejected(Reject::CodeIdCollision);
                }
                if self
                    .records
                    .values()
                    .filter(|r| r.consumed.is_none() && usable_time < r.expires_ms)
                    .count()
                    >= MAX_PENDING
                {
                    return Outcome::Rejected(Reject::TooManyPending);
                }
                if self.records.len() >= MAX_RECORDS
                    || 4866 + (self.records.len() + 1) * 209 > 16384
                {
                    return Outcome::Rejected(Reject::StateFull);
                }
                self.records.insert(
                    *code,
                    CodeRecord {
                        actor: *actor,
                        request: *request,
                        verifier: verifier.clone(),
                        expires_ms: expires,
                        consumed: None,
                    },
                );
                Outcome::Minted {
                    expires_ms: expires,
                    replay: false,
                }
            }
            Command::Redeem {
                request,
                code,
                presented,
                key,
                anchor,
                ..
            } => {
                let Some(record) = self.records.get_mut(code) else {
                    return Outcome::Rejected(Reject::UnknownCode);
                };
                if !bool::from(record.verifier.bytes().ct_eq(presented.bytes())) {
                    return Outcome::Rejected(Reject::BadProof);
                }
                let (node, replay) = if let Some(consumed) = &record.consumed {
                    if consumed.request != *request {
                        return Outcome::Rejected(Reject::Consumed);
                    }
                    if consumed.key != *key {
                        return Outcome::Rejected(Reject::KeyConflict);
                    }
                    if consumed.anchor != *anchor {
                        return Outcome::Rejected(Reject::AnchorChanged);
                    }
                    if self.members.get(&consumed.node) != Some(key) {
                        return Outcome::Rejected(Reject::Revoked);
                    }
                    (consumed.node, true)
                } else {
                    if usable_time >= record.expires_ms {
                        return Outcome::Rejected(Reject::Expired);
                    }
                    if self.members.values().any(|k| k == key) {
                        return Outcome::Rejected(Reject::KeyInUse);
                    }
                    if self.members.len() >= MAX_MEMBERS {
                        return Outcome::Rejected(Reject::MembersFull);
                    }
                    let Some(next) = self.next_node_id.get().checked_add(1).and_then(NodeId::new)
                    else {
                        return Outcome::Rejected(Reject::NodeIdsExhausted);
                    };
                    let node = self.next_node_id;
                    record.consumed = Some(Consumption {
                        request: *request,
                        key: *key,
                        node,
                        anchor: *anchor,
                        origin: pos,
                        original_applied_ms: eff,
                    });
                    self.members.insert(node, *key);
                    self.next_node_id = next;
                    (node, false)
                };
                Outcome::Enrolled(EnrollmentGrant {
                    cluster: self.cluster,
                    node,
                    key: *key,
                    request: *request,
                    code_expires_ms: record.expires_ms,
                    anchor: *anchor,
                    origin: record.consumed.as_ref().unwrap().origin,
                    original_applied_ms: record.consumed.as_ref().unwrap().original_applied_ms,
                    applied_ms: eff,
                    replay,
                })
            }
        }
    }
}
#[cfg(test)]
mod tests;

#[cfg(all(test, target_os = "linux"))]
#[test]
fn receipt_digest_uses_fixed_field_order_and_stored_expiry() {
    use sha2::{Digest, Sha256};

    let cluster = [1; 16];
    let pos = LogPos {
        term: 2,
        leader: 3,
        index: 4,
    };
    let mut state = EnrollmentState::new(cluster, NodeId::new(2).unwrap());
    let found = Command::Found {
        cluster,
        members: FoundMembers::new([(NodeId::new(1).unwrap(), KeyDigest([9; 32]))]).unwrap(),
        proposed_at_ms: 105,
    };
    state.apply(&found, LogPos { index: 0, ..pos }, None);
    let command = Command::Mint {
        cluster,
        actor: [5; 16],
        request: [6; 16],
        code: [7; 16],
        verifier: CodeVerifier::from_bytes([8; 32]),
        proposed_at_ms: 100,
    };
    let expiry = 105 + CODE_TTL_MS;
    assert_eq!(state.pending_mint_receipt(&command, pos), None);
    assert_eq!(
        state.apply(&command, pos, None),
        Outcome::Minted {
            expires_ms: expiry,
            replay: false
        }
    );
    let canonical = [
        b"arxcovo/c16/mint-receipt/v1".as_slice(),
        &2u64.to_be_bytes(),
        &3u64.to_be_bytes(),
        &4u64.to_be_bytes(),
        &[1; 16],
        &[5; 16],
        &[6; 16],
        &[7; 16],
        &[8; 32],
        &100u64.to_be_bytes(),
        &expiry.to_be_bytes(),
    ]
    .concat();
    let expected: [u8; 32] = Sha256::digest(&canonical).into();
    let before = state.clone();
    assert_eq!(state.pending_mint_receipt(&command, pos), Some(expected));
    assert_eq!(
        MintReceiptInput::new(&command).digest(pos, expiry),
        Some(expected)
    );
    assert_eq!(state.pending_mint_receipt(&found, pos), None);
    for field in 0..5 {
        let mut altered = command.clone();
        if let Command::Mint {
            cluster,
            actor,
            request,
            code,
            verifier,
            ..
        } = &mut altered
        {
            match field {
                0 => cluster[0] ^= 1,
                1 => actor[0] ^= 1,
                2 => request[0] ^= 1,
                3 => code[0] ^= 1,
                _ => *verifier = CodeVerifier::from_bytes([42; 32]),
            }
        }
        assert_eq!(state.pending_mint_receipt(&altered, pos), None);
    }
    assert_eq!(state, before);
    state.committed_ms = expiry - 2 * SKEW_MS;
    assert_eq!(state.pending_mint_receipt(&command, pos), None);
}

#[cfg(all(test, target_os = "linux"))]
#[test]
fn six_record_campaign_fits_with_two_consumed_and_four_pending() {
    let cluster = [1; 16];
    let membership = Membership::new(vec![[1].into()], [(1, "voter".into())].into()).unwrap();
    let mut state = EnrollmentState::new(cluster, NodeId::new(2).unwrap());
    let pos = |index| LogPos {
        term: 1,
        leader: 1,
        index,
    };
    let founding = Command::Found {
        cluster,
        members: FoundMembers::new([(NodeId::new(1).unwrap(), KeyDigest([9; 32]))]).unwrap(),
        proposed_at_ms: 100,
    };
    assert!(matches!(
        state.apply(&founding, pos(0), Some(&membership)),
        Outcome::Founded { replay: false }
    ));
    let anchor = state.voter_anchor(&membership).unwrap();
    for n in 1..=6_u8 {
        let code = JoinCode::generate(cluster, [2; 32], anchor, |bytes| {
            bytes.fill(n);
            Ok::<_, ()>(())
        })
        .unwrap();
        let command = Command::Mint {
            cluster,
            actor: [4; 16],
            request: [n; 16],
            code: code.code_id(),
            verifier: code.verifier(),
            proposed_at_ms: 100,
        };
        let position = pos(u64::from(n) * 2 - 1);
        assert_eq!(
            state.apply(&command, position, Some(&membership)),
            Outcome::Minted {
                expires_ms: 100 + CODE_TTL_MS,
                replay: false,
            }
        );
        if n <= 2 {
            let command = Command::redeem(
                cluster,
                [n; 16],
                code.code_id(),
                &code.proof(),
                KeyDigest([n; 32]),
                anchor,
                100,
            );
            assert!(matches!(
                state.apply(&command, pos(position.index + 1), Some(&membership)),
                Outcome::Enrolled(_)
            ));
        } else {
            assert!(state.pending_mint_receipt(&command, position).is_some());
        }
    }
    let pending = state
        .records
        .values()
        .filter(|record| record.consumed.is_none())
        .count();
    assert_eq!((pending, state.records.len() - pending), (4, 2));
    assert!(pending <= MAX_PENDING);
    assert!(state.records.len() <= MAX_RECORDS);
    let estimate = 4866 + state.records.len() * 209;
    let mut encoded = Vec::new();
    codec::encode(&state, &mut encoded);
    assert!(encoded.len() <= estimate && estimate <= 16 * 1024);
}

impl std::fmt::Display for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Found { .. } => "enrollment found",
            Self::Remove { .. } => "enrollment remove",
            Self::Mint { .. } => "enrollment mint",
            Self::Redeem { .. } => "enrollment redeem",
        })
    }
}
