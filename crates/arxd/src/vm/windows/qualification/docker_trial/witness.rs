//! Private witness inputs. Only the scoped digest and verdict enter records.
use super::*;
use crate::vm::public_egress::{PublicEgressPolicy, RouteObservation};
use ring::signature::{ED25519, UnparsedPublicKey};
use std::net::Ipv4Addr;

const NETWORK: NativeRefusal = NativeRefusal::NetworkFenceUnverified;
const ENDPOINT: NativeRefusal = NativeRefusal::GuestEndpointUnverified;
pub(in crate::vm::windows::qualification) const MAX_CAPTURE_BYTES: usize = 64 * 1024;
pub(in crate::vm::windows::qualification) const MAX_INVENTORY_BYTES: u64 = 32 * 1024;

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
) -> Result<ObservedRoute, NativeRefusal> {
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
        .map_err(|_| NETWORK)?;
    Ok(sample.route.into())
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub(in crate::vm::windows::qualification) enum Case {
    Public,
    RecursiveDns,
    Host,
    HostLoopback,
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
    RouteDrift,
    LanInbound,
}

const REQUIRED: [Case; 22] = [
    Case::Public,
    Case::RecursiveDns,
    Case::Host,
    Case::HostLoopback,
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
    Case::RouteDrift,
    Case::LanInbound,
];

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub(in crate::vm::windows::qualification) enum Transport {
    Tcp,
    Udp,
    Icmp,
}

impl Case {
    fn transports(self) -> &'static [Transport] {
        match self {
            Self::Icmp => &[Transport::Icmp],
            Self::LanInbound => &[Transport::Tcp],
            Self::UdpPeerChange => &[Transport::Udp],
            _ => &[Transport::Tcp, Transport::Udp],
        }
    }

    pub(in crate::vm::windows::qualification) fn expected_arrivals(self) -> u16 {
        u16::from(matches!(self, Self::Public | Self::RecursiveDns))
    }
}

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::vm::windows::qualification) struct Target {
    pub case: Case,
    pub transport: Transport,
    pub identity: Hash,
}

/// The reviewed private enrollment is independent of the observer peers.
/// Its exact file digest is included with the three held fixture input pins
/// in the existing Docker dependency key.
#[derive(Clone)]
pub(in crate::vm::windows::qualification) struct ReceiverEnrollment {
    targets: Vec<Target>,
    fixture: Hash,
    signer: Hash,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiverManifest {
    version: u8,
    signer: Hash,
    targets: Vec<Target>,
}

impl ReceiverEnrollment {
    pub(in crate::vm::windows::qualification) fn from_manifest(
        bytes: &[u8],
        fixture_inputs: [Hash; 3],
    ) -> Result<Self, NativeRefusal> {
        if bytes.len() as u64 > MAX_INVENTORY_BYTES {
            return Err(NETWORK);
        }
        let ReceiverManifest {
            version,
            signer,
            mut targets,
        } = serde_json::from_slice(bytes).map_err(|_| NETWORK)?;
        targets.sort_unstable();
        if version != 1
            || signer == [0; 32]
            || targets.len() > 128
            || targets.windows(2).any(|pair| pair[0] == pair[1])
            || targets.iter().any(|target| {
                target.identity == [0; 32] || !target.case.transports().contains(&target.transport)
            })
            || !REQUIRED.iter().all(|case| {
                case.transports().iter().all(|transport| {
                    targets
                        .iter()
                        .any(|target| target.case == *case && target.transport == *transport)
                })
            })
        {
            return Err(NETWORK);
        }
        let binding = serde_json::to_vec(&("arx-docker-fixture-v3", fixture_inputs, digest(bytes)))
            .map_err(|_| NETWORK)?;
        Ok(Self {
            targets,
            fixture: digest(&binding),
            signer,
        })
    }

    pub(in crate::vm::windows::qualification) fn fixture(&self) -> Hash {
        self.fixture
    }

    pub(super) fn require_armed(
        &self,
        request: &CaptureRequest<'_>,
        signature: &[u8],
    ) -> Result<(), NativeRefusal> {
        self.authenticate(request, b"armed", &[], signature)
    }

