//! ADR 0064 E1b: the Rust side of the public-only static boot fence. Pure
//! fixtures only; no QEMU, network or elevation. Real egress is
//! **unverified on real hardware**.

use arxd::{
    config::VmRuntimeProfile,
    vm::{
        paths::{Artifact, ArtifactPin, ArtifactRole, FileIdentity, LocalPath},
        public_egress::{
            CustodiedPolicy, EGRESS_MANIFEST, EgressBinaryPin, EgressEvidence, EgressRefusal,
            FROZEN_DENY, FlowRefusal, GuestFlow, HeldExecutable, Ipv4Cidr, LocalForward,
            MAX_POLICY_BYTES, MAX_POLICY_DENY, PublicEgressPolicy, RouteObservation,
            STOCK_QEMU_1113, STOCK_RECEIPT_1113_SHA256, SYNTHETIC_SUBNET, Transport, admit,
        },
        qemu_args::{BootPurpose, BootRecipe, NETWORK, PlanKind, SerialName, StaticNetwork},
        runtime::NativeRefusal,
        static_web::{ORIGIN_PORT_FIRST, ORIGIN_PORT_LAST, OriginPortLease},
    },
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    net::{Ipv4Addr, SocketAddrV4},
    path::Path,
};

const LUID: u64 = 1_689_399_632_478_208;
const SOURCE: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 20);
const GATEWAY: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 1);
const PUBLIC: Ipv4Addr = Ipv4Addr::new(1, 1, 1, 1);

fn route() -> RouteObservation {
    RouteObservation {
        interface_index: 12,
        interface_luid: LUID,
        source: SOURCE,
        gateway: GATEWAY,
        default_route: true,
    }
}

/// Header lines then deny lines, sealed with the byte fingerprint.
fn seal(head: &str, deny: &[&str]) -> String {
    let mut text = head.to_owned();
    for entry in deny {
        text.push_str(&format!("deny={entry}\n"));
    }
    let digest = hex::encode(Sha256::digest(text.as_bytes()));
    text + &format!("fingerprint={digest}\n")
}

const HEAD: &str = "version=1\ninterface_index=12\ninterface_luid=1689399632478208\nsource=192.168.1.20\ngateway=192.168.1.1\n";
const SITE: [&str; 2] = ["45.12.8.0/24", "45.12.9.7/32"];

fn policy_text() -> String {
    seal(HEAD, &SITE)
}

fn policy() -> PublicEgressPolicy {
    PublicEgressPolicy::parse(policy_text().as_bytes()).unwrap()
}

fn artifact(role: ArtifactRole, path: &str) -> Artifact {
    Artifact::new(role, LocalPath::parse(path).unwrap()).unwrap()
}

fn policy_artifact(pin: ArtifactPin) -> Artifact {
    artifact(ArtifactRole::PublicEgressPolicy, "D:/vm/egress/policy.txt")
        .with_identity(FileIdentity {
            volume: 1,
            file: [7; 16],
        })
        .with_pin(pin)
}

fn digest_pin(body: &[u8]) -> ArtifactPin {
    ArtifactPin {
        size: body.len() as u64,
        sha256: Sha256::digest(body).into(),
    }
}

fn qcow2_header() -> Vec<u8> {
    let mut header = vec![0; 65_536];
    header[..4].copy_from_slice(&[b'Q', b'F', b'I', 0xfb]);
    for (at, val) in [(4, 3u32), (20, 16), (96, 4), (100, 112)] {
        header[at..at + 4].copy_from_slice(&val.to_be_bytes());
    }
    header[24..32].copy_from_slice(&1_048_576u64.to_be_bytes());
    header
}

struct Inputs {
    stock: Artifact,
    patched: Artifact,
    data: Artifact,
    firmware: Artifact,
    static_vars: Artifact,
    static_disk: Artifact,
    vars: Artifact,
    disk: Artifact,
    header: Vec<u8>,
    serial: SerialName,
}

impl Inputs {
    fn new() -> Self {
        use ArtifactRole::*;
        Self {
            stock: artifact(Qemu, "D:/qemu/qemu-system-x86_64.exe"),
            patched: artifact(PublicEgressQemu, "D:/qemu-egress/qemu-system-x86_64.exe")
                .with_identity(FileIdentity {
                    volume: 2,
                    file: [9; 16],
                })
                .with_pin(ArtifactPin {
                    size: 30_000_000,
                    sha256: [0xa1; 32],
                }),
            data: artifact(Data, "D:/qemu/share"),
            firmware: artifact(FirmwareCode, "D:/firmware/code.fd"),
            static_vars: artifact(StaticVars, "D:/vm/vars.fd"),
            static_disk: artifact(StaticDisk, "D:/vm/disk.qcow2"),
            vars: artifact(Vars, "D:/test/vars.fd"),
            disk: artifact(GuestDisk, "D:/test/disk.qcow2"),
            header: qcow2_header(),
            serial: SerialName::generate().unwrap(),
        }
    }

    fn recipe<'a>(&'a self, image: &'a Artifact, boot: BootPurpose<'a>) -> BootRecipe<'a> {
        let test = matches!(boot, BootPurpose::Installed);
        BootRecipe {
            image,
            data: &self.data,
            firmware: &self.firmware,
            vars: if test { &self.vars } else { &self.static_vars },
            disk: if test { &self.disk } else { &self.static_disk },
            disk_header: &self.header,
            backing: None,
            backing_header: None,
            serial: &self.serial,
            boot,
            ram_mib: 256,
            vcpus: 1,
        }
    }
}

fn after<'a>(args: &'a [String], flag: &str) -> Vec<&'a str> {
    args.windows(2)
        .filter(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
        .collect()
}

