//! Private witness inputs. Only the scoped digest and verdict enter records.
use super::*;
use crate::vm::public_egress::{PublicEgressPolicy, RouteObservation};
use std::net::Ipv4Addr;

const NETWORK: NativeRefusal = NativeRefusal::NetworkFenceUnverified;
const ENDPOINT: NativeRefusal = NativeRefusal::GuestEndpointUnverified;
const MAX_CAPTURE_BYTES: usize = 64 * 1024;

pub(in crate::vm::windows::qualification) struct RouteSample {
    pub ticket: Ticket,
    pub peer: Option<PeerId>,
    pub policy_bytes: Vec<u8>,
    pub policy_identity: FileIdentity,
    pub topology: Hash,
    pub route: RouteObservation,
}

pub(super) fn route(
    sample: RouteSample,
    boot: &CheckedBoot<'_>,
    peer: Option<PeerId>,
    dependencies: &DockerDependencies,
) -> Result<(), NativeRefusal> {
    let policy = PublicEgressPolicy::parse(&sample.policy_bytes).map_err(|_| NETWORK)?;
    if sample.ticket != boot.ticket()
        || sample.peer != peer
        || boot
            .argv()
            .public_egress_policy()
            .and_then(|artifact| artifact.identity())
            != Some(sample.policy_identity)
        || digest(&sample.policy_bytes) != dependencies.policy
        || sample.topology != dependencies.topology
    {
        return Err(NETWORK);
    }
    policy
        .verify_route(Some(&sample.route))
        .map_err(|_| NETWORK)
}

#[derive(Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub(in crate::vm::windows::qualification) enum Case {
    PublicTcp,
    PublicUdp,
    RecursiveDnsTcp,
    RecursiveDnsUdp,
    HostTcp,
    HostUdp,
    SlirpHostAlias,
    SlirpDnsAlias,
    Private,
    LinkLocal,
    Cgnat,
    OnLink,
    Vpn,
    Management,
    Router,
    PublicNumberedLan,
    Hairpin,
    Ipv6,
    Icmp,
    GuestOverride,
    DnsRebinding,
    UdpPeerChange,
    RouteDriftTcp,
    RouteDriftUdp,
    LanInbound,
    StockTestTcp,
    StockTestUdp,
}

const REQUIRED: [Case; 27] = [
    Case::PublicTcp,
    Case::PublicUdp,
    Case::RecursiveDnsTcp,
    Case::RecursiveDnsUdp,
    Case::HostTcp,
    Case::HostUdp,
    Case::SlirpHostAlias,
    Case::SlirpDnsAlias,
    Case::Private,
    Case::LinkLocal,
    Case::Cgnat,
    Case::OnLink,
    Case::Vpn,
    Case::Management,
    Case::Router,
    Case::PublicNumberedLan,
    Case::Hairpin,
    Case::Ipv6,
    Case::Icmp,
    Case::GuestOverride,
    Case::DnsRebinding,
    Case::UdpPeerChange,
    Case::RouteDriftTcp,
    Case::RouteDriftUdp,
    Case::LanInbound,
    Case::StockTestTcp,
    Case::StockTestUdp,
];

impl Case {
    pub(in crate::vm::windows::qualification) fn expected_arrivals(self) -> u16 {
        u16::from(matches!(
            self,
            Self::PublicTcp | Self::PublicUdp | Self::RecursiveDnsTcp | Self::RecursiveDnsUdp
        ))
    }
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::vm::windows::qualification) struct Target {
    pub case: Case,
    pub identity: Hash,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::vm::windows::qualification) struct Counts {
    pub target: Target,
    // Counts of exact tagged exchanges in the before/during/after windows.
    pub before_ms: u64,
    pub between_ms: [u64; 2],
    pub after_ms: u64,
    pub before: u16,
    pub attempts: u16,
    pub arrivals: u16,
    pub after: u16,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::vm::windows::qualification) struct Receivers {
    pub run: Id,
    pub boot: Id,
    pub generation: u64,
    pub nonce: [u8; 16],
    pub observations: Vec<Counts>,
}

