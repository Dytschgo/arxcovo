//! DNS effects for one exact owner binding. The caller journals intent before mutations.

#[cfg(test)]
use std::net::SocketAddr;
use std::{fmt, io, sync::Arc, time::Duration};

use rustls::{ClientConfig, RootCertStore, pki_types::ServerName};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};
use tokio_rustls::TlsConnector;

use super::{Hostname, InvalidInput, TunnelId, hex_id};

const API_HOST: &str = "api.cloudflare.com";
const HTTP_LIMIT: usize = 128 * 1024;
const HEADER_LIMIT: usize = 8 * 1024;
const RECORD_LIMIT: usize = 100;

pub(in crate::vm) struct DnsBearer(zeroize::Zeroizing<String>);

impl DnsBearer {
    pub(in crate::vm) fn new(value: String) -> Result<Self, InvalidInput> {
        let value = zeroize::Zeroizing::new(value);
        if value.is_empty()
            || value.len() > 512
            || !value.bytes().all(|b| {
                b.is_ascii_alphanumeric()
                    || matches!(b, b'-' | b'_' | b'.' | b'~' | b'+' | b'/' | b'=')
            })
        {
            return Err(InvalidInput);
        }
        Ok(Self(value))
    }
}

impl fmt::Debug for DnsBearer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DnsBearer([redacted])")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) enum DnsError {
    Unavailable,
    InvalidResponse,
    Rejected,
    Conflict,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) struct RecordId(String);

impl RecordId {
    pub(in crate::vm) fn parse(value: String) -> Result<Self, InvalidInput> {
        hex_id(&value).then_some(Self(value)).ok_or(InvalidInput)
    }

    pub(in crate::vm) fn as_str(&self) -> &str {
        &self.0
    }
}

pub(in crate::vm) struct Route {
    zone: String,
    name: Hostname,
    target: String,
    comment: String,
}

impl Route {
    /// A persisted nonce is required before a POST. Use a fresh random nonce for each new intent.
    pub(in crate::vm) fn new(
        zone: &str,
        name: Hostname,
        tunnel: &TunnelId,
        nonce: &str,
    ) -> Result<Self, InvalidInput> {
        if !hex_id(zone) || !hex_id(nonce) {
            return Err(InvalidInput);
        }
        Ok(Self {
            zone: zone.to_owned(),
            name,
            target: format!("{}.cfargotunnel.com", tunnel.as_str()),
            comment: format!("arxcovo:{nonce}"),
        })
    }

    pub(in crate::vm) fn new_nonce() -> Result<String, DnsError> {
        let mut bytes = [0; 16];
        getrandom::fill(&mut bytes).map_err(|_| DnsError::Unavailable)?;
        Ok(hex::encode(bytes))
    }

    fn owned(&self, record: &Record) -> bool {
        record.name == self.name.as_str()
            && record.kind == "CNAME"
            && record.content == self.target
            && record.proxied == Some(true)
            && record.comment.as_deref() == Some(&self.comment)
    }

    fn records(&self) -> String {
        format!("/client/v4/zones/{}/dns_records", self.zone)
    }

    /// The exact persisted tuple, for fake witnesses only.
    #[cfg(test)]
    pub(in crate::vm) fn witness(&self) -> String {
        format!(
            "{} {} {} {}",
            self.zone,
            self.name.as_str(),
            self.target,
            self.comment
        )
    }
}

#[derive(Deserialize)]
struct Envelope<T> {
    success: bool,
    result: T,
}

#[derive(Deserialize)]
struct Listing {
    success: bool,
    result: Vec<Record>,
    result_info: Page,
}

#[derive(Deserialize)]
struct Page {
    page: usize,
    total_pages: usize,
    total_count: usize,
}

#[derive(Deserialize)]
struct Record {
    id: String,
    name: String,
    #[serde(rename = "type")]
    kind: String,
    content: String,
    proxied: Option<bool>,
    comment: Option<String>,
}

