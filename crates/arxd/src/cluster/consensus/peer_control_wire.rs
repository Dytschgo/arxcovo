//! Closed private peer control protocol. Raft frames keep their existing codec.
use super::{
    LogPos,
    learner_admission::Reply,
    peer_rpc_wire::Route,
    peer_transport::{Refusal, clean_end, endpoint},
};
use std::net::SocketAddr;
use tokio::io::{AsyncRead, AsyncReadExt};

pub(super) const MAGIC: [u8; 4] = *b"ARXC";
const MAX_ADDR: usize = 200;
const MAX_REPLY: usize = 1 + 24 + MAX_ADDR;

pub(super) fn qualified(address: SocketAddr) -> bool {
    match address.ip() {
        std::net::IpAddr::V4(ip) => {
            ip.is_private() || (ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1]))
        }
        std::net::IpAddr::V6(ip) => ip.is_unique_local(),
    }
}

fn address(bytes: &[u8]) -> Result<SocketAddr, Refusal> {
    let text = std::str::from_utf8(bytes).map_err(|_| Refusal::Frame)?;
    let address = endpoint(text)?;
    if !qualified(address) {
        return Err(Refusal::Address);
    }
    Ok(address)
}

fn frame(route: Route, method: u8, body: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(40 + body.len());
    bytes.extend(MAGIC);
    bytes.extend([1, method]);
    bytes.extend(route.cluster);
    bytes.extend(route.source.to_be_bytes());
    bytes.extend(route.target.to_be_bytes());
    bytes.extend((body.len() as u16).to_be_bytes());
    bytes.extend(body);
    bytes
}

async fn body(
    stream: &mut (impl AsyncRead + Unpin),
    route: Route,
    method: u8,
    max: usize,
) -> Result<Vec<u8>, Refusal> {
    let mut header = [0; 36];
    stream
        .read_exact(&mut header)
        .await
        .map_err(|_| Refusal::Frame)?;
    if header[..34] != frame(route, method, &[])[4..38] {
        return Err(Refusal::Frame);
    }
    let len = u16::from_be_bytes([header[34], header[35]]) as usize;
    if len == 0 || len > max {
        return Err(Refusal::Frame);
    }
    let mut bytes = vec![0; len];
    stream
        .read_exact(&mut bytes)
        .await
        .map_err(|_| Refusal::Frame)?;
    Ok(bytes)
}

pub(super) fn request(route: Route, advertised: SocketAddr) -> Vec<u8> {
    frame(route, 1, advertised.to_string().as_bytes())
}

pub(super) async fn read_request(
    stream: &mut (impl AsyncRead + Unpin),
    route: Route,
    source: SocketAddr,
) -> Result<SocketAddr, Refusal> {
    let advertised = address(&body(stream, route, 1, MAX_ADDR).await?)?;
    clean_end(stream).await?;
    if advertised.ip() != source.ip() {
        return Err(Refusal::Address);
    }
    Ok(advertised)
}

pub(super) fn reply(route: Route, reply: Reply) -> Vec<u8> {
    let mut bytes = vec![match reply {
        Reply::Applied(..) => 0,
        Reply::Revoked(..) => 1,
        Reply::Pending => 2,
        Reply::AddressConflict => 3,
        Reply::StaleTail => 4,
        Reply::Refused => 5,
        Reply::Unknown => 6,
    }];
    if let Reply::Applied(position, address) | Reply::Revoked(position, address) = reply {
        bytes.extend(position.term.to_be_bytes());
        bytes.extend(position.leader.to_be_bytes());
        bytes.extend(position.index.to_be_bytes());
        bytes.extend(address.to_string().as_bytes());
    }
    frame(route, 2, &bytes)
}

pub(super) async fn read_reply(
    stream: &mut (impl AsyncRead + Unpin),
    route: Route,
    advertised: SocketAddr,
) -> Result<Reply, Refusal> {
    let mut magic = [0; 4];
    stream
        .read_exact(&mut magic)
        .await
        .map_err(|_| Refusal::Frame)?;
    if magic != MAGIC {
        return Err(Refusal::Frame);
    }
    let bytes = body(stream, route, 2, MAX_REPLY).await?;
    Ok(match bytes.as_slice() {
        [status @ (0 | 1), rest @ ..] if rest.len() > 24 => {
            let position = LogPos {
                term: u64::from_be_bytes(rest[..8].try_into().unwrap()),
                leader: u64::from_be_bytes(rest[8..16].try_into().unwrap()),
                index: u64::from_be_bytes(rest[16..24].try_into().unwrap()),
            };
            let address = address(&rest[24..])?;
            if position.term == 0 || position.leader == 0 || address != advertised {
                return Err(Refusal::Frame);
            }
            if *status == 0 {
                Reply::Applied(position, address)
            } else {
                Reply::Revoked(position, address)
            }
        }
        [2] => Reply::Pending,
        [3] => Reply::AddressConflict,
        [4] => Reply::StaleTail,
        [5] => Reply::Refused,
        [6] => Reply::Unknown,
        _ => return Err(Refusal::Frame),
    })
}
