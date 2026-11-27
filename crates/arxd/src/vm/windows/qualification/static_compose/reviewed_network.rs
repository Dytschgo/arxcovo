//! Independently reviewed acceptance witness for a selected Docker and TEST seal.
use super::*;
use std::{collections::BTreeSet, net::SocketAddrV4};

pub(super) const MAX_BYTES: u64 = 65_536;
const NETWORK: NativeRefusal = NativeRefusal::NetworkFenceUnverified;

/// Exact socket targets shared by the pinned fixture profile and the
/// post-run TEST comparison. Only the addresses go into route sampling.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(in crate::vm::windows::qualification) struct PublicTargets {
    public_tcp_target: SocketAddrV4,
    public_dns_target: SocketAddrV4,
}

impl PublicTargets {
    pub(in crate::vm::windows::qualification) fn parse(
        bytes: &[u8],
        policy: &PublicEgressPolicy,
    ) -> Result<Self, NativeRefusal> {
        let targets: Self = serde_json::from_slice(bytes).map_err(|_| NETWORK)?;
        targets.verify(policy)?;
        Ok(targets)
    }

    fn verify(self, policy: &PublicEgressPolicy) -> Result<(), NativeRefusal> {
        if self.public_tcp_target.port() == 0
            || self.public_dns_target.port() != 53
            || self.addresses().iter().any(|ip| policy.denies(*ip))
        {
            return Err(NETWORK);
        }
        Ok(())
    }

