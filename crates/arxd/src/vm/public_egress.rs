//! Public-only egress for an installed static boot (ADR 0064, design §2).
//!
//! The patched QEMU/libslirp backend enforces the policy outside the guest.
//! This module owns the Rust side of that fence: the strict policy v1
//! artifact (the E1a backend contract), its read-only custody, the reference
//! deny matcher that mirrors the backend's pre- and post-translation checks,
//! the enrolled-uplink route check, the separately pinned patched-binary
//! manifest and the admission gate. Admission success grants nothing by
//! itself: native launch of a public-egress plan stays refused with
//! `NetworkFenceUnverified` and `LaunchAuthority` stays uninhabited. Real
//! egress is **unverified on real hardware**.

use super::{
    paths::ArtifactPin,
    qemu_args::{CommandPlan, PlanKind},
    runtime::NativeRefusal,
    static_web::OriginPortLease,
};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::Read,
    net::{Ipv4Addr, SocketAddrV4},
    path::{Path, PathBuf},
};

/// The only policy version this build accepts.
pub const POLICY_VERSION: u32 = 1;
/// Upper bound of the policy artifact, checked before parsing.
pub const MAX_POLICY_BYTES: u64 = 32 * 1024;
/// Upper bound of `deny=` lines in the policy.
pub const MAX_POLICY_DENY: usize = 512;
/// The manifest identifier of the patched build. It is distinct from the
/// stock `qemu-11.1.3-windows-x64` manifest.
pub const EGRESS_MANIFEST: &str = "qemu-11.1.3-arx-public-egress-windows-x64";

/// Receipt digests of reviewed, qualified patched builds. Empty until the
/// E1a build, its source and its native receipt are independently reviewed;
/// until then every configured patched pin is refused as unqualified.
pub const REVIEWED_EGRESS_RECEIPTS: &[[u8; 32]] = &[];

/// The reviewed stock 11.1.3 executable (ADR 0054, #284). A public-egress pin
/// may never name these bytes: stock QEMU does not express the policy.
pub const STOCK_QEMU_1113: ArtifactPin = ArtifactPin {
    size: 28_531_009,
    sha256: hex32("0eff2d9977e8c8a6963d099ec3d2ebdae999b351c55d59c5ed3abcf97b0535f5"),
};
/// The reviewed stock 11.1.3 `RECEIPT.txt` (ADR 0054, #284).
pub const STOCK_RECEIPT_1113_SHA256: [u8; 32] =
    hex32("e0e33d3fb433c467f5f530bcb8cc8ce41bb8c3b10133bd54718915b7357b852e");

const fn hex32(text: &str) -> [u8; 32] {
    const fn nibble(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            _ => panic!("lowercase hex"),
        }
    }
    let bytes = text.as_bytes();
    assert!(bytes.len() == 64);
    let mut out = [0; 32];
    let mut i = 0;
    while i < 32 {
        out[i] = (nibble(bytes[2 * i]) << 4) | nibble(bytes[2 * i + 1]);
        i += 1;
    }
    out
}

/// A canonical IPv4 network: the host bits below the prefix are zero.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Ipv4Cidr {
    network: u32,
    prefix: u8,
}

impl Ipv4Cidr {
    const fn mask(prefix: u8) -> u32 {
        if prefix == 0 {
            0
        } else {
            u32::MAX << (32 - prefix)
        }
    }

    const fn frozen(a: u8, b: u8, c: u8, d: u8, prefix: u8) -> Self {
        let network = u32::from_be_bytes([a, b, c, d]);
        assert!(prefix <= 32 && network & !Self::mask(prefix) == 0);
        Self { network, prefix }
    }

    /// Strict dotted-quad `a.b.c.d/p`: decimal octets without sign, space
    /// or leading zero, a prefix of 0 to 32, and zero host bits.
    pub fn parse(text: &str) -> Option<Self> {
        let (address, prefix) = text.split_once('/')?;
        let address = parse_ipv4(address)?;
        let prefix = decimal(prefix, 32)? as u8;
        let network = u32::from(address);
        (network & !Self::mask(prefix) == 0).then_some(Self { network, prefix })
    }

    pub fn contains(self, address: Ipv4Addr) -> bool {
        u32::from(address) & Self::mask(self.prefix) == self.network
    }

    pub fn network(self) -> Ipv4Addr {
        self.network.into()
    }

    pub fn prefix(self) -> u8 {
        self.prefix
    }

    /// The highest address inside the network.
    pub fn last(self) -> Ipv4Addr {
        (self.network | !Self::mask(self.prefix)).into()
    }
}

impl std::fmt::Display for Ipv4Cidr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.network(), self.prefix)
    }
}

