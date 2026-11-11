//! Windows configure transaction prerequisite. No native mutation is enabled.
//!
//! LSA and NTFS have no common transaction. A durable intent is not proof that
//! this installation added a right. In particular, recovery never promotes an
//! intent to a receipt by inspecting the current account rights.
use super::{
    config::{MAX_CONFIG_BYTES, Resolver, ServiceConfig},
    layout::{Layout, Platform},
    owner_plan::{Operation, Plan},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    InvalidPlan,
    Io,
    Conflict,
    RecoveryRequired,
    Unsupported,
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidPlan => "service-owner-plan",
            Self::Io => "service-owner-io",
            Self::Conflict => "service-owner-conflict",
            Self::RecoveryRequired => "service-owner-recovery-required",
            Self::Unsupported => "service-owner-unsupported",
        })
    }
}
impl std::error::Error for Failure {}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum Phase {
    Intent,
    Granted,
}

/// Private data: never Debug-print or return through MCP. Native storage must
/// protect and pin the journal exactly as the config, and reject hardlinks.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Journal {
    version: u8,
    sid: String,
    before: Option<[u8; 32]>,
    after: [u8; 32],
    phase: Phase,
}
impl Journal {
    pub fn encode(&self) -> Result<Vec<u8>, Failure> {
        serde_json::to_vec(self).map_err(|_| Failure::Io)
    }
    /// Call only on bytes read from the pinned private journal handle. Parsing
    /// is not provenance. No CLI or caller boolean may supply a receipt.
    pub fn decode(bytes: &[u8]) -> Result<Self, Failure> {
        if bytes.len() > 1024 {
            return Err(Failure::RecoveryRequired);
        }
        let j: Self = serde_json::from_slice(bytes).map_err(|_| Failure::RecoveryRequired)?;
        if j.version != 1 || !ordinary_sid(&j.sid) {
            return Err(Failure::RecoveryRequired);
        }
        Ok(j)
    }
}
fn ordinary_sid(s: &str) -> bool {
    let Some(tail) = s.strip_prefix("S-1-5-21-") else {
        return false;
    };
    let parts: Vec<_> = tail.split('-').collect();
    parts.len() == 4
        && parts.iter().all(|p| {
            !p.is_empty()
                && (p.len() == 1 || !p.starts_with('0'))
                && p.bytes().all(|b| b.is_ascii_digit())
                && p.parse::<u32>().is_ok()
        })
}
fn hash(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// Opaque request extracted from a complete validated Windows configure plan.
/// Contains private bytes, so intentionally has no Debug implementation.
pub struct Configure {
    bytes: Vec<u8>,
    expected: Option<Vec<u8>>,
    sid: String,
    add: bool,
}
impl Configure {
    pub fn from_plan(
        layout: &Layout,
        plan: &Plan,
        resolver: &impl Resolver,
    ) -> Result<Self, Failure> {
        if layout.platform() != Platform::Windows {
            return Err(Failure::Unsupported);
        }
        let ops = plan.operations();
        let Some(Operation::AtomicConfig {
            path,
            bytes,
            expected,
        }) = ops.last()
        else {
            return Err(Failure::InvalidPlan);
        };
        if path != layout.config() || bytes.len() > MAX_CONFIG_BYTES {
            return Err(Failure::InvalidPlan);
        }
        let config =
            ServiceConfig::parse(bytes, layout, resolver).map_err(|_| Failure::InvalidPlan)?;
        let old = expected
            .as_ref()
            .map(|b| ServiceConfig::parse(b, layout, resolver))
            .transpose()
            .map_err(|_| Failure::InvalidPlan)?;
        if old
            .as_ref()
            .is_some_and(|o| o.build_identity() != config.build_identity())
        {
            return Err(Failure::InvalidPlan);
        }
        let mut add = false;
        for op in &ops[..ops.len() - 1] {
            match op {
                Operation::Verify { .. } if !add => {}
                Operation::BatchRight { sid, add: true }
                    if !add && sid == config.build_identity() =>
                {
                    add = true
                }
                _ => return Err(Failure::InvalidPlan),
            }
        }
        if !ordinary_sid(config.build_identity())
            || config.granted_batch_right() != (add || old.is_some_and(|o| o.granted_batch_right()))
        {
            return Err(Failure::InvalidPlan);
        }
        Ok(Self {
            bytes: bytes.clone(),
            expected: expected.clone(),
            sid: config.build_identity().into(),
            add,
        })
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn expected(&self) -> Option<&[u8]> {
        self.expected.as_deref()
    }
    pub fn sid(&self) -> &str {
        &self.sid
    }
    pub fn adds_right(&self) -> bool {
        self.add
    }
}

/// Required native contract; no production implementation is supplied here.
/// One owner transaction lock must cover all methods, including recovery.
/// `prepare` verifies elevation, fixed layout/current binary identity, fresh
/// account pins and exact old config bytes, and pins ancestors/source handles
/// without write/delete sharing. It creates/flushes a SYSTEM-owned protected
/// same-directory temp exclusively, before any right mutation. It must reject
/// conflicting right observations rather than reinterpret a stale plan.
/// Journal writes are atomic, flushed, compare-and-replace publications with
/// the same no-takeover protection. `publish` must preserve the staged bytes
/// and identity through rename; its error may mean publication already occurred.
/// Native failures must map to these fixed errors, never OS error text.
pub trait ConfigurationIo {
    fn journal(&mut self) -> Result<Option<Journal>, Failure>;
    fn prepare(&mut self, request: &Configure) -> Result<(), Failure>;
    fn write_journal(&mut self, journal: &Journal) -> Result<(), Failure>;
    fn grant_batch(&mut self, sid: &str) -> Result<(), Failure>;
    fn revoke_batch(&mut self, sid: &str) -> Result<(), Failure>;
    fn publish(&mut self) -> Result<(), Failure>;
    /// Hash from the pinned, bounded, rechecked private config; None = absent.
    fn config_hash(&mut self) -> Result<Option<[u8; 32]>, Failure>;
    fn clear_journal(&mut self) -> Result<(), Failure>;
}

/// No right grant occurs until staging and durable intent have succeeded.
/// A failed receipt write preserves intent and refuses automatic compensation:
/// the caller must not later infer ownership from right presence.
pub fn configure(request: &Configure, io: &mut impl ConfigurationIo) -> Result<(), Failure> {
    if io.journal()?.is_some() {
        return Err(Failure::RecoveryRequired);
    }
    io.prepare(request)?;
    if !request.add {
        return io.publish();
    }
    let mut j = Journal {
        version: 1,
        sid: request.sid.clone(),
        before: request.expected.as_deref().map(hash),
        after: hash(&request.bytes),
        phase: Phase::Intent,
    };
    io.write_journal(&j)?;
    // Even an error can be ambiguous. Keep the durable intent.
    io.grant_batch(&request.sid)
        .map_err(|_| Failure::RecoveryRequired)?;
    j.phase = Phase::Granted;
    io.write_journal(&j)
        .map_err(|_| Failure::RecoveryRequired)?;
    match io.publish() {
        Ok(()) => io.clear_journal(),
        Err(_) => match recover(io)? {
            Recovery::Published => Ok(()),
            Recovery::Compensated => Err(Failure::Io),
            Recovery::Clean => Err(Failure::RecoveryRequired),
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recovery {
    Clean,
    Published,
    Compensated,
}

/// Never revoke using `granted_batch_right` alone: only a durable Granted
/// receipt plus exact old config identity permits failed-publication rollback.
/// Recovery itself is restartable: clear the receipt only after compensation.
pub fn recover(io: &mut impl ConfigurationIo) -> Result<Recovery, Failure> {
    let Some(j) = io.journal()? else {
        return Ok(Recovery::Clean);
    };
    if j.phase == Phase::Intent {
        return Err(Failure::RecoveryRequired);
    }
    let actual = io.config_hash()?;
    let result = if actual == Some(j.after) {
        Recovery::Published
    } else if actual == j.before {
        io.revoke_batch(&j.sid)
            .map_err(|_| Failure::RecoveryRequired)?;
        Recovery::Compensated
    } else {
        return Err(Failure::Conflict);
    };
    io.clear_journal()?;
    Ok(result)
}

#[cfg(test)]
#[path = "owner_transactions_tests.rs"]
mod tests;