    pub(in crate::vm::windows::qualification) fn addresses(self) -> [std::net::Ipv4Addr; 2] {
        [*self.public_tcp_target.ip(), *self.public_dns_target.ip()]
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: u16,
    qualification_sha256: String,
    m4_qualification_sha256: String,
    recipe_sha256: String,
    policy_sha256: String,
    qemu_sha256: String,
    r#static: StaticObservation,
    test: TestObservation,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StaticObservation {
    tcp_target: SocketAddrV4,
    dns_target: SocketAddrV4,
    public_tcp_connected: bool,
    udp_dns_answer: bool,
    root_capable_fixture: bool,
    ipv6_disabled: bool,
    route_drift_closed: bool,
    denied: Vec<Denial>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Denial {
    target: SocketAddrV4,
    tcp_connected: bool,
    udp_replied: bool,
    witness_tcp_arrivals: u64,
    witness_udp_arrivals: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TestObservation {
    network_class: String,
    tcp_target: SocketAddrV4,
    dns_target: SocketAddrV4,
    public_tcp_connected: bool,
    udp_dns_answer: bool,
    witness_tcp_arrivals: u64,
    witness_udp_arrivals: u64,
}

#[allow(clippy::too_many_arguments)] // Each independently pinned seal and the enrolled targets.
pub(super) fn verify(
    bytes: &[u8],
    docker_seal: Hash,
    test_seal: Hash,
    recipe: Hash,
    policy_hash: Hash,
    executable: Hash,
    policy: &PublicEgressPolicy,
    enrolled: PublicTargets,
) -> Result<PublicTargets, NativeRefusal> {
    if bytes.len() as u64 > MAX_BYTES || docker_seal == test_seal {
        return Err(NETWORK);
    }
    let receipt: Receipt = serde_json::from_slice(bytes).map_err(|_| NETWORK)?;
    let matches = |text: &str, expected: Hash| expected != [0; 32] && text == hex::encode(expected);
    let observed = receipt.r#static;
    let comparison = receipt.test;
    let uplink = policy.uplink();
    let public_targets = PublicTargets {
        public_tcp_target: observed.tcp_target,
        public_dns_target: observed.dns_target,
    };
    public_targets.verify(policy)?;
    // The reviewed run must have exercised exactly the enrolled sockets.
    if public_targets != enrolled {
        return Err(NETWORK);
    }
    if receipt.version != 1
        || !matches(&receipt.qualification_sha256, docker_seal)
        || !matches(&receipt.m4_qualification_sha256, test_seal)
        || !matches(&receipt.recipe_sha256, recipe)
        || !matches(&receipt.policy_sha256, policy_hash)
        || !matches(&receipt.qemu_sha256, executable)
        || !observed.public_tcp_connected
        || !observed.udp_dns_answer
        || !observed.root_capable_fixture
        || !observed.ipv6_disabled
        || !observed.route_drift_closed
        || comparison.network_class != "restricted"
        || comparison.tcp_target != observed.tcp_target
        || comparison.dns_target != observed.dns_target
        || comparison.public_tcp_connected
        || comparison.udp_dns_answer
        || comparison.witness_tcp_arrivals != 0
        || comparison.witness_udp_arrivals != 0
        || !(4..=32).contains(&observed.denied.len())
    {
        return Err(NETWORK);
    }
    let mut targets = BTreeSet::new();
    for row in observed.denied {
        if row.target.port() == 0
            || !policy.denies(*row.target.ip())
            || !targets.insert(row.target)
            || row.tcp_connected
            || row.udp_replied
            || row.witness_tcp_arrivals != 0
            || row.witness_udp_arrivals != 0
        {
            return Err(NETWORK);
        }
    }
    let contains = |ip| targets.iter().any(|target| *target.ip() == ip);
    if !contains(uplink.source)
        || !contains(uplink.gateway)
        || !contains([10, 0, 2, 2].into())
        || !contains([10, 0, 2, 3].into())
        || !targets.iter().any(|t| t.ip().is_loopback())
        || !targets.iter().any(|t| t.ip().is_link_local())
        || !targets.iter().any(|t| {
            let [a, b, ..] = t.ip().octets();
            a == 100 && (64..128).contains(&b)
        })
    {
        return Err(NETWORK);
    }
    Ok(public_targets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn witness_requires_exact_seals_pins_controls_and_denials() {
        let body = "version=1\ninterface_index=7\ninterface_luid=8\nsource=10.8.0.4\ngateway=10.8.0.1\ndeny=11.0.0.0/24\ndeny=12.0.0.9/32\n";
        let policy = PublicEgressPolicy::parse(
            format!(
                "{body}fingerprint={}\n",
                hex::encode(digest(body.as_bytes()))
            )
            .as_bytes(),
        )
        .unwrap();
        let denied: Vec<_> = [
            "10.8.0.4:443",
            "10.8.0.1:443",
            "10.0.2.2:443",
            "10.0.2.3:53",
            "169.254.1.1:80",
            "100.64.0.1:80",
            "127.0.0.1:443",
            "11.0.0.15:443", // enrolled public-numbered LAN
            "12.0.0.9:443",  // enrolled WAN hairpin
        ]
        .iter()
        .map(|target| {
            json!({"target":target,"tcp_connected":false,"udp_replied":false,
                "witness_tcp_arrivals":0,"witness_udp_arrivals":0})
        })
        .collect();
        let receipt = json!({
            "version":1,"qualification_sha256":"01".repeat(32),"m4_qualification_sha256":"02".repeat(32),
            "recipe_sha256":"03".repeat(32),"policy_sha256":"04".repeat(32),"qemu_sha256":"05".repeat(32),
            "static":{"tcp_target":"8.8.8.8:443","dns_target":"9.9.9.9:53","public_tcp_connected":true,
                "udp_dns_answer":true,"root_capable_fixture":true,"ipv6_disabled":true,"route_drift_closed":true,
                "denied":denied},
            "test":{"network_class":"restricted","tcp_target":"8.8.8.8:443","dns_target":"9.9.9.9:53",
                "public_tcp_connected":false,"udp_dns_answer":false,"witness_tcp_arrivals":0,"witness_udp_arrivals":0}
        });
        let profile = br#"{"public_tcp_target":"8.8.8.8:443","public_dns_target":"9.9.9.9:53"}"#;
        let expected = PublicTargets::parse(profile, &policy).unwrap();
        let check_against = |receipt: &serde_json::Value, enrolled| {
            verify(
                &serde_json::to_vec(receipt).unwrap(),
                [1; 32],
                [2; 32],
                [3; 32],
                [4; 32],
                [5; 32],
                &policy,
                enrolled,
            )
        };
        let check = |receipt: &serde_json::Value| check_against(receipt, expected);
        assert_eq!(check(&receipt), Ok(expected));
        // A consistent receipt for another TCP port, or another DNS socket,
        // is refused against the enrolled targets.
        let mut different_port = receipt.clone();
        for role in ["static", "test"] {
            different_port[role]["tcp_target"] = json!("8.8.8.8:8443");
        }
        assert_eq!(check(&different_port), Err(NETWORK));
        let moved = PublicTargets::parse(
            br#"{"public_tcp_target":"8.8.8.8:8443","public_dns_target":"9.9.9.9:53"}"#,
            &policy,
        )
        .unwrap();
        assert_eq!(check_against(&different_port, moved), Ok(moved));
        assert_eq!(check_against(&receipt, moved), Err(NETWORK));
        let mut different_dns = receipt.clone();
        for role in ["static", "test"] {
            different_dns[role]["dns_target"] = json!("9.9.9.10:53");
        }
        assert_eq!(check(&different_dns), Err(NETWORK));
        let mut missing_loopback = receipt.clone();
        missing_loopback["static"]["denied"]
            .as_array_mut()
            .unwrap()
            .remove(6);
        assert_eq!(check(&missing_loopback), Err(NETWORK));
        for path in [
            "/qualification_sha256",
            "/m4_qualification_sha256",
            "/recipe_sha256",
            "/policy_sha256",
            "/qemu_sha256",
            "/static/route_drift_closed",
            "/static/ipv6_disabled",
            "/static/denied/0/udp_replied",
            "/static/denied/3/witness_udp_arrivals",
            "/static/denied/6/witness_tcp_arrivals",
            "/static/denied/7/witness_udp_arrivals",
            "/static/denied/8/witness_tcp_arrivals",
            "/test/witness_tcp_arrivals",
        ] {
            let mut invalid = receipt.clone();
            *invalid.pointer_mut(path).unwrap() = json!(1);
            assert_eq!(check(&invalid), Err(NETWORK), "{path}");
        }
        let mut duplicate = receipt.clone();
        duplicate["static"]["denied"][1] = duplicate["static"]["denied"][0].clone();
        assert_eq!(check(&duplicate), Err(NETWORK));
        let mut private = receipt;
        private["static"]["tcp_target"] = json!("192.168.2.1:443");
        private["test"]["tcp_target"] = json!("192.168.2.1:443");
        assert_eq!(check(&private), Err(NETWORK));
    }
}