#[test]
fn public_egress_plan_emits_the_exact_contract_argv() {
    let inputs = Inputs::new();
    let lease = OriginPortLease::acquire().unwrap();
    let pin = digest_pin(policy_text().as_bytes());
    let policy = policy_artifact(pin);
    let recipe = inputs.recipe(
        &inputs.patched,
        BootPurpose::InstalledStaticWeb(&lease, StaticNetwork::PublicEgress(&policy)),
    );
    let plan = recipe.plan().unwrap();
    assert_eq!(plan.kind(), PlanKind::PublicEgressBoot);
    assert_eq!(
        plan.image().as_str(),
        r"D:\qemu-egress\qemu-system-x86_64.exe"
    );
    assert_eq!(plan.static_origin_lease().unwrap().port(), lease.port());
    assert!(plan.public_egress_policy().is_some());
    assert_eq!(
        after(plan.args(), "-netdev"),
        vec![format!(
            "user,id=arxnet0,restrict=off,ipv4=on,ipv6=off,net=10.0.2.0/24,host=10.0.2.2,dns=10.0.2.3,arx-public-egress=D:\\vm\\egress\\policy.txt,hostfwd=tcp:127.0.0.1:{}-10.0.2.15:443",
            lease.port()
        )]
    );
    let netdev = plan.args().iter().position(|a| a == "-netdev").unwrap();
    assert_eq!(
        plan.args()[netdev + 2..netdev + 4],
        ["-device", "virtio-net-pci,netdev=arxnet0"]
    );
    assert_eq!(
        after(plan.args(), "-device")
            .iter()
            .filter(|d| d.starts_with("virtio-net-pci"))
            .count(),
        1
    );
    for forbidden in [
        "guestfwd",
        "smb=",
        "tftp",
        "bootfile",
        "restrict=on",
        "ipv6=on",
    ] {
        assert!(
            plan.args().iter().all(|arg| !arg.contains(forbidden)),
            "{forbidden}"
        );
    }
}

#[test]
fn public_egress_requires_the_patched_image_and_a_custodied_bounded_policy() {
    use ArtifactRole::*;
    let inputs = Inputs::new();
    let lease = OriginPortLease::acquire().unwrap();
    let pin = digest_pin(policy_text().as_bytes());
    let good = policy_artifact(pin);
    let egress =
        |policy| BootPurpose::InstalledStaticWeb(&lease, StaticNetwork::PublicEgress(policy));
    assert!(inputs.recipe(&inputs.patched, egress(&good)).plan().is_ok());
    assert!(
        inputs.recipe(&inputs.stock, egress(&good)).plan().is_err(),
        "stock QEMU cannot run a public-egress plan"
    );
    let unheld = artifact(PublicEgressPolicy, "D:/vm/egress/policy.txt").with_pin(pin);
    let unpinned =
        artifact(PublicEgressPolicy, "D:/vm/egress/policy.txt").with_identity(FileIdentity {
            volume: 1,
            file: [7; 16],
        });
    let oversized = policy_artifact(ArtifactPin {
        size: MAX_POLICY_BYTES + 1,
        ..pin
    });
    let empty = policy_artifact(ArtifactPin { size: 0, ..pin });
    let wrong_role = artifact(Data, "D:/vm/egress/policy.txt")
        .with_identity(FileIdentity {
            volume: 1,
            file: [7; 16],
        })
        .with_pin(pin);
    for policy in [&unheld, &unpinned, &oversized, &empty, &wrong_role] {
        assert!(
            inputs
                .recipe(&inputs.patched, egress(policy))
                .plan()
                .is_err()
        );
    }
    let unheld_image = artifact(PublicEgressQemu, "D:/qemu-egress/qemu-system-x86_64.exe");
    assert!(
        inputs.recipe(&unheld_image, egress(&good)).plan().is_err(),
        "the patched image must name a held, pinned object"
    );
    let restricted = BootPurpose::InstalledStaticWeb(&lease, StaticNetwork::RestrictedOrigin);
    assert!(
        inputs.recipe(&inputs.patched, restricted).plan().is_err(),
        "the patched build never runs the restricted recipe"
    );
    assert!(
        inputs
            .recipe(&inputs.patched, BootPurpose::Installed)
            .plan()
            .is_err(),
        "the patched build never runs a TEST boot"
    );
    let mut backed = inputs.recipe(&inputs.patched, egress(&good));
    let golden = artifact(Golden, "D:/vm/golden.qcow2");
    backed.backing = Some(&golden);
    backed.backing_header = Some(&inputs.header);
    assert!(backed.plan().is_err(), "static disks are unbacked");
}

#[test]
fn restricted_origin_and_test_argv_are_unchanged_by_the_new_variant() {
    let inputs = Inputs::new();
    let lease = OriginPortLease::acquire().unwrap();
    let policy = policy_artifact(digest_pin(policy_text().as_bytes()));
    let restricted = inputs.recipe(
        &inputs.stock,
        BootPurpose::InstalledStaticWeb(&lease, StaticNetwork::RestrictedOrigin),
    );
    let restricted = restricted.plan().unwrap();
    let egress = inputs.recipe(
        &inputs.patched,
        BootPurpose::InstalledStaticWeb(&lease, StaticNetwork::PublicEgress(&policy)),
    );
    let egress = egress.plan().unwrap();
    let test = inputs.recipe(&inputs.stock, BootPurpose::Installed);
    let test = test.plan().unwrap();
    assert_eq!(restricted.kind(), PlanKind::StaticWebBoot);
    assert!(restricted.public_egress_policy().is_none());
    assert_eq!(
        after(restricted.args(), "-netdev"),
        vec![format!(
            "user,id=arxnet0,restrict=on,ipv6=off,hostfwd=tcp:127.0.0.1:{}-10.0.2.15:443",
            lease.port()
        )]
    );
    assert!(
        after(restricted.args(), "-device")
            .contains(&"virtio-net-pci,netdev=arxnet0,mac=52:54:00:12:34:56")
    );
    assert_eq!(test.kind(), PlanKind::Boot);
    assert_eq!(after(test.args(), "-netdev"), vec![NETWORK]);
    assert_eq!(NETWORK, "user,id=arxnet0,restrict=on,ipv6=off");
    assert!(after(test.args(), "-device").contains(&"virtio-net-pci,netdev=arxnet0"));
    // Apart from the netdev and NIC, the two static recipes are identical.
    let strip = |args: &[String]| -> Vec<String> {
        let netdev = args.iter().position(|a| a == "-netdev").unwrap();
        let mut args = args.to_vec();
        args.drain(netdev..netdev + 4);
        args
    };
    assert_eq!(strip(restricted.args()), strip(egress.args()));
    for plan in [&restricted, &test] {
        assert!(
            plan.args()
                .iter()
                .all(|arg| !arg.contains("arx-public-egress") && !arg.contains("restrict=off"))
        );
    }
}