    pub(super) fn capture_body<'a>(
        &self,
        request: &CaptureRequest<'_>,
        reply: &'a [u8],
    ) -> Result<&'a [u8], NativeRefusal> {
        let (signature, body) = reply.split_at_checked(64).ok_or(NETWORK)?;
        if body.is_empty() || body.len() > MAX_CAPTURE_BYTES {
            return Err(NETWORK);
        }
        self.authenticate(request, b"completed", body, signature)?;
        Ok(body)
    }

    fn authenticate(
        &self,
        request: &CaptureRequest<'_>,
        phase: &[u8],
        body: &[u8],
        signature: &[u8],
    ) -> Result<(), NativeRefusal> {
        UnparsedPublicKey::new(&ED25519, self.signer)
            .verify(&request.signing_message(phase, body)?, signature)
            .map_err(|_| NETWORK)
    }

    pub(super) fn targets(&self) -> &[Target] {
        &self.targets
    }

    pub(super) fn check_targets(
        &self,
        mut observed: Vec<Target>,
    ) -> Result<Vec<Target>, NativeRefusal> {
        if observed.len() != self.targets.len() {
            return Err(NETWORK);
        }
        observed.sort_unstable();
        if observed != self.targets {
            return Err(NETWORK);
        }
        Ok(observed)
    }
}

/// Private, canonical challenge for the enrolled capture observer. Each
/// acknowledgement covers the exact challenge, including the route and
/// inventory measured by the collector before the guest is allowed to run.
#[derive(Clone, Copy, Serialize)]
pub(in crate::vm::windows::qualification) struct CaptureRequest<'a> {
    pub run: Id,
    pub boot: Id,
    pub generation: u64,
    pub nonce: Id,
    pub route: ObservedRoute,
    pub topology: Hash,
    pub policy: Hash,
    pub origin_port: u16,
    pub targets: &'a [Target],
    pub issued_ms: u64,
    pub cutoff_ms: u64,
}

impl CaptureRequest<'_> {
    fn signing_message(&self, phase: &[u8], body: &[u8]) -> Result<Vec<u8>, NativeRefusal> {
        let challenge = serde_json::to_vec(self).map_err(|_| NETWORK)?;
        Ok([
            b"arx-docker-capture-v1\0".as_slice(),
            phase,
            b"\0",
            &digest(&challenge),
            &digest(body),
        ]
        .concat())
    }

    #[cfg(test)]
    pub(in crate::vm::windows::qualification) fn fixture_sign(
        &self,
        phase: &[u8],
        body: &[u8],
        foreign: bool,
    ) -> Vec<u8> {
        use ring::signature::Ed25519KeyPair;
        let seed = if foreign { [23; 32] } else { [17; 32] };
        let key = Ed25519KeyPair::from_seed_unchecked(&seed).unwrap();
        key.sign(&self.signing_message(phase, body).unwrap())
            .as_ref()
            .to_vec()
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::vm::windows::qualification) struct ObservedRoute {
    pub interface_index: u32,
    pub interface_luid: u64,
    pub source: Ipv4Addr,
    pub gateway: Ipv4Addr,
    pub default_route: bool,
}

