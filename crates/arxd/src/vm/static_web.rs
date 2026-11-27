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
    listener: std::sync::Mutex<Option<std::net::TcpListener>>,
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
            match bind_origin(port) {
                Ok(listener) => {
                    return Ok(Self {
                        listener: std::sync::Mutex::new(Some(listener)),
                        port,
                    });
                }
                Err(error) if origin_port_busy(&error) => {}
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
        let held = self
            .listener
            .lock()
            .map_err(|_| std::io::Error::other("origin reservation unavailable"))?;
        let listener = held
            .as_ref()
            .ok_or_else(|| std::io::Error::other("origin reservation consumed"))?
            .try_clone()?;
        listener.set_nonblocking(true)?;
        Ok(listener)
    }

    /// For the qualified launcher to check a still-held provisional claim.
    pub fn verify(&self) -> bool {
        self.listener.lock().is_ok_and(|listener| {
            listener
                .as_ref()
                .is_some_and(|listener| self.matches(listener))
        })
    }

    fn matches(&self, listener: &std::net::TcpListener) -> bool {
        listener.local_addr().is_ok_and(|address| {
            address == (std::net::Ipv4Addr::LOCALHOST, self.port).into()
                && (ORIGIN_PORT_FIRST..=ORIGIN_PORT_LAST).contains(&self.port)
        })
    }

    /// Consume the provisional claim once. The checked launcher keeps the
    /// socket in this token until the verified suspended child's resume edge.
    #[cfg(all(windows, target_arch = "x86_64"))]
    pub(super) fn take_for_handoff(&self) -> Option<std::net::TcpListener> {
        let mut held = self.listener.lock().ok()?;
        if self.matches(held.as_ref()?) {
            held.take()
        } else {
            None
        }
    }
}

fn origin_port_busy(error: &std::io::Error) -> bool {
    if error.kind() == std::io::ErrorKind::AddrInUse {
        return true;
    }
    #[cfg(windows)]
    {
        // Winsock can report access denied for a conflicting exclusive bind.
        error.raw_os_error() == Some(windows_sys::Win32::Networking::WinSock::WSAEACCES)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(not(windows))]
fn bind_origin(port: u16) -> std::io::Result<std::net::TcpListener> {
    std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
}

#[cfg(windows)]
fn bind_origin(port: u16) -> std::io::Result<std::net::TcpListener> {
    use std::io;
    use std::os::windows::io::{AsRawSocket, FromRawSocket, OwnedSocket};
    use std::sync::OnceLock;
    use windows_sys::Win32::Networking::WinSock::*;

    // Keep the Winsock startup reference for the process lifetime. Listeners
    // can outlive this call and must stay valid when they enter Tokio.
    static STARTUP: OnceLock<i32> = OnceLock::new();
    let status = *STARTUP.get_or_init(|| unsafe { WSAStartup(0x0202, &mut WSADATA::default()) });
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status));
    }
    let error = || io::Error::from_raw_os_error(unsafe { WSAGetLastError() });
    let socket = unsafe {
        WSASocketW(
            AF_INET.into(),
            SOCK_STREAM,
            IPPROTO_TCP,
            std::ptr::null(),
            0,
            WSA_FLAG_OVERLAPPED | WSA_FLAG_NO_HANDLE_INHERIT,
        )
    };
    if socket == INVALID_SOCKET {
        return Err(error());
    }
    // Take ownership before any fallible operation so every failure closes it.
    let owned = unsafe { OwnedSocket::from_raw_socket(socket as _) };
    let socket = owned.as_raw_socket() as SOCKET;
    let exclusive = 1i32;
    let address = SOCKADDR_IN {
        sin_family: AF_INET,
        sin_port: port.to_be(),
        sin_addr: IN_ADDR {
            S_un: IN_ADDR_0 {
                S_addr: u32::from_ne_bytes(std::net::Ipv4Addr::LOCALHOST.octets()),
            },
        },
        sin_zero: [0; 8],
    };
    if unsafe {
        setsockopt(
            socket,
            SOL_SOCKET,
            SO_EXCLUSIVEADDRUSE,
            (&raw const exclusive).cast(),
            size_of::<i32>() as i32,
        )
    } != 0
        || unsafe {
            bind(
                socket,
                (&raw const address).cast(),
                size_of::<SOCKADDR_IN>() as i32,
            )
        } != 0
        || unsafe { listen(socket, 8) } != 0
    {
        return Err(error());
    }
    Ok(std::net::TcpListener::from(owned))
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