#[test]
fn policy_parse_accepts_exactly_the_v1_contract() {
    let parsed = policy();
    assert_eq!(parsed.uplink().interface_index, 12);
    assert_eq!(parsed.uplink().interface_luid, LUID);
    assert_eq!(parsed.uplink().source, SOURCE);
    assert_eq!(parsed.uplink().gateway, GATEWAY);
    assert_eq!(
        parsed.deny(),
        SITE.map(|s| Ipv4Cidr::parse(s).unwrap()).as_slice()
    );
    let text = policy_text();
    let digest = text
        .lines()
        .last()
        .unwrap()
        .strip_prefix("fingerprint=")
        .unwrap();
    assert_eq!(hex::encode(parsed.fingerprint()), digest);
    assert!(
        PublicEgressPolicy::parse(seal(HEAD, &[]).as_bytes()).is_ok(),
        "zero deny lines"
    );

    use EgressRefusal::*;
    let head = |replace: &str, with: &str| HEAD.replacen(replace, with, 1);
    let cases: Vec<(String, EgressRefusal, &str)> = vec![
        (
            seal(&head("version=1", "version=2"), &SITE),
            PolicyVersion,
            "v2",
        ),
        (
            seal(&head("version=1\n", ""), &SITE),
            PolicyVersion,
            "no version",
        ),
        (
            seal(&head("version=1", "version=01"), &SITE),
            PolicyVersion,
            "padded version",
        ),
        (
            seal(&head("interface_index=12", "interface_index=012"), &SITE),
            PolicyMalformed,
            "leading zero",
        ),
        (
            seal(&head("interface_index=12", "interface_index=0"), &SITE),
            PolicyMalformed,
            "zero index",
        ),
        (
            seal(
                &head("interface_index=12", "interface_index=4294967296"),
                &SITE,
            ),
            PolicyMalformed,
            "index overflow",
        ),
        (
            seal(&head("interface_luid=1689399632478208\n", ""), &SITE),
            PolicyMalformed,
            "missing luid",
        ),
        (
            seal(
                &head("interface_luid=1689399632478208", "interface_luid=0"),
                &SITE,
            ),
            PolicyMalformed,
            "zero luid",
        ),
        (
            seal(
                &head(
                    "interface_luid=1689399632478208",
                    "interface_luid=18446744073709551616",
                ),
                &SITE,
            ),
            PolicyMalformed,
            "luid overflow",
        ),
        (
            seal(
                "version=1\ninterface_luid=1689399632478208\ninterface_index=12\nsource=192.168.1.20\ngateway=192.168.1.1\n",
                &SITE,
            ),
            PolicyMalformed,
            "reordered",
        ),
        (
            seal(&head("source=192.168.1.20", "source=192.168.01.20"), &SITE),
            PolicyMalformed,
            "octet zero",
        ),
        (
            seal(&head("source=192.168.1.20", "source=192.168.1.256"), &SITE),
            PolicyMalformed,
            "octet range",
        ),
        (
            seal(&head("gateway=192.168.1.1", "gateway=192.168.1"), &SITE),
            PolicyMalformed,
            "short address",
        ),
        (
            seal(
                &head("gateway=192.168.1.1", "gateway=192.168.1.1/32"),
                &SITE,
            ),
            PolicyMalformed,
            "gateway cidr",
        ),
        (
            seal(&head("gateway=", "router="), &SITE),
            PolicyMalformed,
            "unknown field",
        ),
        (seal(HEAD, &["45.12.8.0/0"]), PolicyMalformed, "prefix 0"),
        (seal(HEAD, &["45.12.8.0/33"]), PolicyMalformed, "prefix 33"),
        (seal(HEAD, &["45.12.8.1/24"]), PolicyMalformed, "host bits"),
        (
            seal(HEAD, &["45.12.8.0/024"]),
            PolicyMalformed,
            "padded prefix",
        ),
        (seal(HEAD, &["45.12.8.0"]), PolicyMalformed, "no prefix"),
        (
            seal(HEAD, &["45.12.8.0/24", "45.12.8.0/24"]),
            PolicyMalformed,
            "duplicate",
        ),
        (seal(HEAD, &[" 45.12.8.0/24"]), PolicyMalformed, "space"),
        (
            seal(&head("source=192.168.1.20", "source=0.0.0.0"), &SITE),
            PolicyMalformed,
            "unspecified source",
        ),
        (
            seal(&head("gateway=192.168.1.1", "gateway=0.0.0.0"), &SITE),
            PolicyMalformed,
            "unspecified gateway",
        ),
        (
            seal(&head("gateway=192.168.1.1", "gateway=192.168.1.20"), &SITE),
            PolicyIncomplete,
            "gateway is source",
        ),
        (
            seal(&head("source=192.168.1.20", "source=45.13.0.20"), &SITE),
            PolicyIncomplete,
            "unenrolled public source",
        ),
        (
            seal(&head("gateway=192.168.1.1", "gateway=45.13.0.1"), &SITE),
            PolicyIncomplete,
            "unenrolled public gateway",
        ),
        (text.replace('\n', "\r\n"), PolicyMalformed, "CRLF"),
        (
            text.trim_end_matches('\n').to_owned(),
            PolicyMalformed,
            "no final LF",
        ),
        (format!("{text}\n"), PolicyMalformed, "blank tail"),
        (
            format!("{text}deny=45.12.10.0/24\n"),
            PolicyMalformed,
            "line after fingerprint",
        ),
        (
            text.replacen("deny=45.12.8.0/24\n", "", 1),
            PolicyFingerprint,
            "dropped line",
        ),
        (
            text.replacen("45.12.9.7/32", "45.12.9.8/32", 1),
            PolicyFingerprint,
            "edited line",
        ),
        (
            format!("{}{}\n", &text[..text.len() - 65], digest.to_uppercase()),
            PolicyMalformed,
            "uppercase digest",
        ),
        (
            format!("{}{}\n", &text[..text.len() - 65], &digest[..63]),
            PolicyMalformed,
            "short digest",
        ),
        (
            seal(HEAD, &SITE).replacen("fingerprint=", "digest=", 1),
            PolicyMalformed,
            "no fingerprint",
        ),
        (String::new(), PolicyMalformed, "empty"),
        (
            format!("{text}{}", "x".repeat(MAX_POLICY_BYTES as usize)),
            PolicyMalformed,
            "oversized",
        ),
        ("{\"version\":1}".to_owned(), PolicyMalformed, "json"),
    ];
    for (bytes, refusal, label) in cases {
        assert_eq!(
            PublicEgressPolicy::parse(bytes.as_bytes()),
            Err(refusal),
            "{label}"
        );
    }
    let mut nul = text.clone().into_bytes();
    nul[10] = 0;
    assert_eq!(PublicEgressPolicy::parse(&nul), Err(PolicyMalformed));
    let many: Vec<String> = (0..=MAX_POLICY_DENY as u32)
        .map(|n| format!("{}/32", Ipv4Addr::from(0x2d00_0000 + n)))
        .collect();
    let refs: Vec<&str> = many.iter().map(String::as_str).collect();
    assert!(PublicEgressPolicy::parse(seal(HEAD, &refs[..MAX_POLICY_DENY]).as_bytes()).is_ok());
    assert_eq!(
        PublicEgressPolicy::parse(seal(HEAD, &refs).as_bytes()),
        Err(PolicyMalformed),
        "513 deny lines"
    );
}