#[derive(Deserialize)]
struct Deleted {
    id: String,
}

#[derive(Serialize)]
struct Create<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    name: &'a str,
    content: &'a str,
    proxied: bool,
    ttl: u32,
    comment: &'a str,
}

#[derive(Debug, Eq, PartialEq)]
pub(in crate::vm) enum Reconciled {
    /// Absence is an observation. It cannot settle an earlier uncertain POST on its own.
    Absent,
    /// The exact name holds only this marked record.
    Owned(RecordId),
    /// Exactly one marked record shares the name with foreign records. The caller may
    /// persist the ID for targeted cleanup but must treat the name as conflicted.
    OwnedWithConflict(RecordId),
    /// No marked record, or more than one, so no ID can be attributed.
    ForeignOrAmbiguous,
}

#[derive(Debug, Eq, PartialEq)]
pub(in crate::vm) enum Removal {
    Absent,
    ForeignOrAmbiguous,
}

pub(in crate::vm) trait DnsApi {
    async fn preflight(&self, route: &Route) -> Result<(), DnsError>;
    /// Returns the validated, owned record ID acknowledged by the single POST.
    /// The caller persists it before [`DnsApi::verify`], so a failed
    /// verification never discards an acknowledged ID.
    async fn create(&self, route: &Route) -> Result<RecordId, DnsError>;
    async fn reconcile(&self, route: &Route) -> Result<Reconciled, DnsError>;
    async fn remove(&self, route: &Route, id: &RecordId) -> Result<Removal, DnsError>;

    /// Independent exact-name evidence that `id` is the only record at the name.
    async fn verify(&self, route: &Route, id: &RecordId) -> Result<(), DnsError> {
        match self.reconcile(route).await? {
            Reconciled::Owned(confirmed) if confirmed == *id => Ok(()),
            _ => Err(DnsError::Conflict),
        }
    }
}

enum Transport {
    Cloudflare(TlsConnector),
    #[cfg(test)]
    #[allow(dead_code, reason = "exercised by the DNS integration fixture")]
    Fixture(SocketAddr),
}

pub(in crate::vm) struct CloudflareDns {
    bearer: DnsBearer,
    transport: Transport,
    deadline: Duration,
}

impl CloudflareDns {
    /// Use reviewed public trust anchors supplied by the daemon's HTTPS setup.
    /// Origin CAs and MCP arguments cannot configure this transport.
    pub(in crate::vm) fn new(
        bearer: DnsBearer,
        roots: RootCertStore,
    ) -> Result<Self, InvalidInput> {
        if roots.is_empty() {
            return Err(InvalidInput);
        }
        let config =
            ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .map_err(|_| InvalidInput)?
                .with_root_certificates(roots)
                .with_no_client_auth();
        Ok(Self {
            bearer,
            transport: Transport::Cloudflare(TlsConnector::from(Arc::new(config))),
            deadline: Duration::from_secs(5),
        })
    }

    #[cfg(test)]
    #[allow(dead_code, reason = "exercised by the DNS integration fixture")]
    pub(in crate::vm) fn fixture(bearer: DnsBearer, addr: SocketAddr) -> Self {
        assert!(addr.ip().is_loopback());
        Self {
            bearer,
            transport: Transport::Fixture(addr),
            deadline: Duration::from_millis(250),
        }
    }
}

impl DnsApi for CloudflareDns {
    /// Requires an empty name before the caller starts the connector.
    async fn preflight(&self, route: &Route) -> Result<(), DnsError> {
        match self.reconcile(route).await? {
            Reconciled::Absent => Ok(()),
            _ => Err(DnsError::Conflict),
        }
    }

