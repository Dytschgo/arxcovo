//! Pure Windows owner provenance engine. No native adapter or activation.
//! Authority is supplied only by a private OwnerStore session holding the owner
//! lock. Parsing or the configuration mirror does not establish provenance.
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
    Busy,
    RecoveryRequired,
    RightUnproven,
    Unsupported,
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidPlan => "service-owner-plan",
            Self::Io => "service-owner-io",
            Self::Conflict => "service-owner-conflict",
            Self::Busy => "service-owner-busy",
            Self::RecoveryRequired => "service-owner-recovery-required",
            Self::RightUnproven => "service-owner-right-unproven",
            Self::Unsupported => "service-owner-unsupported",
        })
    }
}
impl std::error::Error for Failure {}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RightState {
    None,
    Intent,
    Granted,
    Unproven,
}
/// Private owner data; intentionally no Debug or public constructor.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerRecord {
    version: u8,
    sid: String,
    state: RightState,
}
impl OwnerRecord {
    pub fn encode(&self) -> Result<Vec<u8>, Failure> {
        serde_json::to_vec(self).map_err(|_| Failure::Io)
    }
    /// Only a read through the pinned private store establishes authority.
    pub fn decode(bytes: &[u8]) -> Result<Self, Failure> {
        if bytes.len() > 512 {
            return Err(Failure::RecoveryRequired);
        }
        let record: Self = serde_json::from_slice(bytes).map_err(|_| Failure::RecoveryRequired)?;
        if record.version != 1 || !ordinary_sid(&record.sid) {
            return Err(Failure::RecoveryRequired);
        }
        Ok(record)
    }
}
fn ordinary_sid(s: &str) -> bool {
    if s.len() > 52 {
        return false;
    }
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

/// Opaque validated Windows request. Contains private bytes; never Debug/log.
pub struct Configure {
    bytes: Vec<u8>,
    expected: Option<Vec<u8>>,
    sid: String,
    add: bool,
    batch: BatchObservation,
}
impl Configure {
    pub fn from_plan(
        layout: &Layout,
        plan: &Plan,
        resolver: &impl Resolver,
        batch: BatchObservation,
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
            || config.granted_batch_right() != (add || batch.recorded_grant)
            || add == batch.present
        {
            return Err(Failure::InvalidPlan);
        }
        Ok(Self {
            bytes: bytes.clone(),
            expected: expected.clone(),
            sid: config.build_identity().into(),
            add,
            batch,
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

/// SYSTEM-private, bounded, pinned store under a single owner lock. Reads
/// validate protection, one link and no reparse points. Replacement compares the
/// exact-byte SHA-256 (None means absent), stages/flushes private bytes, and
/// checks identity through publication. Errors may have taken effect; reread on
/// the next command. Native errors must use fixed Failure tokens only.
pub trait OwnerStore {
    fn record(&mut self) -> Result<Option<(OwnerRecord, [u8; 32])>, Failure>;
    fn replace_record(
        &mut self,
        expected: Option<[u8; 32]>,
        next: &OwnerRecord,
    ) -> Result<[u8; 32], Failure>;
    /// Recheck expected config hash and fresh account pins, pin source/parent,
    /// create-new private staging and flush it. Do not publish or mutate rights.
    fn stage_config(&mut self, bytes: &[u8], expected: Option<[u8; 32]>) -> Result<(), Failure>;
    /// Publish only the staged identity/bytes with compare-and-replace checks.
    /// An error may mean publication already happened; never infer rollback.
    fn publish_config(&mut self) -> Result<(), Failure>;
}
/// Direct assignment only, exactly the validated ordinary SID. Errors from a
/// mutation mean unknown outcome. No production implementation in this slice.
pub trait BatchRight {
    fn observe(&mut self, sid: &str) -> Result<bool, Failure>;
    fn grant(&mut self, sid: &str) -> Result<(), Failure>;
    fn revoke(&mut self, sid: &str) -> Result<(), Failure>;
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BatchObservation {
    present: bool,
    recorded_grant: bool,
}
impl BatchObservation {
    pub(crate) fn present(self) -> bool {
        self.present
    }
    pub(crate) fn recorded_grant(self) -> bool {
        self.recorded_grant
    }
}
pub fn observe_batch(
    sid: &str,
    s: &mut impl OwnerStore,
    r: &mut impl BatchRight,
) -> Result<BatchObservation, Failure> {
    if !ordinary_sid(sid) {
        return Err(Failure::InvalidPlan);
    }
    let record = s.record()?;
    let present = r.observe(sid)?;
    Ok(BatchObservation {
        present,
        recorded_grant: present
            && record.is_some_and(|(v, _)| v.sid == sid && v.state == RightState::Granted),
    })
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recovery {
    Clean,
    NotGranted,
    Unproven,
}
pub fn recover(s: &mut impl OwnerStore, r: &mut impl BatchRight) -> Result<Recovery, Failure> {
    let Some((mut record, expected)) = s.record()? else {
        return Ok(Recovery::Clean);
    };
    if record.state != RightState::Intent {
        return Ok(Recovery::Clean);
    }
    let present = r.observe(&record.sid)?;
    record.state = if present {
        RightState::Unproven
    } else {
        RightState::None
    };
    s.replace_record(Some(expected), &record)?;
    Ok(if present {
        Recovery::Unproven
    } else {
        Recovery::NotGranted
    })
}
/// Caller runs recovery first, all calls under the same owner lock. Never
/// compensates a grant, including publication errors with an unknown effect.
pub fn configure(
    req: &Configure,
    s: &mut impl OwnerStore,
    r: &mut impl BatchRight,
) -> Result<(), Failure> {
    let old = s.record()?;
    if let Some((v, _)) = &old {
        if v.state == RightState::Intent {
            return Err(Failure::RecoveryRequired);
        }
        if v.sid != req.sid && v.state != RightState::None {
            return Err(Failure::Conflict);
        }
    }
    if observe_batch(&req.sid, s, r)? != req.batch {
        return Err(Failure::Conflict);
    }
    s.stage_config(&req.bytes, req.expected.as_deref().map(hash))?;
    let expected = old.as_ref().map(|(_, h)| *h);
    let mut next = OwnerRecord {
        version: 1,
        sid: req.sid.clone(),
        state: RightState::None,
    };
    if req.add {
        next.state = RightState::Intent;
        let intent = s.replace_record(expected, &next)?;
        let granted = r.grant(&req.sid).is_ok();
        next.state = if granted {
            RightState::Granted
        } else if r.observe(&req.sid)? {
            RightState::Unproven
        } else {
            RightState::None
        };
        s.replace_record(Some(intent), &next)
            .map_err(|_| Failure::RecoveryRequired)?;
        if !granted {
            return Err(if next.state == RightState::Unproven {
                Failure::RightUnproven
            } else {
                Failure::Io
            });
        }
    } else if old.as_ref().is_none_or(|(v, _)| v.sid != req.sid) {
        s.replace_record(expected, &next)?;
    }
    s.publish_config().map_err(|_| Failure::Io)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RightReport {
    Removed,
    AlreadyAbsent,
    RetainedUnproven,
    RetainedNotArxcovo,
    NotRecorded,
}
impl std::fmt::Display for RightReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Removed => "batch-right-removed",
            Self::AlreadyAbsent => "batch-right-already-absent",
            Self::RetainedUnproven => "batch-right-retained-unproven",
            Self::RetainedNotArxcovo => "batch-right-retained-not-arxcovo",
            Self::NotRecorded => "batch-right-not-recorded",
        })
    }
}
/// No config, account resolution or workspace dependency. Caller recovers first.
pub fn uninstall_right(
    s: &mut impl OwnerStore,
    r: &mut impl BatchRight,
) -> Result<RightReport, Failure> {
    let Some((mut record, expected)) = s.record()? else {
        return Ok(RightReport::NotRecorded);
    };
    if record.state == RightState::Intent {
        return Err(Failure::RecoveryRequired);
    }
    let present = r.observe(&record.sid)?;
    let report = match (record.state, present) {
        (RightState::Granted, true) => {
            r.revoke(&record.sid).map_err(|_| Failure::Io)?;
            RightReport::Removed
        }
        (RightState::Granted | RightState::Unproven, false) => RightReport::AlreadyAbsent,
        (RightState::Unproven, true) => RightReport::RetainedUnproven,
        (RightState::None, true) => RightReport::RetainedNotArxcovo,
        _ => RightReport::NotRecorded,
    };
    record.state = RightState::None;
    s.replace_record(Some(expected), &record)?;
    Ok(report)
}
#[cfg(test)]
#[path = "owner_transactions_tests.rs"]
mod tests;