fn tcp(address: Ipv4Addr, port: u16) -> GuestFlow {
    GuestFlow {
        transport: Transport::Tcp,
        destination: SocketAddrV4::new(address, port),
    }
}

#[test]
fn deny_matcher_covers_every_frozen_range_before_and_after_translation() {
    let policy = policy();
    let route = route();
    let identity = Some;
    // An independent literal copy, so dropping or widening an entry fails.
    assert_eq!(
        FROZEN_DENY.map(|range| range.to_string()),
        [
            "0.0.0.0/8",
            "10.0.0.0/8",
            "100.64.0.0/10",
            "127.0.0.0/8",
            "169.254.0.0/16",
            "172.16.0.0/12",
            "192.0.0.0/24",
            "192.0.2.0/24",
            "192.88.99.0/24",
            "192.168.0.0/16",
            "198.18.0.0/15",
            "198.51.100.0/24",
            "203.0.113.0/24",
            "224.0.0.0/4",
            "240.0.0.0/4",
            "255.255.255.255/32",
        ]
    );
    for literal in [
        "0.0.0.0",
        "0.255.255.255",
        "10.0.0.1",
        "10.255.255.255",
        "100.64.0.0",
        "100.127.255.255",
        "127.0.0.1",
        "169.254.169.254",
        "172.16.0.1",
        "172.31.255.255",
        "192.0.0.8",
        "192.0.2.1",
        "192.88.99.1",
        "192.168.0.1",
        "198.18.0.1",
        "198.19.255.255",
        "198.51.100.1",
        "203.0.113.1",
        "224.0.0.251",
        "239.255.255.250",
        "240.0.0.1",
        "255.255.255.255",
    ] {
        let address: Ipv4Addr = literal.parse().unwrap();
        assert!(policy.denies(address), "{literal}");
    }
    for range in FROZEN_DENY {
        for address in [range.network(), range.last()] {
            let refusal = if SYNTHETIC_SUBNET.contains(address) {
                FlowRefusal::Synthetic
            } else {
                FlowRefusal::Denied
            };
            for transport in [Transport::Tcp, Transport::Udp] {
                let flow = GuestFlow {
                    transport,
                    destination: SocketAddrV4::new(address, 53),
                };
                assert_eq!(
                    policy.admit_flow(flow, identity, Some(&route)),
                    Err(refusal),
                    "pre {range} {address}"
                );
            }
            // A public guest destination that slirp translates into the range.
            assert_eq!(
                policy.admit_flow(tcp(PUBLIC, 443), |_| Some(address), Some(&route)),
                Err(FlowRefusal::DeniedAfterTranslation),
                "post {range} {address}"
            );
        }
    }
    // Enrolled inventory: public-numbered LAN and hairpin WAN.
    for address in [Ipv4Addr::new(45, 12, 8, 77), Ipv4Addr::new(45, 12, 9, 7)] {
        assert_eq!(
            policy.admit_flow(tcp(address, 443), identity, Some(&route)),
            Err(FlowRefusal::Denied)
        );
        assert_eq!(
            policy.admit_flow(tcp(PUBLIC, 443), |_| Some(address), Some(&route)),
            Err(FlowRefusal::DeniedAfterTranslation)
        );
    }
    // The slirp aliases are dropped before translation even when slirp would
    // map the DNS alias to a public host resolver.
    for alias in [Ipv4Addr::new(10, 0, 2, 2), Ipv4Addr::new(10, 0, 2, 3)] {
        assert_eq!(
            policy.admit_flow(
                tcp(alias, 53),
                |_| Some(Ipv4Addr::new(8, 8, 8, 8)),
                Some(&route)
            ),
            Err(FlowRefusal::Synthetic)
        );
    }
    // Public neighbours of the ranges, outside the enrolled inventory, pass.
    for address in [
        Ipv4Addr::new(1, 0, 0, 0),
        Ipv4Addr::new(9, 255, 255, 255),
        Ipv4Addr::new(11, 0, 0, 0),
        Ipv4Addr::new(100, 63, 255, 255),
        Ipv4Addr::new(100, 128, 0, 0),
        Ipv4Addr::new(126, 255, 255, 255),
        Ipv4Addr::new(128, 0, 0, 0),
        Ipv4Addr::new(172, 15, 255, 255),
        Ipv4Addr::new(172, 32, 0, 0),
        Ipv4Addr::new(192, 0, 1, 0),
        Ipv4Addr::new(198, 17, 255, 255),
        Ipv4Addr::new(198, 20, 0, 0),
        Ipv4Addr::new(223, 255, 255, 255),
        Ipv4Addr::new(45, 12, 9, 6),
    ] {
        assert_eq!(
            policy.admit_flow(tcp(address, 443), identity, Some(&route)),
            Ok(SocketAddrV4::new(address, 443)),
            "{address}"
        );
    }
    for transport in [Transport::Icmp, Transport::Other(47)] {
        let flow = GuestFlow {
            transport,
            destination: SocketAddrV4::new(PUBLIC, 1),
        };
        assert_eq!(
            policy.admit_flow(flow, identity, Some(&route)),
            Err(FlowRefusal::Transport)
        );
    }
    assert_eq!(
        policy.admit_flow(tcp(PUBLIC, 0), identity, Some(&route)),
        Err(FlowRefusal::Port)
    );
    assert_eq!(
        policy.admit_flow(tcp(PUBLIC, 443), |_| None, Some(&route)),
        Err(FlowRefusal::Untranslatable)
    );
}

#[test]
fn route_must_be_the_enrolled_default_uplink() {
    let policy = policy();
    assert_eq!(policy.verify_route(Some(&route())), Ok(()));
    assert_eq!(policy.verify_route(None), Err(EgressRefusal::RouteUnknown));
    let base = route();
    for drift in [
        RouteObservation {
            interface_index: 13,
            ..base
        },
        RouteObservation {
            interface_luid: LUID + 1,
            ..base
        },
        RouteObservation {
            source: Ipv4Addr::new(192, 168, 1, 21),
            ..base
        },
        RouteObservation {
            gateway: Ipv4Addr::new(192, 168, 1, 254),
            ..base
        },
        RouteObservation {
            default_route: false,
            ..base
        },
    ] {
        assert_eq!(
            policy.verify_route(Some(&drift)),
            Err(EgressRefusal::RouteMismatch)
        );
        assert_eq!(
            policy.admit_flow(tcp(PUBLIC, 443), Some, Some(&drift)),
            Err(FlowRefusal::RouteMismatch)
        );
    }
    assert_eq!(
        policy.admit_flow(tcp(PUBLIC, 443), Some, None),
        Err(FlowRefusal::RouteUnknown)
    );
}

