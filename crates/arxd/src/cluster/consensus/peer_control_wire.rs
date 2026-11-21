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

#[cfg(any(test, target_os = "linux"))]
pub(super) mod issuer {
    use super::*;
    use crate::cluster::consensus::issuer_transfer::{CHALLENGE_LEN, CLAIM_LEN, Claim};
    use tokio::io::{AsyncWrite, AsyncWriteExt};

    pub(in crate::cluster::consensus) const MAGIC: [u8; 4] = *b"ARXI";
    pub(in crate::cluster::consensus) const PACKAGE_MAX: usize = 16 * 1024;

    async fn expect(stream: &mut (impl AsyncRead + Unpin), verb: u8) -> Result<(), Refusal> {
        let mut header = [0; 2];
        stream
            .read_exact(&mut header)
            .await
            .map_err(|_| Refusal::Frame)?;
        if header != [1, verb] {
            return Err(Refusal::Frame);
        }
        Ok(())
    }

    async fn read<const N: usize>(
        stream: &mut (impl AsyncRead + Unpin),
        verb: u8,
    ) -> Result<[u8; N], Refusal> {
        expect(stream, verb).await?;
        let mut bytes = [0; N];
        stream
            .read_exact(&mut bytes)
            .await
            .map_err(|_| Refusal::Frame)?;
        Ok(bytes)
    }

    async fn write(
        stream: &mut (impl AsyncWrite + Unpin),
        verb: u8,
        bytes: &[u8],
    ) -> Result<(), Refusal> {
        stream
            .write_all(&[1, verb])
            .await
            .map_err(|_| Refusal::Frame)?;
        stream.write_all(bytes).await.map_err(|_| Refusal::Frame)?;
        stream.flush().await.map_err(|_| Refusal::Frame)
    }

    pub(in crate::cluster::consensus) async fn hello(
        stream: &mut (impl AsyncWrite + Unpin),
        claim: Claim,
    ) -> Result<(), Refusal> {
        stream.write_all(&MAGIC).await.map_err(|_| Refusal::Frame)?;
        write(stream, 1, &claim.encode()).await
    }

    pub(in crate::cluster::consensus) async fn read_hello(
        stream: &mut (impl AsyncRead + Unpin),
    ) -> Result<Claim, Refusal> {
        Claim::decode(read::<CLAIM_LEN>(stream, 1).await?).ok_or(Refusal::Frame)
    }

    pub(in crate::cluster::consensus) async fn challenge(
        stream: &mut (impl AsyncWrite + Unpin),
        challenge: &[u8; CHALLENGE_LEN],
    ) -> Result<(), Refusal> {
        write(stream, 2, challenge).await
    }

    pub(in crate::cluster::consensus) async fn read_challenge(
        stream: &mut (impl AsyncRead + Unpin),
    ) -> Result<[u8; CHALLENGE_LEN], Refusal> {
        read(stream, 2).await
    }

    pub(in crate::cluster::consensus) async fn sealed(
        stream: &mut (impl AsyncWrite + Unpin),
        ciphertext: &[u8],
    ) -> Result<(), Refusal> {
        if !(26..=PACKAGE_MAX).contains(&ciphertext.len()) {
            return Err(Refusal::Frame);
        }
        stream
            .write_all(&[1, 3])
            .await
            .map_err(|_| Refusal::Frame)?;
        stream
            .write_all(&(ciphertext.len() as u16).to_be_bytes())
            .await
            .map_err(|_| Refusal::Frame)?;
        stream
            .write_all(ciphertext)
            .await
            .map_err(|_| Refusal::Frame)?;
        stream.shutdown().await.map_err(|_| Refusal::Frame)
    }

    pub(in crate::cluster::consensus) async fn read_sealed(
        stream: &mut (impl AsyncRead + Unpin),
    ) -> Result<Vec<u8>, Refusal> {
        let length = u16::from_be_bytes(read(stream, 3).await?) as usize;
        if !(26..=PACKAGE_MAX).contains(&length) {
            return Err(Refusal::Frame);
        }
        let mut ciphertext = vec![0; length];
        stream
            .read_exact(&mut ciphertext)
            .await
            .map_err(|_| Refusal::Frame)?;
        clean_end(stream).await?;
        Ok(ciphertext)
    }

    pub(in crate::cluster::consensus) async fn acknowledge(
        stream: &mut (impl AsyncWrite + Unpin),
        challenge: &[u8; CHALLENGE_LEN],
    ) -> Result<(), Refusal> {
        write(stream, 4, challenge).await?;
        stream.shutdown().await.map_err(|_| Refusal::Frame)
    }

    pub(in crate::cluster::consensus) async fn read_acknowledgement(
        stream: &mut (impl AsyncRead + Unpin),
        challenge: &[u8; CHALLENGE_LEN],
    ) -> Result<(), Refusal> {
        if &read::<CHALLENGE_LEN>(stream, 4).await? != challenge {
            return Err(Refusal::Frame);
        }
        clean_end(stream).await
    }
}
