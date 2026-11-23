//! Persistent static web VM metadata and agent policy. Storage and native
//! custody are enforced by the caller at admission and settlement.

use super::{DiskClaim, Id};

/// Display data and the host-scoped uniqueness key. Storage keys and origin
/// DNS names are derived from opaque IDs.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StaticName(String);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NameError {
    Invalid,
    Unavailable,
}

impl StaticName {
    pub fn parse(value: &str) -> Result<Self, NameError> {
        let bytes = value.as_bytes();
        if !(1..=63).contains(&bytes.len())
            || !bytes[0].is_ascii_alphanumeric()
            || !bytes[bytes.len() - 1].is_ascii_alphanumeric()
            || !bytes
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || *b == b'-')
        {
            return Err(NameError::Invalid);
        }
        Ok(Self(value.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Check the normalized name against the complete local host catalog
    /// inside the same transaction as insertion. Include uncommitted names.
    pub fn require_available(&self, occupied: impl FnOnce(&Self) -> bool) -> Result<(), NameError> {
        if occupied(self) {
            return Err(NameError::Unavailable);
        }
        Ok(())
    }
}

/// Immutable source evidence. The source generation can be retired once the
/// independent writable copy is settled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticProvenance {
    pub source_generation: Id,
    pub recipe_digest: [u8; 32],
    pub disk_digest: [u8; 32],
    pub vars_digest: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StaticState {
    Creating,
    Stopped,
    Running,
    Recovering,
    Failed,
}

/// Settlement events from the supervisor after gathering the native evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StaticEvent {
    /// Independent files, origin provisioning and clean stop are proven.
    Created,
    /// Current boot and required readiness are proven.
    Started,
    /// The current boot has released its resources.
    Stopped,
    /// An interrupted effect or loss of current runtime evidence.
    NeedsRecovery,
    /// A complete generation and quiescence are proven after interruption.
    RecoveredStopped,
    /// Creation failed before activation. Retain the attributed charge.
    CreationFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidTransition;

impl StaticState {
    pub fn settle(self, event: StaticEvent) -> Result<Self, InvalidTransition> {
        use StaticEvent as Event;
        match (self, event) {
            (Self::Creating, Event::Created)
            | (Self::Running, Event::Stopped)
            | (Self::Recovering, Event::RecoveredStopped) => Ok(Self::Stopped),
            (Self::Stopped, Event::Started) => Ok(Self::Running),
            (
                Self::Creating | Self::Stopped | Self::Running | Self::Recovering,
                Event::NeedsRecovery,
            ) => Ok(Self::Recovering),
            (Self::Creating, Event::CreationFailed) => Ok(Self::Failed),
            _ => Err(InvalidTransition),
        }
    }
}

/// Closed set of agent actions on an existing static VM. Creation and listing
/// are catalog-scoped. Publishing has its own policy and readiness gate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StaticAgentAction {
    Get,
    Start,
    Stop,
    RunScript,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StaticRefusal {
    NotFound,
    InvalidState,
}

/// Apply this after resolving an ID in the static catalog. The storage
/// adapter must keep TEST and staging cleanup capabilities disjoint.
pub fn authorize_agent(
    target: Option<(&str, StaticState)>,
    principal: &str,
    action: StaticAgentAction,
) -> Result<(), StaticRefusal> {
    let (_, state) = target
        .filter(|(owner, _)| *owner == principal)
        .ok_or(StaticRefusal::NotFound)?;
    match (action, state) {
        (StaticAgentAction::Get, _)
        | (StaticAgentAction::Start, StaticState::Stopped)
        | (StaticAgentAction::Stop | StaticAgentAction::RunScript, StaticState::Running) => Ok(()),
        _ => Err(StaticRefusal::InvalidState),
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WebHealth {
    #[default]
    Unavailable,
    Checking,
    Healthy,
    Unhealthy,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PublishingPhase {
    #[default]
    Unpublished,
    Publishing,
    Published,
    Unpublishing,
    RemovalPending,
}

/// Budget the complete logical destination file lengths, including sparse
/// regions, plus independently bounded qcow2 growth and staging overhead.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FullCopyReservation {
    pub qcow2_bytes: u64,
    pub qcow2_growth_bytes: u64,
    pub vars_bytes: u64,
    pub staging_bytes: u64,
    pub metadata_bytes: u64,
}

impl FullCopyReservation {
    pub fn checked_claim(self) -> Option<DiskClaim> {
        if self.qcow2_bytes == 0 || self.vars_bytes == 0 {
            return None;
        }
        let claim = DiskClaim {
            images: self
                .qcow2_bytes
                .checked_add(self.qcow2_growth_bytes)?
                .checked_add(self.vars_bytes)?,
            staging: self.staging_bytes,
            metadata: self.metadata_bytes,
        };
        claim.total()?;
        Some(claim)
    }
}