#[test]
fn the_localhost_forward_is_host_initiated_only_and_its_port_is_leased() {
    let policy = policy();
    let lease = OriginPortLease::acquire().unwrap();
    let forward = LocalForward::from_lease(&lease).unwrap();
    assert_eq!(forward.port(), lease.port());
    assert!((ORIGIN_PORT_FIRST..=ORIGIN_PORT_LAST).contains(&forward.port()));
    let origin = SocketAddrV4::new(Ipv4Addr::new(10, 0, 2, 15), 443);
    let listener = SocketAddrV4::new(Ipv4Addr::LOCALHOST, forward.port());
    assert_eq!(forward.admit_inbound(listener, origin), Ok(()));
    for (listener, guest) in [
        (
            SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, forward.port()),
            origin,
        ),
        (SocketAddrV4::new(SOURCE, forward.port()), origin),
        (
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, forward.port() ^ 1),
            origin,
        ),
        (listener, SocketAddrV4::new(Ipv4Addr::new(10, 0, 2, 15), 22)),
        (
            listener,
            SocketAddrV4::new(Ipv4Addr::new(10, 0, 2, 16), 443),
        ),
    ] {
        assert_eq!(
            forward.admit_inbound(listener, guest),
            Err(FlowRefusal::Forward)
        );
    }
    // The guest cannot use the forward peer in the reverse direction.
    for port in [forward.port(), 443] {
        assert_eq!(
            policy.admit_flow(
                tcp(Ipv4Addr::new(10, 0, 2, 2), port),
                |_| Some(Ipv4Addr::LOCALHOST),
                Some(&route())
            ),
            Err(FlowRefusal::Synthetic)
        );
    }
    // Port collision: a held lease excludes another listener and lease.
    assert!(std::net::TcpListener::bind(listener).is_err());
    let second = OriginPortLease::acquire().unwrap();
    assert_ne!(second.port(), lease.port());
}

struct PrivateDir(tempfile::TempDir);

impl PrivateDir {
    fn new() -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap();
        #[cfg(windows)]
        let root = std::path::PathBuf::from(root.to_str().unwrap().strip_prefix(r"\\?\").unwrap());
        let dir = tempfile::tempdir_in(root).unwrap();
        #[cfg(unix)]
        set_mode(dir.path(), 0o700);
        #[cfg(windows)]
        win::protect(dir.path());
        Self(dir)
    }

    fn path(&self) -> &Path {
        self.0.path()
    }

    fn write(&self, name: &str, body: &[u8]) -> ArtifactPin {
        let path = self.path().join(name);
        std::fs::write(&path, body).unwrap();
        #[cfg(unix)]
        set_mode(&path, 0o600);
        #[cfg(windows)]
        win::protect(&path);
        digest_pin(body)
    }
}

/// Explicit modes, independent of the process umask.
#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn custody_reads_the_pinned_policy_read_only_through_a_held_handle() {
    let dir = PrivateDir::new();
    let body = policy_text();
    let pin = dir.write("policy.txt", body.as_bytes());
    let custodied = CustodiedPolicy::open(dir.path(), "policy.txt", pin).unwrap();
    assert_eq!(custodied.pin(), pin);
    assert_eq!(custodied.policy(), &policy());
    assert_eq!(custodied.verify(), Ok(()));

    use EgressRefusal::*;
    let other = ArtifactPin {
        sha256: [0; 32],
        ..pin
    };
    let longer = ArtifactPin {
        size: pin.size + 1,
        ..pin
    };
    assert_eq!(
        CustodiedPolicy::open(dir.path(), "policy.txt", other).unwrap_err(),
        PolicyMismatch
    );
    assert_eq!(
        CustodiedPolicy::open(dir.path(), "policy.txt", longer).unwrap_err(),
        PolicyMismatch
    );
    for name in [
        "missing.txt",
        "../policy.txt",
        ".policy",
        "a/b",
        "",
        "policy.txt.",
    ] {
        assert_eq!(
            CustodiedPolicy::open(dir.path(), name, pin).unwrap_err(),
            PolicyCustody,
            "{name:?}"
        );
    }
    let bad = seal(HEAD, &["45.12.8.1/24"]);
    let bad_pin = dir.write("bad.txt", bad.as_bytes());
    assert_eq!(
        CustodiedPolicy::open(dir.path(), "bad.txt", bad_pin).unwrap_err(),
        PolicyMalformed
    );
    // A second hard link to the policy is refused.
    let linked = dir.write("linked.txt", body.as_bytes());
    std::fs::hard_link(dir.path().join("linked.txt"), dir.path().join("alias.txt")).unwrap();
    assert_eq!(
        CustodiedPolicy::open(dir.path(), "linked.txt", linked).unwrap_err(),
        PolicyCustody
    );
    // A file another principal can read is not private.
    let shared = dir.write("shared.txt", body.as_bytes());
    #[cfg(unix)]
    set_mode(&dir.path().join("shared.txt"), 0o644);
    #[cfg(windows)]
    win::set_dacl(
        &dir.path().join("shared.txt"),
        "(A;;FA;;;{u})(A;;FR;;;WD)",
        true,
    );
    assert_eq!(
        CustodiedPolicy::open(dir.path(), "shared.txt", shared).unwrap_err(),
        PolicyCustody
    );
    // Windows: an unprotected DACL, or an inheritable entry on the anchor,
    // is weaker than the ADR 0054 owner-only rule.
    #[cfg(windows)]
    {
        let unprotected = dir.write("unprotected.txt", body.as_bytes());
        win::set_dacl(&dir.path().join("unprotected.txt"), "(A;;FA;;;{u})", false);
        assert_eq!(
            CustodiedPolicy::open(dir.path(), "unprotected.txt", unprotected).unwrap_err(),
            PolicyCustody
        );
        let system = dir.write("system.txt", body.as_bytes());
        win::set_dacl(
            &dir.path().join("system.txt"),
            "(A;;FA;;;{u})(A;;FA;;;SY)",
            true,
        );
        assert_eq!(
            CustodiedPolicy::open(dir.path(), "system.txt", system).unwrap_err(),
            PolicyCustody
        );
        let inheriting = dir.path().join("inheriting");
        std::fs::create_dir(&inheriting).unwrap();
        let inner = dir.write("inheriting/policy.txt", body.as_bytes());
        win::set_dacl(&inheriting, "(A;OICI;FA;;;{u})", true);
        assert_eq!(
            CustodiedPolicy::open(&inheriting, "policy.txt", inner).unwrap_err(),
            PolicyCustody
        );
        win::set_dacl(&inheriting, "(A;;FA;;;{u})", true);
        assert!(CustodiedPolicy::open(&inheriting, "policy.txt", inner).is_ok());
    }
    // Replacement while custody lives: Windows refuses the rename outright;
    // POSIX allows it and `verify` detects the different object.
    let replacement = dir.path().join("replacement.txt");
    std::fs::write(&replacement, body.as_bytes()).unwrap();
    let renamed = std::fs::rename(&replacement, dir.path().join("policy.txt"));
    if cfg!(windows) {
        assert!(renamed.is_err(), "held policy was replaced");
        assert_eq!(custodied.verify(), Ok(()));
    } else {
        renamed.unwrap();
        #[cfg(unix)]
        set_mode(&dir.path().join("policy.txt"), 0o600);
        assert_eq!(custodied.verify(), Err(PolicyCustody));
    }
    #[cfg(windows)]
    assert!(
        std::fs::OpenOptions::new()
            .append(true)
            .open(dir.path().join("policy.txt"))
            .is_err(),
        "held policy is writable"
    );
}

