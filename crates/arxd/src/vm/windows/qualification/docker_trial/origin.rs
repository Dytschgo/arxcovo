//! HTTPS proof anchored in the certificate returned by the held serial peer.
use super::{Hash, NativeRefusal, Shutdown, digest};
use arx_guest::docker_trial::{Challenge, ORIGIN_HEALTH_PATH, ORIGIN_PROFILE};
use rustls::{
    ClientConfig, RootCertStore,
    pki_types::{CertificateDer, ServerName},
};
use std::{net::Ipv4Addr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tokio_rustls::TlsConnector;

const REFUSED: NativeRefusal = NativeRefusal::GuestEndpointUnverified;
const MAX_HEADERS: usize = 2048;
const MAX_PROBE: Duration = Duration::from_secs(5);

pub(in crate::vm::windows::qualification) fn profile_pin() -> Hash {
    digest(ORIGIN_PROFILE)
}

pub(super) async fn probe(
    port: u16,
    challenge: Challenge,
    der: &[u8],
    remaining: Duration,
    cancel: &Shutdown,
) -> Result<(), NativeRefusal> {
    if cancel.is_cancelled() || remaining.is_zero() {
        return Err(REFUSED);
    }
    let leaf = CertificateDer::from(der.to_vec());
    let name = challenge.origin_name();
    let server = ServerName::try_from(name.clone()).map_err(|_| REFUSED)?;
    let mut roots = RootCertStore::empty();
    roots.add(leaf.clone()).map_err(|_| REFUSED)?;
    let config =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|_| REFUSED)?
            .with_root_certificates(roots)
            .with_no_client_auth();
    let body = challenge.origin_health();
    let request =
        format!("GET {ORIGIN_HEALTH_PATH} HTTP/1.1\r\nHost: {name}\r\nConnection: close\r\n\r\n");
    let check = async {
        let socket = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .map_err(|_| REFUSED)?;
        let mut tls = TlsConnector::from(Arc::new(config))
            .connect(server, socket)
            .await
            .map_err(|_| REFUSED)?;
        if tls.get_ref().1.peer_certificates() != Some(std::slice::from_ref(&leaf)) {
            return Err(REFUSED);
        }
        tls.write_all(request.as_bytes())
            .await
            .map_err(|_| REFUSED)?;
        tls.flush().await.map_err(|_| REFUSED)?;
        let mut headers = Vec::new();
        while headers.len() < MAX_HEADERS && !headers.ends_with(b"\r\n\r\n") {
            headers.push(tls.read_u8().await.map_err(|_| REFUSED)?);
        }
        verify_headers(&headers, body.len())?;
        let mut received = vec![0; body.len()];
        tls.read_exact(&mut received).await.map_err(|_| REFUSED)?;
        if received != body.as_bytes() {
            return Err(REFUSED);
        }
        Ok(())
    };
    tokio::select! {
        biased;
        () = cancel.cancelled() => Err(REFUSED),
        result = tokio::time::timeout(remaining.min(MAX_PROBE), check) => result.map_err(|_| REFUSED)?,
    }
}

fn verify_headers(headers: &[u8], expected: usize) -> Result<(), NativeRefusal> {
    let mut lines = std::str::from_utf8(headers)
        .map_err(|_| REFUSED)?
        .strip_suffix("\r\n\r\n")
        .ok_or(REFUSED)?
        .split("\r\n");
    if lines.next() != Some("HTTP/1.1 200 OK") {
        return Err(REFUSED);
    }
    let mut length = None;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(REFUSED)?;
        if name.eq_ignore_ascii_case("content-length") {
            if length.is_some() {
                return Err(REFUSED);
            }
            length = Some(value.trim().parse::<usize>().map_err(|_| REFUSED)?);
        }
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(REFUSED);
        }
    }
    if length != Some(expected) {
        return Err(REFUSED);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_requires_a_bounded_unambiguous_200_response() {
        let status = "HTTP/1.1 200 OK";
        let length = "Content-Length: 40";
        assert!(verify_headers(format!("{status}\r\n{length}\r\n\r\n").as_bytes(), 40).is_ok());
        for headers in [
            format!("{status}\r\n{length}\r\n{length}\r\n\r\n"),
            format!("{status}\r\n{length}\r\nTransfer-Encoding: chunked\r\n\r\n"),
            format!("{status}\r\nContent-Length: 41\r\n\r\n"),
            format!("HTTP/1.1 204 No Content\r\n{length}\r\n\r\n"),
            format!("{status}\r\n{length}\r\n"),
        ] {
            assert_eq!(verify_headers(headers.as_bytes(), 40), Err(REFUSED));
        }
    }
}