    /// One POST only. On error the caller retains the intent and reconciles.
    /// Success still requires [`DnsApi::verify`] after the ID is durable.
    async fn create(&self, route: &Route) -> Result<RecordId, DnsError> {
        let body = serde_json::to_vec(&Create {
            kind: "CNAME",
            name: route.name.as_str(),
            content: &route.target,
            comment: &route.comment,
            proxied: true,
            ttl: 1,
        })
        .map_err(|_| DnsError::InvalidResponse)?;
        let reply = self.request(Method::Post, &route.records(), &body).await?;
        let created: Record = reply.result()?;
        if !route.owned(&created) {
            return Err(DnsError::Conflict);
        }
        RecordId::parse(created.id).map_err(|_| DnsError::InvalidResponse)
    }

    async fn reconcile(&self, route: &Route) -> Result<Reconciled, DnsError> {
        let path = format!(
            "{}?name.exact={}&page=1&per_page={RECORD_LIMIT}",
            route.records(),
            route.name.as_str()
        );
        let response = self.request(Method::Get, &path, &[]).await?;
        let listing: Listing = response.json()?;
        if !listing.success
            || listing.result_info.page != 1
            || listing.result_info.total_pages > 1
            || listing.result_info.total_count != listing.result.len()
            || listing.result.len() > RECORD_LIMIT
        {
            return Err(DnsError::InvalidResponse);
        }
        for record in &listing.result {
            if !hex_id(&record.id) || record.name != route.name.as_str() {
                return Err(DnsError::InvalidResponse);
            }
        }
        let mut owned = listing.result.iter().filter(|record| route.owned(record));
        Ok(match (owned.next(), owned.next()) {
            _ if listing.result.is_empty() => Reconciled::Absent,
            (Some(record), None) if listing.result.len() == 1 => {
                Reconciled::Owned(RecordId(record.id.clone()))
            }
            (Some(record), None) => Reconciled::OwnedWithConflict(RecordId(record.id.clone())),
            _ => Reconciled::ForeignOrAmbiguous,
        })
    }

    /// Call after initiating connector termination. Never delete by name or by marker alone.
    async fn remove(&self, route: &Route, id: &RecordId) -> Result<Removal, DnsError> {
        let path = format!("{}/{}", route.records(), id.as_str());
        let reply = self.request(Method::Get, &path, &[]).await?;
        if (200..300).contains(&reply.status) {
            let record: Record = reply.result()?;
            if id.as_str() != record.id || !route.owned(&record) {
                return Ok(Removal::ForeignOrAmbiguous);
            }
            let deleted: Deleted = self.request(Method::Delete, &path, &[]).await?.result()?;
            if deleted.id != id.as_str() {
                return Err(DnsError::InvalidResponse);
            }
        } else if reply.status != 404 {
            return Err(DnsError::Rejected);
        }
        match self.reconcile(route).await? {
            Reconciled::Absent => Ok(Removal::Absent),
            _ => Ok(Removal::ForeignOrAmbiguous),
        }
    }
}

impl CloudflareDns {
    async fn request(&self, method: Method, path: &str, body: &[u8]) -> Result<Response, DnsError> {
        let headers = zeroize::Zeroizing::new(format!(
            "{} {path} HTTP/1.1\r\nHost: {API_HOST}\r\nAuthorization: Bearer {}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            method.as_str(),
            self.bearer.0.as_str(),
            body.len()
        ));
        let result = timeout(self.deadline, async {
            match &self.transport {
                Transport::Cloudflare(tls) => {
                    let stream = TcpStream::connect((API_HOST, 443)).await?;
                    let name = ServerName::try_from(API_HOST).expect("fixed TLS server name");
                    let stream = tls.connect(name, stream).await?;
                    exchange(stream, &headers, body).await
                }
                #[cfg(test)]
                Transport::Fixture(addr) => {
                    exchange(TcpStream::connect(addr).await?, &headers, body).await
                }
            }
        })
        .await
        .map_err(|_| DnsError::Unavailable)?
        .map_err(|_| DnsError::Unavailable)?;
        Response::decode(&result)
    }
}

