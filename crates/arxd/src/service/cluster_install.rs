//! Owner input for the private Linux cluster installation file (ADR 0046, 0047).
//!
//! Pure validation and serialization run on every platform so the schema stays
//! tested everywhere. Only Linux resolves accounts, checks the layout with the
//! cluster runtime's own loader and writes the file
//! (`cluster_runtime::install`). Other platforms refuse before observing any
//! account or path.
use serde::Serialize;
use std::net::{IpAddr, SocketAddr};

/// Launch intents accepted by `cluster_runtime` (`Intent::parse`).
pub const INTENTS: [&str; 4] = [
    "create-founder",
    "join",
    "recover-founder",
    "recover-joiner",
];
/// The service worker account required by ADR 0047 (`Child::spawn`).
pub const DAEMON_ACCOUNT: &str = "arxd-cluster";
/// Same bound as the runtime loader (`Settings::vetted`).
pub const MAX_INSTALLATION_BYTES: usize = 16384;

/// A founder file carries a fresh cluster ID; a joiner file names its issuer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Founder,
    Joiner,
}

/// Owner input; deliberately neither Debug nor Clone.
#[derive(PartialEq)]
pub struct Request {
    pub out: String,
    pub role: Role,
    pub workspace: String,
    pub state: String,
    pub credentials: String,
    pub wrap: String,
    pub custody: String,
    pub history: String,
    pub owner_socket: String,
    pub peer: String,
    pub enrollment: String,
    pub issuer_peer: Option<String>,
    pub operator_account: String,
    pub control_group: String,
    pub agent_account: String,
    pub build_account: String,
}

/// Resolved numeric identities; the daemon is always `arxd-cluster`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ids {
    pub owner_uid: u32,
    pub owner_gid: u32,
    pub daemon_uid: u32,
    pub daemon_gid: u32,
    pub agent_uid: u32,
    pub build_uid: u32,
}

/// Fixed refusal categories. No path, account or address appears in output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallError {
    Argument,
    Account,
    Address,
    Layout,
    OwnerRequired,
    Exists,
    Io,
    Unsupported,
}

impl InstallError {
    pub fn token(self) -> &'static str {
        match self {
            Self::Argument => "cluster-config-argument",
            Self::Account => "cluster-config-account",
            Self::Address => "cluster-config-address",
            Self::Layout => "cluster-config-layout",
            Self::OwnerRequired => "cluster-config-owner-required",
            Self::Exists => "cluster-config-exists",
            Self::Io => "cluster-config-io",
            Self::Unsupported => "cluster-config-unsupported",
        }
    }
    pub fn exit_code(self) -> i32 {
        match self {
            Self::Argument | Self::Account | Self::Address | Self::Layout => 3,
            Self::OwnerRequired => 4,
            Self::Exists | Self::Io => 1,
            Self::Unsupported => 7,
        }
    }
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.token())
    }
}
impl std::error::Error for InstallError {}

/// Absolute, canonical POSIX path: no empty, `.` or `..` component, no
/// trailing slash, no control character, below `PATH_MAX`.
pub fn posix_path(value: &str) -> bool {
    value.len() > 1
        && value.len() < 4096
        && value.starts_with('/')
        && !value.ends_with('/')
        && !value.chars().any(char::is_control)
        && value[1..]
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != ".." && part.len() <= 255)
}

fn parent(value: &str) -> Option<&str> {
    match value.rfind('/') {
        Some(0) | None => None,
        Some(index) => Some(&value[..index]),
    }
}

/// Component-wise containment of two canonical POSIX paths.
fn overlaps(left: &str, right: &str) -> bool {
    let within = |inner: &str, outer: &str| {
        inner == outer
            || inner
                .strip_prefix(outer)
                .is_some_and(|rest| rest.starts_with('/'))
    };
    within(left, right) || within(right, left)
}

/// Mirrors `cluster_owner::address`: at most 100 bytes, `A-Z a-z 0-9 / _ - .`,
/// canonical, and not directly under `/`.
pub fn owner_socket(value: &str) -> bool {
    posix_path(value)
        && value.len() <= 100
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'-' | b'.'))
        && parent(value).is_some()
}

/// Mirrors `activation::peer_endpoint`: canonical text, nonzero port, and a
/// private IPv4, CGNAT (100.64.0.0/10) or IPv6 unique-local unicast address.
pub fn endpoint(value: &str) -> Option<SocketAddr> {
    if value.len() > 200 {
        return None;
    }
    let address: SocketAddr = value.parse().ok()?;
    let ip = address.ip();
    let qualified = match ip {
        IpAddr::V4(v4) => {
            !v4.is_broadcast()
                && (v4.is_private()
                    || (v4.octets()[0] == 100 && (64..=127).contains(&v4.octets()[1])))
        }
        IpAddr::V6(v6) => v6.is_unique_local() && v6.to_ipv4_mapped().is_none(),
    };
    let scoped = matches!(address, SocketAddr::V6(a) if a.scope_id() != 0);
    (address.port() != 0
        && address.to_string() == value
        && !ip.is_unspecified()
        && !ip.is_multicast()
        && !ip.is_loopback()
        && !scoped
        && qualified)
        .then_some(address)
}