#[cfg(unix)]
#[test]
fn custody_refuses_links_and_a_shared_anchor() {
    let dir = PrivateDir::new();
    let pin = dir.write("policy.txt", policy_text().as_bytes());
    std::os::unix::fs::symlink(dir.path().join("policy.txt"), dir.path().join("link.txt")).unwrap();
    assert_eq!(
        CustodiedPolicy::open(dir.path(), "link.txt", pin).unwrap_err(),
        EgressRefusal::PolicyCustody
    );
    set_mode(dir.path(), 0o755);
    assert_eq!(
        CustodiedPolicy::open(dir.path(), "policy.txt", pin).unwrap_err(),
        EgressRefusal::PolicyCustody
    );
}

#[test]
fn the_patched_executable_is_held_read_only_and_measured() {
    use EgressRefusal::*;
    let dir = PrivateDir::new();
    let body = b"patched qemu bytes";
    std::fs::write(dir.path().join("qemu.exe"), body).unwrap();
    let pin = digest_pin(body);
    let held = HeldExecutable::open(&dir.path().join("qemu.exe"), pin).unwrap();
    assert_eq!(held.pin(), pin);
    assert_eq!(held.verify(), Ok(()));
    let other = ArtifactPin {
        sha256: [0; 32],
        ..pin
    };
    assert_eq!(
        HeldExecutable::open(&dir.path().join("qemu.exe"), other).unwrap_err(),
        BinaryMismatch
    );
    assert_eq!(
        HeldExecutable::open(&dir.path().join("qemu.exe"), ArtifactPin { size: 0, ..pin })
            .unwrap_err(),
        BinaryCustody
    );
    assert_eq!(
        HeldExecutable::open(&dir.path().join("missing.exe"), pin).unwrap_err(),
        BinaryCustody
    );
    std::fs::write(dir.path().join("linked.exe"), body).unwrap();
    std::fs::hard_link(dir.path().join("linked.exe"), dir.path().join("alias.exe")).unwrap();
    assert_eq!(
        HeldExecutable::open(&dir.path().join("linked.exe"), pin).unwrap_err(),
        BinaryCustody
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(dir.path().join("qemu.exe"), dir.path().join("link.exe"))
            .unwrap();
        assert_eq!(
            HeldExecutable::open(&dir.path().join("link.exe"), pin).unwrap_err(),
            BinaryCustody
        );
    }
    #[cfg(windows)]
    {
        std::fs::write(dir.path().join("swap.exe"), body).unwrap();
        assert!(
            std::fs::rename(dir.path().join("swap.exe"), dir.path().join("qemu.exe")).is_err(),
            "held executable was replaced"
        );
        assert!(
            std::fs::OpenOptions::new()
                .append(true)
                .open(dir.path().join("qemu.exe"))
                .is_err(),
            "held executable is writable"
        );
        assert_eq!(held.verify(), Ok(()));
        assert!(held.artifact().is_ok());
    }
}