fn decimal(text: &str, max: u32) -> Option<u32> {
    let bytes = text.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 3
        || (bytes.len() > 1 && bytes[0] == b'0')
        || !bytes.iter().all(u8::is_ascii_digit)
    {
        return None;
    }
    text.parse().ok().filter(|value| *value <= max)
}

/// Strict dotted quad without leading zeros, sign or whitespace.
pub fn parse_ipv4(text: &str) -> Option<Ipv4Addr> {
    let mut octets = [0; 4];
    let mut parts = text.split('.');
    for octet in &mut octets {
        *octet = decimal(parts.next()?, 255)? as u8;
    }
    parts.next().is_none().then(|| Ipv4Addr::from(octets))
}

/// The slirp synthetic subnet. Guest-originated destinations inside it are
/// dropped before translation, including the 10.0.2.2 gateway and 10.0.2.3
/// DNS aliases that slirp would otherwise map to the Windows host.
pub const SYNTHETIC_SUBNET: Ipv4Cidr = Ipv4Cidr::frozen(10, 0, 2, 0, 24);
/// The fixed guest origin that the single loopback forward targets.
pub const GUEST_ORIGIN: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(10, 0, 2, 15), 443);

/// The frozen reviewed IPv4 special-purpose deny table (design §2 step 2,
/// IANA IPv4 special-purpose registry, non-global entries). It is compiled
/// in, as in the backend, so policy files need not repeat it.
pub const FROZEN_DENY: [Ipv4Cidr; 16] = [
    Ipv4Cidr::frozen(0, 0, 0, 0, 8),          // "this network"
    Ipv4Cidr::frozen(10, 0, 0, 0, 8),         // private
    Ipv4Cidr::frozen(100, 64, 0, 0, 10),      // CGNAT shared space
    Ipv4Cidr::frozen(127, 0, 0, 0, 8),        // loopback
    Ipv4Cidr::frozen(169, 254, 0, 0, 16),     // link-local
    Ipv4Cidr::frozen(172, 16, 0, 0, 12),      // private
    Ipv4Cidr::frozen(192, 0, 0, 0, 24),       // IETF protocol assignments
    Ipv4Cidr::frozen(192, 0, 2, 0, 24),       // TEST-NET-1 documentation
    Ipv4Cidr::frozen(192, 88, 99, 0, 24),     // deprecated 6to4 relay anycast
    Ipv4Cidr::frozen(192, 168, 0, 0, 16),     // private
    Ipv4Cidr::frozen(198, 18, 0, 0, 15),      // benchmarking
    Ipv4Cidr::frozen(198, 51, 100, 0, 24),    // TEST-NET-2 documentation
    Ipv4Cidr::frozen(203, 0, 113, 0, 24),     // TEST-NET-3 documentation
    Ipv4Cidr::frozen(224, 0, 0, 0, 4),        // multicast
    Ipv4Cidr::frozen(240, 0, 0, 0, 4),        // reserved
    Ipv4Cidr::frozen(255, 255, 255, 255, 32), // limited broadcast
];

/// The enrolled stable IPv4 uplink (design §2 step 3).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Uplink {
    pub interface_index: u32,
    /// The stable adapter identity; the backend compares it with the index's
    /// Windows LUID.
    pub interface_luid: u64,
    pub source: Ipv4Addr,
    pub gateway: Ipv4Addr,
}

/// A read-only route sample for one new flow or admission: the best route to
/// a public destination and the interface and source it would use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RouteObservation {
    pub interface_index: u32,
    pub interface_luid: u64,
    pub source: Ipv4Addr,
    pub gateway: Ipv4Addr,
    /// The best route is the qualified default route, not a more specific one.
    pub default_route: bool,
}

/// Typed reasons an admission or policy load refuses. Each maps to the
/// existing `NativeRefusal::NetworkFenceUnverified` at the native boundary,
/// so the MCP refusal vocabulary is unchanged.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EgressRefusal {
    /// The plan is not a sealed public-egress boot or its lease is not held.
    PlanMismatch,
    PolicyMissing,
    /// The anchored path, privacy, link or identity checks failed.
    PolicyCustody,
    /// A first line other than `version=1`.
    PolicyVersion,
    /// Not the exact v1 line grammar: encoding, order, field, CIDR, address,
    /// duplicate, bounds or final LF.
    PolicyMalformed,
    /// The enrolled uplink source or gateway is outside the deny union.
    PolicyIncomplete,
    /// The in-file digest does not match the preceding bytes.
    PolicyFingerprint,
    /// The custodied bytes differ from the plan's or the profile's pin.
    PolicyMismatch,
    RouteUnknown,
    RouteMismatch,
    BinaryMissing,
    /// A patched-binary pin names the reviewed stock bytes.
    BinaryStock,
    /// No reviewed receipt qualifies the configured patched build.
    BinaryUnqualified,
    /// The measured binary differs from the qualified pin.
    BinaryMismatch,
}