impl From<RouteObservation> for ObservedRoute {
    fn from(route: RouteObservation) -> Self {
        Self {
            interface_index: route.interface_index,
            interface_luid: route.interface_luid,
            source: route.source,
            gateway: route.gateway,
            default_route: route.default_route,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::vm::windows::qualification) struct FlowEvent {
    pub flow: Id,
    pub at_ms: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::vm::windows::qualification) struct RouteChange {
    pub at_ms: u64,
    pub before: ObservedRoute,
    pub after: ObservedRoute,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::vm::windows::qualification) struct Drift {
    pub established: FlowEvent,
    pub established_arrivals: u16,
    pub closed: FlowEvent,
    pub refused: FlowEvent,
}

impl Drift {
    fn verifies(&self, drift_at: u64, counts: &Counts) -> bool {
        let [open, close] = counts.between_ms;
        let flow = self.established.flow;
        flow != [0; 16]
            && self.established_arrivals == 1
            && self.closed.flow == flow
            && self.refused.flow == flow
            && counts.before_ms <= self.established.at_ms
            && self.established.at_ms < drift_at
            && drift_at < self.closed.at_ms
            && self.closed.at_ms < open
            && (open..close).contains(&self.refused.at_ms)
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drift: Option<Drift>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::vm::windows::qualification) struct Receivers {
    pub run: Id,
    pub boot: Id,
    pub generation: u64,
    pub nonce: [u8; 16],
    pub route_change: RouteChange,
    pub observations: Vec<Counts>,
}

pub(super) fn receivers(
    raw: &[u8],
    boot: &RunningBoot<'_, '_>,
    finished_ms: u64,
) -> Result<Hash, NativeRefusal> {
    if raw.len() > MAX_CAPTURE_BYTES {
        return Err(NETWORK);
    }
    let pre_cont = boot.route.ok_or(NETWORK)?;
    let window = boot.started_ms..finished_ms;
    let capture: Receivers = serde_json::from_slice(raw).map_err(|_| NETWORK)?;
    if capture.run != boot.ticket.scope.run
        || capture.boot != boot.peer.boot
        || capture.generation != boot.ticket.challenge.generation
        || capture.nonce != boot.ticket.challenge.nonce
        || capture.observations.len() != boot.targets.len()
    {
        return Err(NETWORK);
    }
    let change = &capture.route_change;
    if change.before != pre_cont
        || change.after == change.before
        || change.after.interface_index == 0
        || change.after.interface_luid == 0
        || change.after.source.is_unspecified()
        || change.after.gateway.is_unspecified()
        || change.after.source == change.after.gateway
    {
        return Err(NETWORK);
    }
    let mut flows = Vec::new();
    for (counts, expected) in capture.observations.iter().zip(boot.targets) {
        let [open, close] = counts.between_ms;
        if counts.target != *expected
            || !window.contains(&counts.before_ms)
            || !window.contains(&counts.after_ms)
            || counts.before_ms >= open
            || open >= close
            || close >= counts.after_ms
            || counts.before != 1
            || counts.attempts != 1
            || counts.arrivals != expected.case.expected_arrivals()
            || counts.after != 1
        {
            return Err(NETWORK);
        }
        match (expected.case, &counts.drift) {
            (Case::RouteDrift, Some(drift))
                if drift.verifies(change.at_ms, counts)
                    && !flows.contains(&drift.established.flow) =>
            {
                flows.push(drift.established.flow);
            }
            (Case::RouteDrift, _) | (_, Some(_)) => return Err(NETWORK),
            (_, None) => {
                if counts.after_ms >= change.at_ms {
                    return Err(NETWORK);
                }
            }
        }
    }
    let canonical =
        serde_json::to_vec(&("arx-docker-receivers-v2", capture)).map_err(|_| NETWORK)?;
    Ok(digest(&canonical))
}

#[cfg(test)]
pub(in crate::vm::windows::qualification) fn fixture_manifest() -> Vec<u8> {
    use ring::signature::{Ed25519KeyPair, KeyPair};
    let key = Ed25519KeyPair::from_seed_unchecked(&[17; 32]).unwrap();
    serde_json::to_vec(&ReceiverManifest {
        version: 1,
        signer: key.public_key().as_ref().try_into().unwrap(),
        targets: fixture_targets(),
    })
    .unwrap()
}

#[cfg(test)]
pub(in crate::vm::windows::qualification) fn fixture_targets() -> Vec<Target> {
    let mut targets: Vec<_> = REQUIRED
        .iter()
        .flat_map(|case| {
            case.transports()
                .iter()
                .map(move |transport| (*case, *transport))
        })
        .enumerate()
        .map(|(index, (case, transport))| Target {
            case,
            transport,
            identity: [index as u8 + 1; 32],
        })
        .collect();
    targets.push(Target {
        case: Case::Private,
        transport: Transport::Tcp,
        identity: [100; 32],
    });
    targets.sort_unstable();
    targets
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