#[test]
fn admission_refuses_missing_or_mismatched_evidence_with_typed_reasons() {
    let dir = PrivateDir::new();
    let pin = dir.write("policy.txt", policy_text().as_bytes());
    let custodied = CustodiedPolicy::open(dir.path(), "policy.txt", pin).unwrap();
    let exe_body = b"patched qemu bytes";
    std::fs::write(dir.path().join("qemu.exe"), exe_body).unwrap();
    let exe_pin = digest_pin(exe_body);
    let executable = HeldExecutable::open(&dir.path().join("qemu.exe"), exe_pin).unwrap();
    let inputs = Inputs::new();
    let lease = OriginPortLease::acquire().unwrap();
    // A plan naming another object that carries the same policy pin.
    let forged = policy_artifact(pin);
    let forged_plan = inputs.recipe(
        &inputs.patched,
        BootPurpose::InstalledStaticWeb(&lease, StaticNetwork::PublicEgress(&forged)),
    );
    let forged_plan = forged_plan.plan().unwrap();
    let restricted = inputs.recipe(
        &inputs.stock,
        BootPurpose::InstalledStaticWeb(&lease, StaticNetwork::RestrictedOrigin),
    );
    let restricted = restricted.plan().unwrap();
    let patched = EgressBinaryPin {
        qemu: exe_pin,
        receipt: ArtifactPin {
            size: 4096,
            sha256: [0xa2; 32],
        },
    };
    let stock = EgressBinaryPin {
        qemu: STOCK_QEMU_1113,
        receipt: ArtifactPin {
            size: 4096,
            sha256: STOCK_RECEIPT_1113_SHA256,
        },
    };
    let route = route();
    // Admission reads both pins from a real validated profile.
    let enrolled = enrolled_profile(pin, patched);
    let other_policy = enrolled_profile(
        ArtifactPin {
            sha256: [0x55; 32],
            ..pin
        },
        patched,
    );
    let unenrolled = parse(&profile()).unwrap();
    let full = EgressEvidence {
        policy: Some(&custodied),
        profile: Some(&enrolled),
        route: Some(&route),
        executable: Some(&executable),
    };

    use EgressRefusal::*;
    let mut checked = vec![(admit(&restricted, full), PlanMismatch)];
    for (evidence, refusal) in [
        // Enrollment is checked before custody, so no evidence at all, or a
        // missing held policy without a profile, refuses as `Unenrolled`.
        (EgressEvidence::default(), Unenrolled),
        (
            EgressEvidence {
                policy: None,
                profile: None,
                ..full
            },
            Unenrolled,
        ),
        (
            EgressEvidence {
                policy: None,
                ..full
            },
            PolicyMissing,
        ),
        (
            EgressEvidence {
                profile: None,
                ..full
            },
            Unenrolled,
        ),
        (
            EgressEvidence {
                profile: Some(&unenrolled),
                ..full
            },
            Unenrolled,
        ),
        (
            EgressEvidence {
                profile: Some(&other_policy),
                ..full
            },
            PolicyMismatch,
        ),
        (full, PolicyMismatch),
    ] {
        checked.push((admit(&forged_plan, evidence), refusal));
    }
    // Only a Windows drive path can name a plan input, so the bound plan is
    // sealed from the held objects there; POSIX custody cannot bind at all.
    #[cfg(windows)]
    {
        let policy = custodied.artifact().unwrap();
        let image = executable.artifact().unwrap();
        let bound = inputs.recipe(
            &image,
            BootPurpose::InstalledStaticWeb(&lease, StaticNetwork::PublicEgress(&policy)),
        );
        let bound = bound.plan().unwrap();
        let drifted = RouteObservation {
            gateway: Ipv4Addr::new(192, 168, 1, 254),
            ..route
        };
        let stock_profile = enrolled_profile(pin, stock);
        for (evidence, refusal) in [
            (
                EgressEvidence {
                    route: None,
                    ..full
                },
                RouteUnknown,
            ),
            (
                EgressEvidence {
                    route: Some(&drifted),
                    ..full
                },
                RouteMismatch,
            ),
            (
                EgressEvidence {
                    profile: Some(&other_policy),
                    ..full
                },
                PolicyMismatch,
            ),
            (
                EgressEvidence {
                    profile: Some(&stock_profile),
                    ..full
                },
                BinaryStock,
            ),
            // No reviewed build exists yet, so even a complete set refuses.
            (full, BinaryUnqualified),
        ] {
            checked.push((admit(&bound, evidence), refusal));
        }
    }
    #[cfg(unix)]
    {
        let _ = stock;
        assert_eq!(custodied.artifact().unwrap_err(), PolicyCustody);
        assert_eq!(executable.artifact().unwrap_err(), BinaryCustody);
    }
    for (result, refusal) in checked {
        assert_eq!(result, Err(refusal));
        assert_eq!(
            NativeRefusal::from(refusal),
            NativeRefusal::NetworkFenceUnverified
        );
    }
}

/// Permissions that drift after custody opened are caught at verification,
/// for the held anchor directory as well as the leaf.
#[test]
fn custody_rechecks_the_held_anchor_and_leaf_at_verification() {
    let dir = PrivateDir::new();
    let pin = dir.write("policy.txt", policy_text().as_bytes());
    let custodied = CustodiedPolicy::open(dir.path(), "policy.txt", pin).unwrap();
    assert_eq!(custodied.verify(), Ok(()));
    let leaf = dir.path().join("policy.txt");
    let refused = Err(EgressRefusal::PolicyCustody);
    #[cfg(unix)]
    {
        set_mode(dir.path(), 0o750);
        assert_eq!(custodied.verify(), refused, "anchor group bits");
        set_mode(dir.path(), 0o700);
        assert_eq!(custodied.verify(), Ok(()));
        set_mode(&leaf, 0o640);
        assert_eq!(custodied.verify(), refused, "leaf group bits");
        set_mode(&leaf, 0o600);
        assert_eq!(custodied.verify(), Ok(()));
    }
    #[cfg(windows)]
    {
        for (path, aces, protected, label) in [
            (
                dir.path(),
                "(A;;FA;;;{u})(A;;FA;;;SY)",
                true,
                "anchor extra entry",
            ),
            (dir.path(), "(A;;FA;;;{u})", false, "anchor unprotected"),
            (dir.path(), "(A;OICI;FA;;;{u})", true, "anchor inheritable"),
            (
                leaf.as_path(),
                "(A;;FA;;;{u})(A;;FR;;;WD)",
                true,
                "leaf extra entry",
            ),
            (leaf.as_path(), "(A;;FA;;;{u})", false, "leaf unprotected"),
        ] {
            win::set_dacl(path, aces, protected);
            assert_eq!(custodied.verify(), refused, "{label}");
            win::protect(path);
            assert_eq!(custodied.verify(), Ok(()), "{label} restored");
        }
    }
}

/// Admission reads its pins only from a `VmRuntimeProfile`, and only
/// `VmRuntimeProfile::parse` (with its size limit and semantic validation)
/// can produce one. The two bypasses do not compile:
///
/// - `serde_json::from_value::<VmRuntimeProfile>(..)` needs `Deserialize`,
///   which the type does not implement. The probe below observes that at
///   compile time: the inherent `deserializable` exists only when the bound
///   holds, and method resolution otherwise picks the trait default.
/// - `VmRuntimeProfile(..)` or a struct literal needs its private field,
///   visible only inside `arxd::config`.
#[test]
fn only_parse_can_produce_the_profile_that_admission_reads() {
    use std::marker::PhantomData;
    struct Probe<T>(PhantomData<T>);
    trait Fallback {
        fn deserializable(&self) -> bool {
            false
        }
    }
    impl<T> Fallback for Probe<T> {}
    impl<T: serde::de::DeserializeOwned> Probe<T> {
        fn deserializable(&self) -> bool {
            true
        }
    }
    assert!(
        Probe::<Value>(PhantomData).deserializable(),
        "positive control"
    );
    assert!(
        !Probe::<VmRuntimeProfile>(PhantomData).deserializable(),
        "a deserialized profile would skip parse"
    );
    // Bytes that parse refuses never become a profile, enrolled or not.
    for (field, value) in [
        ("state", json!("test_vms/state")),
        ("owner_sid", json!("S-1-5-21-1-2-3-1002")),
        ("disk_ceiling_bytes", json!(0)),
    ] {
        let mut bad = profile();
        bad["public_egress"] = egress_section();
        bad[field] = value;
        assert!(parse(&bad).is_err(), "{field}");
    }
    let mut oversized = profile();
    oversized["public_egress"] = egress_section();
    let mut bytes = serde_json::to_vec(&oversized).unwrap();
    bytes.splice(1..1, std::iter::repeat_n(b' ', 16_384));
    assert!(VmRuntimeProfile::parse(&bytes).is_err(), "size limit");
}