impl From<EgressRefusal> for NativeRefusal {
    fn from(_: EgressRefusal) -> Self {
        NativeRefusal::NetworkFenceUnverified
    }
}

/// Typed reasons the reference matcher refuses one flow.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FlowRefusal {
    /// Only TCP and UDP are proxied; ICMP, raw and other protocols are not.
    Transport,
    Port,
    /// Pre-translation destination inside the slirp synthetic subnet.
    Synthetic,
    /// Pre-translation destination in the deny union.
    Denied,
    /// The backend has no translation for this destination.
    Untranslatable,
    /// Final socket destination in the deny union.
    DeniedAfterTranslation,
    RouteUnknown,
    RouteMismatch,
    /// A host-initiated forward that is not the leased loopback listener to
    /// the guest origin.
    Forward,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Transport {
    Tcp,
    Udp,
    Icmp,
    Other(u8),
}

/// One guest-originated flow before translation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GuestFlow {
    pub transport: Transport,
    pub destination: SocketAddrV4,
}

/// A parsed, validated, fingerprint-checked policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicEgressPolicy {
    uplink: Uplink,
    deny: Vec<Ipv4Cidr>,
    fingerprint: [u8; 32],
}

/// Canonical nonzero decimal without sign or leading zero; overflow refuses.
fn nonzero_decimal<T: std::str::FromStr>(text: &str) -> Option<T> {
    let bytes = text.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 20
        || bytes[0] == b'0'
        || !bytes.iter().all(u8::is_ascii_digit)
    {
        return None;
    }
    text.parse().ok()
}

const FINGERPRINT_LINE: usize = "fingerprint=".len() + 64 + 1;