pub(super) fn receivers(
    raw: &[u8],
    targets: &[Target],
    ticket: Ticket,
    peer: PeerId,
    window: std::ops::Range<u64>,
) -> Result<Hash, NativeRefusal> {
    if raw.len() > MAX_CAPTURE_BYTES
        || targets.len() > 128
        || !REQUIRED
            .iter()
            .all(|case| targets.iter().any(|target| target.case == *case))
        || targets
            .iter()
            .enumerate()
            .any(|(index, target)| target.identity == [0; 32] || targets[..index].contains(target))
    {
        return Err(NETWORK);
    }
    let capture: Receivers = serde_json::from_slice(raw).map_err(|_| NETWORK)?;
    if capture.run != ticket.scope.run
        || capture.boot != peer.boot
        || capture.generation != ticket.challenge.generation
        || capture.nonce != ticket.challenge.nonce
        || capture.observations.len() != targets.len()
    {
        return Err(NETWORK);
    }
    for (counts, expected) in capture.observations.iter().zip(targets) {
        let [open, close] = counts.between_ms;
        if counts.target != *expected
            || !window.contains(&counts.before_ms)
            || !window.contains(&counts.after_ms)
            || !(counts.before_ms < open && open < close && close < counts.after_ms)
            || counts.before != 1
            || counts.attempts != 1
            || counts.arrivals != expected.case.expected_arrivals()
            || counts.after != 1
        {
            return Err(NETWORK);
        }
    }
    let canonical =
        serde_json::to_vec(&("arx-docker-receivers-v1", capture)).map_err(|_| NETWORK)?;
    Ok(digest(&canonical))
}

#[cfg(test)]
pub(in crate::vm::windows::qualification) fn fixture_targets() -> Vec<Target> {
    REQUIRED
        .iter()
        .enumerate()
        .map(|(index, case)| Target {
            case: *case,
            identity: [index as u8 + 1; 32],
        })
        .collect()
}

/// One exact IPv4 loopback listener on the selected port. Keep the process
/// hold through both of the collector's listener samples.
#[derive(Clone, Copy)]
pub(in crate::vm::windows) struct Listener {
    port: u16,
    pid: u32,
}

impl Listener {
    pub(in crate::vm::windows) fn check(self, port: u16, pid: u32) -> Result<(), NativeRefusal> {
        if self.port != port || self.pid != pid {
            return Err(ENDPOINT);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(in crate::vm::windows::qualification) fn fixture(port: u16, pid: u32) -> Self {
        Self { port, pid }
    }
}

pub(in crate::vm::windows) fn listener(
    child: &Child,
    port: u16,
) -> Result<Listener, NativeRefusal> {
    let pid = child.verify_live().map_err(|_| ENDPOINT)?;
    // TCP_TABLE_OWNER_PID_LISTENER. Include IPv6 to exclude any other
    // listener on the same port, including a dual-stack wildcard.
    let ipv4 = listener_table(2, 6)?;
    let ipv6 = listener_table(23, 14)?;
    let matches_port = |value: u32| u16::from_be(value as u16) == port;
    if ipv6.chunks_exact(14).any(|row| matches_port(row[5])) {
        return Err(ENDPOINT);
    }
    let mut rows = ipv4.chunks_exact(6).filter(|row| matches_port(row[2]));
    let row = rows.next().ok_or(ENDPOINT)?;
    if rows.next().is_some()
        || row[0] != 2
        || row[1].to_ne_bytes() != Ipv4Addr::LOCALHOST.octets()
        || child.verify_live().map_err(|_| ENDPOINT)? != pid
    {
        return Err(ENDPOINT);
    }
    let observed = Listener { port, pid: row[5] };
    observed.check(port, pid)?;
    Ok(observed)
}

fn listener_table(family: u32, row_words: usize) -> Result<Vec<u32>, NativeRefusal> {
    const OWNER_PID_LISTENER: i32 = 3;
    let mut bytes = 0;
    // Call once for the size and refuse table churn, oversize or short reads.
    let sizing =
        unsafe { GetExtendedTcpTable(null_mut(), &mut bytes, 0, family, OWNER_PID_LISTENER, 0) };
    if sizing != ERROR_INSUFFICIENT_BUFFER || !(4..=1024 * 1024).contains(&bytes) {
        return Err(ENDPOINT);
    }
    let capacity = bytes as usize;
    let mut data = vec![0u32; capacity.div_ceil(4)];
    let result = unsafe {
        GetExtendedTcpTable(
            data.as_mut_ptr().cast(),
            &mut bytes,
            0,
            family,
            OWNER_PID_LISTENER,
            0,
        )
    };
    let words = (data[0] as usize).checked_mul(row_words).ok_or(ENDPOINT)?;
    let needed = words
        .checked_add(1)
        .and_then(|n| n.checked_mul(4))
        .ok_or(ENDPOINT)?;
    if result != 0 || bytes as usize > capacity || needed > bytes as usize {
        return Err(ENDPOINT);
    }
    data.truncate(words + 1);
    data.remove(0);
    Ok(data)
}

// Win32_NetworkManagement_IpHelper is outside the manifest fence.
#[link(name = "iphlpapi")]
unsafe extern "system" {
    fn GetExtendedTcpTable(
        table: *mut std::ffi::c_void,
        size: *mut u32,
        order: windows_sys::core::BOOL,
        family: u32,
        class: i32,
        reserved: u32,
    ) -> u32;
}