fn enrolled_profile(policy: ArtifactPin, binary: EgressBinaryPin) -> VmRuntimeProfile {
    let pin = |pin: ArtifactPin| json!({"length": pin.size, "sha256": hex::encode(pin.sha256)});
    let mut value = profile();
    value["public_egress"] = json!({
        "manifest": EGRESS_MANIFEST,
        "qemu": pin(binary.qemu),
        "receipt": pin(binary.receipt),
        "policy": pin(policy),
    });
    parse(&value).unwrap()
}

fn profile() -> Value {
    json!({
        "enabled": true, "owner_sid": "S-1-5-21-1-2-3-1001",
        "supervisor_sid": "S-1-5-21-1-2-3-1002", "bundle": "E:/pinned bundle",
        "volume": "D:/", "state": "D:/test_vms/state",
        "disk_ceiling_bytes": 10_737_418_240_u64, "manifest": "qemu-11.1.3-windows-x64",
    })
}

fn egress_section() -> Value {
    json!({
        "manifest": EGRESS_MANIFEST,
        "qemu": {"length": 30_000_000, "sha256": "a1".repeat(32)},
        "receipt": {"length": 4096, "sha256": "a2".repeat(32)},
        "policy": {"length": 512, "sha256": "a3".repeat(32)},
    })
}

fn parse(value: &Value) -> anyhow::Result<VmRuntimeProfile> {
    VmRuntimeProfile::parse(&serde_json::to_vec(value).unwrap())
}

#[test]
fn the_patched_manifest_is_a_separate_config_pin_that_stays_refused() {
    let plain = parse(&profile()).unwrap();
    assert_eq!(plain.public_egress_manifest_id(), None);
    assert_eq!(plain.public_egress_binary(), None);
    assert_eq!(plain.public_egress_policy_pin(), None);

    let mut value = profile();
    value["public_egress"] = egress_section();
    let configured = parse(&value).unwrap();
    assert_eq!(
        configured.manifest_id(),
        "qemu-11.1.3-windows-x64",
        "TEST keeps stock"
    );
    assert_eq!(
        configured.public_egress_manifest_id(),
        Some(EGRESS_MANIFEST)
    );
    let binary = configured.public_egress_binary().unwrap();
    assert_eq!(binary.qemu.size, 30_000_000);
    assert_eq!(binary.receipt.sha256, [0xa2; 32]);
    assert_eq!(
        configured.public_egress_policy_pin(),
        Some(ArtifactPin {
            size: 512,
            sha256: [0xa3; 32]
        })
    );
    assert_eq!(binary.qualify(), Err(EgressRefusal::BinaryUnqualified));

    let mut stock = profile();
    stock["public_egress"] = egress_section();
    stock["public_egress"]["qemu"] = json!({
        "length": STOCK_QEMU_1113.size, "sha256": hex::encode(STOCK_QEMU_1113.sha256),
    });
    let stock = parse(&stock).unwrap().public_egress_binary().unwrap();
    assert_eq!(stock.qualify(), Err(EgressRefusal::BinaryStock));

    type Mutation = (&'static str, Box<dyn Fn(&mut Value)>);
    let mutations: Vec<Mutation> = vec![
        ("null", Box::new(|v| v["public_egress"] = Value::Null)),
        (
            "stock manifest id",
            Box::new(|v| v["public_egress"]["manifest"] = json!("qemu-11.1.3-windows-x64")),
        ),
        (
            "unknown manifest",
            Box::new(|v| {
                v["public_egress"]["manifest"] = json!("qemu-11.1.4-arx-public-egress-windows-x64")
            }),
        ),
        (
            "dll slot",
            Box::new(|v| {
                v["public_egress"]["slirp"] = json!({"length": 1, "sha256": "a4".repeat(32)})
            }),
        ),
        (
            "missing receipt",
            Box::new(|v| {
                v["public_egress"]
                    .as_object_mut()
                    .unwrap()
                    .remove("receipt");
            }),
        ),
        (
            "missing policy",
            Box::new(|v| {
                v["public_egress"].as_object_mut().unwrap().remove("policy");
            }),
        ),
        (
            "uppercase",
            Box::new(|v| v["public_egress"]["qemu"]["sha256"] = json!("A1".repeat(32))),
        ),
        (
            "short digest",
            Box::new(|v| v["public_egress"]["qemu"]["sha256"] = json!("a1".repeat(31))),
        ),
        (
            "zero length",
            Box::new(|v| v["public_egress"]["receipt"]["length"] = json!(0)),
        ),
        (
            "oversized policy",
            Box::new(|v| v["public_egress"]["policy"]["length"] = json!(MAX_POLICY_BYTES + 1)),
        ),
        (
            "path",
            Box::new(|v| v["public_egress"]["policy"]["path"] = json!("D:/vm/policy.txt")),
        ),
        (
            "argv",
            Box::new(|v| v["public_egress"]["netdev"] = json!("user,restrict=off")),
        ),
    ];
    for (label, mutate) in mutations {
        let mut value = profile();
        value["public_egress"] = egress_section();
        mutate(&mut value);
        assert!(parse(&value).is_err(), "{label}");
    }
}

#[cfg(windows)]
mod win {
    use std::{os::windows::ffi::OsStrExt, path::Path, ptr};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, LocalFree},
        Security::{Authorization::*, *},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    /// TokenUser owner and a protected DACL with one explicit,
    /// non-inheritable TokenUser FullControl entry.
    pub(super) fn protect(path: &Path) {
        set_dacl(path, "(A;;FA;;;{u})", true);
    }

    /// TokenUser owner and the DACL `aces`, where `{u}` names TokenUser.
    pub(super) fn set_dacl(path: &Path, aces: &str, protected: bool) {
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
            let ok = GetTokenInformation(token, TokenUser, user.as_mut_ptr().cast(), len, &mut len);
            CloseHandle(token);
            assert_ne!(ok, 0);
            let sid = (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid;
            let mut text = ptr::null_mut();
            assert_ne!(ConvertSidToStringSidW(sid, &mut text), 0);
            let n = (0..184).find(|i| *text.add(*i) == 0).unwrap();
            let sid_text = String::from_utf16(std::slice::from_raw_parts(text, n)).unwrap();
            LocalFree(text.cast());
            let sddl: Vec<u16> = format!("D:{}", aces.replace("{u}", &sid_text))
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
            let protection = if protected {
                PROTECTED_DACL_SECURITY_INFORMATION
            } else {
                UNPROTECTED_DACL_SECURITY_INFORMATION
            };
            let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            let dacl = SetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | protection,
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
}