impl PublicEgressPolicy {
    /// Parse a policy v1 artifact, the E1a backend contract: ASCII, LF line
    /// endings and a final LF, at most 32 KiB, no spaces or blank lines, and
    /// the fields in this order:
    ///
    /// ```text
    /// version=1
    /// interface_index=<nonzero canonical u32 decimal>
    /// interface_luid=<nonzero canonical u64 decimal>
    /// source=<canonical IPv4>
    /// gateway=<canonical IPv4>
    /// deny=<canonical CIDR, prefix 1..=32, host bits clear>   0 to 512, distinct
    /// fingerprint=<lowercase SHA-256 of every preceding byte>
    /// ```
    ///
    /// The compiled special-purpose table and the synthetic subnet apply in
    /// addition to the `deny=` lines. The in-file digest checks integrity
    /// only; the supervisor authenticates the whole file by its pin.
    pub fn parse(bytes: &[u8]) -> Result<Self, EgressRefusal> {
        use EgressRefusal::{PolicyFingerprint, PolicyMalformed, PolicyVersion};
        if bytes.is_empty() || bytes.len() as u64 > MAX_POLICY_BYTES {
            return Err(PolicyMalformed);
        }
        // Printable ASCII and LF only: no CR, NUL, tab or space anywhere.
        if !bytes.iter().all(|b| *b == b'\n' || b.is_ascii_graphic()) {
            return Err(PolicyMalformed);
        }
        let text = std::str::from_utf8(bytes).map_err(|_| PolicyMalformed)?;
        let body = text.strip_suffix('\n').ok_or(PolicyMalformed)?;
        let mut lines = body.split('\n');
        let mut field = |name: &str| {
            lines
                .next()
                .and_then(|line| line.strip_prefix(name)?.strip_prefix('='))
        };
        if field("version") != Some("1") {
            return Err(PolicyVersion);
        }
        let uplink = Uplink {
            interface_index: field("interface_index")
                .and_then(nonzero_decimal)
                .ok_or(PolicyMalformed)?,
            interface_luid: field("interface_luid")
                .and_then(nonzero_decimal)
                .ok_or(PolicyMalformed)?,
            source: field("source")
                .and_then(parse_ipv4)
                .ok_or(PolicyMalformed)?,
            gateway: field("gateway")
                .and_then(parse_ipv4)
                .ok_or(PolicyMalformed)?,
        };
        let mut deny = Vec::new();
        let digest = loop {
            let line = lines.next().ok_or(PolicyMalformed)?;
            let Some(value) = line.strip_prefix("deny=") else {
                break line.strip_prefix("fingerprint=").ok_or(PolicyMalformed)?;
            };
            let entry = Ipv4Cidr::parse(value)
                .filter(|entry| entry.prefix >= 1)
                .ok_or(PolicyMalformed)?;
            if deny.len() == MAX_POLICY_DENY || deny.contains(&entry) {
                return Err(PolicyMalformed);
            }
            deny.push(entry);
        };
        if lines.next().is_some()
            || digest.len() != 64
            || !digest
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(PolicyMalformed);
        }
        let mut fingerprint = [0; 32];
        hex::decode_to_slice(digest, &mut fingerprint).map_err(|_| PolicyMalformed)?;
        // Every byte before the fingerprint line, including the LF before it.
        let covered = &bytes[..bytes.len() - FINGERPRINT_LINE];
        if <[u8; 32]>::from(Sha256::digest(covered)) != fingerprint {
            return Err(PolicyFingerprint);
        }
        let policy = Self {
            uplink,
            deny,
            fingerprint,
        };
        policy.check_complete()?;
        Ok(policy)
    }

    /// Admission is stricter than the backend, which also captures interface
    /// addresses at runtime: the enrolled source and gateway must already be
    /// in the deny union, so the uplink's own host and router are refused.
    fn check_complete(&self) -> Result<(), EgressRefusal> {
        let uplink = self.uplink;
        if uplink.source == uplink.gateway
            || !self.denies(uplink.source)
            || !self.denies(uplink.gateway)
        {
            return Err(EgressRefusal::PolicyIncomplete);
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint
    }

    pub fn uplink(&self) -> Uplink {
        self.uplink
    }

    /// The enrolled host, LAN, VPN, router and hairpin inventory.
    pub fn deny(&self) -> &[Ipv4Cidr] {
        &self.deny
    }

    /// The deny union for the boot: the compiled frozen table, the synthetic
    /// subnet and the enrolled `deny=` inventory.
    pub fn denies(&self, address: Ipv4Addr) -> bool {
        FROZEN_DENY
            .iter()
            .chain([&SYNTHETIC_SUBNET])
            .chain(&self.deny)
            .any(|entry| entry.contains(address))
    }

    /// The best route must be the qualified default via the enrolled uplink,
    /// interface and source.
    pub fn verify_route(&self, route: Option<&RouteObservation>) -> Result<(), EgressRefusal> {
        let route = route.ok_or(EgressRefusal::RouteUnknown)?;
        let uplink = self.uplink;
        if !route.default_route
            || route.interface_index != uplink.interface_index
            || route.interface_luid != uplink.interface_luid
            || route.source != uplink.source
            || route.gateway != uplink.gateway
        {
            return Err(EgressRefusal::RouteMismatch);
        }
        Ok(())
    }

    /// The reference matcher for one guest-initiated flow, mirroring the
    /// backend: transport, synthetic subnet and deny union before
    /// translation, the deny union again on the final socket destination,
    /// then the route. `translate` is slirp's address translation.
    pub fn admit_flow(
        &self,
        flow: GuestFlow,
        translate: impl FnOnce(Ipv4Addr) -> Option<Ipv4Addr>,
        route: Option<&RouteObservation>,
    ) -> Result<SocketAddrV4, FlowRefusal> {
        if !matches!(flow.transport, Transport::Tcp | Transport::Udp) {
            return Err(FlowRefusal::Transport);
        }
        if flow.destination.port() == 0 {
            return Err(FlowRefusal::Port);
        }
        let before = *flow.destination.ip();
        if SYNTHETIC_SUBNET.contains(before) {
            return Err(FlowRefusal::Synthetic);
        }
        if self.denies(before) {
            return Err(FlowRefusal::Denied);
        }
        let after = translate(before).ok_or(FlowRefusal::Untranslatable)?;
        if self.denies(after) {
            return Err(FlowRefusal::DeniedAfterTranslation);
        }
        self.verify_route(route).map_err(|refusal| match refusal {
            EgressRefusal::RouteUnknown => FlowRefusal::RouteUnknown,
            _ => FlowRefusal::RouteMismatch,
        })?;
        Ok(SocketAddrV4::new(after, flow.destination.port()))
    }
}

/// The single inbound forward. It is host-initiated only: the leased IPv4
/// loopback listener to the fixed guest origin. A guest-initiated flow to the
/// synthetic forward peer is refused by [`PublicEgressPolicy::admit_flow`]
/// before translation, so the forward grants no guest egress.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalForward {
    port: u16,
}

impl LocalForward {
    /// Reuses the A2 supervisor lease; refuses a lease no longer held.
    pub fn from_lease(lease: &OriginPortLease) -> Option<Self> {
        lease.verify().then(|| Self { port: lease.port() })
    }

    pub fn port(self) -> u16 {
        self.port
    }

    pub fn admit_inbound(
        self,
        listener: SocketAddrV4,
        guest: SocketAddrV4,
    ) -> Result<(), FlowRefusal> {
        if listener != SocketAddrV4::new(Ipv4Addr::LOCALHOST, self.port) || guest != GUEST_ORIGIN {
            return Err(FlowRefusal::Forward);
        }
        Ok(())
    }
}