fn account_name(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.contains('\0')
}

/// Every check that needs no account, file or interface observation.
pub fn check(r: &Request) -> Result<(), InstallError> {
    let dirs = [
        &r.workspace,
        &r.state,
        &r.credentials,
        &r.wrap,
        &r.custody,
        &r.history,
    ];
    if !posix_path(&r.out)
        || dirs.iter().any(|dir| !posix_path(dir))
        || !owner_socket(&r.owner_socket)
        || parent(&r.out).is_none()
        // The custody parent must be a private daemon directory, never `/`.
        || parent(&r.custody).is_none()
    {
        return Err(InstallError::Argument);
    }
    let socket_dir = parent(&r.owner_socket).ok_or(InstallError::Argument)?;
    let mut paths = vec![r.out.as_str()];
    paths.extend(dirs.iter().map(|dir| dir.as_str()));
    paths.push(socket_dir);
    for (index, path) in paths.iter().enumerate() {
        if paths[..index].iter().any(|other| overlaps(path, other)) {
            return Err(InstallError::Argument);
        }
    }
    let peer = endpoint(&r.peer).ok_or(InstallError::Argument)?;
    let enrollment = endpoint(&r.enrollment).ok_or(InstallError::Argument)?;
    if peer.ip() != enrollment.ip() || peer.port() == enrollment.port() {
        return Err(InstallError::Argument);
    }
    match (r.role, r.issuer_peer.as_deref()) {
        (Role::Founder, None) => {}
        (Role::Joiner, Some(issuer)) => {
            let issuer = endpoint(issuer).ok_or(InstallError::Argument)?;
            if issuer == peer || issuer == enrollment {
                return Err(InstallError::Argument);
            }
        }
        _ => return Err(InstallError::Argument),
    }
    if ![
        &r.operator_account,
        &r.control_group,
        &r.agent_account,
        &r.build_account,
    ]
    .iter()
    .all(|name| account_name(name))
    {
        return Err(InstallError::Argument);
    }
    Ok(())
}

#[derive(Serialize)]
struct Wire<'a> {
    workspace: &'a str,
    state: &'a str,
    credentials: &'a str,
    wrap: &'a str,
    custody: &'a str,
    history: &'a str,
    owner_socket: &'a str,
    peer: &'a str,
    enrollment: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    issuer_peer: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    founder_cluster: Option<[u8; 16]>,
    owner_uid: u32,
    daemon_uid: u32,
    daemon_gid: u32,
    agent_uid: u32,
    build_uid: u32,
    owner_gid: u32,
}

fn ids_valid(ids: &Ids) -> bool {
    let uids = [ids.owner_uid, ids.daemon_uid, ids.agent_uid, ids.build_uid];
    let valid = |id: u32| id != 0 && id != u32::MAX;
    uids.iter()
        .enumerate()
        .all(|(index, uid)| valid(*uid) && !uids[..index].contains(uid))
        && valid(ids.owner_gid)
        && valid(ids.daemon_gid)
}

/// Canonical installation bytes in the exact `cluster_runtime` `Settings`
/// schema. Optional executor and delegated-build settings are never written.
pub fn document(
    r: &Request,
    ids: &Ids,
    founder_cluster: Option<[u8; 16]>,
) -> Result<Vec<u8>, InstallError> {
    check(r)?;
    if !ids_valid(ids) {
        return Err(InstallError::Account);
    }
    match (r.role, founder_cluster) {
        (Role::Founder, Some(id)) if id != [0; 16] => {}
        (Role::Joiner, None) => {}
        _ => return Err(InstallError::Argument),
    }
    let bytes = serde_json::to_vec(&Wire {
        workspace: &r.workspace,
        state: &r.state,
        credentials: &r.credentials,
        wrap: &r.wrap,
        custody: &r.custody,
        history: &r.history,
        owner_socket: &r.owner_socket,
        peer: &r.peer,
        enrollment: &r.enrollment,
        issuer_peer: r.issuer_peer.as_deref(),
        founder_cluster,
        owner_uid: ids.owner_uid,
        daemon_uid: ids.daemon_uid,
        daemon_gid: ids.daemon_gid,
        agent_uid: ids.agent_uid,
        build_uid: ids.build_uid,
        owner_gid: ids.owner_gid,
    })
    .map_err(|_| InstallError::Io)?;
    if bytes.len() > MAX_INSTALLATION_BYTES {
        return Err(InstallError::Argument);
    }
    Ok(bytes)
}

#[cfg(test)]
pub(crate) mod tests;
