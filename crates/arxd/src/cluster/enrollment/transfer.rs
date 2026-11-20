//! Bounded one-paste envelope for the original join code and public CA bytes.
use super::JoinCode;
use crate::cluster::pki::ca::fingerprint_of;
use sha2::{Digest, Sha256};
use std::{
    fmt,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
};

const PREFIX: &str = "arxje1-";
const MAX_PASTE: usize = 8_800;
const MAX_ROOT: usize = 4_096;
const INNER: usize = 270;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cluster) enum Refusal {
    Envelope,
    Root,
    Route,
}

pub(in crate::cluster) struct OwnerPaste(String);

impl OwnerPaste {
    /// Use only with a protected owner control.
    pub(in crate::cluster) fn expose_for_owner_transfer(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for OwnerPaste {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// One explicit prefix from the joiner's locally qualified LAN or Tailscale
/// routes. Callers must derive these from local interface policy, never a paste.
pub(in crate::cluster) struct LocalRoute {
    pub network: IpAddr,
    pub prefix: u8,
}

impl LocalRoute {
    /// Return the prefix length for a match and whether it is a usable host.
    fn match_host(&self, ip: IpAddr) -> Option<(u8, bool)> {
        let prefix = self.prefix;
        match (self.network, ip) {
            (IpAddr::V4(network), IpAddr::V4(ip)) if (1..=32).contains(&prefix) => {
                let mask = u32::MAX << (32 - prefix);
                let ip = u32::from(ip);
                let base = u32::from(network);
                let host = ip & !mask;
                (base & !mask == 0 && ip & mask == base)
                    .then_some((prefix, prefix >= 31 || (host != 0 && host != !mask)))
            }
            (IpAddr::V6(network), IpAddr::V6(ip)) if (1..=128).contains(&prefix) => {
                let mask = u128::MAX << (128 - prefix);
                let base = u128::from(network);
                (base & !mask == 0 && u128::from(ip) & mask == base).then_some((prefix, true))
            }
            _ => None,
        }
    }
}

pub(in crate::cluster) struct Transfer {
    pub endpoint: SocketAddr,
    pub root: Vec<u8>,
    pub code: JoinCode,
}

pub(in crate::cluster) fn encode(
    endpoint: SocketAddr,
    root: &[u8],
    code: &JoinCode,
) -> Result<OwnerPaste, Refusal> {
    check_endpoint(endpoint)?;
    check_root(root, code)?;
    let mut body = Vec::with_capacity(1 + 16 + 4 + root.len() + INNER + 4);
    match endpoint.ip() {
        IpAddr::V4(ip) => {
            body.push(4);
            body.extend(ip.octets());
        }
        IpAddr::V6(ip) => {
            body.push(6);
            body.extend(ip.octets());
        }
    }
    body.extend(endpoint.port().to_be_bytes());
    body.extend(
        u16::try_from(root.len())
            .expect("bounded root")
            .to_be_bytes(),
    );
    body.extend(root);
    body.extend(code.expose_for_owner_transfer().bytes());
    body.extend(checksum(&body));
    Ok(OwnerPaste(format!("{PREFIX}{}", hex::encode(body))))
}

pub(in crate::cluster) fn parse(text: &str, routes: &[LocalRoute]) -> Result<Transfer, Refusal> {
    if text.len() > MAX_PASTE {
        return Err(Refusal::Envelope);
    }
    let payload = text
        .trim_matches(|c: char| c.is_ascii_whitespace() || c == '\x0b')
        .strip_prefix(PREFIX)
        .ok_or(Refusal::Envelope)?;
    if !payload
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Refusal::Envelope);
    }
    let bytes = hex::decode(payload).map_err(|_| Refusal::Envelope)?;
    let (body, check) = bytes.split_last_chunk::<4>().ok_or(Refusal::Envelope)?;
    if *check != checksum(body) {
        return Err(Refusal::Envelope);
    }

    let (&family, address) = body.split_first().ok_or(Refusal::Envelope)?;
    let (ip, fields) = match family {
        4 => {
            let (ip, rest) = address.split_first_chunk::<4>().ok_or(Refusal::Envelope)?;
            (IpAddr::V4(Ipv4Addr::from(*ip)), rest)
        }
        6 => {
            let (ip, rest) = address.split_first_chunk::<16>().ok_or(Refusal::Envelope)?;
            (IpAddr::V6(Ipv6Addr::from(*ip)), rest)
        }
        _ => return Err(Refusal::Envelope),
    };
    let (&[p0, p1, l0, l1], rest) = fields.split_first_chunk().ok_or(Refusal::Envelope)?;
    let endpoint = SocketAddr::new(ip, u16::from_be_bytes([p0, p1]));
    check_endpoint(endpoint)?;
    let root_len = usize::from(u16::from_be_bytes([l0, l1]));
    if !(1..=MAX_ROOT).contains(&root_len) || rest.len() != root_len + INNER {
        return Err(Refusal::Envelope);
    }
    let (root, inner) = rest.split_at(root_len);
    let inner = std::str::from_utf8(inner).map_err(|_| Refusal::Envelope)?;
    let code = JoinCode::parse(inner).map_err(|_| Refusal::Envelope)?;
    if code.expose_for_owner_transfer() != inner {
        return Err(Refusal::Envelope);
    }
    check_root(root, &code)?;
    if !routes
        .iter()
        .filter_map(|route| route.match_host(ip))
        .max_by_key(|&(prefix, _)| prefix)
        .is_some_and(|(_, host)| host)
    {
        return Err(Refusal::Route);
    }
    Ok(Transfer {
        endpoint,
        root: root.to_vec(),
        code,
    })
}

fn check_endpoint(endpoint: SocketAddr) -> Result<(), Refusal> {
    let ip = endpoint.ip();
    let special = match endpoint {
        SocketAddr::V4(v) => v.ip().is_link_local() || v.ip().is_broadcast(),
        SocketAddr::V6(v) => {
            v.ip().is_unicast_link_local()
                || v.ip().to_ipv4_mapped().is_some()
                || v.scope_id() != 0
                || v.flowinfo() != 0
        }
    };
    if endpoint.port() == 0
        || ip.is_unspecified()
        || ip.is_multicast()
        || ip.is_loopback()
        || special
    {
        return Err(Refusal::Route);
    }
    Ok(())
}

fn check_root(root: &[u8], code: &JoinCode) -> Result<(), Refusal> {
    if !(1..=MAX_ROOT).contains(&root.len()) || fingerprint_of(root) != code.ca_fingerprint() {
        return Err(Refusal::Root);
    }
    Ok(())
}

fn checksum(body: &[u8]) -> [u8; 4] {
    let digest = Sha256::new()
        .chain_update(b"arxcovo/join/transfer/v1")
        .chain_update(body)
        .finalize();
    [digest[0], digest[1], digest[2], digest[3]]
}

#[cfg(test)]
mod tests;