/// The configured pin set of a patched build: the console executable, which
/// statically links the patched libslirp 4.9.7 (there is no libslirp DLL
/// slot in this build), and the build/native receipt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EgressBinaryPin {
    pub qemu: ArtifactPin,
    pub receipt: ArtifactPin,
}

/// Proof that a configured pin is a reviewed, qualified patched build.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QualifiedEgressBinary(EgressBinaryPin);

impl QualifiedEgressBinary {
    pub fn pin(self) -> EgressBinaryPin {
        self.0
    }
}

impl EgressBinaryPin {
    /// Refuses the stock bytes and any build without a reviewed receipt.
    pub fn qualify(self) -> Result<QualifiedEgressBinary, EgressRefusal> {
        self.qualify_against(REVIEWED_EGRESS_RECEIPTS)
    }

    fn qualify_against(
        self,
        reviewed: &[[u8; 32]],
    ) -> Result<QualifiedEgressBinary, EgressRefusal> {
        let pins = [self.qemu, self.receipt];
        if pins.iter().any(|pin| pin.size == 0) {
            return Err(EgressRefusal::BinaryMissing);
        }
        if pins.iter().any(|pin| {
            pin.sha256 == STOCK_QEMU_1113.sha256 || pin.sha256 == STOCK_RECEIPT_1113_SHA256
        }) {
            return Err(EgressRefusal::BinaryStock);
        }
        if !reviewed.contains(&self.receipt.sha256) {
            return Err(EgressRefusal::BinaryUnqualified);
        }
        Ok(QualifiedEgressBinary(self))
    }
}

/// Evidence presented to admission. Every member is required.
#[derive(Clone, Copy, Default)]
pub struct EgressEvidence<'a> {
    pub policy: Option<&'a CustodiedPolicy>,
    pub route: Option<&'a RouteObservation>,
    /// The profile's configured patched-binary pin.
    pub binary: Option<&'a EgressBinaryPin>,
    /// The pins measured through the launcher's held custody.
    pub measured: Option<&'a EgressBinaryPin>,
}

/// Admission of a sealed public-egress plan. Success is a portable check
/// only: native launch independently refuses `PublicEgressBoot`.
pub fn admit(
    plan: &CommandPlan<'_>,
    evidence: EgressEvidence<'_>,
) -> Result<QualifiedEgressBinary, EgressRefusal> {
    admit_against(plan, evidence, REVIEWED_EGRESS_RECEIPTS)
}

fn admit_against(
    plan: &CommandPlan<'_>,
    evidence: EgressEvidence<'_>,
    reviewed: &[[u8; 32]],
) -> Result<QualifiedEgressBinary, EgressRefusal> {
    let planned = plan
        .public_egress_policy()
        .and_then(super::paths::Artifact::pin)
        .filter(|_| plan.kind() == PlanKind::PublicEgressBoot)
        .filter(|_| {
            plan.static_origin_lease()
                .is_some_and(OriginPortLease::verify)
        })
        .ok_or(EgressRefusal::PlanMismatch)?;
    let policy = evidence.policy.ok_or(EgressRefusal::PolicyMissing)?;
    policy.verify()?;
    if policy.pin() != planned {
        return Err(EgressRefusal::PolicyMismatch);
    }
    policy.policy().verify_route(evidence.route)?;
    let qualified = evidence
        .binary
        .ok_or(EgressRefusal::BinaryMissing)?
        .qualify_against(reviewed)?;
    if evidence.measured != Some(&qualified.pin()) {
        return Err(EgressRefusal::BinaryMismatch);
    }
    Ok(qualified)
}

/// A policy read through a held, read-only handle under an anchored private
/// directory. On Windows the file is held without write or delete sharing
/// and the directory without delete sharing, so neither can be replaced
/// while custody lives. POSIX holds the descriptors and re-verifies the
/// identity: the 0700/0600 owner-only mode leaves the same effective UID as
/// the only principal that could rename, the stated POSIX trust boundary.
pub struct CustodiedPolicy {
    _directory: File,
    file: File,
    identity: (u64, u64),
    path: PathBuf,
    pin: ArtifactPin,
    policy: PublicEgressPolicy,
}

impl std::fmt::Debug for CustodiedPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CustodiedPolicy")
            .field("pin", &self.pin)
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

fn leaf_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && !name.starts_with('.')
        && !name.ends_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

