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
#[repr(i64)]
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

/// Fixed loopback range selected by the supervisor. A held socket is a
/// provisional reservation. Native activation must hand off and prove the
/// listener against the held process and current boot before exposing health.
pub const ORIGIN_PORT_FIRST: u16 = 49152;
pub const ORIGIN_PORT_LAST: u16 = 49215;

/// A daemon-selected reservation with no caller-supplied address or port.
pub struct OriginPortLease {
    listener: std::net::TcpListener,
    port: u16,
}

impl OriginPortLease {
    pub fn acquire() -> std::io::Result<Self> {
        let mut nonce = [0];
        // getrandom's error type implements std::error::Error only with its
        // `std` feature, which feature unification enables on some targets
        // but not others; go through Display so every target builds.
        getrandom::fill(&mut nonce).map_err(|e| std::io::Error::other(e.to_string()))?;
        let size = ORIGIN_PORT_LAST - ORIGIN_PORT_FIRST + 1;
        for attempt in 0..size {
            let port = ORIGIN_PORT_FIRST + (u16::from(nonce[0]) + attempt) % size;
            match std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)) {
                Ok(listener) => return Ok(Self { listener, port }),
                Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {}
                Err(error) => return Err(error),
            }
        }
        Err(std::io::Error::from(std::io::ErrorKind::AddrInUse))
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    #[cfg(test)]
    pub(crate) fn fixture_listener(&self) -> std::io::Result<std::net::TcpListener> {
        let listener = self.listener.try_clone()?;
        listener.set_nonblocking(true)?;
        Ok(listener)
    }

    /// For the qualified launcher to check a still-held provisional claim.
    pub fn verify(&self) -> bool {
        self.listener.local_addr().is_ok_and(|address| {
            address == (std::net::Ipv4Addr::LOCALHOST, self.port).into()
                && (ORIGIN_PORT_FIRST..=ORIGIN_PORT_LAST).contains(&self.port)
        })
    }
}

/// Stable provisioned identity. The TLS name is derived from the first active
/// generation ID and is independent of an agent-supplied display name.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct StaticOriginIdentity(Id);

impl StaticOriginIdentity {
    pub fn for_generation(generation: Id) -> Self {
        Self(generation)
    }

    pub fn provisioning(self) -> super::ubuntu_web::OriginProvisioning {
        super::ubuntu_web::OriginProvisioning::new(self.0)
    }
}
