//! Pure proposal-time enrollment. This module performs no I/O and proves no consensus commit.
mod code;
pub(super) mod codec;
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
        proposed_at_ms: u64,
    ) -> Self {
        Self::Redeem {
            cluster,
            request,
            code,
            presented: CodeVerifier::of_proof(proof),
            key,
            proposed_at_ms,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reject {
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
    cluster: ClusterId,
    next_node_id: NodeId,
    committed_ms: u64,
    records: BTreeMap<CodeId, CodeRecord>,
}
impl EnrollmentState {
    pub fn new(cluster: ClusterId, first_free: NodeId) -> Self {
        Self {
            cluster,
            next_node_id: first_free,
            committed_ms: 0,
            records: BTreeMap::new(),
        }
    }
    pub fn apply(&mut self, command: &Command) -> Outcome {
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
        self.committed_ms = eff;
        self.records.retain(|_, r| {
            r.expires_ms
                .checked_add(TOMBSTONE_MS)
                .is_some_and(|t| t > eff)
        });
        match command {
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
                if self.records.len() >= MAX_RECORDS {
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
                    (consumed.node, true)
                } else {
                    if usable_time >= record.expires_ms {
                        return Outcome::Rejected(Reject::Expired);
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
                    });
                    self.next_node_id = next;
                    (node, false)
                };
                Outcome::Enrolled(EnrollmentGrant {
                    cluster: self.cluster,
                    node,
                    key: *key,
                    request: *request,
                    code_expires_ms: record.expires_ms,
                    applied_ms: eff,
                    replay,
                })
            }
        }
    }
}
#[cfg(test)]
mod tests;

impl std::fmt::Display for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Mint { .. } => "enrollment mint",
            Self::Redeem { .. } => "enrollment redeem",
        })
    }
}