impl CustodiedPolicy {
    /// Open `directory/name` read-only, verify the expected pin over the
    /// exact bytes read through the held handle, then parse.
    pub fn open(
        directory: &Path,
        name: &str,
        expected: ArtifactPin,
    ) -> Result<Self, EgressRefusal> {
        let custody = |_| EgressRefusal::PolicyCustody;
        if !leaf_name(name) || !(1..=MAX_POLICY_BYTES).contains(&expected.size) {
            return Err(EgressRefusal::PolicyCustody);
        }
        crate::config::checked_path(directory).map_err(custody)?;
        crate::security::private(directory).map_err(custody)?;
        let held_directory = custody::directory(directory).map_err(custody)?;
        let path = directory.join(name);
        custody::check_leaf(&path).map_err(custody)?;
        let mut file = custody::read_only(&path).map_err(custody)?;
        let identity = custody::identity(&file).map_err(custody)?;
        let mut bytes = Vec::new();
        (&mut file)
            .take(MAX_POLICY_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| EgressRefusal::PolicyCustody)?;
        if !matches!(
            expected.verify(bytes.as_slice(), MAX_POLICY_BYTES),
            Ok(true)
        ) {
            return Err(EgressRefusal::PolicyMismatch);
        }
        let policy = PublicEgressPolicy::parse(&bytes)?;
        let custodied = Self {
            _directory: held_directory,
            file,
            identity,
            path,
            pin: expected,
            policy,
        };
        custodied.verify()?;
        Ok(custodied)
    }

    /// The name still denotes the held object.
    pub fn verify(&self) -> Result<(), EgressRefusal> {
        let held = custody::identity(&self.file);
        let named = custody::check_leaf(&self.path)
            .and_then(|()| custody::read_only(&self.path))
            .and_then(|file| custody::identity(&file));
        match (held, named) {
            (Ok(held), Ok(named)) if held == self.identity && named == self.identity => Ok(()),
            _ => Err(EgressRefusal::PolicyCustody),
        }
    }

    pub fn pin(&self) -> ArtifactPin {
        self.pin
    }

    pub fn policy(&self) -> &PublicEgressPolicy {
        &self.policy
    }
}

mod custody {
    use anyhow::{Result, ensure};
    use std::{fs, path::Path};

    pub(super) fn check_leaf(path: &Path) -> Result<()> {
        crate::config::checked_path(path)?;
        ensure!(
            fs::symlink_metadata(path)?.is_file(),
            "regular policy file required"
        );
        crate::security::private(path)
    }

