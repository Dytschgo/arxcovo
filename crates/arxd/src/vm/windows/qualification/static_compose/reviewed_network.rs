//! Independently reviewed acceptance witness for a selected Docker and TEST seal.
use super::*;
use std::{collections::BTreeSet, net::SocketAddrV4};

pub(super) const MAX_BYTES: u64 = 65_536;
const NETWORK: NativeRefusal = NativeRefusal::NetworkFenceUnverified;

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

pub(super) fn verify(
    bytes: &[u8],
    docker_seal: Hash,
    test_seal: Hash,
    recipe: Hash,
    policy_hash: Hash,
    executable: Hash,
    policy: &PublicEgressPolicy,
) -> Result<[std::net::Ipv4Addr; 2], NativeRefusal> {
    if bytes.len() as u64 > MAX_BYTES || docker_seal == test_seal {
        return Err(NETWORK);
    }
    let receipt: Receipt = serde_json::from_slice(bytes).map_err(|_| NETWORK)?;
    let matches = |text: &str, expected: Hash| expected != [0; 32] && text == hex::encode(expected);
    let observed = receipt.r#static;
    let comparison = receipt.test;
    let uplink = policy.uplink();
    let public_targets = [*observed.tcp_target.ip(), *observed.dns_target.ip()];
    if receipt.version != 1
        || !matches(&receipt.qualification_sha256, docker_seal)
        || !matches(&receipt.m4_qualification_sha256, test_seal)
        || !matches(&receipt.recipe_sha256, recipe)
        || !matches(&receipt.policy_sha256, policy_hash)
        || !matches(&receipt.qemu_sha256, executable)
        || observed.tcp_target.port() == 0
        || observed.dns_target.port() != 53
        || public_targets.iter().any(|ip| policy.denies(*ip))
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
        let body =
            "version=1\ninterface_index=7\ninterface_luid=8\nsource=10.8.0.4\ngateway=10.8.0.1\n";
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
        let check = |receipt: &serde_json::Value| {
            verify(
                &serde_json::to_vec(receipt).unwrap(),
                [1; 32],
                [2; 32],
                [3; 32],
                [4; 32],
                [5; 32],
                &policy,
            )
        };
        assert_eq!(check(&receipt), Ok([[8; 4].into(), [9; 4].into()]));
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