enum Method {
    Get,
    Post,
    Delete,
}

impl Method {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Delete => "DELETE",
        }
    }
}

async fn exchange(
    mut stream: impl AsyncRead + AsyncWrite + Unpin,
    headers: &str,
    body: &[u8],
) -> io::Result<Vec<u8>> {
    stream.write_all(headers.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await?;
    let mut response = Vec::new();
    // A complete self-delimited response can survive a TLS peer closing without close_notify.
    let read = stream
        .take((HTTP_LIMIT + 1) as u64)
        .read_to_end(&mut response)
        .await;
    if let Err(error) = read
        && error.kind() != io::ErrorKind::UnexpectedEof
    {
        return Err(error);
    }
    Ok(response)
}

struct Response {
    status: u16,
    body: Vec<u8>,
}

impl Response {
    fn decode(wire: &[u8]) -> Result<Self, DnsError> {
        let invalid = DnsError::InvalidResponse;
        if wire.len() > HTTP_LIMIT {
            return Err(invalid);
        }
        let end = wire
            .windows(4)
            .position(|p| p == b"\r\n\r\n")
            .ok_or(invalid)?;
        if end > HEADER_LIMIT {
            return Err(invalid);
        }
        let mut head = std::str::from_utf8(&wire[..end])
            .map_err(|_| invalid)?
            .split("\r\n");
        let status = status_code(head.next().ok_or(invalid)?).ok_or(invalid)?;
        let mut length = None;
        let mut chunked = false;
        for line in head {
            let (name, value) = line.split_once(':').ok_or(invalid)?;
            if name.eq_ignore_ascii_case("content-length") {
                if length.is_some() {
                    return Err(invalid);
                }
                let value = value.trim();
                if !value.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(invalid);
                }
                length = Some(value.parse::<usize>().map_err(|_| invalid)?);
            }
            if name.eq_ignore_ascii_case("transfer-encoding") {
                if chunked || !value.trim().eq_ignore_ascii_case("chunked") {
                    return Err(invalid);
                }
                chunked = true;
            }
        }
        let bytes = &wire[end + 4..];
        let body = match (chunked, length) {
            (false, Some(expected)) if expected == bytes.len() => bytes.to_vec(),
            (true, None) => decode_chunks(bytes).ok_or(invalid)?,
            _ => return Err(invalid),
        };
        Ok(Self { status, body })
    }

    fn json<T: DeserializeOwned>(&self) -> Result<T, DnsError> {
        if !(200..300).contains(&self.status) {
            return Err(DnsError::Rejected);
        }
        serde_json::from_slice(&self.body).map_err(|_| DnsError::InvalidResponse)
    }

    fn result<T: DeserializeOwned>(&self) -> Result<T, DnsError> {
        let envelope: Envelope<T> = self.json()?;
        envelope
            .success
            .then_some(envelope.result)
            .ok_or(DnsError::Rejected)
    }
}

pub(super) fn status_code(line: &str) -> Option<u16> {
    let tail = line
        .strip_prefix("HTTP/1.1 ")
        .or_else(|| line.strip_prefix("HTTP/1.0 "))?;
    let code = tail.split(' ').next()?;
    if code.len() != 3 || !code.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    code.parse().ok()
}

fn decode_chunks(mut bytes: &[u8]) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let end = bytes.windows(2).take(16).position(|p| p == b"\r\n")?;
        let size = std::str::from_utf8(&bytes[..end]).ok()?;
        if size.is_empty() || !size.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let size = usize::from_str_radix(size, 16).ok()?;
        bytes = &bytes[end + 2..];
        if size == 0 {
            return (bytes == b"\r\n").then_some(body);
        }
        body.extend_from_slice(bytes.get(..size)?);
        bytes = bytes.get(size..)?.strip_prefix(b"\r\n")?;
    }
}