    pub(super) fn read_only(path: &Path) -> Result<fs::File> {
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::{
                FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
            };
            // No write or delete sharing: the bytes cannot change and the
            // name cannot be renamed or deleted while this handle lives.
            options
                .share_mode(FILE_SHARE_READ)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        }
        Ok(options.open(path)?)
    }

    pub(super) fn directory(path: &Path) -> Result<fs::File> {
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::{
                FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
                FILE_SHARE_WRITE,
            };
            // No delete sharing: the anchor cannot be renamed while held.
            options
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
        }
        let directory = options.open(path)?;
        ensure!(directory.metadata()?.is_dir(), "policy directory required");
        Ok(directory)
    }

    #[cfg(unix)]
    pub(super) fn identity(file: &fs::File) -> Result<(u64, u64)> {
        use std::os::unix::fs::MetadataExt;
        let meta = file.metadata()?;
        ensure!(
            meta.is_file() && meta.nlink() == 1,
            "single-link policy file required"
        );
        Ok((meta.dev(), meta.ino()))
    }

    #[cfg(windows)]
    pub(super) fn identity(file: &fs::File) -> Result<(u64, u64)> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
            FILE_TYPE_DISK, GetFileInformationByHandle, GetFileType,
        };
        let handle = file.as_raw_handle();
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // `file` owns the handle for the duration of both queries.
        ensure!(
            unsafe {
                GetFileType(handle) == FILE_TYPE_DISK
                    && GetFileInformationByHandle(handle, &mut info) != 0
            } && info.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT)
                == 0
                && info.nNumberOfLinks == 1,
            "single-link policy file required"
        );
        Ok((
            u64::from(info.dwVolumeSerialNumber),
            (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::{
        paths::{Artifact, ArtifactRole, FileIdentity, LocalPath},
        qemu_args::{BootPurpose, BootRecipe, SerialName, StaticNetwork},
    };

    fn pin(byte: u8) -> ArtifactPin {
        ArtifactPin {
            size: 1000 + u64::from(byte),
            sha256: [byte; 32],
        }
    }

    fn binary() -> EgressBinaryPin {
        EgressBinaryPin {
            qemu: pin(1),
            receipt: pin(3),
        }
    }

    #[test]
    fn frozen_table_is_canonical_and_the_stock_constants_decode() {
        assert!(
            FROZEN_DENY
                .iter()
                .all(|entry| Ipv4Cidr::parse(&entry.to_string()) == Some(*entry))
        );
        assert_eq!(STOCK_QEMU_1113.sha256[..2], [0x0e, 0xff]);
        assert_eq!(STOCK_RECEIPT_1113_SHA256[31], 0x2e);
        assert!(
            REVIEWED_EGRESS_RECEIPTS.is_empty(),
            "no patched build is reviewed yet"
        );
    }

    #[test]
    fn only_a_reviewed_non_stock_receipt_qualifies() {
        let reviewed = [[3; 32]];
        assert_eq!(binary().qualify(), Err(EgressRefusal::BinaryUnqualified));
        assert_eq!(
            binary()
                .qualify_against(&reviewed)
                .map(QualifiedEgressBinary::pin),
            Ok(binary())
        );
        let mut stock = binary();
        stock.qemu = STOCK_QEMU_1113;
        assert_eq!(
            stock.qualify_against(&reviewed),
            Err(EgressRefusal::BinaryStock)
        );
        let mut stock = binary();
        stock.receipt.sha256 = STOCK_RECEIPT_1113_SHA256;
        assert_eq!(
            stock.qualify_against(&[STOCK_RECEIPT_1113_SHA256]),
            Err(EgressRefusal::BinaryStock)
        );
        let mut empty = binary();
        empty.qemu.size = 0;
        assert_eq!(
            empty.qualify_against(&reviewed),
            Err(EgressRefusal::BinaryMissing)
        );
    }

    /// The positive path needs a reviewed receipt, which only this private
    /// seam can supply; production always refuses until one is reviewed.
    #[test]
    fn admission_accepts_only_matching_qualified_evidence() {
        let dir = fixtures::private_dir();
        let body = fixtures::policy_text();
        let expected = fixtures::write_private(dir.path(), "policy.txt", body.as_bytes());
        let custodied = CustodiedPolicy::open(dir.path(), "policy.txt", expected).unwrap();
        let lease = OriginPortLease::acquire().unwrap();
        let artifact =
            |role, path: &str| Artifact::new(role, LocalPath::parse(path).unwrap()).unwrap();
        let policy = artifact(ArtifactRole::PublicEgressPolicy, "D:/vm/egress/policy.txt")
            .with_identity(FileIdentity {
                volume: 1,
                file: [7; 16],
            })
            .with_pin(expected);
        let image = artifact(
            ArtifactRole::PublicEgressQemu,
            "D:/egress/qemu-system-x86_64.exe",
        );
        let data = artifact(ArtifactRole::Data, "D:/egress/share");
        let firmware = artifact(ArtifactRole::FirmwareCode, "D:/firmware/code.fd");
        let vars = artifact(ArtifactRole::StaticVars, "D:/vm/vars.fd");
        let disk = artifact(ArtifactRole::StaticDisk, "D:/vm/disk.qcow2");
        let header = fixtures::qcow2_header();
        let serial = SerialName::generate().unwrap();
        let recipe = BootRecipe {
            image: &image,
            data: &data,
            firmware: &firmware,
            vars: &vars,
            disk: &disk,
            disk_header: &header,
            backing: None,
            backing_header: None,
            serial: &serial,
            boot: BootPurpose::InstalledStaticWeb(&lease, StaticNetwork::PublicEgress(&policy)),
            ram_mib: 256,
            vcpus: 1,
        };
        let plan = recipe.plan().unwrap();
        let route = fixtures::route();
        let configured = binary();
        let reviewed = [configured.receipt.sha256];
        let evidence = EgressEvidence {
            policy: Some(&custodied),
            route: Some(&route),
            binary: Some(&configured),
            measured: Some(&configured),
        };
        assert_eq!(
            admit_against(&plan, evidence, &reviewed).map(QualifiedEgressBinary::pin),
            Ok(configured)
        );
        assert_eq!(
            admit(&plan, evidence),
            Err(EgressRefusal::BinaryUnqualified)
        );
        let mut other = configured;
        other.qemu.sha256 = [9; 32];
        for (evidence, refusal) in [
            (
                EgressEvidence {
                    measured: Some(&other),
                    ..evidence
                },
                EgressRefusal::BinaryMismatch,
            ),
            (
                EgressEvidence {
                    measured: None,
                    ..evidence
                },
                EgressRefusal::BinaryMismatch,
            ),
            (
                EgressEvidence {
                    binary: None,
                    ..evidence
                },
                EgressRefusal::BinaryMissing,
            ),
        ] {
            assert_eq!(admit_against(&plan, evidence, &reviewed), Err(refusal));
        }
    }

    mod fixtures {
        use super::*;

        pub(super) fn route() -> RouteObservation {
            RouteObservation {
                interface_index: 12,
                interface_luid: 1_689_399_632_478_208,
                source: Ipv4Addr::new(192, 168, 1, 20),
                gateway: Ipv4Addr::new(192, 168, 1, 1),
                default_route: true,
            }
        }

        pub(super) fn policy_text() -> String {
            let mut text = String::from(
                "version=1\ninterface_index=12\ninterface_luid=1689399632478208\nsource=192.168.1.20\ngateway=192.168.1.1\ndeny=45.12.8.0/24\n",
            );
            let digest = hex::encode(Sha256::digest(text.as_bytes()));
            text.push_str(&format!("fingerprint={digest}\n"));
            text
        }

        pub(super) struct PrivateDir(tempfile::TempDir);

        impl PrivateDir {
            pub(super) fn path(&self) -> &Path {
                self.0.path()
            }
        }

        /// 0700, or TokenUser owner with a protected TokenUser-only DACL.
        pub(super) fn private_dir() -> PrivateDir {
            let root = std::env::temp_dir().canonicalize().unwrap();
            #[cfg(windows)]
            let root = PathBuf::from(
                root.to_str()
                    .and_then(|path| path.strip_prefix(r"\\?\"))
                    .unwrap(),
            );
            let dir = tempfile::tempdir_in(root).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))
                    .unwrap();
            }
            #[cfg(windows)]
            protect(dir.path(), true);
            PrivateDir(dir)
        }

        pub(super) fn write_private(dir: &Path, name: &str, body: &[u8]) -> ArtifactPin {
            let path = dir.join(name);
            std::fs::write(&path, body).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            }
            #[cfg(windows)]
            protect(&path, false);
            ArtifactPin {
                size: body.len() as u64,
                sha256: Sha256::digest(body).into(),
            }
        }

        /// TokenUser owner and a protected DACL granting only TokenUser.
        #[cfg(windows)]
        fn protect(path: &Path, directory: bool) {
            use std::{os::windows::ffi::OsStrExt, ptr};
            use windows_sys::Win32::{
                Foundation::{CloseHandle, LocalFree},
                Security::{Authorization::*, *},
                System::Threading::{GetCurrentProcess, OpenProcessToken},
            };
            // Test-only Win32 calls on owned buffers that outlive each use.
            unsafe {
                let mut token = ptr::null_mut();
                assert_ne!(
                    OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token),
                    0
                );
                let mut len = 0;
                GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut len);
                let mut user = vec![0usize; (len as usize).div_ceil(size_of::<usize>())];
                let ok =
                    GetTokenInformation(token, TokenUser, user.as_mut_ptr().cast(), len, &mut len);
                CloseHandle(token);
                assert_ne!(ok, 0);
                let sid = (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid;
                let mut text = ptr::null_mut();
                assert_ne!(ConvertSidToStringSidW(sid, &mut text), 0);
                let n = (0..184).find(|i| *text.add(*i) == 0).unwrap();
                let sid_text = String::from_utf16(std::slice::from_raw_parts(text, n)).unwrap();
                LocalFree(text.cast());
                let flags = if directory { "OICI" } else { "" };
                let sddl: Vec<u16> = format!("D:P(A;{flags};FA;;;{sid_text})")
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                let mut descriptor = ptr::null_mut();
                assert_ne!(
                    ConvertStringSecurityDescriptorToSecurityDescriptorW(
                        sddl.as_ptr(),
                        1,
                        &mut descriptor,
                        ptr::null_mut()
                    ),
                    0
                );
                let (mut present, mut defaulted, mut acl) = (0, 0, ptr::null_mut());
                assert_ne!(
                    GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted),
                    0
                );
                let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
                let dacl = SetNamedSecurityInfoW(
                    wide.as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    acl,
                    ptr::null_mut(),
                );
                LocalFree(descriptor);
                assert_eq!(dacl, 0);
                let owner = SetNamedSecurityInfoW(
                    wide.as_ptr(),
                    SE_FILE_OBJECT,
                    OWNER_SECURITY_INFORMATION,
                    sid,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
                assert_eq!(owner, 0);
            }
        }

        /// An unbacked 11.1.x qcow2 v3 header: magic, version 3, cluster bits
        /// 16, 1 MiB size, refcount order 4 and a 112-byte header length.
        pub(super) fn qcow2_header() -> Vec<u8> {
            let mut header = vec![0; crate::vm::qemu_img::CLUSTER];
            header[..4].copy_from_slice(&[b'Q', b'F', b'I', 0xfb]);
            for (at, val) in [(4, 3u32), (20, 16), (96, 4), (100, 112)] {
                header[at..at + 4].copy_from_slice(&val.to_be_bytes());
            }
            header[24..32].copy_from_slice(&1_048_576u64.to_be_bytes());
            header
        }
    }
}
